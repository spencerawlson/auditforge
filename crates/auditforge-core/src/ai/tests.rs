use super::provider::ScriptedProvider;
use super::*;
use crate::models::{AiSettings, DiffKind, SummaryKind};
use crate::test_support::seeded;
use serde_json::json;

fn settings(provider: AiProviderKind) -> AiSettings {
    AiSettings {
        provider,
        redact_before_send: true,
        ..Default::default()
    }
}

fn service_returning(body: serde_json::Value) -> AiService {
    AiService::new(
        Box::new(ScriptedProvider::always(body.to_string())),
        settings(AiProviderKind::Local),
    )
}

fn text_request(function: AiFunction, text: &str) -> AiTextRequest {
    AiTextRequest {
        function,
        text: text.to_string(),
        summary_kind: None,
        context: String::new(),
        target_type: "finding".to_string(),
        target_id: "f-1".to_string(),
    }
}

#[test]
fn ai_is_off_by_default_and_says_so() {
    let service = AiService::from_settings(&AiSettings::default(), None);
    let status = service.status();

    assert_eq!(status.provider, AiProviderKind::Disabled);
    assert!(!status.enabled);
    assert!(!status.leaves_machine);
    assert_eq!(status.banner, "AI Processing: Disabled / Offline");

    let (db, actor) = seeded();
    let err = service
        .suggest(
            &db,
            &actor,
            &text_request(AiFunction::ImproveWriting, "some text to improve"),
        )
        .unwrap_err();
    assert_eq!(err.kind(), "ai_policy");
}

#[test]
fn selecting_openai_without_consent_does_not_produce_a_cloud_provider() {
    let settings = AiSettings {
        provider: AiProviderKind::OpenAi,
        ..Default::default()
    };
    let service = AiService::from_settings(&settings, Some("sk-test".to_string()));
    let status = service.status();

    assert_eq!(
        status.provider,
        AiProviderKind::Disabled,
        "consent gates the cloud provider"
    );
    assert!(!status.leaves_machine);
}

#[test]
fn consent_and_a_key_together_produce_a_cloud_provider() {
    let settings = AiSettings {
        provider: AiProviderKind::OpenAi,
        cloud_consent: true,
        ..Default::default()
    };
    let service = AiService::from_settings(&settings, Some("sk-test".to_string()));
    let status = service.status();

    assert_eq!(status.provider, AiProviderKind::OpenAi);
    assert_eq!(status.privacy_mode, PrivacyMode::Cloud);
    assert!(status.leaves_machine);
    assert_eq!(status.banner, "AI Processing: OpenAI Cloud");
    assert!(status.destination.contains("OpenAI"));
}

#[test]
fn a_suggestion_is_returned_with_a_diff_and_is_never_applied() {
    let (db, actor) = seeded();
    let service = service_returning(json!({
        "suggestion": "The host was running an unsupported TLS version.",
        "confidence": "high",
        "missing_information": [],
        "notes": []
    }));

    let original = "the box was runing an old tls verison";
    let suggestion = service
        .suggest(
            &db,
            &actor,
            &text_request(AiFunction::ImproveWriting, original),
        )
        .unwrap();

    assert_eq!(
        suggestion.original, original,
        "the original must be returned for comparison"
    );
    assert_eq!(
        suggestion.suggestion,
        "The host was running an unsupported TLS version."
    );
    assert_eq!(suggestion.confidence, AiConfidence::High);
    assert_eq!(suggestion.provenance, Provenance::AiWording);
    assert!(suggestion
        .diff
        .iter()
        .any(|segment| segment.kind == DiffKind::Insert));
    assert!(suggestion
        .diff
        .iter()
        .any(|segment| segment.kind == DiffKind::Delete));

    // Nothing was written anywhere except the request log.
    let requests = db
        .with_conn(|conn| requests_for_target(conn, "finding", "f-1", 10))
        .unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["status"], "succeeded");
    assert_eq!(requests[0]["decision"], serde_json::Value::Null);
}

#[test]
fn a_suggestion_that_invents_an_identifier_is_refused() {
    let (db, actor) = seeded();
    let service = service_returning(json!({
        "suggestion": "The host at 10.0.0.7 is affected by CVE-2024-6387.",
        "confidence": "high"
    }));

    let err = service
        .suggest(
            &db,
            &actor,
            &text_request(
                AiFunction::ImproveWriting,
                "the host was running an old sshd build",
            ),
        )
        .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("10.0.0.7"), "got {message}");
    assert!(message.contains("CVE-2024-6387"), "got {message}");

    let requests = db
        .with_conn(|conn| requests_for_target(conn, "finding", "f-1", 10))
        .unwrap();
    assert_eq!(requests[0]["status"], "blocked");

    let blocked = db
        .with_conn(|conn| {
            audit::query(
                conn,
                &crate::models::AuditQuery {
                    action: Some(AuditAction::AiRequestBlocked),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].detail["reason"], "fabricated_identifiers");
}

#[test]
fn facts_already_in_the_text_are_allowed_through() {
    let (db, actor) = seeded();
    let service = service_returning(json!({
        "suggestion": "The host at 10.0.0.7 is affected by CVE-2024-6387.",
        "confidence": "high"
    }));

    let suggestion = service
        .suggest(
            &db,
            &actor,
            &text_request(
                AiFunction::ImproveWriting,
                "10.0.0.7 is vulnerable to CVE-2024-6387 (regreSSHion)",
            ),
        )
        .unwrap();
    assert!(suggestion.suggestion.contains("10.0.0.7"));
}

#[test]
fn a_fact_supplied_in_the_context_line_is_not_treated_as_invented() {
    let (db, actor) = seeded();
    let service = service_returning(json!({
        "suggestion": "The finding concerns vpn.example.internal at 203.0.113.9.",
        "confidence": "medium"
    }));

    let mut request = text_request(AiFunction::Rewrite, "this box has a weak configuration");
    request.context = "Finding affects 203.0.113.9".to_string();
    let suggestion = service.suggest(&db, &actor, &request).unwrap();
    assert!(suggestion.suggestion.contains("203.0.113.9"));
}

#[test]
fn redaction_hides_values_from_the_provider_and_restores_them_afterwards() {
    let (db, actor) = seeded();

    // A provider that echoes back what it was sent, so the test can inspect it.
    struct EchoProvider {
        seen: std::sync::Mutex<String>,
    }
    impl provider::AiProvider for EchoProvider {
        fn kind(&self) -> AiProviderKind {
            AiProviderKind::Local
        }
        fn model(&self) -> String {
            "echo".to_string()
        }
        fn destination(&self) -> String {
            "test".to_string()
        }
        fn complete(
            &self,
            request: &provider::ProviderRequest,
        ) -> crate::error::Result<provider::ProviderResponse> {
            *self.seen.lock().unwrap() = request.user.clone();
            // Reply using the placeholder, as a real model would.
            let start = request
                .user
                .find("[REDACTED_IP_1]")
                .map(|_| "[REDACTED_IP_1]");
            let body = json!({
                "suggestion": format!("The host {} did not respond.", start.unwrap_or("?")),
                "confidence": "medium"
            });
            Ok(provider::ProviderResponse {
                raw: body.to_string(),
                model: "echo".into(),
            })
        }
    }

    let echo = std::sync::Arc::new(EchoProvider {
        seen: std::sync::Mutex::new(String::new()),
    });
    struct Wrapper(std::sync::Arc<EchoProvider>);
    impl provider::AiProvider for Wrapper {
        fn kind(&self) -> AiProviderKind {
            self.0.kind()
        }
        fn model(&self) -> String {
            self.0.model()
        }
        fn destination(&self) -> String {
            self.0.destination()
        }
        fn complete(
            &self,
            request: &provider::ProviderRequest,
        ) -> crate::error::Result<provider::ProviderResponse> {
            self.0.complete(request)
        }
    }

    let service = AiService::new(
        Box::new(Wrapper(echo.clone())),
        settings(AiProviderKind::Local),
    );
    let suggestion = service
        .suggest(
            &db,
            &actor,
            &text_request(
                AiFunction::ImproveWriting,
                "the host 198.51.100.23 did not respond",
            ),
        )
        .unwrap();

    let sent = echo.seen.lock().unwrap().clone();
    assert!(
        !sent.contains("198.51.100.23"),
        "the address must not reach the provider"
    );
    assert!(sent.contains("[REDACTED_IP_1]"));
    assert!(
        suggestion.suggestion.contains("198.51.100.23"),
        "the author must see the real value back: {}",
        suggestion.suggestion
    );
}

#[test]
fn expanding_notes_produces_an_inference_that_the_author_must_confirm() {
    let (db, actor) = seeded();
    let service = service_returning(json!({
        "suggestion": "The scan identified an outdated service. Information required: the exact \
                       version was not recorded.",
        "confidence": "low",
        "missing_information": ["exact service version"],
        "notes": ["expanded from three words of notes"]
    }));

    let suggestion = service
        .suggest(
            &db,
            &actor,
            &text_request(AiFunction::Expand, "outdated service found on scan"),
        )
        .unwrap();

    assert_eq!(
        suggestion.provenance,
        Provenance::AiInference,
        "expanded prose is not the author's assertion until they confirm it"
    );
    assert_eq!(
        suggestion.missing_information,
        vec!["exact service version"]
    );
    assert_eq!(suggestion.confidence, AiConfidence::Low);
}

#[test]
fn rewording_functions_produce_authoritative_wording() {
    for function in [
        AiFunction::ImproveWriting,
        AiFunction::Rewrite,
        AiFunction::TechnicalRewrite,
        AiFunction::ExecutiveRewrite,
        AiFunction::Summarize,
    ] {
        assert_eq!(
            proposed_provenance(function),
            Provenance::AiWording,
            "{function}"
        );
        assert!(proposed_provenance(function).is_authoritative());
    }
    for function in [
        AiFunction::Expand,
        AiFunction::ExecutiveSummary,
        AiFunction::FindingAnalysis,
    ] {
        assert_eq!(
            proposed_provenance(function),
            Provenance::AiInference,
            "{function}"
        );
    }
}

#[test]
fn a_summary_request_carries_its_audience() {
    let (db, actor) = seeded();
    let service =
        service_returning(json!({ "suggestion": "Short summary.", "confidence": "medium" }));
    let mut request = text_request(AiFunction::Summarize, "a long technical passage about tls");
    request.summary_kind = Some(SummaryKind::Executive);
    let suggestion = service.suggest(&db, &actor, &request).unwrap();
    assert_eq!(suggestion.function, AiFunction::Summarize);
    assert_eq!(suggestion.provenance, Provenance::AiWording);
}

#[test]
fn very_short_input_is_refused_before_any_call() {
    let (db, actor) = seeded();
    let service = service_returning(json!({ "suggestion": "x", "confidence": "high" }));
    let err = service
        .suggest(
            &db,
            &actor,
            &text_request(AiFunction::ImproveWriting, "too short"),
        )
        .unwrap_err();
    assert_eq!(err.kind(), "validation");

    let requests = db
        .with_conn(|conn| requests_for_target(conn, "finding", "f-1", 10))
        .unwrap();
    assert!(
        requests.is_empty(),
        "nothing should be logged for a request that never happened"
    );
}

#[test]
fn malformed_provider_output_is_recorded_as_a_failure() {
    let (db, actor) = seeded();
    let service = AiService::new(
        Box::new(ScriptedProvider::always("I'm afraid I can't do that")),
        settings(AiProviderKind::Local),
    );
    let err = service
        .suggest(
            &db,
            &actor,
            &text_request(AiFunction::Rewrite, "some text worth rewriting"),
        )
        .unwrap_err();
    assert_eq!(err.kind(), "ai_provider");

    let requests = db
        .with_conn(|conn| requests_for_target(conn, "finding", "f-1", 10))
        .unwrap();
    assert_eq!(requests[0]["status"], "failed");
}

#[test]
fn decisions_are_recorded_against_the_request() {
    let (db, actor) = seeded();
    let service =
        service_returning(json!({ "suggestion": "Improved text here.", "confidence": "high" }));
    let suggestion = service
        .suggest(
            &db,
            &actor,
            &text_request(AiFunction::ImproveWriting, "text that needs help"),
        )
        .unwrap();

    record_decision(&db, &actor, &suggestion.request_id, AiDecision::Accepted).unwrap();

    let requests = db
        .with_conn(|conn| requests_for_target(conn, "finding", "f-1", 10))
        .unwrap();
    assert_eq!(requests[0]["decision"], "accepted");

    let entries = db
        .with_conn(|conn| {
            audit::query(
                conn,
                &crate::models::AuditQuery {
                    action: Some(AuditAction::AiSuggestionAccepted),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(entries.len(), 1);

    assert_eq!(
        record_decision(&db, &actor, "no-such-request", AiDecision::Rejected)
            .unwrap_err()
            .kind(),
        "not_found"
    );
}

#[test]
fn a_review_returns_warnings_and_changes_nothing() {
    let (db, actor) = seeded();
    let service = service_returning(json!({
        "summary": "Two problems worth fixing.",
        "confidence": "medium",
        "warnings": [
            {"category": "contradiction", "message": "The scope says two hosts, the findings cover three.",
             "quote": "Scope was limited to two hosts."},
            {"category": "unsupported_claim", "message": "No evidence supports this.",
             "quote": "A statement that is not in the report at all."}
        ]
    }));

    let mut content = sample_content();
    content.sections[0].answers[0].value = "Scope was limited to two hosts.".to_string();

    let review = service
        .review_report(&db, &actor, "report-1", &content)
        .unwrap();
    assert_eq!(review.summary, "Two problems worth fixing.");
    assert_eq!(review.warnings.len(), 2);
    assert!(review.warnings[0].quote.is_some());
    assert!(
        review.warnings[1].quote.is_none(),
        "a quote the report does not contain must be dropped"
    );

    let entries = db
        .with_conn(|conn| {
            audit::query(
                conn,
                &crate::models::AuditQuery {
                    action: Some(AuditAction::AiReviewRun),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(entries.len(), 1);
}

#[test]
fn assembled_review_text_omits_excluded_and_empty_sections() {
    let mut content = sample_content();
    content.sections[0].answers[0].value = "Included text.".to_string();
    content.sections.push(crate::models::SectionContent {
        key: "excluded".to_string(),
        title: "Excluded Section".to_string(),
        order: 2,
        included: false,
        answers: vec![crate::models::AnswerValue::user_provided(
            "k",
            "Secret excluded text.",
        )],
    });

    let assembled = assemble_report_text(&content);
    assert!(assembled.contains("Included text."));
    assert!(!assembled.contains("Secret excluded text."));
    assert!(assembled.contains("TITLE: Sample Report"));
}

#[test]
fn assembled_review_text_is_capped() {
    let mut content = sample_content();
    content.sections[0].answers[0].value = "word ".repeat(20_000);
    let assembled = assemble_report_text(&content);
    assert!(assembled.chars().count() <= MAX_REVIEW_CHARS + 32);
}

fn sample_content() -> ReportContent {
    ReportContent {
        schema_version: crate::models::CONTENT_SCHEMA_VERSION,
        template_key: "custom_report".to_string(),
        report_type: crate::models::ReportType::Custom,
        title: "Sample Report".to_string(),
        classification: crate::models::Classification::Internal,
        author: crate::models::AuthorSnapshot {
            user_id: "u1".to_string(),
            username: "analyst".to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            captured_at: Timestamp::now(),
        },
        engagement: crate::models::EngagementSnapshot::default(),
        sections: vec![crate::models::SectionContent {
            key: "scope".to_string(),
            title: "Scope".to_string(),
            order: 1,
            included: true,
            answers: vec![crate::models::AnswerValue::user_provided("scope_in", "")],
        }],
        findings: Vec::new(),
        evidence: Vec::new(),
        report_ref: "RPT-2026-0001".to_string(),
        version_label: "v0.1 Draft".to_string(),
        generated_at: Timestamp::now(),
    }
}
