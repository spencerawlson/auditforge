//! Helpers shared by the unit tests in this crate.
//!
//! Compiled only under `cfg(test)`; nothing here ships in the application.

use crate::auth;
use crate::auth::session::SessionPolicy;
use crate::database::Database;
use crate::models::{Actor, NewUser};

pub const TEST_PASSWORD: &str = "Correct-Horse-9-Battery";

/// A database with the built-in templates loaded and one registered, signed-in user.
pub fn seeded() -> (Database, Actor) {
    let db = Database::open_in_memory().unwrap();
    db.with_conn(crate::templates::sync_builtins).unwrap();
    auth::register(
        &db,
        &NewUser {
            username: "analyst".to_string(),
            password: TEST_PASSWORD.to_string(),
            confirm_password: TEST_PASSWORD.to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            email: String::new(),
        },
    )
    .unwrap();
    let session = auth::login(&db, "analyst", TEST_PASSWORD, SessionPolicy::default()).unwrap();
    let actor = auth::resolve(&db, &session.token, SessionPolicy::default()).unwrap();
    (db, actor)
}
