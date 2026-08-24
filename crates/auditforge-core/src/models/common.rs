//! Shared domain vocabulary: the enums, the timestamp type and the author snapshot.
//!
//! Every enum here is stored in SQLite as its snake_case string and serialized to the
//! frontend as the same string. Keeping one spelling for both ends means a value read
//! from an old database always round-trips, and a typo in either direction fails loudly
//! instead of silently defaulting.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::ToSql;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Error;

macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// Every variant, in declaration order. Used to populate UI pickers.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            pub fn as_str(&self) -> &'static str {
                match self { $( $name::$variant => $text ),+ }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::error::Error;
            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                match s {
                    $( $text => Ok($name::$variant), )+
                    other => Err($crate::error::Error::Validation(format!(
                        concat!("unknown ", stringify!($name), " value: {}"), other
                    ))),
                }
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(
                &self,
                s: S,
            ) -> ::std::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                d: D,
            ) -> ::std::result::Result<Self, D::Error> {
                let raw = <::std::string::String as ::serde::Deserialize>::deserialize(d)?;
                raw.parse().map_err(::serde::de::Error::custom)
            }
        }

        impl ::rusqlite::ToSql for $name {
            fn to_sql(&self) -> ::rusqlite::Result<::rusqlite::types::ToSqlOutput<'_>> {
                Ok(::rusqlite::types::ToSqlOutput::from(self.as_str()))
            }
        }

        impl ::rusqlite::types::FromSql for $name {
            fn column_result(
                value: ::rusqlite::types::ValueRef<'_>,
            ) -> ::rusqlite::types::FromSqlResult<Self> {
                let raw = value.as_str()?;
                raw.parse().map_err(|_| ::rusqlite::types::FromSqlError::InvalidType)
            }
        }
    };
}

pub(crate) use string_enum;

string_enum! {
    /// Where a piece of text in a report came from.
    ///
    /// This is the data-model half of the AI safety rule: the application can always
    /// tell the human which words they wrote, which words a model phrased, which
    /// statements are a model's inference, and where information is still missing.
    #[derive(Default)]
    pub enum Provenance {
        /// Typed or pasted by the human author. The only kind that asserts fact.
        #[default]
        UserProvided => "user_provided",
        /// A model reworded user-provided facts; the facts are the author's.
        AiWording => "ai_wording",
        /// A model drew a conclusion that the author has not yet confirmed.
        AiInference => "ai_inference",
        /// Explicitly marked as unknown. Renders as "Information required."
        MissingInformation => "missing_information",
    }
}

impl Provenance {
    /// True when the value may be presented as an assertion by the author.
    pub fn is_authoritative(&self) -> bool {
        matches!(self, Provenance::UserProvided | Provenance::AiWording)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Provenance::UserProvided => "Author",
            Provenance::AiWording => "AI wording",
            Provenance::AiInference => "AI inference (unconfirmed)",
            Provenance::MissingInformation => "Information required",
        }
    }
}

string_enum! {
    pub enum Severity {
        Critical => "critical",
        High => "high",
        Medium => "medium",
        Low => "low",
        Informational => "informational",
    }
}

impl Severity {
    /// Sort weight, highest severity first.
    pub fn rank(&self) -> u8 {
        match self {
            Severity::Critical => 0,
            Severity::High => 1,
            Severity::Medium => 2,
            Severity::Low => 3,
            Severity::Informational => 4,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Severity::Critical => "Critical",
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
            Severity::Informational => "Informational",
        }
    }

    /// RGB used for the severity chip in generated PDFs.
    pub fn rgb(&self) -> (u8, u8, u8) {
        match self {
            Severity::Critical => (155, 28, 40),
            Severity::High => (200, 74, 30),
            Severity::Medium => (183, 137, 20),
            Severity::Low => (42, 106, 143),
            Severity::Informational => (94, 104, 118),
        }
    }
}

string_enum! {
    #[derive(Default)]
    pub enum Classification {
        Public => "public",
        Internal => "internal",
        /// The default for new reports: most security work is not for general release.
        #[default]
        Confidential => "confidential",
        Restricted => "restricted",
    }
}

impl Classification {
    /// Banner text placed in the page header and footer of exported documents.
    pub fn banner(&self) -> &'static str {
        match self {
            Classification::Public => "PUBLIC",
            Classification::Internal => "INTERNAL USE ONLY",
            Classification::Confidential => "CONFIDENTIAL",
            Classification::Restricted => "RESTRICTED",
        }
    }
}

string_enum! {
    pub enum EngagementStatus {
        Draft => "draft",
        Active => "active",
        Review => "review",
        Completed => "completed",
        Archived => "archived",
    }
}

string_enum! {
    pub enum ReportStatus {
        Draft => "draft",
        Review => "review",
        Final => "final",
        Archived => "archived",
    }
}

string_enum! {
    pub enum ReportType {
        PenetrationTest => "penetration_test",
        VulnerabilityAssessment => "vulnerability_assessment",
        SecurityAssessment => "security_assessment",
        NetworkAssessment => "network_assessment",
        IncidentReport => "incident_report",
        Postmortem => "postmortem",
        CloudSecurityAssessment => "cloud_security_assessment",
        ComplianceAudit => "compliance_audit",
        EngagementReport => "engagement_report",
        ExecutiveSummary => "executive_summary",
        Custom => "custom",
    }
}

impl ReportType {
    pub fn title(&self) -> &'static str {
        match self {
            ReportType::PenetrationTest => "Penetration Test Report",
            ReportType::VulnerabilityAssessment => "Vulnerability Assessment",
            ReportType::SecurityAssessment => "Security Assessment",
            ReportType::NetworkAssessment => "Network Assessment",
            ReportType::IncidentReport => "Incident Report",
            ReportType::Postmortem => "Postmortem",
            ReportType::CloudSecurityAssessment => "Cloud Security Assessment",
            ReportType::ComplianceAudit => "Compliance / Audit Report",
            ReportType::EngagementReport => "Engagement Report",
            ReportType::ExecutiveSummary => "Executive Summary",
            ReportType::Custom => "Custom Report",
        }
    }
}

string_enum! {
    pub enum VerificationStatus {
        Unverified => "unverified",
        Verified => "verified",
        Remediated => "remediated",
        RiskAccepted => "risk_accepted",
        FalsePositive => "false_positive",
    }
}

string_enum! {
    pub enum EvidenceKind {
        Image => "image",
        Log => "log",
        Text => "text",
        Csv => "csv",
        Json => "json",
        Capture => "capture",
        Archive => "archive",
        Other => "other",
    }
}

string_enum! {
    pub enum IncidentStatus {
        Open => "open",
        Investigating => "investigating",
        Contained => "contained",
        Resolved => "resolved",
        Closed => "closed",
    }
}

string_enum! {
    pub enum PostmortemStatus {
        Draft => "draft",
        Review => "review",
        Complete => "complete",
    }
}

string_enum! {
    pub enum CorrectiveActionStatus {
        Open => "open",
        InProgress => "in_progress",
        Complete => "complete",
        Verified => "verified",
        Cancelled => "cancelled",
    }
}

string_enum! {
    pub enum TemplateSource {
        Builtin => "builtin",
        User => "user",
    }
}

string_enum! {
    /// Entity kinds that evidence and tags can point at.
    pub enum EntityType {
        Engagement => "engagement",
        Report => "report",
        Finding => "finding",
        Incident => "incident",
        Postmortem => "postmortem",
        Evidence => "evidence",
    }
}

/// An RFC 3339 UTC instant.
///
/// A newtype rather than a bare `DateTime<Utc>` so that one place decides the storage
/// format. Everything in the database and every JSON payload uses the same
/// `2026-08-24T09:15:00Z` spelling, which also sorts correctly as text in SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// The current instant, truncated to whole seconds.
    ///
    /// Truncation happens here rather than at serialization so an in-memory value is
    /// always equal to the value that comes back out of the database or a JSON export.
    /// Without it, a report's content hash would change simply by saving and reloading it.
    pub fn now() -> Self {
        Timestamp::from_datetime(Utc::now())
    }

    pub fn from_datetime(value: DateTime<Utc>) -> Self {
        Timestamp(DateTime::from_timestamp(value.timestamp(), 0).unwrap_or(value))
    }

    pub fn inner(&self) -> DateTime<Utc> {
        self.0
    }

    pub fn to_rfc3339(&self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    /// `2026-08-24` - used in report headers and filenames.
    pub fn date_string(&self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }

    /// `24 August 2026 09:15 UTC` - used in rendered documents.
    pub fn display_long(&self) -> String {
        self.0
            .format("%e %B %Y %H:%M UTC")
            .to_string()
            .trim()
            .to_string()
    }

    pub fn plus_seconds(&self, secs: i64) -> Self {
        Timestamp(self.0 + chrono::Duration::seconds(secs))
    }

    pub fn seconds_until(&self, other: Timestamp) -> i64 {
        (other.0 - self.0).num_seconds()
    }

    pub fn is_before(&self, other: Timestamp) -> bool {
        self.0 < other.0
    }
}

impl FromStr for Timestamp {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| Timestamp(dt.with_timezone(&Utc)))
            .map_err(|e| Error::Validation(format!("invalid timestamp: {e}")))
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

impl ToSql for Timestamp {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_rfc3339()))
    }
}

impl FromSql for Timestamp {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        raw.parse().map_err(|_| FromSqlError::InvalidType)
    }
}

/// The author identity as it was at the moment a version was saved.
///
/// Reports embed this rather than joining to `users`, so editing a job title today
/// cannot rewrite the attribution on a report finalized last year.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorSnapshot {
    pub user_id: String,
    pub username: String,
    pub full_name: String,
    pub organization: String,
    pub job_title: String,
    pub captured_at: Timestamp,
}

/// A single answer to a guided question, carrying its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerValue {
    pub question_key: String,
    pub value: String,
    #[serde(default)]
    pub provenance: Provenance,
    /// Set when the current text originated from an accepted AI suggestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_request_id: Option<String>,
    pub updated_at: Timestamp,
}

impl AnswerValue {
    pub fn user_provided(question_key: impl Into<String>, value: impl Into<String>) -> Self {
        AnswerValue {
            question_key: question_key.into(),
            value: value.into(),
            provenance: Provenance::UserProvided,
            ai_request_id: None,
            updated_at: Timestamp::now(),
        }
    }

    pub fn is_blank(&self) -> bool {
        self.value.trim().is_empty()
    }
}

/// Validate a `YYYY-MM-DD` calendar date. Returns the normalized string.
pub fn validate_date(value: &str, field: &str) -> Result<String, Error> {
    let trimmed = value.trim();
    chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| Error::Validation(format!("{field} must be a date in YYYY-MM-DD form")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_round_trips_through_text() {
        for value in Severity::ALL {
            let parsed: Severity = value.as_str().parse().unwrap();
            assert_eq!(parsed, *value);
        }
        for value in ReportType::ALL {
            let parsed: ReportType = value.as_str().parse().unwrap();
            assert_eq!(parsed, *value);
        }
    }

    #[test]
    fn enum_rejects_unknown_text() {
        assert!("catastrophic".parse::<Severity>().is_err());
    }

    #[test]
    fn enum_json_uses_the_same_spelling_as_sql() {
        let json = serde_json::to_string(&Severity::Critical).unwrap();
        assert_eq!(json, "\"critical\"");
        let back: Severity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Severity::Critical);
    }

    #[test]
    fn severity_ranks_critical_first() {
        let mut all = Severity::ALL.to_vec();
        all.sort_by_key(|s| s.rank());
        assert_eq!(all[0], Severity::Critical);
        assert_eq!(all[all.len() - 1], Severity::Informational);
    }

    #[test]
    fn a_live_timestamp_equals_the_one_that_comes_back_from_storage() {
        // Sub-second precision would make an in-memory value differ from its stored form,
        // which would change a report's content hash on every save/reload cycle.
        let now = Timestamp::now();
        let reloaded: Timestamp = now.to_rfc3339().parse().unwrap();
        assert_eq!(now, reloaded);
        assert_eq!(now.inner().timestamp_subsec_nanos(), 0);

        let via_json: Timestamp =
            serde_json::from_str(&serde_json::to_string(&now).unwrap()).unwrap();
        assert_eq!(now, via_json);
    }

    #[test]
    fn timestamps_round_trip_and_sort_as_text() {
        let now = Timestamp::now();
        let text = now.to_rfc3339();
        let parsed: Timestamp = text.parse().unwrap();
        assert_eq!(parsed.to_rfc3339(), text);

        let later = now.plus_seconds(60);
        assert!(now.to_rfc3339() < later.to_rfc3339());
        assert!(now.is_before(later));
        assert_eq!(now.seconds_until(later), 60);
    }

    #[test]
    fn provenance_defaults_to_user_provided_and_gates_assertions() {
        assert_eq!(Provenance::default(), Provenance::UserProvided);
        assert!(Provenance::UserProvided.is_authoritative());
        assert!(Provenance::AiWording.is_authoritative());
        assert!(!Provenance::AiInference.is_authoritative());
        assert!(!Provenance::MissingInformation.is_authoritative());
    }

    #[test]
    fn date_validation_rejects_junk() {
        assert_eq!(
            validate_date(" 2026-08-24 ", "start date").unwrap(),
            "2026-08-24"
        );
        assert!(validate_date("24/08/2026", "start date").is_err());
        assert!(validate_date("2026-13-01", "start date").is_err());
    }
}
