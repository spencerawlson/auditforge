//! Forward-only schema migrations.
//!
//! Migration SQL is embedded in the binary, so a packaged application never depends on
//! files being installed next to it. Each applied migration records a SHA-256 of the SQL
//! it ran; if a shipped migration is later edited, startup fails loudly instead of
//! leaving two installs with silently different schemas.

use rusqlite::Transaction;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::models::Timestamp;

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: include_str!("../../../../migrations/0001_initial_schema.sql"),
    },
    Migration {
        version: 2,
        name: "seed_tags",
        sql: include_str!("../../../../migrations/0002_seed_tags.sql"),
    },
];

pub fn apply(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            checksum   TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
    )?;

    for migration in MIGRATIONS {
        let checksum = checksum(migration.sql);
        let existing: Option<String> = tx
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get(0),
            )
            .ok();

        match existing {
            Some(previous) if previous == checksum => continue,
            Some(_) => {
                return Err(Error::Database(format!(
                    "migration {} ({}) was modified after it was applied; \
                     restore the original file or add a new migration instead",
                    migration.version, migration.name
                )))
            }
            None => {}
        }

        tx.execute_batch(migration.sql).map_err(|e| {
            Error::Database(format!(
                "migration {} ({}) failed: {e}",
                migration.version, migration.name
            ))
        })?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                migration.version,
                migration.name,
                checksum,
                Timestamp::now().to_rfc3339()
            ],
        )?;
    }

    Ok(())
}

fn checksum(sql: &str) -> String {
    let mut hasher = Sha256::new();
    // Normalize line endings so a checkout with CRLF does not look like a tampered file.
    hasher.update(sql.replace("\r\n", "\n").as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_unique_and_ordered() {
        let mut previous = 0;
        for migration in MIGRATIONS {
            assert!(
                migration.version > previous,
                "migration versions must be strictly increasing"
            );
            previous = migration.version;
        }
    }

    #[test]
    fn migration_sql_is_embedded_not_empty() {
        for migration in MIGRATIONS {
            assert!(
                !migration.sql.trim().is_empty(),
                "{} is empty",
                migration.name
            );
        }
    }

    #[test]
    fn checksum_ignores_line_endings() {
        assert_eq!(
            checksum("SELECT 1;\nSELECT 2;\n"),
            checksum("SELECT 1;\r\nSELECT 2;\r\n")
        );
    }

    #[test]
    fn tampered_migration_is_rejected() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        let tx = conn.transaction().unwrap();
        apply(&tx).unwrap();
        // Simulate a shipped migration being edited after the fact.
        tx.execute(
            "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 1",
            [],
        )
        .unwrap();
        let err = apply(&tx).unwrap_err();
        assert!(err.to_string().contains("was modified"), "got {err}");
    }
}
