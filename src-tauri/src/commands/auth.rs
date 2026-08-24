//! Account and session commands.

use auditforge_core::auth::{self, password};
use auditforge_core::models::{NewUser, ProfileUpdate, User};
use serde::Serialize;
use tauri::State;

use crate::error::{blocking, CommandResult};
use crate::state::AppState;

/// What the shell tells the frontend at startup, before anyone has signed in.
#[derive(Serialize)]
pub struct StartupStatus {
    /// False on a fresh install, which is what shows the Create Account screen.
    pub has_account: bool,
    pub signed_in: bool,
    pub user: Option<User>,
    pub version: String,
    /// Where the database and evidence vault live, shown in Settings.
    pub data_dir: String,
    /// Name of the backend holding secrets, e.g. "OS keychain".
    pub secret_backend: String,
    /// False when secrets fall back to the encrypted local file.
    pub secrets_os_backed: bool,
}

#[tauri::command]
pub fn startup_status(state: State<'_, AppState>) -> CommandResult<StartupStatus> {
    let has_account = auth::has_any_user(&state.app.db)?;
    let user = state.actor().ok().and_then(|actor| {
        state
            .app
            .db
            .with_conn(|conn| auth::load_user(conn, &actor.user_id))
            .ok()
    });

    Ok(StartupStatus {
        has_account,
        signed_in: user.is_some(),
        user,
        version: auditforge_core::VERSION.to_string(),
        data_dir: state.app.paths.data_dir.display().to_string(),
        secret_backend: state.app.secret_store().backend_name().to_string(),
        secrets_os_backed: state.app.secret_store().is_os_backed(),
    })
}

#[derive(Serialize)]
pub struct SignedIn {
    pub user: User,
    pub expires_at: String,
}

/// Create the first (or an additional) account. Does not sign the user in: they then log
/// in normally, which keeps one code path for authentication.
#[tauri::command]
pub async fn register(state: State<'_, AppState>, input: NewUser) -> CommandResult<User> {
    let app = state.app.clone();
    blocking(move || auth::register(&app.db, &input)).await
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> CommandResult<SignedIn> {
    let app = state.app.clone();
    let policy = app.session_policy(None);
    let session = blocking(move || auth::login(&app.db, &username, &password, policy)).await?;

    // The raw token stays in Rust; the frontend never sees it.
    state.set_token(session.token.clone());
    Ok(SignedIn {
        user: session.user,
        expires_at: session.session.expires_at.to_rfc3339(),
    })
}

#[tauri::command]
pub fn logout(state: State<'_, AppState>) -> CommandResult<()> {
    if let Some(token) = state.clear_token() {
        auth::logout(&state.app.db, &token)?;
    }
    Ok(())
}

#[tauri::command]
pub fn current_user(state: State<'_, AppState>) -> CommandResult<User> {
    let actor = state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| auth::load_user(conn, &actor.user_id))?)
}

/// Seconds of inactivity left before the session ends, for the countdown in the UI.
/// `None` means no live session.
#[tauri::command]
pub fn session_remaining(state: State<'_, AppState>) -> CommandResult<Option<i64>> {
    Ok(state.seconds_remaining())
}

#[tauri::command]
pub async fn update_profile(
    state: State<'_, AppState>,
    update: ProfileUpdate,
) -> CommandResult<User> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || auth::update_profile(&app.db, &actor, &update)).await
}

#[tauri::command]
pub async fn change_password(
    state: State<'_, AppState>,
    current_password: String,
    new_password: String,
    confirm_password: String,
) -> CommandResult<()> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        auth::change_password(
            &app.db,
            &actor,
            &current_password,
            &new_password,
            &confirm_password,
        )
    })
    .await?;
    // Every session was revoked, including this one.
    state.clear_token();
    Ok(())
}

/// Advisory strength meter for the sign-up form. Runs locally; nothing is sent anywhere.
#[tauri::command]
pub fn password_strength(password: String) -> CommandResult<u8> {
    Ok(password::strength_score(&password))
}
