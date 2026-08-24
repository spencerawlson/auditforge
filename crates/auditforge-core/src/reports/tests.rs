use super::*;
use crate::models::{
    FindingUpdate, NewEngagement, NewFinding, ProfileUpdate, Severity, SeverityDecision,
};
use crate::test_support::seeded;

fn new_report(template_key: &str) -> NewReport {
    NewReport {
        title: "Q3 External Penetration Test".to_string(),
        template_key: template_key.to_string(),
        engagement_id: None,
        classification: None,
    }
}

/// Answer every required question in every included guided section.
fn fill_required(db: &Database, actor: &Actor, report_id: &str) {
    let answers = db
        .with_conn(|conn| {
            let report = get(conn, report_id)?;
            let template = templates::get(conn, &report.template_key)?;
            let version = current_version(conn, report_id)?;
            let mut answers = Vec::new();
            for section in &template.sections {
                let included = version
                    .content
                    .section(&section.key)
                    .map(|s| s.included)
                    .unwrap_or(false);
                if !included || section.kind != SectionKind::Guided {
                    continue;
                }
                for question in &section.questions {
                    if question.required {
                        answers.push(AnswerInput {
                            section_key: section.key.clone(),
                            question_key: question.key.clone(),
                            value: format!("Author's answer to: {}", question.label),
                            provenance: Provenance::UserProvided,
                            ai_request_id: None,
                        });
                    }
                }
            }
            Ok(answers)
        })
        .unwrap();
    save_draft(
        db,
        actor,
        report_id,
        &ReportDraftUpdate {
            answers,
            ..Default::default()
        },
    )
    .unwrap();
}

fn confirmed_finding(db: &Database, actor: &Actor, title: &str, severity: Severity) -> String {
    let finding = findings::create(
        db,
        actor,
        &NewFinding {
            title: title.to_string(),
            description: "Observed during testing.".to_string(),
            remediation: "Apply the vendor patch.".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    findings::set_severity(
        db,
        actor,
        &finding.id,
        &SeverityDecision {
            severity,
            confirmed_by_user: true,
            rationale: String::new(),
        },
    )
    .unwrap();
    finding.id
}

#[test]
fn a_new_report_starts_at_v0_1_draft_with_every_question_present() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("penetration_test")).unwrap();

    assert!(report.report_ref.starts_with("RPT-"));
    assert_eq!(report.status, ReportStatus::Draft);
    assert_eq!(report.classification, Classification::Confidential);

    let version = db
        .with_conn(|conn| current_version(conn, &report.id))
        .unwrap();
    assert_eq!(version.version_label, "v0.1 Draft");
    assert!(!version.is_immutable);
    assert_eq!(version.content.author.full_name, "Spencer Rawlson Spady");
    assert_eq!(version.content.author.organization, "PLH Ecosystem, Inc.");
    assert!(version
        .content
        .sections
        .iter()
        .any(|s| s.key == "executive_summary"));
    assert!(!version.content_hash.is_empty());
}

#[test]
fn a_report_inherits_the_engagement_snapshot() {
    let (db, actor) = seeded();
    let engagement = engagements::create(
        &db,
        &actor,
        &NewEngagement {
            name: "Northwind Perimeter".to_string(),
            client_name: "Northwind Traders".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    let mut input = new_report("penetration_test");
    input.engagement_id = Some(engagement.id.clone());
    let report = create(&db, &actor, &input).unwrap();

    let version = db
        .with_conn(|conn| current_version(conn, &report.id))
        .unwrap();
    assert_eq!(version.content.engagement.name, "Northwind Perimeter");
    assert_eq!(version.content.engagement.client_name, "Northwind Traders");
    assert_eq!(
        version.content.engagement.engagement_ref,
        engagement.engagement_ref
    );
}

#[test]
fn saving_a_draft_writes_answers_in_place() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();

    let version = save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "overview".to_string(),
                question_key: "es_context".to_string(),
                value: "We reviewed the perimeter in September.".to_string(),
                provenance: Provenance::UserProvided,
                ai_request_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        version.version_label, "v0.1 Draft",
        "an ordinary save must not mint a version"
    );
    let answer = version
        .content
        .section("overview")
        .unwrap()
        .answer("es_context")
        .unwrap();
    assert_eq!(answer.value, "We reviewed the perimeter in September.");
    assert_eq!(answer.provenance, Provenance::UserProvided);

    let history = db.with_conn(|conn| versions(conn, &report.id)).unwrap();
    assert_eq!(history.len(), 1);
}

#[test]
fn answers_are_rejected_for_questions_the_template_does_not_have() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    let err = save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "overview".to_string(),
                question_key: "invented_by_a_model".to_string(),
                value: "...".to_string(),
                provenance: Provenance::UserProvided,
                ai_request_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown question"), "got {err}");
}

#[test]
fn sections_can_be_turned_off_without_losing_their_text() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("custom_report")).unwrap();
    save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "background".to_string(),
                question_key: "custom_background".to_string(),
                value: "Context the author typed.".to_string(),
                provenance: Provenance::UserProvided,
                ai_request_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();

    let version = save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            included_sections: Some(vec!["analysis".to_string()]),
            ..Default::default()
        },
    )
    .unwrap();

    let background = version.content.section("background").unwrap();
    assert!(!background.included);
    assert_eq!(
        background.answer("custom_background").unwrap().value,
        "Context the author typed."
    );
}

#[test]
fn snapshot_seals_the_current_version_and_opens_the_next() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    let sealed = snapshot(&db, &actor, &report.id, "Before the client call").unwrap();

    assert_eq!(sealed.version_label, "v0.2 Draft");
    let history = db.with_conn(|conn| versions(conn, &report.id)).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version_label, "v0.2 Draft");
    assert!(
        history[1].is_immutable,
        "the previous version must be sealed"
    );
}

#[test]
fn submitting_for_review_moves_to_a_review_version() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    let review = submit_for_review(&db, &actor, &report.id).unwrap();

    assert_eq!(review.version_label, "v0.2 Review");
    assert_eq!(review.status, ReportStatus::Review);
    let reloaded = db.with_conn(|conn| get(conn, &report.id)).unwrap();
    assert_eq!(reloaded.status, ReportStatus::Review);
    // A report in review is still editable.
    save_draft(&db, &actor, &report.id, &ReportDraftUpdate::default()).unwrap();
}

#[test]
fn readiness_reports_blockers_and_warnings_separately() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();

    let readiness = db
        .with_conn(|conn| super::readiness(conn, &report.id))
        .unwrap();
    assert!(!readiness.can_finalize);
    assert!(readiness
        .issues
        .iter()
        .any(|issue| issue.code == "required_answer_missing"
            && issue.severity == IssueSeverity::Blocker));

    fill_required(&db, &actor, &report.id);
    let readiness = db
        .with_conn(|conn| super::readiness(conn, &report.id))
        .unwrap();
    assert!(
        readiness.can_finalize,
        "unanswered optional questions must not block: {readiness:?}"
    );
    assert!(readiness
        .issues
        .iter()
        .all(|issue| issue.severity == IssueSeverity::Warning));
}

#[test]
fn an_unconfirmed_ai_inference_blocks_finalization() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);

    save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "risk_posture".to_string(),
                question_key: "es_posture".to_string(),
                value: "The estate is probably improving.".to_string(),
                provenance: Provenance::AiInference,
                ai_request_id: Some("req-1".to_string()),
            }],
            ..Default::default()
        },
    )
    .unwrap();

    let readiness = db
        .with_conn(|conn| super::readiness(conn, &report.id))
        .unwrap();
    assert!(!readiness.can_finalize);
    assert!(readiness
        .issues
        .iter()
        .any(|issue| issue.code == "unconfirmed_ai_inference"));

    let err = finalize(&db, &actor, &report.id, true, "").unwrap_err();
    assert!(
        err.to_string().contains("unconfirmed AI inference"),
        "got {err}"
    );

    // Claiming the wording as the author's own clears the blocker.
    save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "risk_posture".to_string(),
                question_key: "es_posture".to_string(),
                value: "The estate is improving.".to_string(),
                provenance: Provenance::AiWording,
                ai_request_id: Some("req-1".to_string()),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        db.with_conn(|conn| super::readiness(conn, &report.id))
            .unwrap()
            .can_finalize
    );
}

#[test]
fn a_finding_with_unconfirmed_severity_blocks_finalization() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);

    let unconfirmed = findings::create(
        &db,
        &actor,
        &NewFinding {
            title: "Weak cipher suites".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            finding_ids: Some(vec![unconfirmed.id.clone()]),
            ..Default::default()
        },
    )
    .unwrap();

    let readiness = db
        .with_conn(|conn| super::readiness(conn, &report.id))
        .unwrap();
    assert!(!readiness.can_finalize);
    assert!(readiness
        .issues
        .iter()
        .any(|issue| issue.code == "severity_unconfirmed"));

    findings::set_severity(
        &db,
        &actor,
        &unconfirmed.id,
        &SeverityDecision {
            severity: Severity::Medium,
            confirmed_by_user: true,
            rationale: String::new(),
        },
    )
    .unwrap();
    assert!(
        db.with_conn(|conn| super::readiness(conn, &report.id))
            .unwrap()
            .can_finalize
    );
}

#[test]
fn finalizing_requires_explicit_confirmation() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);

    let err = finalize(&db, &actor, &report.id, false, "").unwrap_err();
    assert!(
        err.to_string().contains("explicit confirmation"),
        "got {err}"
    );
    assert_eq!(
        db.with_conn(|conn| get(conn, &report.id)).unwrap().status,
        ReportStatus::Draft
    );
}

#[test]
fn finalizing_produces_an_immutable_v1_0() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);

    let final_version = finalize(&db, &actor, &report.id, true, "Signed off").unwrap();
    assert_eq!(final_version.version_label, "v1.0 Final");
    assert!(final_version.is_immutable);
    assert!(final_version.finalized_at.is_some());

    let reloaded = db.with_conn(|conn| get(conn, &report.id)).unwrap();
    assert_eq!(reloaded.status, ReportStatus::Final);
    assert!(reloaded.finalized_at.is_some());
}

#[test]
fn a_finalized_report_refuses_edits() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);
    finalize(&db, &actor, &report.id, true, "").unwrap();

    let err = save_draft(&db, &actor, &report.id, &ReportDraftUpdate::default()).unwrap_err();
    assert_eq!(err.kind(), "immutable");
    assert!(err.to_string().contains("new revision"), "got {err}");

    assert!(snapshot(&db, &actor, &report.id, "").is_err());
    assert!(submit_for_review(&db, &actor, &report.id).is_err());
    assert!(finalize(&db, &actor, &report.id, true, "").is_err());
    assert_eq!(
        delete(&db, &actor, &report.id).unwrap_err().kind(),
        "immutable"
    );
}

#[test]
fn the_database_refuses_to_rewrite_a_finalized_version_even_with_raw_sql() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);
    let final_version = finalize(&db, &actor, &report.id, true, "").unwrap();

    let update = db.with_conn(|conn| {
        conn.execute(
            "UPDATE report_versions SET content_json = '{}' WHERE id = ?1",
            [&final_version.id],
        )?;
        Ok(())
    });
    assert!(update.is_err(), "the trigger must reject the write");

    let delete_attempt = db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM report_versions WHERE id = ?1",
            [&final_version.id],
        )?;
        Ok(())
    });
    assert!(
        delete_attempt.is_err(),
        "the trigger must reject the delete"
    );

    let unchanged = db
        .with_conn(|conn| load_version(conn, &final_version.id))
        .unwrap();
    assert_eq!(unchanged.content_hash, final_version.content_hash);
}

#[test]
fn a_revision_reopens_editing_without_touching_the_finalized_version() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);
    let sealed = finalize(&db, &actor, &report.id, true, "").unwrap();

    let revision = create_revision(&db, &actor, &report.id).unwrap();
    assert_eq!(revision.version_label, "v1.1 Revised");
    assert!(!revision.is_immutable);
    assert_eq!(revision.status, ReportStatus::Draft);
    assert_eq!(
        revision
            .content
            .section("overview")
            .unwrap()
            .answer("es_context")
            .unwrap()
            .value,
        sealed
            .content
            .section("overview")
            .unwrap()
            .answer("es_context")
            .unwrap()
            .value,
        "a revision starts from the finalized content"
    );

    save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            answers: vec![AnswerInput {
                section_key: "overview".to_string(),
                question_key: "es_context".to_string(),
                value: "Corrected after client feedback.".to_string(),
                provenance: Provenance::UserProvided,
                ai_request_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();

    let sealed_again = db.with_conn(|conn| load_version(conn, &sealed.id)).unwrap();
    assert_ne!(
        sealed_again
            .content
            .section("overview")
            .unwrap()
            .answer("es_context")
            .unwrap()
            .value,
        "Corrected after client feedback.",
        "the finalized version must be untouched"
    );

    // Finalizing the revision moves to the next major.
    fill_required(&db, &actor, &report.id);
    let second = finalize(&db, &actor, &report.id, true, "").unwrap();
    assert_eq!(second.version_label, "v2.0 Final");
}

#[test]
fn a_revision_is_only_available_after_finalization() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    let err = create_revision(&db, &actor, &report.id).unwrap_err();
    assert!(err.to_string().contains("still editable"), "got {err}");
}

#[test]
fn changing_the_profile_does_not_rewrite_a_finalized_report() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    fill_required(&db, &actor, &report.id);
    let sealed = finalize(&db, &actor, &report.id, true, "").unwrap();
    assert_eq!(sealed.author.job_title, "Cloud Computing & Cybersecurity");

    crate::auth::update_profile(
        &db,
        &actor,
        &ProfileUpdate {
            job_title: Some("Chief Information Security Officer".to_string()),
            organization: Some("Somewhere Else Ltd".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let reloaded = db.with_conn(|conn| load_version(conn, &sealed.id)).unwrap();
    assert_eq!(reloaded.author.job_title, "Cloud Computing & Cybersecurity");
    assert_eq!(reloaded.author.organization, "PLH Ecosystem, Inc.");
    assert_eq!(reloaded.content.author.organization, "PLH Ecosystem, Inc.");

    // A new revision picks up the current identity, as it should.
    let revision = create_revision(&db, &actor, &report.id).unwrap();
    assert_eq!(
        revision.author.job_title,
        "Chief Information Security Officer"
    );
}

#[test]
fn findings_are_embedded_in_severity_order_and_survive_deletion() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("penetration_test")).unwrap();

    let low = confirmed_finding(&db, &actor, "Missing security header", Severity::Low);
    let critical = confirmed_finding(&db, &actor, "Unauthenticated RCE", Severity::Critical);
    let medium = confirmed_finding(&db, &actor, "Verbose error messages", Severity::Medium);

    let version = save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            finding_ids: Some(vec![low.clone(), critical.clone(), medium.clone()]),
            ..Default::default()
        },
    )
    .unwrap();

    let severities: Vec<Severity> = version
        .content
        .findings
        .iter()
        .map(|f| f.severity)
        .collect();
    assert_eq!(
        severities,
        vec![Severity::Critical, Severity::Medium, Severity::Low]
    );

    fill_required(&db, &actor, &report.id);
    let sealed = finalize(&db, &actor, &report.id, true, "").unwrap();

    findings::delete(&db, &actor, &critical).unwrap();
    let reloaded = db.with_conn(|conn| load_version(conn, &sealed.id)).unwrap();
    assert_eq!(
        reloaded.content.findings.len(),
        3,
        "the report keeps its own copy"
    );
    assert_eq!(reloaded.content.findings[0].title, "Unauthenticated RCE");
}

#[test]
fn editing_a_finding_does_not_change_a_finalized_report() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("penetration_test")).unwrap();
    let finding = confirmed_finding(&db, &actor, "Original title", Severity::High);
    save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            finding_ids: Some(vec![finding.clone()]),
            ..Default::default()
        },
    )
    .unwrap();
    fill_required(&db, &actor, &report.id);
    let sealed = finalize(&db, &actor, &report.id, true, "").unwrap();

    findings::update(
        &db,
        &actor,
        &finding,
        &FindingUpdate {
            title: Some("Retitled later".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let reloaded = db.with_conn(|conn| load_version(conn, &sealed.id)).unwrap();
    assert_eq!(reloaded.content.findings[0].title, "Original title");
}

#[test]
fn linking_a_finding_that_does_not_exist_is_rejected() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("penetration_test")).unwrap();
    let err = save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            finding_ids: Some(vec!["not-a-real-finding".to_string()]),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("does not exist"), "got {err}");
}

#[test]
fn version_history_is_ordered_newest_first_and_records_hashes() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    snapshot(&db, &actor, &report.id, "first").unwrap();
    fill_required(&db, &actor, &report.id);
    submit_for_review(&db, &actor, &report.id).unwrap();
    finalize(&db, &actor, &report.id, true, "signed").unwrap();

    let history = db.with_conn(|conn| versions(conn, &report.id)).unwrap();
    let labels: Vec<&str> = history.iter().map(|v| v.version_label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["v1.0 Final", "v0.3 Review", "v0.2 Draft", "v0.1 Draft"]
    );
    assert!(history.iter().all(|v| !v.content_hash.is_empty()));
    assert!(history
        .iter()
        .all(|v| v.author_name == "Spencer Rawlson Spady"));
    assert_eq!(history.iter().filter(|v| !v.is_immutable).count(), 0);
}

#[test]
fn listing_reports_shows_status_and_counts() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("penetration_test")).unwrap();
    let finding = confirmed_finding(&db, &actor, "Something", Severity::Low);
    save_draft(
        &db,
        &actor,
        &report.id,
        &ReportDraftUpdate {
            finding_ids: Some(vec![finding]),
            ..Default::default()
        },
    )
    .unwrap();

    let listed = db
        .with_conn(|conn| list(conn, &ReportFilter::default()))
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].finding_count, 1);
    assert_eq!(
        listed[0].current_version_label.as_deref(),
        Some("v0.1 Draft")
    );
    assert_eq!(listed[0].author_name, "Spencer Rawlson Spady");

    let none = db
        .with_conn(|conn| {
            list(
                conn,
                &ReportFilter {
                    status: Some(ReportStatus::Final),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert!(none.is_empty());
}

#[test]
fn a_draft_report_can_be_deleted() {
    let (db, actor) = seeded();
    let report = create(&db, &actor, &new_report("executive_summary")).unwrap();
    delete(&db, &actor, &report.id).unwrap();
    assert!(db.with_conn(|conn| get(conn, &report.id)).is_err());
}

#[test]
fn creating_a_report_from_an_unknown_template_fails_cleanly() {
    let (db, actor) = seeded();
    let err = create(&db, &actor, &new_report("no_such_template")).unwrap_err();
    assert_eq!(err.kind(), "not_found");
}
