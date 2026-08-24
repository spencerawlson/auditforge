//! Local session handling.
//!
//! A login mints a random 256-bit token. The token is returned to the caller once and
//! kept in frontend memory; only its SHA-256 is stored, so a copy of the database (or a
//! backup, or a support bundle) cannot be replayed as a live session.
//!
//! Two independent clocks bound a session: an idle timeout that slides on each use, and
//! an absolute lifetime that does not.

use base64::Engine;
use rusqlite::{Connection, Transaction};
use sha2::{Digest, Sha256};

use super::password::random_bytes;
use crate::database::new_id;
use crate::error::{Error, Result};
use crate::models::{Session, Timestamp};

/// Timeouts, both configurable in Settings > Security.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    pub idle_timeout_secs: i64,
    pub absolute_timeout_secs: i64,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        SessionPolicy {
            idle_timeout_secs: 30 * 60,
            absolute_timeout_secs: 12 * 60 * 60,
        }
    }
}

impl SessionPolicy {
    pub const MIN_IDLE_SECS: i64 = 60;
    pub const MAX_IDLE_SECS: i64 = 8 * 60 * 60;

    pub fn validated(idle_timeout_secs: i64, absolute_timeout_secs: i64) -> Result<Self> {
        if !(Self::MIN_IDLE_SECS..=Self::MAX_IDLE_SECS).contains(&idle_timeout_secs) {
            return Err(Error::validation(format!(
                "session idle timeout must be between {} and {} seconds",
                Self::MIN_IDLE_SECS,
                Self::MAX_IDLE_SECS
            )));
        }
        if absolute_timeout_secs < idle_timeout_secs {
            return Err(Error::validation(
                "absolute session lifetime cannot be shorter than the idle timeout",
            ));
        }
        Ok(SessionPolicy {
            idle_timeout_secs,
            absolute_timeout_secs,
        })
    }
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn generate_token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(32))
}

/// Create a session and return `(raw_token, session)`. The raw token is not stored.
pub fn create(
    tx: &Transaction<'_>,
    user_id: &str,
    policy: SessionPolicy,
) -> Result<(String, Session)> {
    let token = generate_token();
    let now = Timestamp::now();
    let session = Session {
        id: new_id(),
        user_id: user_id.to_string(),
        created_at: now,
        last_seen_at: now,
        expires_at: now.plus_seconds(policy.absolute_timeout_secs),
    };
    tx.execute(
        "INSERT INTO sessions (id, user_id, token_hash, created_at, last_seen_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            session.id,
            session.user_id,
            hash_token(&token),
            session.created_at,
            session.last_seen_at,
            session.expires_at,
        ],
    )?;
    Ok((token, session))
}

/// Resolve a token to a live session, sliding the idle window.
///
/// Every failure path returns `Unauthorized`: the caller cannot tell an unknown token
/// from a revoked or expired one.
pub fn validate(conn: &Connection, token: &str, policy: SessionPolicy) -> Result<Session> {
    let token_hash = hash_token(token);
    let mut stmt = conn.prepare(
        "SELECT id, user_id, created_at, last_seen_at, expires_at, revoked_at
         FROM sessions WHERE token_hash = ?1",
    )?;
    let row = stmt
        .query_row([&token_hash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Timestamp>(2)?,
                row.get::<_, Timestamp>(3)?,
                row.get::<_, Timestamp>(4)?,
                row.get::<_, Option<Timestamp>>(5)?,
            ))
        })
        .map_err(|_| Error::Unauthorized)?;

    let (id, user_id, created_at, last_seen_at, expires_at, revoked_at) = row;
    if revoked_at.is_some() {
        return Err(Error::Unauthorized);
    }

    let now = Timestamp::now();
    if !now.is_before(expires_at) {
        expire(conn, &id)?;
        return Err(Error::Unauthorized);
    }
    if last_seen_at.seconds_until(now) > policy.idle_timeout_secs {
        expire(conn, &id)?;
        return Err(Error::Unauthorized);
    }

    conn.execute(
        "UPDATE sessions SET last_seen_at = ?1 WHERE id = ?2",
        rusqlite::params![now, id],
    )?;

    Ok(Session {
        id,
        user_id,
        created_at,
        last_seen_at: now,
        expires_at,
    })
}

/// Look at a session without sliding its idle window. Used by the UI to show the
/// remaining time without keeping the session alive on its own.
pub fn peek(conn: &Connection, token: &str, policy: SessionPolicy) -> Result<i64> {
    let token_hash = hash_token(token);
    let (last_seen_at, expires_at, revoked_at) = conn
        .query_row(
            "SELECT last_seen_at, expires_at, revoked_at FROM sessions WHERE token_hash = ?1",
            [&token_hash],
            |row| {
                Ok((
                    row.get::<_, Timestamp>(0)?,
                    row.get::<_, Timestamp>(1)?,
                    row.get::<_, Option<Timestamp>>(2)?,
                ))
            },
        )
        .map_err(|_| Error::Unauthorized)?;
    if revoked_at.is_some() {
        return Err(Error::Unauthorized);
    }
    let now = Timestamp::now();
    let idle_remaining = policy.idle_timeout_secs - last_seen_at.seconds_until(now);
    let absolute_remaining = now.seconds_until(expires_at);
    Ok(idle_remaining.min(absolute_remaining).max(0))
}

fn expire(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET revoked_at = ?1 WHERE id = ?2 AND revoked_at IS NULL",
        rusqlite::params![Timestamp::now(), session_id],
    )?;
    Ok(())
}

/// Revoke one session. Returns the user id if a live session was revoked.
pub fn revoke(conn: &Connection, token: &str) -> Result<Option<String>> {
    let token_hash = hash_token(token);
    let user_id: Option<String> = conn
        .query_row(
            "SELECT user_id FROM sessions WHERE token_hash = ?1 AND revoked_at IS NULL",
            [&token_hash],
            |row| row.get(0),
        )
        .ok();
    if user_id.is_some() {
        conn.execute(
            "UPDATE sessions SET revoked_at = ?1 WHERE token_hash = ?2",
            rusqlite::params![Timestamp::now(), token_hash],
        )?;
    }
    Ok(user_id)
}

/// Revoke every live session for a user. Called on password change.
pub fn revoke_all_for_user(conn: &Connection, user_id: &str) -> Result<usize> {
    let count = conn.execute(
        "UPDATE sessions SET revoked_at = ?1 WHERE user_id = ?2 AND revoked_at IS NULL",
        rusqlite::params![Timestamp::now(), user_id],
    )?;
    Ok(count)
}

/// Drop rows for sessions that ended more than a day ago. Housekeeping only.
pub fn purge_stale(conn: &Connection) -> Result<usize> {
    let cutoff = Timestamp::now().plus_seconds(-24 * 60 * 60);
    let count = conn.execute(
        "DELETE FROM sessions WHERE (revoked_at IS NOT NULL AND revoked_at < ?1)
            OR expires_at < ?1",
        rusqlite::params![cutoff],
    )?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn seed_user(db: &Database) -> String {
        let id = new_id();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, username, password_hash, full_name, password_changed_at,
                                    created_at, updated_at)
                 VALUES (?1, 'tester', 'x', 'Tester', ?2, ?2, ?2)",
                rusqlite::params![id, Timestamp::now()],
            )?;
            Ok(())
        })
        .unwrap();
        id
    }

    #[test]
    fn tokens_are_unique_and_only_hashes_are_stored() {
        let db = Database::open_in_memory().unwrap();
        let user_id = seed_user(&db);
        let (token_a, _) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();
        let (token_b, _) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();
        assert_ne!(token_a, token_b);

        let stored: Vec<String> = db
            .with_conn(|conn| {
                let mut stmt = conn.prepare("SELECT token_hash FROM sessions")?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(stored.len(), 2);
        assert!(!stored.contains(&token_a), "raw token must never be stored");
        assert!(stored.contains(&hash_token(&token_a)));
    }

    #[test]
    fn valid_token_resolves_and_unknown_token_is_unauthorized() {
        let db = Database::open_in_memory().unwrap();
        let user_id = seed_user(&db);
        let (token, _) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();

        let session = db
            .with_conn(|conn| validate(conn, &token, SessionPolicy::default()))
            .unwrap();
        assert_eq!(session.user_id, user_id);

        let err = db
            .with_conn(|conn| validate(conn, "not-a-real-token", SessionPolicy::default()))
            .unwrap_err();
        assert_eq!(err.kind(), "unauthorized");
    }

    #[test]
    fn idle_timeout_ends_the_session() {
        let db = Database::open_in_memory().unwrap();
        let user_id = seed_user(&db);
        let (token, session) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();

        // Backdate last activity beyond the idle window.
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE sessions SET last_seen_at = ?1 WHERE id = ?2",
                rusqlite::params![Timestamp::now().plus_seconds(-3600), session.id],
            )?;
            Ok(())
        })
        .unwrap();

        let policy = SessionPolicy {
            idle_timeout_secs: 60,
            absolute_timeout_secs: 86_400,
        };
        let err = db
            .with_conn(|conn| validate(conn, &token, policy))
            .unwrap_err();
        assert_eq!(err.kind(), "unauthorized");

        // And the session is now revoked, so a later attempt still fails.
        let err = db
            .with_conn(|conn| validate(conn, &token, policy))
            .unwrap_err();
        assert_eq!(err.kind(), "unauthorized");
    }

    #[test]
    fn absolute_lifetime_ends_the_session_even_when_active() {
        let db = Database::open_in_memory().unwrap();
        let user_id = seed_user(&db);
        let (token, session) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE sessions SET expires_at = ?1, last_seen_at = ?2 WHERE id = ?3",
                rusqlite::params![
                    Timestamp::now().plus_seconds(-1),
                    Timestamp::now(),
                    session.id
                ],
            )?;
            Ok(())
        })
        .unwrap();
        let err = db
            .with_conn(|conn| validate(conn, &token, SessionPolicy::default()))
            .unwrap_err();
        assert_eq!(err.kind(), "unauthorized");
    }

    #[test]
    fn revoked_session_cannot_be_reused() {
        let db = Database::open_in_memory().unwrap();
        let user_id = seed_user(&db);
        let (token, _) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();
        let revoked = db.with_conn(|conn| revoke(conn, &token)).unwrap();
        assert_eq!(revoked.as_deref(), Some(user_id.as_str()));
        let err = db
            .with_conn(|conn| validate(conn, &token, SessionPolicy::default()))
            .unwrap_err();
        assert_eq!(err.kind(), "unauthorized");
    }

    #[test]
    fn password_change_revokes_every_session() {
        let db = Database::open_in_memory().unwrap();
        let user_id = seed_user(&db);
        let (first, _) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();
        let (second, _) = db
            .with_tx(|tx| create(tx, &user_id, SessionPolicy::default()))
            .unwrap();
        let count = db
            .with_conn(|conn| revoke_all_for_user(conn, &user_id))
            .unwrap();
        assert_eq!(count, 2);
        for token in [first, second] {
            assert!(db
                .with_conn(|conn| validate(conn, &token, SessionPolicy::default()))
                .is_err());
        }
    }

    #[test]
    fn policy_validation_rejects_nonsense_timeouts() {
        assert!(SessionPolicy::validated(30, 3600).is_err());
        assert!(SessionPolicy::validated(1800, 600).is_err());
        assert!(SessionPolicy::validated(1800, 43_200).is_ok());
    }
}
