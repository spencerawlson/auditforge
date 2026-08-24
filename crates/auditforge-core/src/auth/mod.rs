//! Accounts, login, lockout and profile management.
//!
//! The schema and this module are already multi-user: every record carries an owning
//! user id and every write takes an `Actor`. The MVP UI happens to sign in one person at
//! a time, but nothing here assumes that.

pub mod password;
pub mod session;

use rusqlite::{Connection, OptionalExtension, Transaction};
use serde_json::json;

use crate::audit;
use crate::database::{new_id, Database};
use crate::error::{Error, Result};
use crate::models::{
    Actor, AuditAction, AuditOutcome, AuthenticatedSession, NewUser, Organization, ProfileUpdate,
    Timestamp, User,
};
use session::SessionPolicy;

/// Consecutive failures before an account is locked.
pub const MAX_FAILED_ATTEMPTS: i64 = 5;
/// How long a lockout lasts. Long enough to stop online guessing, short enough that a
/// locked-out user is not stuck for the afternoon.
pub const LOCKOUT_SECONDS: i64 = 15 * 60;

const USERNAME_MIN: usize = 3;
const USERNAME_MAX: usize = 64;

/// Whether any account exists. Drives the first-run "Create Account" screen.
pub fn has_any_user(db: &Database) -> Result<bool> {
    db.with_conn(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        Ok(count > 0)
    })
}

pub fn register(db: &Database, input: &NewUser) -> Result<User> {
    let username = normalize_username(&input.username)?;
    let full_name = require_text(&input.full_name, "full name", 200)?;
    let organization = input.organization.trim().to_string();
    let job_title = input.job_title.trim().to_string();
    let email = input.email.trim().to_string();

    if organization.len() > 200 {
        return Err(Error::validation("organization name is too long"));
    }
    if job_title.len() > 200 {
        return Err(Error::validation("job title is too long"));
    }
    if input.password != input.confirm_password {
        return Err(Error::validation("passwords do not match"));
    }
    // Hash before opening the transaction: Argon2id deliberately takes time and there is
    // no reason to hold the write lock for it.
    let password_hash = password::hash_password(&input.password)?;

    let user_id = db.with_tx(|tx| {
        let taken: i64 = tx.query_row(
            "SELECT COUNT(*) FROM users WHERE username = ?1 COLLATE NOCASE",
            [&username],
            |row| row.get(0),
        )?;
        if taken > 0 {
            return Err(Error::validation("that username is already taken"));
        }

        let organization_id = if organization.is_empty() {
            None
        } else {
            Some(find_or_create_organization(tx, &organization)?)
        };

        let id = new_id();
        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO users (id, username, password_hash, full_name, job_title, email,
                                organization_id, is_active, failed_login_attempts,
                                password_changed_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 0, ?8, ?8, ?8)",
            rusqlite::params![
                id,
                username,
                password_hash,
                full_name,
                job_title,
                email,
                organization_id,
                now
            ],
        )?;

        let actor = Actor {
            user_id: id.clone(),
            username: username.clone(),
            session_id: String::new(),
        };
        audit::record(
            tx,
            Some(&actor),
            AuditAction::AccountCreated,
            "user",
            &id,
            AuditOutcome::Success,
            json!({ "organization": organization }),
        )?;
        Ok(id)
    })?;

    db.with_conn(|conn| load_user(conn, &user_id))
}

pub fn login(
    db: &Database,
    username: &str,
    supplied_password: &str,
    policy: SessionPolicy,
) -> Result<AuthenticatedSession> {
    let username = username.trim().to_string();

    let candidate = db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, username, password_hash, is_active, failed_login_attempts, locked_until
             FROM users WHERE username = ?1 COLLATE NOCASE",
            [&username],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<Timestamp>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(Error::from)
    })?;

    let Some((user_id, stored_username, stored_hash, is_active, failures, locked_until)) =
        candidate
    else {
        // Spend comparable time on an unknown username so response timing does not
        // reveal which accounts exist.
        let _ = password::verify_password(supplied_password, dummy_hash());
        db.with_conn(|conn| {
            audit::record_on(
                conn,
                None,
                AuditAction::LoginFailed,
                "user",
                &username,
                AuditOutcome::Failure,
                json!({ "reason": "unknown_user" }),
            )
        })?;
        return Err(Error::Unauthorized);
    };

    if let Some(until) = locked_until {
        let remaining = Timestamp::now().seconds_until(until);
        if remaining > 0 {
            db.with_conn(|conn| {
                audit::record_on(
                    conn,
                    None,
                    AuditAction::LoginFailed,
                    "user",
                    &user_id,
                    AuditOutcome::Denied,
                    json!({ "reason": "locked", "retry_after_secs": remaining }),
                )
            })?;
            return Err(Error::Locked {
                retry_after_secs: remaining,
            });
        }
    }

    if !is_active {
        return Err(Error::Unauthorized);
    }

    if !password::verify_password(supplied_password, &stored_hash) {
        let attempts = failures + 1;
        let lock_until = (attempts >= MAX_FAILED_ATTEMPTS)
            .then(|| Timestamp::now().plus_seconds(LOCKOUT_SECONDS));
        db.with_tx(|tx| {
            tx.execute(
                "UPDATE users SET failed_login_attempts = ?1, locked_until = ?2, updated_at = ?3
                 WHERE id = ?4",
                rusqlite::params![attempts, lock_until, Timestamp::now(), user_id],
            )?;
            audit::record(
                tx,
                None,
                AuditAction::LoginFailed,
                "user",
                &user_id,
                AuditOutcome::Failure,
                json!({ "reason": "bad_password", "attempts": attempts, "locked": lock_until.is_some() }),
            )
        })?;
        if lock_until.is_some() {
            return Err(Error::Locked {
                retry_after_secs: LOCKOUT_SECONDS,
            });
        }
        return Err(Error::Unauthorized);
    }

    let (token, sess) = db.with_tx(|tx| {
        tx.execute(
            "UPDATE users SET failed_login_attempts = 0, locked_until = NULL, last_login_at = ?1,
                              updated_at = ?1
             WHERE id = ?2",
            rusqlite::params![Timestamp::now(), user_id],
        )?;
        let (token, sess) = session::create(tx, &user_id, policy)?;
        let actor = Actor {
            user_id: user_id.clone(),
            username: stored_username.clone(),
            session_id: sess.id.clone(),
        };
        audit::success(tx, &actor, AuditAction::Login, "user", &user_id, json!({}))?;
        Ok((token, sess))
    })?;

    // Housekeeping; a failure here must not block a valid login.
    let _ = db.with_conn(session::purge_stale);

    let user = db.with_conn(|conn| load_user(conn, &user_id))?;
    Ok(AuthenticatedSession {
        token,
        session: sess,
        user,
    })
}

pub fn logout(db: &Database, token: &str) -> Result<()> {
    db.with_tx(|tx| {
        if let Some(user_id) = session::revoke(tx, token)? {
            let username: String = tx.query_row(
                "SELECT username FROM users WHERE id = ?1",
                [&user_id],
                |row| row.get(0),
            )?;
            let actor = Actor {
                user_id: user_id.clone(),
                username,
                session_id: String::new(),
            };
            audit::success(tx, &actor, AuditAction::Logout, "user", &user_id, json!({}))?;
        }
        Ok(())
    })
}

/// Resolve a session token to the acting identity, sliding the idle window.
pub fn resolve(db: &Database, token: &str, policy: SessionPolicy) -> Result<Actor> {
    db.with_conn(|conn| {
        let sess = session::validate(conn, token, policy)?;
        let username: String = conn.query_row(
            "SELECT username FROM users WHERE id = ?1",
            [&sess.user_id],
            |row| row.get(0),
        )?;
        Ok(Actor {
            user_id: sess.user_id,
            username,
            session_id: sess.id,
        })
    })
}

pub fn change_password(
    db: &Database,
    actor: &Actor,
    current_password: &str,
    new_password: &str,
    confirm_password: &str,
) -> Result<()> {
    if new_password != confirm_password {
        return Err(Error::validation("passwords do not match"));
    }
    if current_password == new_password {
        return Err(Error::validation(
            "the new password must differ from the current one",
        ));
    }

    let stored_hash: String = db.with_conn(|conn| {
        Ok(conn.query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            [&actor.user_id],
            |row| row.get(0),
        )?)
    })?;

    if !password::verify_password(current_password, &stored_hash) {
        db.with_conn(|conn| {
            audit::record_on(
                conn,
                Some(actor),
                AuditAction::PasswordChanged,
                "user",
                &actor.user_id,
                AuditOutcome::Denied,
                json!({ "reason": "current_password_incorrect" }),
            )
        })?;
        return Err(Error::Unauthorized);
    }

    let new_hash = password::hash_password(new_password)?;
    db.with_tx(|tx| {
        tx.execute(
            "UPDATE users SET password_hash = ?1, password_changed_at = ?2, updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![new_hash, Timestamp::now(), actor.user_id],
        )?;
        // Anything holding an old token loses it: a password change is also how a user
        // responds to a suspected compromise.
        session::revoke_all_for_user(tx, &actor.user_id)?;
        audit::success(
            tx,
            actor,
            AuditAction::PasswordChanged,
            "user",
            &actor.user_id,
            json!({ "sessions_revoked": true }),
        )
    })
}

/// Update the profile that becomes the default document author.
///
/// Reports finalized before this call keep the author metadata they were finalized with:
/// versions embed an `AuthorSnapshot` rather than joining back to this row.
pub fn update_profile(db: &Database, actor: &Actor, update: &ProfileUpdate) -> Result<User> {
    db.with_tx(|tx| {
        if let Some(full_name) = &update.full_name {
            let value = require_text(full_name, "full name", 200)?;
            tx.execute(
                "UPDATE users SET full_name = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![value, Timestamp::now(), actor.user_id],
            )?;
        }
        if let Some(job_title) = &update.job_title {
            tx.execute(
                "UPDATE users SET job_title = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![job_title.trim(), Timestamp::now(), actor.user_id],
            )?;
        }
        if let Some(email) = &update.email {
            tx.execute(
                "UPDATE users SET email = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![email.trim(), Timestamp::now(), actor.user_id],
            )?;
        }
        if let Some(organization) = &update.organization {
            let trimmed = organization.trim();
            let organization_id = if trimmed.is_empty() {
                None
            } else {
                Some(find_or_create_organization(tx, trimmed)?)
            };
            tx.execute(
                "UPDATE users SET organization_id = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![organization_id, Timestamp::now(), actor.user_id],
            )?;
        }
        audit::success(
            tx,
            actor,
            AuditAction::ProfileUpdated,
            "user",
            &actor.user_id,
            json!({
                "full_name": update.full_name.is_some(),
                "organization": update.organization.is_some(),
                "job_title": update.job_title.is_some(),
            }),
        )?;
        load_user(tx, &actor.user_id)
    })
}

pub fn load_user(conn: &Connection, user_id: &str) -> Result<User> {
    conn.query_row(
        "SELECT u.id, u.username, u.full_name, u.job_title, u.email, u.organization_id,
                COALESCE(o.name, ''), u.is_active, u.last_login_at, u.password_changed_at,
                u.created_at, u.updated_at
         FROM users u LEFT JOIN organizations o ON o.id = u.organization_id
         WHERE u.id = ?1",
        [user_id],
        |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                full_name: row.get(2)?,
                job_title: row.get(3)?,
                email: row.get(4)?,
                organization_id: row.get(5)?,
                organization_name: row.get(6)?,
                is_active: row.get::<_, i64>(7)? != 0,
                last_login_at: row.get(8)?,
                password_changed_at: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        },
    )
    .map_err(|_| Error::not_found("user"))
}

pub fn list_organizations(conn: &Connection) -> Result<Vec<Organization>> {
    let mut stmt =
        conn.prepare("SELECT id, name, created_at, updated_at FROM organizations ORDER BY name")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Organization {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn find_or_create_organization(tx: &Transaction<'_>, name: &str) -> Result<String> {
    if let Some(id) = tx
        .query_row(
            "SELECT id FROM organizations WHERE name = ?1",
            [name],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let id = new_id();
    let now = Timestamp::now();
    tx.execute(
        "INSERT INTO organizations (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        rusqlite::params![id, name, now],
    )?;
    Ok(id)
}

fn normalize_username(raw: &str) -> Result<String> {
    let username = raw.trim().to_string();
    if username.chars().count() < USERNAME_MIN {
        return Err(Error::validation(format!(
            "username must be at least {USERNAME_MIN} characters"
        )));
    }
    if username.chars().count() > USERNAME_MAX {
        return Err(Error::validation("username is too long"));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(Error::validation(
            "username may contain only letters, digits, dot, underscore and hyphen",
        ));
    }
    Ok(username)
}

fn require_text(raw: &str, field: &str, max: usize) -> Result<String> {
    let value = raw.trim().to_string();
    if value.is_empty() {
        return Err(Error::validation(format!("{field} is required")));
    }
    if value.chars().count() > max {
        return Err(Error::validation(format!("{field} is too long")));
    }
    Ok(value)
}

/// A real Argon2id hash, computed once, used to keep unknown-username logins on the same
/// code path - and therefore roughly the same duration - as real ones. Generated rather
/// than hard-coded so it can never drift out of the parameter set `hash_password` uses.
fn dummy_hash() -> &'static str {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| {
        password::hash_password("AuditForge unknown-user timing equalizer 2026!")
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_user() -> NewUser {
        NewUser {
            username: "s.rawlson".to_string(),
            password: "Correct-Horse-9-Battery".to_string(),
            confirm_password: "Correct-Horse-9-Battery".to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            email: "analyst@example.test".to_string(),
        }
    }

    fn fresh() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn registration_stores_a_hash_and_creates_the_organization() {
        let db = fresh();
        assert!(!has_any_user(&db).unwrap());
        let user = register(&db, &sample_user()).unwrap();
        assert_eq!(user.username, "s.rawlson");
        assert_eq!(user.organization_name, "PLH Ecosystem, Inc.");
        assert!(has_any_user(&db).unwrap());

        let stored: String = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT password_hash FROM users WHERE id = ?1",
                    [&user.id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert!(stored.starts_with("$argon2id$"));
        assert!(!stored.contains("Correct-Horse"));
    }

    #[test]
    fn registration_rejects_mismatched_passwords_and_duplicate_usernames() {
        let db = fresh();
        let mut input = sample_user();
        input.confirm_password = "something-else-9A!".to_string();
        assert!(register(&db, &input).is_err());

        register(&db, &sample_user()).unwrap();
        let mut duplicate = sample_user();
        duplicate.username = "S.RAWLSON".to_string();
        let err = register(&db, &duplicate).unwrap_err();
        assert!(err.to_string().contains("already taken"), "got {err}");
    }

    #[test]
    fn registration_rejects_hostile_usernames() {
        let db = fresh();
        for bad in ["ab", "user name", "user;drop", "../etc", "<script>"] {
            let mut input = sample_user();
            input.username = bad.to_string();
            assert!(register(&db, &input).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn login_succeeds_and_issues_a_usable_session() {
        let db = fresh();
        register(&db, &sample_user()).unwrap();
        let auth = login(
            &db,
            "s.rawlson",
            "Correct-Horse-9-Battery",
            SessionPolicy::default(),
        )
        .unwrap();
        assert!(!auth.token.is_empty());
        assert_eq!(auth.user.full_name, "Spencer Rawlson Spady");

        let actor = resolve(&db, &auth.token, SessionPolicy::default()).unwrap();
        assert_eq!(actor.user_id, auth.user.id);
    }

    #[test]
    fn login_is_case_insensitive_on_username_only() {
        let db = fresh();
        register(&db, &sample_user()).unwrap();
        assert!(login(
            &db,
            "S.Rawlson",
            "Correct-Horse-9-Battery",
            SessionPolicy::default()
        )
        .is_ok());
        assert!(login(
            &db,
            "s.rawlson",
            "correct-horse-9-battery",
            SessionPolicy::default()
        )
        .is_err());
    }

    #[test]
    fn unknown_user_and_wrong_password_report_the_same_error() {
        let db = fresh();
        register(&db, &sample_user()).unwrap();
        let unknown =
            login(&db, "nobody", "whatever-12345A!", SessionPolicy::default()).unwrap_err();
        let wrong = login(
            &db,
            "s.rawlson",
            "whatever-12345A!",
            SessionPolicy::default(),
        )
        .unwrap_err();
        assert_eq!(unknown.kind(), wrong.kind());
        assert_eq!(unknown.to_string(), wrong.to_string());
    }

    #[test]
    fn repeated_failures_lock_the_account_then_unlock_after_the_window() {
        let db = fresh();
        let user = register(&db, &sample_user()).unwrap();
        for _ in 0..(MAX_FAILED_ATTEMPTS - 1) {
            assert_eq!(
                login(
                    &db,
                    "s.rawlson",
                    "wrong-password-1A!",
                    SessionPolicy::default()
                )
                .unwrap_err()
                .kind(),
                "unauthorized"
            );
        }
        let err = login(
            &db,
            "s.rawlson",
            "wrong-password-1A!",
            SessionPolicy::default(),
        )
        .unwrap_err();
        assert_eq!(err.kind(), "locked");

        // Even the correct password is refused while locked.
        let err = login(
            &db,
            "s.rawlson",
            "Correct-Horse-9-Battery",
            SessionPolicy::default(),
        )
        .unwrap_err();
        assert_eq!(err.kind(), "locked");

        // Once the window passes, the correct password works and the counter resets.
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE users SET locked_until = ?1 WHERE id = ?2",
                rusqlite::params![Timestamp::now().plus_seconds(-1), user.id],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(login(
            &db,
            "s.rawlson",
            "Correct-Horse-9-Battery",
            SessionPolicy::default()
        )
        .is_ok());
        let failures: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT failed_login_attempts FROM users WHERE id = ?1",
                    [&user.id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(failures, 0);
    }

    #[test]
    fn logout_invalidates_the_token() {
        let db = fresh();
        register(&db, &sample_user()).unwrap();
        let auth = login(
            &db,
            "s.rawlson",
            "Correct-Horse-9-Battery",
            SessionPolicy::default(),
        )
        .unwrap();
        logout(&db, &auth.token).unwrap();
        assert!(resolve(&db, &auth.token, SessionPolicy::default()).is_err());
    }

    #[test]
    fn changing_password_requires_the_current_one_and_kills_sessions() {
        let db = fresh();
        register(&db, &sample_user()).unwrap();
        let auth = login(
            &db,
            "s.rawlson",
            "Correct-Horse-9-Battery",
            SessionPolicy::default(),
        )
        .unwrap();
        let actor = resolve(&db, &auth.token, SessionPolicy::default()).unwrap();

        let err = change_password(
            &db,
            &actor,
            "wrong",
            "New-Passphrase-42!",
            "New-Passphrase-42!",
        )
        .unwrap_err();
        assert_eq!(err.kind(), "unauthorized");

        change_password(
            &db,
            &actor,
            "Correct-Horse-9-Battery",
            "New-Passphrase-42!",
            "New-Passphrase-42!",
        )
        .unwrap();

        assert!(resolve(&db, &auth.token, SessionPolicy::default()).is_err());
        assert!(login(
            &db,
            "s.rawlson",
            "New-Passphrase-42!",
            SessionPolicy::default()
        )
        .is_ok());
    }

    #[test]
    fn profile_updates_apply_and_are_audited() {
        let db = fresh();
        register(&db, &sample_user()).unwrap();
        let auth = login(
            &db,
            "s.rawlson",
            "Correct-Horse-9-Battery",
            SessionPolicy::default(),
        )
        .unwrap();
        let actor = resolve(&db, &auth.token, SessionPolicy::default()).unwrap();

        let updated = update_profile(
            &db,
            &actor,
            &ProfileUpdate {
                job_title: Some("Principal Cloud Security Architect".to_string()),
                organization: Some("Aegis CloudOps".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.job_title, "Principal Cloud Security Architect");
        assert_eq!(updated.organization_name, "Aegis CloudOps");
        assert_eq!(updated.full_name, "Spencer Rawlson Spady");
    }

    #[test]
    fn dummy_hash_is_well_formed_so_the_timing_path_actually_runs_argon2() {
        // If this stops parsing, verify_password would return early and the
        // unknown-username path would get measurably faster than a real one.
        assert!(argon2::password_hash::PasswordHash::new(dummy_hash()).is_ok());
        assert!(dummy_hash().starts_with("$argon2id$"));
    }
}
