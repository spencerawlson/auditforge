//! AI-facing types: settings, requests, suggestions and review results.
//!
//! Nothing here writes to a report. An AI call produces a *suggestion* that the human
//! must accept, edit or reject; only that decision changes stored content.

use serde::{Deserialize, Serialize};

use super::common::{string_enum, Provenance, Timestamp};

string_enum! {
    #[derive(Default)]
    pub enum AiProviderKind {
        OpenAi => "openai",
        Local => "local",
        /// The default: no AI features and no network calls.
        #[default]
        Disabled => "disabled",
    }
}

string_enum! {
    /// Where text is allowed to travel. Displayed prominently in the UI.
    pub enum PrivacyMode {
        /// No network calls of any kind.
        Offline => "offline",
        /// Local inference endpoint only (Ollama or compatible).
        Local => "local",
        /// A third-party cloud API.
        Cloud => "cloud",
    }
}

impl PrivacyMode {
    pub fn banner(&self) -> &'static str {
        match self {
            PrivacyMode::Offline => "AI Processing: Disabled / Offline",
            PrivacyMode::Local => "AI Processing: Local / Private",
            PrivacyMode::Cloud => "AI Processing: OpenAI Cloud",
        }
    }

    pub fn leaves_machine(&self) -> bool {
        matches!(self, PrivacyMode::Cloud)
    }
}

string_enum! {
    pub enum AiFunction {
        ImproveWriting => "improve_writing",
        Rewrite => "rewrite",
        TechnicalRewrite => "technical_rewrite",
        ExecutiveRewrite => "executive_rewrite",
        Summarize => "summarize",
        Expand => "expand",
        ReportReview => "report_review",
        FindingAnalysis => "finding_analysis",
        ExecutiveSummary => "executive_summary",
    }
}

impl AiFunction {
    pub fn title(&self) -> &'static str {
        match self {
            AiFunction::ImproveWriting => "Improve Writing",
            AiFunction::Rewrite => "Rewrite",
            AiFunction::TechnicalRewrite => "Technical Rewrite",
            AiFunction::ExecutiveRewrite => "Executive Rewrite",
            AiFunction::Summarize => "Summarize",
            AiFunction::Expand => "Expand",
            AiFunction::ReportReview => "Report Review",
            AiFunction::FindingAnalysis => "Finding Analysis",
            AiFunction::ExecutiveSummary => "Executive Summary",
        }
    }
}

string_enum! {
    pub enum SummaryKind {
        Executive => "executive",
        Technical => "technical",
        Incident => "incident",
        Finding => "finding",
    }
}

string_enum! {
    pub enum AiConfidence {
        High => "high",
        Medium => "medium",
        Low => "low",
    }
}

string_enum! {
    pub enum AiDecision {
        Accepted => "accepted",
        Edited => "edited",
        Rejected => "rejected",
    }
}

string_enum! {
    /// The classes of problem the report reviewer is asked to look for.
    pub enum ReviewCategory {
        Grammar => "grammar",
        Clarity => "clarity",
        Contradiction => "contradiction",
        MissingInformation => "missing_information",
        UnsupportedClaim => "unsupported_claim",
        InconsistentTerminology => "inconsistent_terminology",
        InconsistentSeverity => "inconsistent_severity",
        IncompleteRemediation => "incomplete_remediation",
        MissingEvidence => "missing_evidence",
        TimelineInconsistency => "timeline_inconsistency",
        Other => "other",
    }
}

/// User-configurable AI settings. The API key is not here: it lives in the OS keychain
/// and is fetched only at call time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSettings {
    pub provider: AiProviderKind,
    pub openai_model: String,
    pub openai_base_url: String,
    pub local_endpoint: String,
    pub local_model: String,
    pub temperature: f32,
    pub max_output_tokens: u32,
    pub request_timeout_secs: u64,
    /// Must be explicitly turned on by the user before any request leaves the machine.
    /// Selecting the OpenAI provider is not by itself consent.
    pub cloud_consent: bool,
    /// Strip candidate hostnames, IPs and credentials from prompts before sending.
    pub redact_before_send: bool,
    /// True once a key has been stored in the keychain. The key itself is never
    /// returned to the frontend.
    #[serde(default)]
    pub has_api_key: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            provider: AiProviderKind::Disabled,
            openai_model: "gpt-4.1-mini".to_string(),
            openai_base_url: "https://api.openai.com/v1".to_string(),
            local_endpoint: "http://127.0.0.1:11434".to_string(),
            local_model: "llama3.1:8b".to_string(),
            temperature: 0.2,
            max_output_tokens: 1500,
            request_timeout_secs: 90,
            cloud_consent: false,
            redact_before_send: true,
            has_api_key: false,
        }
    }
}

impl AiSettings {
    pub fn privacy_mode(&self) -> PrivacyMode {
        match self.provider {
            AiProviderKind::OpenAi => PrivacyMode::Cloud,
            AiProviderKind::Local => PrivacyMode::Local,
            AiProviderKind::Disabled => PrivacyMode::Offline,
        }
    }
}

/// What the caller asks the AI layer to do. `text` is the only content sent; callers
/// assemble the minimum context needed rather than handing over whole records.
#[derive(Debug, Clone, Deserialize)]
pub struct AiTextRequest {
    pub function: AiFunction,
    pub text: String,
    #[serde(default)]
    pub summary_kind: Option<SummaryKind>,
    /// Short, factual context, e.g. "Finding: TLS 1.0 enabled on the VPN concentrator".
    /// Truncated before sending.
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub target_type: String,
    #[serde(default)]
    pub target_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSegment {
    pub kind: DiffKind,
    pub text: String,
}

/// A proposal. Never applied automatically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSuggestion {
    pub request_id: String,
    pub function: AiFunction,
    pub provider: AiProviderKind,
    pub privacy_mode: PrivacyMode,
    pub model: String,
    /// Exactly what the user had before, so the review pane can show both sides.
    pub original: String,
    pub suggestion: String,
    pub confidence: AiConfidence,
    /// The provenance that will be recorded if the user accepts as-is.
    pub provenance: Provenance,
    /// Facts the model reports as absent. Surfaced as "Information required".
    #[serde(default)]
    pub missing_information: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    /// Word-level diff for the Changes view.
    #[serde(default)]
    pub diff: Vec<DiffSegment>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewWarning {
    pub category: ReviewCategory,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_key: Option<String>,
    /// A short quote from the report so the author can find the passage. Must be text
    /// that actually appears in the report; quotes that do not are dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}

/// Output of a report-quality review. Warnings only: the reviewer never edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiReviewResult {
    pub request_id: String,
    pub provider: AiProviderKind,
    pub privacy_mode: PrivacyMode,
    pub model: String,
    pub summary: String,
    pub confidence: AiConfidence,
    pub warnings: Vec<ReviewWarning>,
    pub created_at: Timestamp,
}

/// Audit row for a single AI call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequestRecord {
    pub id: String,
    pub user_id: Option<String>,
    pub provider: AiProviderKind,
    pub privacy_mode: PrivacyMode,
    pub model: String,
    pub function: AiFunction,
    pub target_type: String,
    pub target_id: String,
    pub input_chars: i64,
    pub output_chars: i64,
    pub confidence: String,
    pub status: String,
    pub error: String,
    pub decision: Option<AiDecision>,
    pub created_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub decided_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_mode_tracks_provider() {
        let mut settings = AiSettings::default();
        assert_eq!(settings.privacy_mode(), PrivacyMode::Offline);
        settings.provider = AiProviderKind::Local;
        assert_eq!(settings.privacy_mode(), PrivacyMode::Local);
        settings.provider = AiProviderKind::OpenAi;
        assert_eq!(settings.privacy_mode(), PrivacyMode::Cloud);
        assert!(settings.privacy_mode().leaves_machine());
    }

    #[test]
    fn defaults_are_private_and_without_consent() {
        let settings = AiSettings::default();
        assert_eq!(settings.provider, AiProviderKind::Disabled);
        assert!(!settings.cloud_consent);
        assert!(settings.redact_before_send);
        assert!(!settings.has_api_key);
    }
}
