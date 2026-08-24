//! AI commands.
//!
//! Each of these can take tens of seconds against a local model, so they all run off the
//! main thread. None of them writes to a report: `ai_suggest` returns a proposal, and the
//! frontend applies it only after the user accepts, through `save_report_draft`.

use auditforge_core::ai::{self, AiStatus};
use auditforge_core::models::{AiDecision, AiReviewResult, AiSuggestion, AiTextRequest};
use auditforge_core::security::redact;
use serde::Serialize;
use tauri::State;

use crate::error::{blocking, CommandResult};
use crate::state::AppState;

/// Where text would go on the next call, for the banner above the AI panel.
#[tauri::command]
pub fn ai_status(state: State<'_, AppState>) -> CommandResult<AiStatus> {
    let actor = state.actor()?;
    Ok(state.app.ai_service(&actor.user_id)?.status())
}

/// What redaction would remove from a passage before sending it.
///
/// Shown before a cloud call so the user can see exactly what leaves the machine,
/// rather than being told after the fact.
#[derive(Serialize)]
pub struct RedactionPreview {
    pub detections: Vec<redact::Detection>,
    pub redacted_text: String,
}

#[tauri::command]
pub fn ai_redaction_preview(
    state: State<'_, AppState>,
    text: String,
) -> CommandResult<RedactionPreview> {
    state.actor()?;
    let mut redactor = redact::Redactor::new();
    let redacted_text = redactor.redact(&text);
    Ok(RedactionPreview {
        detections: redact::scan(&text),
        redacted_text,
    })
}

#[tauri::command]
pub async fn ai_suggest(
    state: State<'_, AppState>,
    request: AiTextRequest,
) -> CommandResult<AiSuggestion> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let service = app.ai_service(&actor.user_id)?;
        service.suggest(&app.db, &actor, &request)
    })
    .await
}

#[tauri::command]
pub async fn ai_review_report(
    state: State<'_, AppState>,
    report_id: String,
    version_id: Option<String>,
) -> CommandResult<AiReviewResult> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let version = app.report_version(&report_id, version_id.as_deref())?;
        let service = app.ai_service(&actor.user_id)?;
        service.review_report(&app.db, &actor, &report_id, &version.content)
    })
    .await
}

/// Record what the human decided. Called on Accept, Edit and Reject alike, so the audit
/// trail shows rejected suggestions too.
#[tauri::command]
pub async fn ai_record_decision(
    state: State<'_, AppState>,
    request_id: String,
    decision: AiDecision,
) -> CommandResult<()> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || ai::record_decision(&app.db, &actor, &request_id, decision)).await
}

/// The AI activity log for one record, shown in the report's history panel.
#[tauri::command]
pub fn ai_requests_for(
    state: State<'_, AppState>,
    target_type: String,
    target_id: String,
    limit: Option<i64>,
) -> CommandResult<Vec<serde_json::Value>> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| {
        ai::requests_for_target(conn, &target_type, &target_id, limit.unwrap_or(50))
    })?)
}

/// The text that would be sent for a report review, so the user can read it first.
#[tauri::command]
pub fn ai_review_preview(
    state: State<'_, AppState>,
    report_id: String,
    version_id: Option<String>,
) -> CommandResult<String> {
    state.actor()?;
    let version = state
        .app
        .report_version(&report_id, version_id.as_deref())?;
    Ok(ai::assemble_report_text(&version.content))
}
