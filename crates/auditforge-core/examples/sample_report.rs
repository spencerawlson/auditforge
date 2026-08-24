//! Render a filled-in sample report to PDF, for eyeballing the renderer.
//!
//! ```text
//! cargo run -p auditforge-core --example sample_report -- /tmp/sample.pdf
//! ```
//!
//! Useful when changing typography or pagination: the test suite asserts on the rendered
//! text, but some things are only visible by looking at the page.

use std::path::PathBuf;

use auditforge_core::app::{App, AppPaths};
use auditforge_core::auth::session::SessionPolicy;
use auditforge_core::export::ExportFormat;
use auditforge_core::models::*;
use auditforge_core::security::secrets::MemorySecretStore;
use auditforge_core::templates::SectionKind;
use auditforge_core::{auth, engagements, evidence, findings, reports, templates};

const PASSWORD: &str = "Correct-Horse-9-Battery";

fn main() -> auditforge_core::Result<()> {
    let destination = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "sample-report.pdf".to_string()),
    );
    let workspace = std::env::temp_dir().join(format!("auditforge-sample-{}", std::process::id()));

    let app = App::bootstrap(AppPaths::with_base(workspace.join("data")))?
        .with_secret_store(Box::new(MemorySecretStore::default()));

    auth::register(
        &app.db,
        &NewUser {
            username: "s.rawlson".into(),
            password: PASSWORD.into(),
            confirm_password: PASSWORD.into(),
            full_name: "Spencer Rawlson Spady".into(),
            organization: "PLH Ecosystem, Inc.".into(),
            job_title: "Cloud Computing & Cybersecurity".into(),
            email: String::new(),
        },
    )?;
    let session = auth::login(&app.db, "s.rawlson", PASSWORD, SessionPolicy::default())?;
    let actor = auth::resolve(&app.db, &session.token, SessionPolicy::default())?;

    let engagement = engagements::create(
        &app.db,
        &actor,
        &NewEngagement {
            name: "Northwind Q3 Perimeter Test".into(),
            client_name: "Northwind Traders".into(),
            start_date: Some("2026-09-01".into()),
            end_date: Some("2026-09-12".into()),
            scope: "203.0.113.0/24 and the VPN concentrator".into(),
            authorization_ref: "Signed ROE 2026-08-20".into(),
            team: vec![NewTeamMember {
                name: "A. Tester".into(),
                role: "Lead".into(),
                email: String::new(),
            }],
            ..Default::default()
        },
    )?;

    let report = reports::create(
        &app.db,
        &actor,
        &NewReport {
            title: "Q3 External Penetration Test".into(),
            template_key: "penetration_test".into(),
            engagement_id: Some(engagement.id.clone()),
            classification: Some(Classification::Confidential),
        },
    )?;

    // Answer every required question, plus a couple with real prose.
    let mut answers: Vec<AnswerInput> = app.db.with_conn(|conn| {
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
                        value: format!(
                            "{} This paragraph exists so the sample exercises line breaking, \
                         justification and page flow across a realistic amount of prose.",
                            question.label
                        ),
                        provenance: Provenance::UserProvided,
                        ai_request_id: None,
                    })
            })
            .collect::<Vec<_>>())
    })?;
    answers.push(AnswerInput {
        section_key: "executive_summary".into(),
        question_key: "exec_purpose".into(),
        value: "Northwind Traders commissioned an external penetration test of its internet-\
                facing perimeter ahead of a compliance review. Testing ran for two weeks in \
                September against the address range and VPN concentrator named in the scope."
            .into(),
        provenance: Provenance::UserProvided,
        ai_request_id: None,
    });
    answers.push(AnswerInput {
        section_key: "executive_summary".into(),
        question_key: "exec_outcome".into(),
        value: "The perimeter held against the tested attack paths. One configuration \
                weakness was identified on the VPN concentrator and should be corrected \
                before the review."
            .into(),
        provenance: Provenance::AiWording,
        ai_request_id: Some("sample".into()),
    });

    // Two findings, both with a confirmed severity.
    let mut finding_ids = Vec::new();
    for (title, asset, severity, remediation) in [
        (
            "TLS 1.0 enabled on the VPN concentrator",
            "vpn.example.internal",
            Severity::Medium,
            "Disable TLS 1.0 and 1.1 and restrict the cipher list to modern suites.",
        ),
        (
            "Verbose error messages on the login form",
            "www.example.test",
            Severity::Low,
            "Return a generic failure message and log the detail server-side.",
        ),
    ] {
        let finding = findings::create(
            &app.db,
            &actor,
            &NewFinding {
                title: title.into(),
                engagement_id: Some(engagement.id.clone()),
                affected_asset: asset.into(),
                category: "Configuration".into(),
                description: format!("{title} was observed during external testing."),
                technical_details: "Reproduced with openssl s_client and a manual request.".into(),
                impact: "An on-path attacker could downgrade the connection.".into(),
                remediation: remediation.into(),
                recommendation: "Adopt a hardened baseline for all edge devices.".into(),
                cvss_score: Some(5.9),
                cvss_vector: "CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:H/I:N/A:N".into(),
                ..Default::default()
            },
        )?;
        findings::set_severity(
            &app.db,
            &actor,
            &finding.id,
            &SeverityDecision {
                severity,
                confirmed_by_user: true,
                rationale: "Confirmed by the author for the sample.".into(),
            },
        )?;
        finding_ids.push(finding.id);
    }

    // One piece of evidence, so the appendix has something to cite.
    let capture = workspace.join("handshake.txt");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(&capture, b"SSL-Session:\n    Protocol  : TLSv1\n")?;
    let attached = evidence::import(
        &app.db,
        &actor,
        &app.paths.evidence_dir,
        None,
        &NewEvidence {
            source_path: capture.to_string_lossy().into_owned(),
            title: "TLS handshake showing TLS 1.0".into(),
            description: "openssl s_client output captured during testing.".into(),
            engagement_id: Some(engagement.id.clone()),
            captured_at: Some(Timestamp::now()),
            encrypt: false,
            acknowledge_executable: false,
            link_target: None,
        },
    )?;

    reports::save_draft(
        &app.db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers,
            finding_ids: Some(finding_ids),
            evidence_ids: Some(vec![attached.id]),
            ..Default::default()
        },
    )?;

    let readiness = app
        .db
        .with_conn(|conn| reports::readiness(conn, &report.id))?;
    if !readiness.can_finalize {
        for issue in &readiness.issues {
            println!("{:?}: {}", issue.severity, issue.message);
        }
    }
    reports::finalize(&app.db, &actor, &report.id, true, "Sample")?;

    let exported = app.export(&actor, &report.id, None, ExportFormat::Pdf)?;
    std::fs::write(&destination, &exported.bytes)?;

    println!(
        "wrote {} ({} bytes) as {}",
        destination.display(),
        exported.bytes.len(),
        exported.filename
    );

    // Leave nothing behind but the document.
    let _ = std::fs::remove_dir_all(&workspace);
    Ok(())
}
