//! Rendering a standalone postmortem as a professional document.
//!
//! A postmortem is a fixed question set, not a templated report, so to reuse the report
//! renderer, the exporters and the completeness standard we adapt it into a [`ReportContent`]
//! plus a synthetic [`ReportTemplate`]. The section keys deliberately match the canonical
//! postmortem sections in [`crate::reports::standards`], so the same four-page /
//! required-section standard governs a standalone postmortem and a postmortem report alike.

use crate::models::{
    AnswerValue, AuthorSnapshot, Classification, EngagementSnapshot, Postmortem, PostmortemStatus,
    ReportContent, ReportType, SectionContent, TemplateSource, Timestamp, CONTENT_SCHEMA_VERSION,
    POSTMORTEM_QUESTIONS,
};
use crate::pdf;
use crate::reports::standards::{self, Completeness};
use crate::templates::{InputKind, ReportTemplate, SectionKind, TemplateQuestion, TemplateSection};

const TEMPLATE_KEY: &str = "__postmortem_document";

/// The synthetic template's structure: (section key, title, required, question keys), in
/// reading order. The section keys match [`crate::reports::standards`]'s postmortem set.
const LAYOUT: &[(&str, &str, bool, &[&str])] = &[
    (
        "summary",
        "Summary",
        true,
        &["what_happened", "when_did_it_happen", "how_detected"],
    ),
    ("scope_impact", "Scope & Impact", true, &["systems_affected", "impact"]),
    ("timeline", "Timeline", false, &["timeline"]),
    ("analysis", "Analysis", true, &["root_cause", "actions_taken"]),
    (
        "reflection",
        "Reflection",
        true,
        &["what_went_well", "what_went_badly", "improvements"],
    ),
    (
        "corrective_actions",
        "Corrective Actions",
        true,
        &["corrective_actions", "action_owners", "deadlines", "verification"],
    ),
];

/// Extra synthetic question carrying the structured corrective-action register.
const REGISTER_KEY: &str = "corrective_actions_register";

fn prompt(key: &str) -> String {
    POSTMORTEM_QUESTIONS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, p)| (*p).to_string())
        .unwrap_or_else(|| key.replace('_', " "))
}

fn status_label(status: PostmortemStatus) -> &'static str {
    match status {
        PostmortemStatus::Draft => "Draft",
        PostmortemStatus::Review => "In review",
        PostmortemStatus::Complete => "Complete",
    }
}

/// The synthetic template that tells the renderer how each section is filled and the
/// completeness standard which sections are expected.
pub fn template() -> ReportTemplate {
    let mut sections = Vec::new();
    for (order, (key, title, required, qkeys)) in LAYOUT.iter().enumerate() {
        let mut questions: Vec<TemplateQuestion> = qkeys
            .iter()
            .map(|qk| TemplateQuestion {
                key: (*qk).to_string(),
                label: prompt(qk),
                help: String::new(),
                input: InputKind::Textarea,
                required: *required,
                placeholder: String::new(),
                columns: Vec::new(),
                row_label: String::new(),
            })
            .collect();
        if *key == "corrective_actions" {
            questions.push(TemplateQuestion {
                key: REGISTER_KEY.to_string(),
                label: "Corrective actions register".to_string(),
                help: String::new(),
                input: InputKind::Textarea,
                required: false,
                placeholder: String::new(),
                columns: Vec::new(),
                row_label: String::new(),
            });
        }
        sections.push(TemplateSection {
            key: (*key).to_string(),
            title: (*title).to_string(),
            order: order as i64,
            kind: SectionKind::Guided,
            required: *required,
            guidance: String::new(),
            questions,
        });
    }
    ReportTemplate {
        key: TEMPLATE_KEY.to_string(),
        name: "Postmortem".to_string(),
        report_type: ReportType::Postmortem,
        version: 1,
        description: String::new(),
        default_classification: Classification::Internal,
        sections,
        source: TemplateSource::Builtin,
    }
}

/// A human-readable register of the postmortem's corrective actions, or `None` when there
/// are none. Rendered as one guided answer so it appears in every export format.
fn register_text(pm: &Postmortem) -> Option<String> {
    if pm.corrective_actions.is_empty() {
        return None;
    }
    let mut blocks = Vec::new();
    for ca in &pm.corrective_actions {
        let owner = if ca.owner.trim().is_empty() {
            "unassigned"
        } else {
            ca.owner.trim()
        };
        let due = ca.due_date.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let verification = if ca.verification_method.trim().is_empty() {
            "not specified"
        } else {
            ca.verification_method.trim()
        };
        blocks.push(format!(
            "{} — {}\nOwner: {} · Due: {} · Status: {} · Verification: {}",
            ca.action_ref,
            ca.description.trim(),
            owner,
            due.unwrap_or("no date set"),
            ca.status.as_str().replace('_', " "),
            verification,
        ));
    }
    Some(blocks.join("\n\n"))
}

/// Adapt a postmortem into a self-contained report body attributed to `author`.
pub fn to_content(pm: &Postmortem, author: AuthorSnapshot) -> ReportContent {
    let mut sections = Vec::new();
    for (order, (key, title, _required, qkeys)) in LAYOUT.iter().enumerate() {
        let mut answers: Vec<AnswerValue> = Vec::new();
        for qk in qkeys.iter() {
            if let Some(answer) = pm.answers.get(*qk) {
                answers.push(answer.clone());
            }
        }
        if *key == "corrective_actions" {
            if let Some(text) = register_text(pm) {
                answers.push(AnswerValue::user_provided(REGISTER_KEY, text));
            }
        }
        sections.push(SectionContent {
            key: (*key).to_string(),
            title: (*title).to_string(),
            order: order as i64,
            included: true,
            answers,
        });
    }

    ReportContent {
        schema_version: CONTENT_SCHEMA_VERSION,
        template_key: TEMPLATE_KEY.to_string(),
        report_type: ReportType::Postmortem,
        title: pm.title.clone(),
        classification: Classification::Internal,
        author,
        engagement: EngagementSnapshot::default(),
        sections,
        findings: Vec::new(),
        evidence: Vec::new(),
        report_ref: pm.postmortem_ref.clone(),
        version_label: format!("Postmortem · {}", status_label(pm.status)),
        generated_at: Timestamp::now(),
    }
}

fn placeholder_author() -> AuthorSnapshot {
    AuthorSnapshot {
        user_id: String::new(),
        username: String::new(),
        full_name: String::new(),
        organization: String::new(),
        job_title: String::new(),
        captured_at: Timestamp::now(),
    }
}

/// Completeness of a standalone postmortem against the postmortem standard (four pages,
/// required sections). Author identity does not affect the result, so a placeholder is used.
pub fn completeness(pm: &Postmortem) -> Completeness {
    let content = to_content(pm, placeholder_author());
    let template = template();
    let pages = pdf::estimate_pages(&content, Some(&template), &pdf::RenderOptions::default());
    standards::evaluate(&content, Some(&template), pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewPostmortem, PostmortemStatus};
    use crate::postmortems;
    use crate::reports::standards::CompletenessStatus;
    use crate::test_support::seeded;

    #[test]
    fn an_empty_postmortem_is_incomplete_with_missing_sections() {
        let (db, actor) = seeded();
        let pm = postmortems::create(
            &db,
            &actor,
            &NewPostmortem {
                title: "Outage".into(),
                incident_id: None,
                engagement_id: None,
            },
        )
        .unwrap();
        let c = completeness(&pm);
        assert_eq!(c.status, CompletenessStatus::Incomplete);
        assert_eq!(c.min_pages, 4);
        assert!(!c.missing_sections.is_empty());
        assert!(c.percent < 100);
    }

    #[test]
    fn a_fully_answered_postmortem_has_every_required_section() {
        let (db, actor) = seeded();
        let pm = postmortems::create(
            &db,
            &actor,
            &NewPostmortem {
                title: "Outage".into(),
                incident_id: None,
                engagement_id: None,
            },
        )
        .unwrap();
        let answers = POSTMORTEM_QUESTIONS
            .iter()
            .map(|(key, _)| postmortems::PostmortemAnswerInput {
                question_key: (*key).to_string(),
                value: "A full, substantive answer for the record.".to_string(),
                provenance: crate::models::Provenance::UserProvided,
                ai_request_id: None,
            })
            .collect();
        postmortems::update(
            &db,
            &actor,
            &pm.id,
            &postmortems::PostmortemUpdate {
                status: Some(PostmortemStatus::Complete),
                answers,
                ..Default::default()
            },
        )
        .unwrap();
        let pm = db.with_conn(|conn| postmortems::get(conn, &pm.id)).unwrap();

        let c = completeness(&pm);
        assert!(
            c.missing_sections.is_empty(),
            "all required sections should be satisfied: {c:?}"
        );
        assert_eq!(c.percent, 100);
        // A short but complete postmortem is flagged for review, never rejected.
        assert_ne!(c.status, CompletenessStatus::Incomplete);

        // The adapted content renders without panicking and produces a real document.
        let content = to_content(&pm, placeholder_author());
        let bytes = pdf::render(
            &content,
            Some(&template()),
            &pdf::RenderOptions::default(),
            &|_| None,
        )
        .unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }
}
