//! Report, template and export commands.

use auditforge_core::export::ExportFormat;
use auditforge_core::models::*;
use auditforge_core::reports::{self, ReportFilter};
use auditforge_core::templates::{self, ReportTemplate, TemplateSummary};
use serde::Serialize;
use tauri::State;

use crate::error::{blocking, CommandResult};
use crate::state::AppState;

/// A report version together with the template that describes it, which is what the
/// guided editor needs to render its questions.
#[derive(Serialize)]
pub struct ReportWorkspace {
    pub report: Report,
    pub version: ReportVersion,
    pub template: Option<ReportTemplate>,
    pub readiness: ReportReadiness,
    pub history: Vec<ReportVersionSummary>,
}

#[tauri::command]
pub fn list_templates(state: State<'_, AppState>) -> CommandResult<Vec<TemplateSummary>> {
    state.actor()?;
    Ok(state.app.db.with_conn(templates::list)?)
}

#[tauri::command]
pub fn get_template(state: State<'_, AppState>, key: String) -> CommandResult<ReportTemplate> {
    state.actor()?;
    Ok(state.app.db.with_conn(|conn| templates::get(conn, &key))?)
}

#[tauri::command]
pub fn list_reports(
    state: State<'_, AppState>,
    filter: ReportFilter,
) -> CommandResult<Vec<ReportSummary>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| reports::list(conn, &filter))?)
}

/// Everything the report editor needs in one round trip.
#[tauri::command]
pub fn open_report(
    state: State<'_, AppState>,
    id: String,
    version_id: Option<String>,
) -> CommandResult<ReportWorkspace> {
    state.actor()?;
    let version = state.app.report_version(&id, version_id.as_deref())?;
    Ok(state.app.db.with_conn(|conn| {
        let report = reports::get(conn, &id)?;
        let template = templates::get(conn, &report.template_key).ok();
        let readiness = reports::readiness(conn, &id)?;
        let history = reports::versions(conn, &id)?;
        Ok(ReportWorkspace {
            report,
            version,
            template,
            readiness,
            history,
        })
    })?)
}

#[tauri::command]
pub async fn create_report(state: State<'_, AppState>, input: NewReport) -> CommandResult<Report> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || reports::create(&app.db, &actor, &input)).await
}

#[tauri::command]
pub async fn save_report_draft(
    state: State<'_, AppState>,
    id: String,
    update: ReportDraftUpdate,
) -> CommandResult<ReportVersion> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || reports::save_draft(&app.db, &actor, &id, &update)).await
}

/// Seal the current version and open the next one - "keep a copy of this".
#[tauri::command]
pub async fn snapshot_report(
    state: State<'_, AppState>,
    id: String,
    notes: String,
) -> CommandResult<ReportVersion> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || reports::snapshot(&app.db, &actor, &id, &notes)).await
}

#[tauri::command]
pub fn report_readiness(state: State<'_, AppState>, id: String) -> CommandResult<ReportReadiness> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| reports::readiness(conn, &id))?)
}

#[tauri::command]
pub async fn submit_report_for_review(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<ReportVersion> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || reports::submit_for_review(&app.db, &actor, &id)).await
}

/// Finalize. `confirmed` must be true and comes from an explicit confirmation dialog;
/// the core refuses the call otherwise.
#[tauri::command]
pub async fn finalize_report(
    state: State<'_, AppState>,
    id: String,
    confirmed: bool,
    notes: String,
) -> CommandResult<ReportVersion> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || reports::finalize(&app.db, &actor, &id, confirmed, &notes)).await
}

#[tauri::command]
pub async fn create_report_revision(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<ReportVersion> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || reports::create_revision(&app.db, &actor, &id)).await
}

#[tauri::command]
pub fn report_versions(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Vec<ReportVersionSummary>> {
    state.actor()?;
    Ok(state
        .app
        .db
        .with_conn(|conn| reports::versions(conn, &id))?)
}

#[tauri::command]
pub async fn delete_report(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || reports::delete(&app.db, &actor, &id)).await
}

// ------------------------------------------------------------------ preview and export

/// The report rendered as a self-contained HTML page, for the in-app preview.
///
/// HTML rather than PDF: WebKitGTK, the web view on the Linux target, has no built-in PDF
/// viewer, so an embedded PDF would show a blank frame there. Returned as a string rather
/// than written to a temporary file, so a draft containing client material does not leave
/// a copy behind in `/tmp`.
#[tauri::command]
pub async fn preview_report_html(
    state: State<'_, AppState>,
    id: String,
    version_id: Option<String>,
) -> CommandResult<String> {
    state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let version = app.report_version(&id, version_id.as_deref())?;
        let template = app
            .db
            .with_conn(|conn| templates::get(conn, &version.content.template_key))
            .ok();
        Ok(auditforge_core::export::to_html(
            &version.content,
            template.as_ref(),
        ))
    })
    .await
}

/// Render the PDF into the exports directory and return its path, so the caller can open
/// it in the platform's own viewer. This does write a file; the UI says so.
#[tauri::command]
pub async fn render_pdf_for_viewing(
    state: State<'_, AppState>,
    id: String,
    version_id: Option<String>,
) -> CommandResult<String> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let path = app.export_to_disk(&actor, &id, version_id.as_deref(), ExportFormat::Pdf)?;
        Ok(path.display().to_string())
    })
    .await
}

/// Write an export to a path the user chose in the save dialog.
#[tauri::command]
pub async fn export_report_to(
    state: State<'_, AppState>,
    id: String,
    version_id: Option<String>,
    format: ExportFormat,
    destination: String,
) -> CommandResult<String> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let exported = app.export(&actor, &id, version_id.as_deref(), format)?;
        std::fs::write(&destination, &exported.bytes)?;
        Ok(destination)
    })
    .await
}

/// Write an export into the application's own exports directory.
#[tauri::command]
pub async fn export_report(
    state: State<'_, AppState>,
    id: String,
    version_id: Option<String>,
    format: ExportFormat,
) -> CommandResult<String> {
    let actor = state.actor()?;
    let app = state.app.clone();
    blocking(move || {
        let path = app.export_to_disk(&actor, &id, version_id.as_deref(), format)?;
        Ok(path.display().to_string())
    })
    .await
}

/// A suggested filename for the save dialog.
#[tauri::command]
pub fn export_filename(
    state: State<'_, AppState>,
    id: String,
    version_id: Option<String>,
    format: ExportFormat,
) -> CommandResult<String> {
    state.actor()?;
    let version = state.app.report_version(&id, version_id.as_deref())?;
    Ok(auditforge_core::export::suggested_filename(
        &version.content,
        format,
    ))
}
