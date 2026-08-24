//! Settings commands.
//!
//! The OpenAI API key is written through here into the OS keychain and is never read back
//! out to the frontend; `AppSettings.ai.has_api_key` is the only thing the UI learns
//! about it.

use auditforge_core::models::AiSettings;
use auditforge_core::settings::{self, AppSettings, ReportSettings, SecuritySettings, UiSettings};
use tauri::State;

use crate::error::{blocking, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CommandResult<AppSettings> {
    let actor = state.actor()?;
    Ok(state.app.settings(&actor.user_id)?)
}

#[tauri::command]
pub async fn save_security_settings(
    state: State<'_, AppState>,
    value: SecuritySettings,
) -> CommandResult<AppSettings> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        settings::validate_security(&value)?;
        app.db.with_tx(|tx| {
            settings::save_section(tx, &actor.user_id, settings::SECTION_SECURITY, &value)?;
            auditforge_core::audit::success(
                tx,
                &actor,
                auditforge_core::models::AuditAction::SettingsChanged,
                "settings",
                settings::SECTION_SECURITY,
                serde_json::json!({ "idle_timeout_secs": value.idle_timeout_secs }),
            )
        })?;
        app.settings(&actor.user_id)
    })
    .await
}

#[tauri::command]
pub async fn save_ai_settings(
    state: State<'_, AppState>,
    value: AiSettings,
) -> CommandResult<AppSettings> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        settings::validate_ai(&value)?;
        // `has_api_key` is derived from the keychain, never written by the client.
        let mut stored = value.clone();
        stored.has_api_key = false;
        app.db.with_tx(|tx| {
            settings::save_section(tx, &actor.user_id, settings::SECTION_AI, &stored)?;
            auditforge_core::audit::success(
                tx,
                &actor,
                auditforge_core::models::AuditAction::SettingsChanged,
                "settings",
                settings::SECTION_AI,
                serde_json::json!({
                    "provider": stored.provider.as_str(),
                    "privacy_mode": stored.privacy_mode().as_str(),
                    "cloud_consent": stored.cloud_consent,
                    "redact_before_send": stored.redact_before_send,
                }),
            )
        })?;
        app.settings(&actor.user_id)
    })
    .await
}

#[tauri::command]
pub async fn save_report_settings(
    state: State<'_, AppState>,
    value: ReportSettings,
) -> CommandResult<AppSettings> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        app.db.with_tx(|tx| {
            settings::save_section(tx, &actor.user_id, settings::SECTION_REPORTS, &value)?;
            auditforge_core::audit::success(
                tx,
                &actor,
                auditforge_core::models::AuditAction::SettingsChanged,
                "settings",
                settings::SECTION_REPORTS,
                serde_json::json!({}),
            )
        })?;
        app.settings(&actor.user_id)
    })
    .await
}

#[tauri::command]
pub async fn save_ui_settings(
    state: State<'_, AppState>,
    value: UiSettings,
) -> CommandResult<AppSettings> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        app.db.with_conn(|conn| {
            settings::save_section(conn, &actor.user_id, settings::SECTION_UI, &value)
        })?;
        app.settings(&actor.user_id)
    })
    .await
}

#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> CommandResult<AppSettings> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        app.store_api_key(&actor, &key)?;
        app.settings(&actor.user_id)
    })
    .await
}

#[tauri::command]
pub async fn clear_api_key(state: State<'_, AppState>) -> CommandResult<AppSettings> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        app.clear_api_key(&actor)?;
        app.settings(&actor.user_id)
    })
    .await
}

/// Write a consistent copy of the database to a path the user chose.
#[tauri::command]
pub async fn backup_database(
    state: State<'_, AppState>,
    destination: String,
) -> CommandResult<String> {
    state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        app.db.backup_to(std::path::Path::new(&destination))?;
        Ok(destination)
    })
    .await
}
