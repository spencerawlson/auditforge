//! Users, organizations and sessions.

use serde::{Deserialize, Serialize};

use super::common::{AuthorSnapshot, Timestamp};

/// A stored user, minus anything secret.
///
/// `password_hash` is deliberately absent: this struct crosses the boundary to the
/// frontend, and the hash has no business leaving the auth module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub full_name: String,
    pub job_title: String,
    pub email: String,
    pub organization_id: Option<String>,
    pub organization_name: String,
    pub is_active: bool,
    pub last_login_at: Option<Timestamp>,
    pub password_changed_at: Timestamp,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl User {
    /// Freeze the author identity for embedding in a report version.
    pub fn author_snapshot(&self) -> AuthorSnapshot {
        AuthorSnapshot {
            user_id: self.id.clone(),
            username: self.username.clone(),
            full_name: self.full_name.clone(),
            organization: self.organization_name.clone(),
            job_title: self.job_title.clone(),
            captured_at: Timestamp::now(),
        }
    }
}

/// Account creation input, straight from the sign-up form.
#[derive(Debug, Clone, Deserialize)]
pub struct NewUser {
    pub username: String,
    pub password: String,
    pub confirm_password: String,
    pub full_name: String,
    pub organization: String,
    pub job_title: String,
    #[serde(default)]
    pub email: String,
}

/// Profile edit input. `None` leaves a field untouched.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfileUpdate {
    pub full_name: Option<String>,
    pub organization: Option<String>,
    pub job_title: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// An authenticated session. The raw token is returned exactly once, at login; only its
/// hash is stored.
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub created_at: Timestamp,
    pub last_seen_at: Timestamp,
    pub expires_at: Timestamp,
}

/// The identity performing an operation.
///
/// Produced by resolving a session token and passed down to every write, so audit rows
/// and author attribution never have to guess who acted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Actor {
    pub user_id: String,
    pub username: String,
    pub session_id: String,
}

/// Login result handed to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct AuthenticatedSession {
    /// Opaque bearer token. Held in frontend memory only, never written to disk.
    pub token: String,
    pub session: Session,
    pub user: User,
}
