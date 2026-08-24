//! Shell state.
//!
//! The session token lives here, in Rust, and is never handed to the frontend. A command
//! resolves the acting user from it on every call, which means a compromised web view
//! cannot lift a bearer token out of JavaScript memory, and "am I still signed in?" has
//! exactly one answer.

use std::sync::{Arc, Mutex};

use auditforge_core::auth::session::SessionPolicy;
use auditforge_core::models::Actor;
use auditforge_core::{auth, App, Error, Result};

pub struct AppState {
    /// Shared so blocking work can be moved onto a worker thread.
    pub app: Arc<App>,
    token: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(app: App) -> Self {
        AppState {
            app: Arc::new(app),
            token: Mutex::new(None),
        }
    }

    pub fn set_token(&self, token: String) {
        *self.lock() = Some(token);
    }

    pub fn clear_token(&self) -> Option<String> {
        self.lock().take()
    }

    pub fn token(&self) -> Option<String> {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Resolve the signed-in user, sliding the idle timeout.
    ///
    /// An expired session is cleared here, so the next call reports "signed out" rather
    /// than repeatedly failing against a token that will never work again.
    pub fn actor(&self) -> Result<Actor> {
        let token = self.token().ok_or(Error::Unauthorized)?;
        let policy = self.policy_for_token(&token);
        match auth::resolve(&self.app.db, &token, policy) {
            Ok(actor) => Ok(actor),
            Err(error) => {
                self.clear_token();
                Err(error)
            }
        }
    }

    /// The signed-in user's configured timeouts, or the defaults if that cannot be read.
    fn policy_for_token(&self, token: &str) -> SessionPolicy {
        // Peek at the owning user without sliding the window, so loading settings is not
        // itself an activity that keeps a session alive.
        let user_id = self.app.db.with_conn(|conn| {
            let hash = auditforge_core::auth::session::hash_token(token);
            let user_id: String = conn.query_row(
                "SELECT user_id FROM sessions WHERE token_hash = ?1",
                [&hash],
                |row| row.get(0),
            )?;
            Ok(user_id)
        });
        match user_id {
            Ok(user_id) => self.app.session_policy(Some(&user_id)),
            Err(_) => SessionPolicy::default(),
        }
    }

    /// Seconds of inactivity remaining, for the session countdown in the UI.
    pub fn seconds_remaining(&self) -> Option<i64> {
        let token = self.token()?;
        let policy = self.policy_for_token(&token);
        self.app
            .db
            .with_conn(|conn| auditforge_core::auth::session::peek(conn, &token, policy))
            .ok()
    }
}
