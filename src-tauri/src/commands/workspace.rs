//! Engagements, findings, evidence, incidents, postmortems, search, tags and the audit
//! log. The dashboard lives here too, since it is a read across all of them.

use auditforge_core::app::Dashboard;
use auditforge_core::export::ExportFormat;
use auditforge_core::models::*;
use auditforge_core::postmortems::{PostmortemCompletion, PostmortemUpdate, UnansweredQuestion};
use auditforge_core::reports::standards::Completeness;
use auditforge_core::search::{SearchHit, SearchQuery};
use auditforge_core::{audit, engagements, evidence, findings, incidents, postmortems, search};
use tauri::State;

use crate::error::{blocking, CommandResult};
use crate::state::AppState;

// ------------------------------------------------------------------ dashboard

#[tauri::command]
pub fn dashboard(state: State<'_, AppState>) -> CommandResult<Dashboard> {
    let actor = state.actor()?;
    Ok(state.app.dashboard(&actor)?)
}

// ------------------------------------------------------------------ engagements

#[tauri::command]
pub fn list_engagements(
    state: State<'_, AppState>,
    filter: engagements::EngagementFilter,
) -> CommandResult<Vec<EngagementSummary>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| engagements::list(conn, &filter))?)
}

#[tauri::command]
pub fn get_engagement(state: State<'_, AppState>, id: String) -> CommandResult<Engagement> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| engagements::get(conn, &id))?)
}

#[tauri::command]
pub async fn create_engagement(
    state: State<'_, AppState>,
    input: NewEngagement,
) -> CommandResult<Engagement> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || engagements::create(&app.db, &actor, &input)).await
}

#[tauri::command]
pub async fn update_engagement(
    state: State<'_, AppState>,
    id: String,
    update: EngagementUpdate,
) -> CommandResult<Engagement> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || engagements::update(&app.db, &actor, &id, &update)).await
}

#[tauri::command]
pub async fn delete_engagement(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || engagements::delete(&app.db, &actor, &id)).await
}

// ------------------------------------------------------------------ findings

#[tauri::command]
pub fn list_findings(
    state: State<'_, AppState>,
    filter: findings::FindingFilter,
) -> CommandResult<Vec<Finding>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| findings::list(conn, &filter))?)
}

#[tauri::command]
pub fn get_finding(state: State<'_, AppState>, id: String) -> CommandResult<Finding> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| findings::get(conn, &id))?)
}

#[tauri::command]
pub async fn create_finding(
    state: State<'_, AppState>,
    input: NewFinding,
) -> CommandResult<Finding> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || findings::create(&app.db, &actor, &input)).await
}

#[tauri::command]
pub async fn update_finding(
    state: State<'_, AppState>,
    id: String,
    update: FindingUpdate,
) -> CommandResult<Finding> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || findings::update(&app.db, &actor, &id, &update)).await
}

/// Severity is a separate command on purpose: it is the one field a person must confirm,
/// and keeping it off the ordinary edit path makes that impossible to bypass by accident.
#[tauri::command]
pub async fn set_finding_severity(
    state: State<'_, AppState>,
    id: String,
    decision: SeverityDecision,
) -> CommandResult<Finding> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || findings::set_severity(&app.db, &actor, &id, &decision)).await
}

#[tauri::command]
pub async fn delete_finding(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || findings::delete(&app.db, &actor, &id)).await
}

// ------------------------------------------------------------------ evidence

#[tauri::command]
pub fn list_evidence(
    state: State<'_, AppState>,
    filter: evidence::EvidenceFilter,
) -> CommandResult<Vec<Evidence>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| evidence::list(conn, &filter))?)
}

/// Import a file the user picked with the system dialog. The path comes from the picker,
/// and is validated again here before anything is read.
#[tauri::command]
pub async fn import_evidence(
    state: State<'_, AppState>,
    input: NewEvidence,
) -> CommandResult<Evidence> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let key = app.vault_key();
        evidence::import(
            &app.db,
            &actor,
            &app.paths.evidence_dir,
            key.as_ref(),
            &input,
        )
    })
    .await
}

#[tauri::command]
pub async fn update_evidence(
    state: State<'_, AppState>,
    id: String,
    update: EvidenceUpdate,
) -> CommandResult<Evidence> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || evidence::update(&app.db, &actor, &id, &update)).await
}

#[tauri::command]
pub async fn link_evidence(
    state: State<'_, AppState>,
    id: String,
    request: EvidenceLinkRequest,
) -> CommandResult<Evidence> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || evidence::link(&app.db, &actor, &id, &request)).await
}

#[tauri::command]
pub async fn unlink_evidence(
    state: State<'_, AppState>,
    id: String,
    request: EvidenceLinkRequest,
) -> CommandResult<Evidence> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || evidence::unlink(&app.db, &actor, &id, &request)).await
}

#[tauri::command]
pub async fn delete_evidence(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || evidence::delete(&app.db, &actor, &app.paths.evidence_dir, &id)).await
}

/// Absolute path of a stored file, so the UI can offer "reveal in file manager".
///
/// AuditForge never opens evidence itself: the path is handed back and the user decides.
#[tauri::command]
pub fn evidence_path(state: State<'_, AppState>, id: String) -> CommandResult<String> {
    state.actor()?;
    let path = state
        .app
        .db
        .with_conn(|conn| evidence::absolute_path(conn, &state.app.paths.evidence_dir, &id))?;
    Ok(path.display().to_string())
}

// ------------------------------------------------------------------ incidents

#[tauri::command]
pub fn list_incidents(
    state: State<'_, AppState>,
    filter: incidents::IncidentFilter,
) -> CommandResult<Vec<Incident>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| incidents::list(conn, &filter))?)
}

#[tauri::command]
pub fn get_incident(state: State<'_, AppState>, id: String) -> CommandResult<Incident> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| incidents::get(conn, &id))?)
}

#[tauri::command]
pub async fn create_incident(
    state: State<'_, AppState>,
    input: NewIncident,
) -> CommandResult<Incident> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || incidents::create(&app.db, &actor, &input)).await
}

#[tauri::command]
pub async fn update_incident(
    state: State<'_, AppState>,
    id: String,
    update: IncidentUpdate,
) -> CommandResult<Incident> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || incidents::update(&app.db, &actor, &id, &update)).await
}

#[tauri::command]
pub async fn set_incident_severity(
    state: State<'_, AppState>,
    id: String,
    decision: SeverityDecision,
) -> CommandResult<Incident> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || incidents::set_severity(&app.db, &actor, &id, &decision)).await
}

// ------------------------------------------------------------------ postmortems

#[tauri::command]
pub fn postmortem_questions() -> CommandResult<Vec<UnansweredQuestion>> {
    Ok(postmortems::questions())
}

#[tauri::command]
pub fn list_postmortems(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> CommandResult<Vec<Postmortem>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| postmortems::list(conn, limit))?)
}

#[tauri::command]
pub fn get_postmortem(state: State<'_, AppState>, id: String) -> CommandResult<Postmortem> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| postmortems::get(conn, &id))?)
}

#[tauri::command]
pub fn postmortem_completion(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<PostmortemCompletion> {
    state.actor()?;
    let postmortem = state.app.db.with_conn(|conn| postmortems::get(conn, &id))?;
    Ok(postmortems::completion(&postmortem))
}

/// How complete the postmortem is against the professional standard for its kind: which
/// required sections are filled in, the completeness percentage, and whether it meets the
/// four-page minimum. Advisory - it never blocks anything.
#[tauri::command]
pub fn postmortem_completeness(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Completeness> {
    state.actor()?;
    Ok(state.app.postmortem_completeness(&id)?)
}

/// Render the postmortem to a document in the exports directory and return its path, so the
/// caller can open it in the platform's own viewer.
#[tauri::command]
pub async fn render_postmortem_pdf(state: State<'_, AppState>, id: String) -> CommandResult<String> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let exported = app.export_postmortem(&actor, &id, ExportFormat::Pdf)?;
        std::fs::create_dir_all(&app.paths.exports_dir)?;
        let path = app.paths.exports_dir.join(&exported.filename);
        std::fs::write(&path, &exported.bytes)?;
        Ok(path.display().to_string())
    })
    .await
}

/// Write a postmortem export to a path the user chose in the save dialog.
#[tauri::command]
pub async fn export_postmortem_to(
    state: State<'_, AppState>,
    id: String,
    format: ExportFormat,
    destination: String,
) -> CommandResult<String> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let exported = app.export_postmortem(&actor, &id, format)?;
        std::fs::write(&destination, &exported.bytes)?;
        Ok(destination)
    })
    .await
}

/// A suggested filename for the postmortem save dialog.
#[tauri::command]
pub fn postmortem_export_filename(
    state: State<'_, AppState>,
    id: String,
    format: ExportFormat,
) -> CommandResult<String> {
    state.actor()?;
    let postmortem = state.app.db.with_conn(|conn| postmortems::get(conn, &id))?;
    let content = postmortems::document::to_content(
        &postmortem,
        auditforge_core::models::AuthorSnapshot {
            user_id: String::new(),
            username: String::new(),
            full_name: String::new(),
            organization: String::new(),
            job_title: String::new(),
            captured_at: auditforge_core::models::Timestamp::now(),
        },
    );
    Ok(auditforge_core::export::suggested_filename(&content, format))
}

#[tauri::command]
pub async fn create_postmortem(
    state: State<'_, AppState>,
    input: NewPostmortem,
) -> CommandResult<Postmortem> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || postmortems::create(&app.db, &actor, &input)).await
}

#[tauri::command]
pub async fn update_postmortem(
    state: State<'_, AppState>,
    id: String,
    update: PostmortemUpdate,
) -> CommandResult<Postmortem> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || postmortems::update(&app.db, &actor, &id, &update)).await
}

#[tauri::command]
pub async fn add_corrective_action(
    state: State<'_, AppState>,
    input: NewCorrectiveAction,
) -> CommandResult<CorrectiveAction> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || postmortems::add_corrective_action(&app.db, &actor, &input)).await
}

#[tauri::command]
pub async fn update_corrective_action(
    state: State<'_, AppState>,
    id: String,
    update: CorrectiveActionUpdate,
) -> CommandResult<CorrectiveAction> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || postmortems::update_corrective_action(&app.db, &actor, &id, &update)).await
}

// ------------------------------------------------------------------ search, tags, audit

#[tauri::command]
pub fn global_search(
    state: State<'_, AppState>,
    query: SearchQuery,
) -> CommandResult<Vec<SearchHit>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| search::search(conn, &query))?)
}

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> CommandResult<Vec<String>> {
    state.actor()?;
    Ok(state.app.db.with_conn(search::all_tags)?)
}

#[tauri::command]
pub fn tags_for(
    state: State<'_, AppState>,
    entity_type: String,
    entity_id: String,
) -> CommandResult<Vec<String>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| search::tags_for(conn, &entity_type, &entity_id))?)
}

#[tauri::command]
pub fn tag_entity(
    state: State<'_, AppState>,
    entity_type: String,
    entity_id: String,
    tag: String,
) -> CommandResult<Vec<String>> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| {
        search::tag_entity(conn, &entity_type, &entity_id, &tag)?;
        search::tags_for(conn, &entity_type, &entity_id)
    })?)
}

#[tauri::command]
pub fn untag_entity(
    state: State<'_, AppState>,
    entity_type: String,
    entity_id: String,
    tag: String,
) -> CommandResult<Vec<String>> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| {
        search::untag_entity(conn, &entity_type, &entity_id, &tag)?;
        search::tags_for(conn, &entity_type, &entity_id)
    })?)
}

#[tauri::command]
pub fn audit_log(state: State<'_, AppState>, query: AuditQuery) -> CommandResult<Vec<AuditEntry>> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| audit::query(conn, &query))?)
}
