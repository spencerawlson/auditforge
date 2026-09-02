//! Professional documentation standards: minimum substantive length and required-section
//! completeness, evaluated per report kind.
//!
//! # The rule this module encodes
//!
//! Required content completeness outranks page count. A report that contains every
//! required section but is short is *flagged for review* ([`CompletenessStatus::RequiresReview`]),
//! never rejected — some reports are legitimately concise. The page minimum is a target
//! the author is shown, not a gate that blocks finalization. Only a genuinely missing
//! required section makes a report [`CompletenessStatus::Incomplete`], and that mirrors the
//! blockers `readiness` already raises for unanswered required questions; this layer adds a
//! human-readable completeness picture on top, it does not invent new reasons to refuse a
//! finalize.
//!
//! # Where the required set comes from
//!
//! For each report kind we keep a canonical list of the sections a professional document of
//! that kind is expected to carry (the "standard"). At evaluation time that list is:
//!
//! * intersected with the template actually in use — a canonical section the template does
//!   not define is simply not applicable ("where applicable"), and
//! * unioned with every section the template itself marks `required`,
//!
//! so type-specific required sections (a compliance report's *Criteria*, an incident
//! report's *Affected Systems*) are always counted without hand-maintaining a list per
//! template.

use serde::Serialize;

use crate::models::{ReportContent, ReportType};
use crate::templates::{ReportTemplate, SectionKind};

/// The completeness verdict for a report, in the vocabulary the UI shows the author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessStatus {
    /// Every required section has content and the length target is met.
    Complete,
    /// One or more required sections have no content yet.
    Incomplete,
    /// All required sections are present, but the document is shorter than the target for
    /// its kind. Flagged for a human to confirm it is genuinely complete — not rejected.
    RequiresReview,
}

impl CompletenessStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CompletenessStatus::Complete => "complete",
            CompletenessStatus::Incomplete => "incomplete",
            CompletenessStatus::RequiresReview => "requires_review",
        }
    }
}

/// The standard applied to one family of report kinds.
#[derive(Debug, Clone, Copy)]
pub struct ReportStandard {
    /// Human label for the family, e.g. `"Engagement report"`.
    pub label: &'static str,
    /// The professional minimum substantive length, in pages, for this kind.
    pub min_pages: usize,
    /// Canonical section keys a report of this kind should carry, in reading order. Only
    /// those a given template defines are evaluated; see the module docs.
    pub canonical_sections: &'static [&'static str],
}

/// Substantive assessment, audit and incident reports: at least five pages.
const ENGAGEMENT: ReportStandard = ReportStandard {
    label: "Engagement report",
    min_pages: 5,
    canonical_sections: &[
        "executive_summary",
        "scope",
        "objectives",
        "methodology",
        "findings",
        "risk_analysis",
        "recommendations",
        "conclusion",
    ],
};

/// A deliberately brief, leadership-facing summary: at least two pages.
const EXECUTIVE: ReportStandard = ReportStandard {
    label: "Executive summary",
    min_pages: 2,
    canonical_sections: &[
        "overview",
        "key_findings",
        "business_impact",
        "next_steps",
    ],
};

/// A blameless postmortem: at least four pages.
const POSTMORTEM: ReportStandard = ReportStandard {
    label: "Postmortem report",
    min_pages: 4,
    canonical_sections: &[
        "summary",
        "scope_impact",
        "timeline",
        "analysis",
        "reflection",
        "corrective_actions",
    ],
};

/// The standard that governs a given report type.
pub fn standard_for(report_type: ReportType) -> &'static ReportStandard {
    match report_type {
        ReportType::ExecutiveSummary => &EXECUTIVE,
        ReportType::Postmortem => &POSTMORTEM,
        _ => &ENGAGEMENT,
    }
}

/// The minimum substantive page count for a report type.
pub fn min_pages_for(report_type: ReportType) -> usize {
    standard_for(report_type).min_pages
}

/// One required section and whether the report currently satisfies it.
#[derive(Debug, Clone, Serialize)]
pub struct SectionRequirement {
    pub key: String,
    pub label: String,
    pub present: bool,
}

/// The full completeness picture for one report version.
#[derive(Debug, Clone, Serialize)]
pub struct Completeness {
    pub status: CompletenessStatus,
    /// Proportion of required sections that have content, 0..=100. This is the "92% Complete"
    /// figure and is driven by *content*, not by page count.
    pub percent: u8,
    /// The professional minimum length for this kind, in pages.
    pub min_pages: usize,
    /// Estimated rendered length. A text-only lower bound — embedded evidence only adds to it.
    pub estimated_pages: usize,
    /// Whether the estimate meets the minimum. `false` never blocks finalization.
    pub meets_page_target: bool,
    /// Every required section, in reading order, with its present/absent state.
    pub required_sections: Vec<SectionRequirement>,
    /// Labels of the required sections that still have no content.
    pub missing_sections: Vec<String>,
    /// Human label for the standard, e.g. `"Engagement report"`.
    pub standard_label: String,
}

/// Does the report carry real content for `key`, given the template's notion of that
/// section's kind? Guided sections need a non-blank answer; findings/evidence sections
/// need at least one linked record.
fn section_present(content: &ReportContent, template: Option<&ReportTemplate>, key: &str) -> bool {
    let Some(section) = content.section(key) else {
        return false;
    };
    if !section.included {
        return false;
    }
    let kind = template
        .and_then(|t| t.section(key))
        .map(|s| s.kind)
        .unwrap_or(SectionKind::Guided);
    match kind {
        SectionKind::Guided => section.has_content(),
        SectionKind::Findings => !content.findings.is_empty(),
        SectionKind::Evidence => !content.evidence.is_empty(),
    }
}

/// The human title for a section key, preferring the template, then the report content.
fn section_label(content: &ReportContent, template: Option<&ReportTemplate>, key: &str) -> String {
    template
        .and_then(|t| t.section(key))
        .map(|s| s.title.clone())
        .or_else(|| content.section(key).map(|s| s.title.clone()))
        .unwrap_or_else(|| key.replace('_', " "))
}

/// Compute the completeness of a report version.
///
/// `estimated_pages` is supplied by the caller (via [`crate::pdf::estimate_pages`]) so this
/// function stays free of the PDF layer and is trivial to unit-test.
pub fn evaluate(
    content: &ReportContent,
    template: Option<&ReportTemplate>,
    estimated_pages: usize,
) -> Completeness {
    let standard = standard_for(content.report_type);

    // Build the required set: canonical sections the template actually defines, plus every
    // section the template marks required. Order follows the canonical list first, then any
    // extra template-required sections in template order, and duplicates are dropped.
    let mut keys: Vec<String> = Vec::new();
    for key in standard.canonical_sections {
        if content.section(key).is_some() && !keys.iter().any(|k| k == key) {
            keys.push((*key).to_string());
        }
    }
    if let Some(template) = template {
        for section in &template.sections {
            if section.required && !keys.iter().any(|k| k == &section.key) {
                keys.push(section.key.clone());
            }
        }
    }

    let required_sections: Vec<SectionRequirement> = keys
        .iter()
        .map(|key| SectionRequirement {
            key: key.clone(),
            label: section_label(content, template, key),
            present: section_present(content, template, key),
        })
        .collect();

    let missing_sections: Vec<String> = required_sections
        .iter()
        .filter(|s| !s.present)
        .map(|s| s.label.clone())
        .collect();

    let total = required_sections.len();
    let present = required_sections.iter().filter(|s| s.present).count();
    let percent = if total == 0 {
        100
    } else {
        ((present as f64 / total as f64) * 100.0).round() as u8
    };

    let meets_page_target = estimated_pages >= standard.min_pages;
    let status = if !missing_sections.is_empty() {
        CompletenessStatus::Incomplete
    } else if !meets_page_target {
        CompletenessStatus::RequiresReview
    } else {
        CompletenessStatus::Complete
    };

    Completeness {
        status,
        percent,
        min_pages: standard.min_pages,
        estimated_pages,
        meets_page_target,
        required_sections,
        missing_sections,
        standard_label: standard.label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AuthorSnapshot, Classification, EngagementSnapshot, SectionContent, Timestamp,
    };

    fn author() -> AuthorSnapshot {
        AuthorSnapshot {
            user_id: "u1".into(),
            username: "tester".into(),
            full_name: "Tester".into(),
            organization: "Org".into(),
            job_title: "Analyst".into(),
            captured_at: Timestamp::now(),
        }
    }

    fn content_with(report_type: ReportType, sections: Vec<SectionContent>) -> ReportContent {
        ReportContent {
            schema_version: crate::models::CONTENT_SCHEMA_VERSION,
            template_key: "t".into(),
            report_type,
            title: "T".into(),
            classification: Classification::Internal,
            author: author(),
            engagement: EngagementSnapshot::default(),
            sections,
            findings: Vec::new(),
            evidence: Vec::new(),
            report_ref: "RPT-1".into(),
            version_label: "v0.1 Draft".into(),
            generated_at: Timestamp::now(),
        }
    }

    fn guided(key: &str, filled: bool) -> SectionContent {
        use crate::models::AnswerValue;
        let answers = if filled {
            vec![AnswerValue::user_provided("q", "Substantive content.")]
        } else {
            Vec::new()
        };
        SectionContent {
            key: key.into(),
            title: key.replace('_', " "),
            order: 0,
            included: true,
            answers,
        }
    }

    #[test]
    fn min_pages_follow_the_report_kind() {
        assert_eq!(min_pages_for(ReportType::PenetrationTest), 5);
        assert_eq!(min_pages_for(ReportType::SecurityAssessment), 5);
        assert_eq!(min_pages_for(ReportType::Postmortem), 4);
        assert_eq!(min_pages_for(ReportType::ExecutiveSummary), 2);
    }

    #[test]
    fn all_sections_present_and_long_enough_is_complete() {
        let sections = ENGAGEMENT
            .canonical_sections
            .iter()
            .map(|k| guided(k, true))
            .collect();
        let content = content_with(ReportType::SecurityAssessment, sections);
        let c = evaluate(&content, None, 6);
        assert_eq!(c.status, CompletenessStatus::Complete);
        assert_eq!(c.percent, 100);
        assert!(c.missing_sections.is_empty());
        assert!(c.meets_page_target);
    }

    #[test]
    fn complete_content_but_short_requires_review_not_rejection() {
        let sections = ENGAGEMENT
            .canonical_sections
            .iter()
            .map(|k| guided(k, true))
            .collect();
        let content = content_with(ReportType::SecurityAssessment, sections);
        let c = evaluate(&content, None, 3);
        assert_eq!(c.status, CompletenessStatus::RequiresReview);
        assert_eq!(c.percent, 100, "page count must not lower the content percentage");
        assert!(!c.meets_page_target);
        assert!(c.missing_sections.is_empty());
    }

    #[test]
    fn a_missing_required_section_is_incomplete_and_listed() {
        let mut sections: Vec<SectionContent> = ENGAGEMENT
            .canonical_sections
            .iter()
            .map(|k| guided(k, true))
            .collect();
        // Blank out the conclusion.
        *sections.last_mut().unwrap() = guided("conclusion", false);
        let content = content_with(ReportType::SecurityAssessment, sections);
        let c = evaluate(&content, None, 6);
        assert_eq!(c.status, CompletenessStatus::Incomplete);
        assert!(c.percent < 100);
        assert!(c.missing_sections.iter().any(|s| s.contains("conclusion")));
    }

    #[test]
    fn findings_section_needs_a_linked_finding_to_count() {
        // executive_summary + findings, findings section present but no findings linked.
        let content = content_with(
            ReportType::SecurityAssessment,
            vec![guided("executive_summary", true), guided("findings", true)],
        );
        // Without a template, "findings" is treated as guided; give it its own template so
        // its kind is Findings and the emptiness of content.findings is what matters.
        let template = ReportTemplate {
            key: "t".into(),
            name: "T".into(),
            report_type: ReportType::SecurityAssessment,
            version: 1,
            description: String::new(),
            default_classification: Classification::Internal,
            source: crate::models::TemplateSource::Builtin,
            sections: vec![
                crate::templates::TemplateSection {
                    key: "executive_summary".into(),
                    title: "Executive Summary".into(),
                    order: 0,
                    kind: SectionKind::Guided,
                    required: true,
                    guidance: String::new(),
                    questions: vec![],
                },
                crate::templates::TemplateSection {
                    key: "findings".into(),
                    title: "Findings".into(),
                    order: 1,
                    kind: SectionKind::Findings,
                    required: false,
                    guidance: String::new(),
                    questions: vec![],
                },
            ],
        };
        let c = evaluate(&content, Some(&template), 6);
        assert!(
            c.missing_sections.iter().any(|s| s == "Findings"),
            "a findings section with no findings must read as missing"
        );
    }
}
