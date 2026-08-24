//! End-to-end walkthrough of the MVP acceptance criteria.
//!
//! One test drives the whole product the way a person does: create an account, sign in,
//! edit the profile, run an engagement, write a report through the guided questions,
//! record findings and evidence, use the assistant, review it, finalize, generate a PDF,
//! close the application, reopen it, and find the work again.
//!
//! The AI provider is scripted rather than live, so the accept/reject workflow is
//! exercised without a network or an API key.

use auditforge_core::ai::provider::ScriptedProvider;
use auditforge_core::ai::AiService;
use auditforge_core::app::{App, AppPaths};
use auditforge_core::auth::session::SessionPolicy;
use auditforge_core::export::ExportFormat;
use auditforge_core::models::*;
use auditforge_core::search::{self, HitKind, SearchQuery};
use auditforge_core::security::secrets::MemorySecretStore;
use auditforge_core::templates::SectionKind;
use auditforge_core::{auth, engagements, evidence, findings, reports, templates};

const PASSWORD: &str = "Correct-Horse-9-Battery";

fn scripted(body: serde_json::Value) -> AiService {
    AiService::new(
        Box::new(ScriptedProvider::always(body.to_string())),
        AiSettings {
            provider: AiProviderKind::Local,
            ..Default::default()
        },
    )
}

/// Answer every required question of a report's template.
fn answer_required_questions(app: &App, actor: &Actor, report_id: &str) {
    let answers = app
        .db
        .with_conn(|conn| {
            let report = reports::get(conn, report_id)?;
            let template = templates::get(conn, &report.template_key)?;
            Ok(template
                .sections
                .iter()
                .filter(|section| section.kind == SectionKind::Guided)
                .flat_map(|section| {
                    section
                        .questions
                        .iter()
                        .filter(|question| question.required)
                        .map(|question| AnswerInput {
                            section_key: section.key.clone(),
                            question_key: question.key.clone(),
                            value: format!("Author's answer: {}", question.label),
                            provenance: Provenance::UserProvided,
                            ai_request_id: None,
                        })
                })
                .collect::<Vec<_>>())
        })
        .unwrap();
    reports::save_draft(
        &app.db,
        actor,
        report_id,
        &ReportDraftUpdate {
            answers,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn the_full_reporting_workflow_from_signup_to_pdf() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_base(home.path().join("auditforge"));

    // 1-4. Install, launch, create an account, sign in. -------------------------------
    let app = App::bootstrap(paths.clone())
        .unwrap()
        .with_secret_store(Box::new(MemorySecretStore::default()));
    assert!(
        !auth::has_any_user(&app.db).unwrap(),
        "a fresh install has no accounts"
    );

    let user = auth::register(
        &app.db,
        &NewUser {
            username: "s.rawlson".to_string(),
            password: PASSWORD.to_string(),
            confirm_password: PASSWORD.to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            email: "analyst@example.test".to_string(),
        },
    )
    .unwrap();
    assert_eq!(user.organization_name, "PLH Ecosystem, Inc.");

    let session = auth::login(&app.db, "s.rawlson", PASSWORD, SessionPolicy::default()).unwrap();
    let actor = auth::resolve(&app.db, &session.token, SessionPolicy::default()).unwrap();

    // 5. Edit the author profile. ------------------------------------------------------
    let updated = auth::update_profile(
        &app.db,
        &actor,
        &ProfileUpdate {
            job_title: Some("Cloud Computing & Cybersecurity Lead".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(updated.job_title, "Cloud Computing & Cybersecurity Lead");

    // 6. Create an engagement. ---------------------------------------------------------
    let engagement = engagements::create(
        &app.db,
        &actor,
        &NewEngagement {
            name: "Northwind Q3 Perimeter Test".to_string(),
            client_name: "Northwind Traders".to_string(),
            start_date: Some("2026-09-01".to_string()),
            end_date: Some("2026-09-12".to_string()),
            objectives: "Establish whether the perimeter can be breached from the internet."
                .to_string(),
            scope: "203.0.113.0/24 and the VPN concentrator".to_string(),
            authorization_ref: "Signed ROE 2026-08-20".to_string(),
            team: vec![NewTeamMember {
                name: "A. Tester".to_string(),
                role: "Lead".to_string(),
                email: String::new(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(engagement.engagement_ref.starts_with("ENG-"));

    // 7-8. Create a report and choose its type. ---------------------------------------
    let available = app.db.with_conn(templates::list).unwrap();
    assert_eq!(available.len(), 11, "all eleven report types are offered");

    let report = reports::create(
        &app.db,
        &actor,
        &NewReport {
            title: "Q3 External Penetration Test".to_string(),
            template_key: "penetration_test".to_string(),
            engagement_id: Some(engagement.id.clone()),
            classification: Some(Classification::Confidential),
        },
    )
    .unwrap();
    assert_eq!(report.status, ReportStatus::Draft);

    let version = app.report_version(&report.id, None).unwrap();
    assert_eq!(version.version_label, "v0.1 Draft");
    assert_eq!(version.content.author.full_name, "Spencer Rawlson Spady");
    assert_eq!(
        version.content.author.job_title, "Cloud Computing & Cybersecurity Lead",
        "a new report inherits the current profile"
    );
    assert_eq!(version.content.engagement.client_name, "Northwind Traders");

    // 9. Answer the guided questions. --------------------------------------------------
    answer_required_questions(&app, &actor, &report.id);
    reports::save_draft(
        &app.db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "executive_summary".to_string(),
                question_key: "exec_purpose".to_string(),
                value: "we tested the outside of northwinds network in september, it mostly \
                        held up but the vpn box is running old tls"
                    .to_string(),
                provenance: Provenance::UserProvided,
                ai_request_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();

    // 10. Create a finding, with severity confirmed by a person. -----------------------
    let finding = findings::create(
        &app.db,
        &actor,
        &NewFinding {
            title: "TLS 1.0 enabled on the VPN concentrator".to_string(),
            engagement_id: Some(engagement.id.clone()),
            affected_asset: "vpn.example.internal".to_string(),
            description: "The concentrator negotiates TLS 1.0 with external clients.".to_string(),
            remediation: "Disable TLS 1.0 and 1.1 on the concentrator.".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!finding.severity_confirmed, "severity is never assumed");

    findings::set_severity(
        &app.db,
        &actor,
        &finding.id,
        &SeverityDecision {
            severity: Severity::Medium,
            confirmed_by_user: true,
            rationale: "Reachable from the internet but needs an on-path position".to_string(),
        },
    )
    .unwrap();

    // 11. Attach evidence. -------------------------------------------------------------
    let capture = workspace.path().join("handshake.txt");
    std::fs::write(&capture, b"SSL-Session:\n    Protocol  : TLSv1\n").unwrap();
    let attached = evidence::import(
        &app.db,
        &actor,
        &app.paths.evidence_dir,
        app.vault_key().as_ref(),
        &NewEvidence {
            source_path: capture.to_str().unwrap().to_string(),
            title: "TLS handshake showing TLS 1.0".to_string(),
            description: "openssl s_client output".to_string(),
            engagement_id: Some(engagement.id.clone()),
            captured_at: Some(Timestamp::now()),
            encrypt: true,
            acknowledge_executable: false,
            link_target: Some(EvidenceLinkRequest {
                target_type: EntityType::Finding,
                target_id: finding.id.clone(),
            }),
        },
    )
    .unwrap();
    assert!(attached.is_encrypted);

    reports::save_draft(
        &app.db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            finding_ids: Some(vec![finding.id.clone()]),
            evidence_ids: Some(vec![attached.id.clone()]),
            ..Default::default()
        },
    )
    .unwrap();

    // 12-13. Use the assistant, then review and accept its suggestion. -----------------
    let improved = "We tested the external perimeter of Northwind's network in September. \
                    The perimeter largely held; the VPN concentrator remains configured for \
                    an obsolete TLS version.";
    let assistant = scripted(serde_json::json!({
        "suggestion": improved,
        "confidence": "high",
        "missing_information": [],
        "notes": ["Tightened the wording; no facts changed."]
    }));

    let original = app
        .report_version(&report.id, None)
        .unwrap()
        .content
        .section("executive_summary")
        .unwrap()
        .answer("exec_purpose")
        .unwrap()
        .value
        .clone();

    let suggestion = assistant
        .suggest(
            &app.db,
            &actor,
            &AiTextRequest {
                function: AiFunction::ImproveWriting,
                text: original.clone(),
                summary_kind: None,
                context: "Executive summary of a penetration test".to_string(),
                target_type: "report".to_string(),
                target_id: report.id.clone(),
            },
        )
        .unwrap();

    assert_eq!(
        suggestion.original, original,
        "the original is preserved for comparison"
    );
    assert!(
        !suggestion.diff.is_empty(),
        "the reviewer sees what changed"
    );
    assert_eq!(suggestion.provenance, Provenance::AiWording);

    // Nothing has been written yet: the suggestion is a proposal.
    assert_eq!(
        app.report_version(&report.id, None)
            .unwrap()
            .content
            .section("executive_summary")
            .unwrap()
            .answer("exec_purpose")
            .unwrap()
            .value,
        original
    );

    // The human accepts it, which is what actually applies the text.
    auditforge_core::ai::record_decision(
        &app.db,
        &actor,
        &suggestion.request_id,
        AiDecision::Accepted,
    )
    .unwrap();
    reports::save_draft(
        &app.db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "executive_summary".to_string(),
                question_key: "exec_purpose".to_string(),
                value: suggestion.suggestion.clone(),
                provenance: suggestion.provenance,
                ai_request_id: Some(suggestion.request_id.clone()),
            }],
            ..Default::default()
        },
    )
    .unwrap();

    // 14. Run an AI report-quality review. ---------------------------------------------
    let reviewer = scripted(serde_json::json!({
        "summary": "One wording inconsistency.",
        "confidence": "medium",
        "warnings": [{
            "category": "inconsistent_terminology",
            "message": "The report calls the same device both a concentrator and a VPN box.",
            "quote": "The perimeter largely held; the VPN concentrator remains configured for an obsolete TLS version."
        }]
    }));
    let content = app.report_version(&report.id, None).unwrap().content;
    let review = reviewer
        .review_report(&app.db, &actor, &report.id, &content)
        .unwrap();
    assert_eq!(review.warnings.len(), 1);
    assert_eq!(
        review.warnings[0].category,
        ReviewCategory::InconsistentTerminology
    );

    // 15. Correct the issue the review raised. -----------------------------------------
    reports::save_draft(
        &app.db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "conclusion".to_string(),
                question_key: "conclusion_summary".to_string(),
                value: "The perimeter is sound apart from the VPN concentrator's TLS \
                        configuration, which should be corrected before the compliance review."
                    .to_string(),
                provenance: Provenance::UserProvided,
                ai_request_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();

    // 16. Preview before committing to anything. ---------------------------------------
    let preview = app.render_pdf(&actor, &report.id, None).unwrap();
    assert!(preview.starts_with(b"%PDF"));

    // 17. Finalize, which is refused until the report is actually ready. ---------------
    let readiness = app
        .db
        .with_conn(|conn| reports::readiness(conn, &report.id))
        .unwrap();
    assert!(
        readiness.can_finalize,
        "outstanding blockers: {:?}",
        readiness.issues
    );
    assert!(reports::finalize(&app.db, &actor, &report.id, false, "").is_err());

    let sealed =
        reports::finalize(&app.db, &actor, &report.id, true, "Approved by the author").unwrap();
    assert_eq!(sealed.version_label, "v1.0 Final");
    assert!(sealed.is_immutable);

    // 18-19. Generate and save the PDF. ------------------------------------------------
    let pdf_path = app
        .export_to_disk(&actor, &report.id, None, ExportFormat::Pdf)
        .unwrap();
    let pdf = std::fs::read(&pdf_path).unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    assert!(
        pdf.len() > 5_000,
        "a finished report should be a real document"
    );

    // A finalized report refuses further edits.
    let err = reports::save_draft(&app.db, &actor, &report.id, &ReportDraftUpdate::default())
        .unwrap_err();
    assert_eq!(err.kind(), "immutable");

    // 23. Sign out.
    auth::logout(&app.db, &session.token).unwrap();
    assert!(auth::resolve(&app.db, &session.token, SessionPolicy::default()).is_err());
    drop(app);

    // 20. Reopen the application later and find the work. ------------------------------
    let app = App::bootstrap(paths)
        .unwrap()
        .with_secret_store(Box::new(MemorySecretStore::default()));
    let session = auth::login(&app.db, "s.rawlson", PASSWORD, SessionPolicy::default()).unwrap();
    let actor = auth::resolve(&app.db, &session.token, SessionPolicy::default()).unwrap();

    let reopened = app.report_version(&report.id, None).unwrap();
    assert_eq!(reopened.version_label, "v1.0 Final");
    assert_eq!(reopened.content.findings.len(), 1);
    assert_eq!(reopened.content.evidence.len(), 1);
    assert!(reopened
        .content
        .section("executive_summary")
        .unwrap()
        .answer("exec_purpose")
        .unwrap()
        .value
        .contains("Northwind"));

    // 21. View the version history. ----------------------------------------------------
    let history = app
        .db
        .with_conn(|conn| reports::versions(conn, &report.id))
        .unwrap();
    let labels: Vec<&str> = history
        .iter()
        .map(|entry| entry.version_label.as_str())
        .collect();
    assert_eq!(labels, vec!["v1.0 Final", "v0.1 Draft"]);
    assert!(history.iter().all(|entry| !entry.content_hash.is_empty()));

    // 22. Search. ----------------------------------------------------------------------
    let hits = app
        .db
        .with_conn(|conn| {
            search::search(
                conn,
                &SearchQuery {
                    text: "Northwind".to_string(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert!(hits.iter().any(|hit| hit.kind == HitKind::Engagement));
    assert!(hits.iter().any(|hit| hit.kind == HitKind::Report));

    let by_reference = app
        .db
        .with_conn(|conn| {
            search::search(
                conn,
                &SearchQuery {
                    text: finding.finding_ref.clone(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(by_reference[0].kind, HitKind::Finding);

    // The audit trail records the whole story.
    let trail = app
        .db
        .with_conn(|conn| {
            auditforge_core::audit::query(
                conn,
                &AuditQuery {
                    limit: Some(500),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let actions: Vec<AuditAction> = trail.iter().map(|entry| entry.action).collect();
    for expected in [
        AuditAction::AccountCreated,
        AuditAction::Login,
        AuditAction::ProfileUpdated,
        AuditAction::EngagementCreated,
        AuditAction::ReportCreated,
        AuditAction::FindingCreated,
        AuditAction::FindingSeverityConfirmed,
        AuditAction::EvidenceAdded,
        AuditAction::AiSuggestionGenerated,
        AuditAction::AiSuggestionAccepted,
        AuditAction::AiReviewRun,
        AuditAction::ReportFinalized,
        AuditAction::PdfGenerated,
        AuditAction::ReportExported,
        AuditAction::Logout,
    ] {
        assert!(
            actions.contains(&expected),
            "audit trail is missing {expected}"
        );
    }
    let _ = &actor;
}

#[test]
fn a_finalized_report_is_revised_rather_than_edited() {
    let home = tempfile::tempdir().unwrap();
    let app = App::bootstrap(AppPaths::with_base(home.path().join("auditforge")))
        .unwrap()
        .with_secret_store(Box::new(MemorySecretStore::default()));

    auth::register(
        &app.db,
        &NewUser {
            username: "analyst".to_string(),
            password: PASSWORD.to_string(),
            confirm_password: PASSWORD.to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            email: String::new(),
        },
    )
    .unwrap();
    let session = auth::login(&app.db, "analyst", PASSWORD, SessionPolicy::default()).unwrap();
    let actor = auth::resolve(&app.db, &session.token, SessionPolicy::default()).unwrap();

    let report = reports::create(
        &app.db,
        &actor,
        &NewReport {
            title: "Incident 2026-09-04".to_string(),
            template_key: "incident_report".to_string(),
            engagement_id: None,
            classification: None,
        },
    )
    .unwrap();
    answer_required_questions(&app, &actor, &report.id);

    let first =
        reports::finalize(&app.db, &actor, &report.id, true, "Issued to the client").unwrap();
    let first_hash = first.content_hash.clone();
    let first_pdf = app.render_pdf(&actor, &report.id, None).unwrap();

    // A correction is a new revision; the issued document is untouched.
    reports::create_revision(&app.db, &actor, &report.id).unwrap();
    reports::save_draft(
        &app.db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "root_cause".to_string(),
                question_key: "root_cause_analysis".to_string(),
                value: "Root cause confirmed after further log analysis.".to_string(),
                provenance: Provenance::UserProvided,
                ai_request_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let revised = reports::finalize(&app.db, &actor, &report.id, true, "Revision 1").unwrap();

    assert_eq!(revised.version_label, "v2.0 Final");
    let reloaded_first = app.report_version(&report.id, Some(&first.id)).unwrap();
    assert_eq!(
        reloaded_first.content_hash, first_hash,
        "the issued version must not change"
    );

    // And the original can still be re-rendered byte-for-byte identically apart from its
    // generation timestamp, because the version carries its own content.
    let reissued = app.render_pdf(&actor, &report.id, Some(&first.id)).unwrap();
    assert_eq!(reissued.len(), first_pdf.len());

    let history = app
        .db
        .with_conn(|conn| reports::versions(conn, &report.id))
        .unwrap();
    // v0.1 Draft, v1.0 Final, v1.1 Revised, v2.0 Final.
    assert_eq!(history.len(), 4);
    assert_eq!(
        history.iter().filter(|entry| entry.is_immutable).count(),
        4,
        "every version in the history is sealed once the latest is final"
    );
    let labels: Vec<&str> = history
        .iter()
        .map(|entry| entry.version_label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["v2.0 Final", "v1.1 Revised", "v1.0 Final", "v0.1 Draft"]
    );
}
