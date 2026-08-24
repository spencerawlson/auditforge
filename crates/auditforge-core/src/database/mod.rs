//! SQLite connection handling, migrations and shared query helpers.
//!
//! One process-wide connection guarded by a mutex. AuditForge is a single-user desktop
//! application with short transactions; a pool would add contention handling for a
//! problem it does not have, and a single connection makes "one writer at a time"
//! trivially true.

pub mod migrations;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OpenFlags, Transaction};

use crate::error::{Error, Result};
use crate::models::Timestamp;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (creating if needed) the database at `path` and bring the schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        restrict_permissions(path)?;
        Self::from_connection(conn)
    }

    /// In-memory database, used by tests and by the `--check` startup path.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        configure(&conn)?;
        let db = Database {
            conn: Mutex::new(conn),
        };
        db.with_tx(migrations::apply)?;
        Ok(db)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        // A poisoned mutex means another thread panicked mid-transaction. SQLite has
        // already rolled that transaction back, so the connection itself is usable and
        // recovering is preferable to poisoning the whole application.
        Ok(self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    /// Run a read-only or single-statement operation.
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.lock()?;
        f(&conn)
    }

    /// Run a closure inside a transaction, committing on `Ok` and rolling back on `Err`.
    pub fn with_tx<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Write a consistent copy of the live database to `dest`, safe to run while the
    /// application is in use.
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        let conn = self.lock()?;
        conn.backup(rusqlite::DatabaseName::Main, dest, None)
            .map_err(|e| Error::Database(format!("backup failed: {e}")))?;
        restrict_permissions(dest)?;
        Ok(())
    }
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL keeps readers from blocking the writer; NORMAL is the documented companion
    // setting for WAL and still survives an application crash.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// Restrict the database file to the owner. The database holds password hashes, report
/// content and audit history; other local accounts have no business reading it.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

/// On Windows the file inherits the ACL of the per-user application data directory,
/// which is already owner-only. There is no portable mode bit to set.
#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// Allocate the next human-facing reference for `scope` within `period`, e.g.
/// `("engagement", "2026")` -> `ENG-2026-0007`.
///
/// Runs inside the caller's transaction so the counter and the row that uses it commit
/// together; a rolled-back insert does not burn a number.
pub fn next_reference(tx: &Transaction<'_>, scope: &str, prefix: &str) -> Result<String> {
    let period = Timestamp::now().inner().format("%Y").to_string();
    tx.execute(
        "INSERT INTO reference_counters (scope, period, next_value) VALUES (?1, ?2, 1)
         ON CONFLICT(scope, period) DO UPDATE SET next_value = next_value + 1",
        rusqlite::params![scope, period],
    )?;
    let value: i64 = tx.query_row(
        "SELECT next_value FROM reference_counters WHERE scope = ?1 AND period = ?2",
        rusqlite::params![scope, period],
        |row| row.get(0),
    )?;
    Ok(format!("{prefix}-{period}-{value:04}"))
}

/// Escape `%`, `_` and `\` so user input cannot turn a LIKE filter into a wildcard
/// scan. Pair with `ESCAPE '\'` in the SQL.
pub fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_on_open_and_are_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let applied: i64 = db
            .with_conn(|c| {
                Ok(c.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(applied, migrations::MIGRATIONS.len() as i64);

        // Applying again must be a no-op rather than an error or a duplicate.
        db.with_tx(migrations::apply).unwrap();
        let after: i64 = db
            .with_conn(|c| {
                Ok(c.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(applied, after);
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Database::open_in_memory().unwrap();
        let result = db.with_conn(|c| {
            c.execute(
                "INSERT INTO sessions (id, user_id, token_hash, created_at, last_seen_at, expires_at)
                 VALUES ('s1', 'nobody', 'h', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        });
        assert!(
            result.is_err(),
            "insert with a dangling user_id must be rejected"
        );
    }

    #[test]
    fn references_increment_and_are_zero_padded() {
        let db = Database::open_in_memory().unwrap();
        let first = db
            .with_tx(|tx| next_reference(tx, "engagement", "ENG"))
            .unwrap();
        let second = db
            .with_tx(|tx| next_reference(tx, "engagement", "ENG"))
            .unwrap();
        assert!(first.ends_with("-0001"), "got {first}");
        assert!(second.ends_with("-0002"), "got {second}");
        assert!(first.starts_with("ENG-"));

        // Separate scopes keep separate counters.
        let finding = db
            .with_tx(|tx| next_reference(tx, "finding", "FND"))
            .unwrap();
        assert!(finding.ends_with("-0001"), "got {finding}");
    }

    #[test]
    fn rolled_back_transaction_does_not_consume_a_reference() {
        let db = Database::open_in_memory().unwrap();
        let _: Result<()> = db.with_tx(|tx| {
            next_reference(tx, "report", "RPT")?;
            Err(Error::internal("forced rollback"))
        });
        let after = db
            .with_tx(|tx| next_reference(tx, "report", "RPT"))
            .unwrap();
        assert!(after.ends_with("-0001"), "got {after}");
    }

    #[test]
    fn like_escaping_neutralizes_wildcards() {
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("c:\\path"), "c:\\\\path");
    }
}
