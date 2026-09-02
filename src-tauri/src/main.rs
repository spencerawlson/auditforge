// Hide the console window on Windows release builds. On Linux, the target platform, this
// attribute has no effect.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The AuditForge desktop shell.
//!
//! This crate is deliberately thin. It opens the window, owns the session token, and
//! translates Tauri commands into calls on `auditforge_core::App`. No domain rule lives
//! here: everything the application decides - what may be edited, when a report may be
//! finalized, where text is allowed to travel - is decided in the core and is covered by
//! the core's tests.

mod commands;
mod error;
mod state;

use auditforge_core::{App, AppPaths};
use state::AppState;

fn main() {
    init_logging();

    let paths = match AppPaths::resolve() {
        Ok(paths) => paths,
        Err(error) => {
            fatal(&format!(
                "AuditForge could not work out where to store its data.\n\n{error}"
            ));
            return;
        }
    };

    let app = match App::bootstrap(paths.clone()) {
        Ok(app) => app,
        Err(error) => {
            fatal(&format!(
                "AuditForge could not open its database at {}.\n\n{error}\n\n\
                 If another copy of AuditForge is running, close it and try again.",
                paths.database_path.display()
            ));
            return;
        }
    };

    tracing::info!(
        version = auditforge_core::VERSION,
        data_dir = %paths.data_dir.display(),
        secrets = app.secret_store().backend_name(),
        "AuditForge starting"
    );
    if !app.secret_store().is_os_backed() {
        tracing::warn!(
            "no OS keychain is available; secrets are held in an encrypted local file. \
             See docs/security.md."
        );
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(app))
        .invoke_handler(tauri::generate_handler![
            // account and session
            commands::auth::startup_status,
            commands::auth::register,
            commands::auth::login,
            commands::auth::logout,
            commands::auth::current_user,
            commands::auth::session_remaining,
            commands::auth::update_profile,
            commands::auth::change_password,
            commands::auth::password_strength,
            // workspace
            commands::workspace::dashboard,
            commands::workspace::list_engagements,
            commands::workspace::get_engagement,
            commands::workspace::create_engagement,
            commands::workspace::update_engagement,
            commands::workspace::delete_engagement,
            commands::workspace::list_findings,
            commands::workspace::get_finding,
            commands::workspace::create_finding,
            commands::workspace::update_finding,
            commands::workspace::set_finding_severity,
            commands::workspace::delete_finding,
            commands::workspace::list_evidence,
            commands::workspace::import_evidence,
            commands::workspace::update_evidence,
            commands::workspace::link_evidence,
            commands::workspace::unlink_evidence,
            commands::workspace::delete_evidence,
            commands::workspace::evidence_path,
            commands::workspace::list_incidents,
            commands::workspace::get_incident,
            commands::workspace::create_incident,
            commands::workspace::update_incident,
            commands::workspace::set_incident_severity,
            commands::workspace::postmortem_questions,
            commands::workspace::list_postmortems,
            commands::workspace::get_postmortem,
            commands::workspace::postmortem_completion,
            commands::workspace::postmortem_completeness,
            commands::workspace::render_postmortem_pdf,
            commands::workspace::export_postmortem_to,
            commands::workspace::postmortem_export_filename,
            commands::workspace::create_postmortem,
            commands::workspace::update_postmortem,
            commands::workspace::add_corrective_action,
            commands::workspace::update_corrective_action,
            commands::workspace::global_search,
            commands::workspace::list_tags,
            commands::workspace::tags_for,
            commands::workspace::tag_entity,
            commands::workspace::untag_entity,
            commands::workspace::audit_log,
            // reports
            commands::reports::list_templates,
            commands::reports::get_template,
            commands::reports::list_reports,
            commands::reports::open_report,
            commands::reports::create_report,
            commands::reports::save_report_draft,
            commands::reports::snapshot_report,
            commands::reports::report_readiness,
            commands::reports::submit_report_for_review,
            commands::reports::finalize_report,
            commands::reports::create_report_revision,
            commands::reports::report_versions,
            commands::reports::delete_report,
            commands::reports::preview_report_html,
            commands::reports::render_pdf_for_viewing,
            commands::reports::export_report,
            commands::reports::export_report_to,
            commands::reports::export_filename,
            // AI
            commands::ai::ai_status,
            commands::ai::ai_redaction_preview,
            commands::ai::ai_suggest,
            commands::ai::ai_review_report,
            commands::ai::ai_review_preview,
            commands::ai::ai_record_decision,
            commands::ai::ai_requests_for,
            // settings
            commands::settings::get_settings,
            commands::settings::save_security_settings,
            commands::settings::save_ai_settings,
            commands::settings::save_report_settings,
            commands::settings::save_ui_settings,
            commands::settings::set_api_key,
            commands::settings::clear_api_key,
            commands::settings::backup_database,
        ])
        .run(tauri::generate_context!())
        .expect("AuditForge failed to start the application window");
}

/// Logging goes to stderr. `AUDITFORGE_LOG` follows the usual `RUST_LOG` syntax.
///
/// Report content is never logged: entries carry ids, counts and outcomes only.
fn init_logging() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("AUDITFORGE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("auditforge=info,auditforge_core=info,warn"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}

/// Report a startup failure where the user will see it, then exit.
///
/// A dialog rather than a silent exit: if the database cannot be opened there is no
/// window to show the error in.
fn fatal(message: &str) {
    tracing::error!("{message}");
    eprintln!("AuditForge: {message}");
    std::process::exit(1);
}
