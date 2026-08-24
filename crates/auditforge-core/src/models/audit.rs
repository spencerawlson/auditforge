//! The audit trail.
//!
//! Audit entries are append-only by convention and by API: nothing in the core exposes
//! an update or delete for `audit_logs`.

use serde::{Deserialize, Serialize};

use super::common::{string_enum, Timestamp};

string_enum! {
    pub enum AuditAction {
        AccountCreated => "account_created",
        Login => "login",
        LoginFailed => "login_failed",
        Logout => "logout",
        PasswordChanged => "password_changed",
        ProfileUpdated => "profile_updated",
        SessionExpired => "session_expired",
        EngagementCreated => "engagement_created",
        EngagementUpdated => "engagement_updated",
        EngagementDeleted => "engagement_deleted",
        ReportCreated => "report_created",
        ReportEdited => "report_edited",
        ReportSubmittedForReview => "report_submitted_for_review",
        ReportFinalized => "report_finalized",
        ReportRevisionCreated => "report_revision_created",
        ReportDeleted => "report_deleted",
        ReportExported => "report_exported",
        PdfGenerated => "pdf_generated",
        FindingCreated => "finding_created",
        FindingModified => "finding_modified",
        FindingSeverityConfirmed => "finding_severity_confirmed",
        FindingDeleted => "finding_deleted",
        EvidenceAdded => "evidence_added",
        EvidenceUpdated => "evidence_updated",
        EvidenceLinked => "evidence_linked",
        EvidenceDeleted => "evidence_deleted",
        IncidentCreated => "incident_created",
        IncidentUpdated => "incident_updated",
        PostmortemCreated => "postmortem_created",
        PostmortemUpdated => "postmortem_updated",
        AiSuggestionGenerated => "ai_suggestion_generated",
        AiSuggestionAccepted => "ai_suggestion_accepted",
        AiSuggestionEdited => "ai_suggestion_edited",
        AiSuggestionRejected => "ai_suggestion_rejected",
        AiReviewRun => "ai_review_run",
        AiRequestBlocked => "ai_request_blocked",
        SettingsChanged => "settings_changed",
        SecretStored => "secret_stored",
        SecretCleared => "secret_cleared",
    }
}

string_enum! {
    pub enum AuditOutcome {
        Success => "success",
        Failure => "failure",
        Denied => "denied",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub occurred_at: Timestamp,
    pub user_id: Option<String>,
    pub username: String,
    pub action: AuditAction,
    pub target_type: String,
    pub target_id: String,
    pub outcome: AuditOutcome,
    /// Small JSON object with context. Never contains report bodies, secrets or
    /// evidence contents.
    pub detail: serde_json::Value,
}

/// Filter for the audit log viewer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub action: Option<AuditAction>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub target_type: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub since: Option<Timestamp>,
    #[serde(default)]
    pub until: Option<Timestamp>,
    #[serde(default)]
    pub limit: Option<i64>,
}
