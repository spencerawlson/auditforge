use super::*;
use crate::auth;
use crate::models::{
    AnswerInput, NewEngagement, NewFinding, NewReport, NewUser, Provenance, ReportDraftUpdate,
    Severity, SeverityDecision,
};
use crate::templates::SectionKind;

struct Harness {
    app: App,
    actor: Actor,
    _dir: tempfile::TempDir,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let app = App::bootstrap(AppPaths::with_base(dir.path().join("data")))
        .unwrap()
        .with_secret_store(Box::new(secrets::MemorySecretStore::default()));

    auth::register(
        &app.db,
        &NewUser {
            username: "analyst".to_string(),
            password: "Correct-Horse-9-Battery".to_string(),
            confirm_password: "Correct-Horse-9-Battery".to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            email: String::new(),
        },
    )
    .unwrap();
    let session = auth::login(
        &app.db,
        "analyst",
        "Correct-Horse-9-Battery",
        SessionPolicy::default(),
    )
    .unwrap();
    let actor = auth::resolve(&app.db, &session.token, SessionPolicy::default()).unwrap();

    Harness {
        app,
        actor,
        _dir: dir,
    }
}

fn ready_report(harness: &Harness) -> String {
    let engagement = engagements::create(
        &harness.app.db,
        &harness.actor,
        &NewEngagement {
            name: "Northwind Perimeter".to_string(),
            client_name: "Northwind Traders".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    let report = reports::create(
        &harness.app.db,
        &harness.actor,
        &NewReport {
            title: "Q3 External Penetration Test".to_string(),
            template_key: "penetration_test".to_string(),
            engagement_id: Some(engagement.id),
            classification: None,
        },
    )
    .unwrap();

    let finding = findings::create(
        &harness.app.db,
        &harness.actor,
        &NewFinding {
            title: "TLS 1.0 enabled on the VPN concentrator".to_string(),
            description: "The concentrator negotiates TLS 1.0.".to_string(),
            remediation: "Disable TLS 1.0 and 1.1.".to_string(),
            affected_asset: "vpn.example.internal".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    findings::set_severity(
        &harness.app.db,
        &harness.actor,
        &finding.id,
        &SeverityDecision {
            severity: Severity::Medium,
            confirmed_by_user: true,
            rationale: "Exploitable from the internet".to_string(),
        },
    )
    .unwrap();

    // Answer every required question so the report can be finalized.
    let answers = harness
        .app
        .db
        .with_conn(|conn| {
            let template = templates::get(conn, "penetration_test")?;
            Ok(template
                .sections
                .iter()
                .filter(|section| section.kind == SectionKind::Guided)
                .flat_map(|section| {
                    section
                        .questions
                        .iter()
                        .filter(|q| q.required)
                        .map(|question| AnswerInput {
                            section_key: section.key.clone(),
                            question_key: question.key.clone(),
                            value: format!("The author's answer to: {}", question.label),
                            provenance: Provenance::UserProvided,
                            ai_request_id: None,
                        })
                })
                .collect::<Vec<_>>())
        })
        .unwrap();

    reports::save_draft(
        &harness.app.db,
        &harness.actor,
        &report.id,
        &ReportDraftUpdate {
            answers,
            finding_ids: Some(vec![finding.id]),
            ..Default::default()
        },
    )
    .unwrap();

    report.id
}

#[test]
fn bootstrap_creates_the_data_layout_and_loads_templates() {
    let harness = harness();
    assert!(harness.app.paths.data_dir.is_dir());
    assert!(harness.app.paths.evidence_dir.is_dir());
    assert!(harness.app.paths.templates_dir.is_dir());
    assert!(harness.app.paths.exports_dir.is_dir());
    assert!(harness.app.paths.database_path.is_file());

    let listed = harness.app.db.with_conn(templates::list).unwrap();
    assert_eq!(listed.len(), 11);
}

#[test]
fn paths_hang_together_under_one_directory() {
    let paths = AppPaths::with_base("/tmp/example");
    assert!(paths.database_path.starts_with(&paths.data_dir));
    assert!(paths.evidence_dir.starts_with(&paths.data_dir));
    assert!(paths.exports_dir.starts_with(&paths.data_dir));
    assert_eq!(paths.database_path.file_name().unwrap(), "auditforge.db");
}

#[test]
fn reopening_the_same_directory_keeps_the_data() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_base(dir.path().join("data"));

    {
        let app = App::bootstrap(paths.clone()).unwrap();
        auth::register(
            &app.db,
            &NewUser {
                username: "analyst".to_string(),
                password: "Correct-Horse-9-Battery".to_string(),
                confirm_password: "Correct-Horse-9-Battery".to_string(),
                full_name: "Spencer Rawlson Spady".to_string(),
                organization: "PLH Ecosystem, Inc.".to_string(),
                job_title: "Cloud".to_string(),
                email: String::new(),
            },
        )
        .unwrap();
    }

    let reopened = App::bootstrap(paths).unwrap();
    assert!(
        auth::has_any_user(&reopened.db).unwrap(),
        "the account must survive a restart"
    );
    assert!(auth::login(
        &reopened.db,
        "analyst",
        "Correct-Horse-9-Battery",
        SessionPolicy::default()
    )
    .is_ok());
}

#[test]
fn settings_report_whether_a_key_exists_without_revealing_it() {
    let harness = harness();
    assert!(
        !harness
            .app
            .settings(&harness.actor.user_id)
            .unwrap()
            .ai
            .has_api_key
    );

    harness
        .app
        .store_api_key(&harness.actor, "sk-secret-canary")
        .unwrap();
    let settings = harness.app.settings(&harness.actor.user_id).unwrap();
    assert!(settings.ai.has_api_key);

    let serialized = serde_json::to_string(&settings).unwrap();
    assert!(
        !serialized.contains("sk-secret-canary"),
        "the key must never be serialized"
    );

    harness.app.clear_api_key(&harness.actor).unwrap();
    assert!(
        !harness
            .app
            .settings(&harness.actor.user_id)
            .unwrap()
            .ai
            .has_api_key
    );
}

#[test]
fn storing_an_empty_key_is_refused() {
    let harness = harness();
    assert!(harness.app.store_api_key(&harness.actor, "   ").is_err());
}

#[test]
fn the_vault_key_is_created_once_and_then_reused() {
    let harness = harness();
    let first = harness.app.vault_key().expect("a key should be created");
    let second = harness
        .app
        .vault_key()
        .expect("the same key should come back");
    assert_eq!(first.to_base64(), second.to_base64());
}

#[test]
fn the_ai_service_is_disabled_until_it_is_configured() {
    let harness = harness();
    let status = harness
        .app
        .ai_service(&harness.actor.user_id)
        .unwrap()
        .status();
    assert!(!status.enabled);
    assert!(!status.leaves_machine);
}

#[test]
fn a_draft_can_be_previewed_and_a_final_version_exported() {
    let harness = harness();
    let report_id = ready_report(&harness);

    let draft_pdf = harness
        .app
        .render_pdf(&harness.actor, &report_id, None)
        .unwrap();
    assert!(draft_pdf.starts_with(b"%PDF"));

    reports::finalize(
        &harness.app.db,
        &harness.actor,
        &report_id,
        true,
        "Signed off",
    )
    .unwrap();
    let final_pdf = harness
        .app
        .render_pdf(&harness.actor, &report_id, None)
        .unwrap();
    assert!(final_pdf.starts_with(b"%PDF"));
    assert!(final_pdf.len() > 4_000);
}

#[test]
fn every_export_format_produces_a_file_with_a_sensible_name() {
    let harness = harness();
    let report_id = ready_report(&harness);

    for format in [
        ExportFormat::Pdf,
        ExportFormat::Html,
        ExportFormat::Json,
        ExportFormat::Markdown,
        ExportFormat::Csv,
    ] {
        let exported = harness
            .app
            .export(&harness.actor, &report_id, None, format)
            .unwrap();
        assert!(!exported.bytes.is_empty(), "{format:?} produced nothing");
        assert!(exported.filename.ends_with(format.extension()));
        assert!(exported.filename.starts_with("RPT-"));
    }
}

#[test]
fn exporting_to_disk_writes_into_the_exports_directory() {
    let harness = harness();
    let report_id = ready_report(&harness);
    let path = harness
        .app
        .export_to_disk(&harness.actor, &report_id, None, ExportFormat::Html)
        .unwrap();

    assert!(path.starts_with(&harness.app.paths.exports_dir));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("Q3 External Penetration Test"));
}

#[test]
fn generating_a_pdf_is_audited() {
    let harness = harness();
    let report_id = ready_report(&harness);
    harness
        .app
        .render_pdf(&harness.actor, &report_id, None)
        .unwrap();

    let entries = harness
        .app
        .db
        .with_conn(|conn| {
            crate::audit::query(
                conn,
                &crate::models::AuditQuery {
                    action: Some(crate::models::AuditAction::PdfGenerated),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].target_id, report_id);
    assert_eq!(entries[0].detail["finalized"], false);
}

#[test]
fn a_specific_historical_version_can_be_rendered() {
    let harness = harness();
    let report_id = ready_report(&harness);
    let draft = harness.app.report_version(&report_id, None).unwrap();
    reports::finalize(&harness.app.db, &harness.actor, &report_id, true, "").unwrap();

    let old = harness
        .app
        .report_version(&report_id, Some(&draft.id))
        .unwrap();
    assert_eq!(old.version_label, "v0.1 Draft");
    assert!(harness
        .app
        .render_pdf(&harness.actor, &report_id, Some(&draft.id))
        .is_ok());
}

#[test]
fn a_version_belonging_to_another_report_is_refused() {
    let harness = harness();
    let first = ready_report(&harness);
    let second = reports::create(
        &harness.app.db,
        &harness.actor,
        &NewReport {
            title: "Another report".to_string(),
            template_key: "executive_summary".to_string(),
            engagement_id: None,
            classification: None,
        },
    )
    .unwrap();

    let other_version = harness.app.report_version(&second.id, None).unwrap();
    let err = harness
        .app
        .report_version(&first, Some(&other_version.id))
        .unwrap_err();
    assert_eq!(err.kind(), "not_found");
}

#[test]
fn the_dashboard_summarizes_the_workspace() {
    let harness = harness();
    let report_id = ready_report(&harness);

    let dashboard = harness.app.dashboard(&harness.actor).unwrap();
    assert_eq!(dashboard.counts.reports, 1);
    assert_eq!(dashboard.counts.findings, 1);
    assert_eq!(dashboard.counts.engagements, 1);
    assert_eq!(dashboard.counts.unconfirmed_severities, 0);
    assert_eq!(dashboard.drafts.len(), 1);
    assert!(dashboard.awaiting_review.is_empty());
    assert!(dashboard.ready_to_export.is_empty());

    reports::submit_for_review(&harness.app.db, &harness.actor, &report_id).unwrap();
    let dashboard = harness.app.dashboard(&harness.actor).unwrap();
    assert_eq!(dashboard.awaiting_review.len(), 1);
    assert!(dashboard.drafts.is_empty());

    reports::finalize(&harness.app.db, &harness.actor, &report_id, true, "").unwrap();
    let dashboard = harness.app.dashboard(&harness.actor).unwrap();
    assert_eq!(dashboard.ready_to_export.len(), 1);
}

#[test]
fn the_dashboard_counts_findings_that_still_need_a_severity_decision() {
    let harness = harness();
    findings::create(
        &harness.app.db,
        &harness.actor,
        &NewFinding {
            title: "Not yet triaged".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let dashboard = harness.app.dashboard(&harness.actor).unwrap();
    assert_eq!(dashboard.counts.unconfirmed_severities, 1);
}

#[test]
fn an_in_memory_app_works_without_touching_disk() {
    let app = App::in_memory().unwrap();
    assert_eq!(app.db.with_conn(templates::list).unwrap().len(), 11);
    assert!(!app.secret_store().is_os_backed());
}
