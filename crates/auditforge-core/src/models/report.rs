//! Reports, their immutable version history, and the document content they carry.

use serde::{Deserialize, Serialize};

use super::common::{
    AnswerValue, AuthorSnapshot, Classification, Provenance, ReportStatus, ReportType, Timestamp,
};
use super::engagement::EngagementSnapshot;
use super::evidence::EvidenceSnapshot;
use super::finding::FindingSnapshot;

/// Bumped whenever `ReportContent` gains a field that older stored versions will not
/// have. Deserialization stays backward compatible through `#[serde(default)]`; this
/// number tells the renderer which shape it is looking at.
pub const CONTENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    /// Human-facing identifier, e.g. `RPT-2026-0011`.
    pub report_ref: String,
    pub engagement_id: Option<String>,
    pub template_key: String,
    pub report_type: ReportType,
    pub title: String,
    pub classification: Classification,
    pub status: ReportStatus,
    pub current_version_id: Option<String>,
    pub created_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub finalized_at: Option<Timestamp>,
}

/// List-view shape: enough to render a row without loading document content.
#[derive(Debug, Clone, Serialize)]
pub struct ReportSummary {
    #[serde(flatten)]
    pub report: Report,
    pub engagement_name: Option<String>,
    pub current_version_label: Option<String>,
    pub author_name: String,
    pub finding_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportVersion {
    pub id: String,
    pub report_id: String,
    pub version_major: i64,
    pub version_minor: i64,
    /// Display label, e.g. `v1.0 Final`.
    pub version_label: String,
    pub status: ReportStatus,
    pub content: ReportContent,
    /// Author identity frozen when this version was written.
    pub author: AuthorSnapshot,
    pub content_hash: String,
    pub notes: String,
    pub is_immutable: bool,
    pub created_by: String,
    pub created_at: Timestamp,
    pub finalized_at: Option<Timestamp>,
}

/// Version history row without the document body.
#[derive(Debug, Clone, Serialize)]
pub struct ReportVersionSummary {
    pub id: String,
    pub report_id: String,
    pub version_label: String,
    pub status: ReportStatus,
    pub is_immutable: bool,
    pub notes: String,
    pub author_name: String,
    pub content_hash: String,
    pub created_at: Timestamp,
    pub finalized_at: Option<Timestamp>,
}

/// Build the display label for a version.
///
/// Draft numbering lives in the `0.x` range until the first finalization; finalizing
/// bumps the major and resets the minor, and a revision of a finalized report continues
/// in that major as `Revised`.
pub fn version_label(major: i64, minor: i64, status: ReportStatus) -> String {
    let suffix = match status {
        ReportStatus::Final => "Final",
        ReportStatus::Review => "Review",
        ReportStatus::Archived => "Archived",
        ReportStatus::Draft if major >= 1 => "Revised",
        ReportStatus::Draft => "Draft",
    };
    format!("v{major}.{minor} {suffix}")
}

/// The complete document body for one version. Everything the renderer needs is here:
/// a finalized version can be re-rendered years later without touching any other table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportContent {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub template_key: String,
    pub report_type: ReportType,
    pub title: String,
    pub classification: Classification,
    pub author: AuthorSnapshot,
    #[serde(default)]
    pub engagement: EngagementSnapshot,
    #[serde(default)]
    pub sections: Vec<SectionContent>,
    #[serde(default)]
    pub findings: Vec<FindingSnapshot>,
    #[serde(default)]
    pub evidence: Vec<EvidenceSnapshot>,
    #[serde(default)]
    pub report_ref: String,
    #[serde(default)]
    pub version_label: String,
    pub generated_at: Timestamp,
}

fn default_schema_version() -> u32 {
    CONTENT_SCHEMA_VERSION
}

impl ReportContent {
    pub fn section(&self, key: &str) -> Option<&SectionContent> {
        self.sections.iter().find(|section| section.key == key)
    }

    pub fn section_mut(&mut self, key: &str) -> Option<&mut SectionContent> {
        self.sections.iter_mut().find(|section| section.key == key)
    }

    /// Every answer that is blank or explicitly marked as missing.
    pub fn missing_answers(&self) -> Vec<(&str, &str)> {
        let mut out = Vec::new();
        for section in self.sections.iter().filter(|s| s.included) {
            for answer in &section.answers {
                if answer.is_blank() || answer.provenance == Provenance::MissingInformation {
                    out.push((section.key.as_str(), answer.question_key.as_str()));
                }
            }
        }
        out
    }

    /// Answers whose text is an unconfirmed AI inference. These block finalization.
    pub fn unconfirmed_inferences(&self) -> Vec<(&str, &str)> {
        let mut out = Vec::new();
        for section in self.sections.iter().filter(|s| s.included) {
            for answer in &section.answers {
                if answer.provenance == Provenance::AiInference {
                    out.push((section.key.as_str(), answer.question_key.as_str()));
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionContent {
    pub key: String,
    pub title: String,
    pub order: i64,
    /// Sections the template offers but this report does not use are kept with
    /// `included = false` rather than deleted, so toggling one back on restores the text.
    pub included: bool,
    pub answers: Vec<AnswerValue>,
}

impl SectionContent {
    pub fn answer(&self, question_key: &str) -> Option<&AnswerValue> {
        self.answers.iter().find(|a| a.question_key == question_key)
    }

    pub fn has_content(&self) -> bool {
        self.answers.iter().any(|a| !a.is_blank())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewReport {
    pub title: String,
    pub template_key: String,
    #[serde(default)]
    pub engagement_id: Option<String>,
    #[serde(default)]
    pub classification: Option<Classification>,
}

/// One answer written by the guided form.
#[derive(Debug, Clone, Deserialize)]
pub struct AnswerInput {
    pub section_key: String,
    pub question_key: String,
    pub value: String,
    /// Defaults to `user_provided`. The AI layer sets this explicitly when the user
    /// accepts a suggestion.
    #[serde(default)]
    pub provenance: Provenance,
    #[serde(default)]
    pub ai_request_id: Option<String>,
}

/// A draft save. Only the supplied fields change; answers not mentioned are left alone.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReportDraftUpdate {
    pub title: Option<String>,
    pub classification: Option<Classification>,
    #[serde(default)]
    pub answers: Vec<AnswerInput>,
    /// Section keys to include; omit to leave inclusion untouched.
    pub included_sections: Option<Vec<String>>,
    pub finding_ids: Option<Vec<String>>,
    pub evidence_ids: Option<Vec<String>>,
    #[serde(default)]
    pub notes: String,
}

/// A problem that must be resolved before a report can be finalized, or a warning the
/// author should look at first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    /// Finalization is refused while any of these remain.
    Blocker,
    /// Shown to the author; does not block.
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportReadiness {
    pub can_finalize: bool,
    pub issues: Vec<ValidationIssue>,
    /// How complete the report is against the professional standard for its kind: which
    /// required sections are filled in, the completeness percentage, and whether it meets
    /// the minimum length. Advisory only - it never affects `can_finalize`.
    pub completeness: crate::reports::standards::Completeness,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_labels_match_the_documented_progression() {
        assert_eq!(version_label(0, 1, ReportStatus::Draft), "v0.1 Draft");
        assert_eq!(version_label(0, 3, ReportStatus::Review), "v0.3 Review");
        assert_eq!(version_label(1, 0, ReportStatus::Final), "v1.0 Final");
        assert_eq!(version_label(1, 1, ReportStatus::Draft), "v1.1 Revised");
    }
}
