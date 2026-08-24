//! Incident records.

use serde::{Deserialize, Serialize};

use super::common::{IncidentStatus, Severity, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incident {
    pub id: String,
    /// Human-facing identifier, e.g. `INC-2026-0004`.
    pub incident_ref: String,
    pub engagement_id: Option<String>,
    pub title: String,
    pub detected_at: Option<Timestamp>,
    pub resolved_at: Option<Timestamp>,
    pub reporter: String,
    pub severity: Severity,
    pub severity_confirmed: bool,
    pub affected_systems: String,
    pub description: String,
    pub impact: String,
    pub timeline: String,
    pub detection_method: String,
    pub response_actions: String,
    pub root_cause: String,
    pub remediation: String,
    pub lessons_learned: String,
    pub status: IncidentStatus,
    pub created_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewIncident {
    pub title: String,
    #[serde(default)]
    pub engagement_id: Option<String>,
    #[serde(default)]
    pub detected_at: Option<Timestamp>,
    #[serde(default)]
    pub reporter: String,
    #[serde(default)]
    pub affected_systems: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub impact: String,
    #[serde(default)]
    pub detection_method: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct IncidentUpdate {
    pub title: Option<String>,
    pub engagement_id: Option<Option<String>>,
    pub detected_at: Option<Option<Timestamp>>,
    pub resolved_at: Option<Option<Timestamp>>,
    pub reporter: Option<String>,
    pub affected_systems: Option<String>,
    pub description: Option<String>,
    pub impact: Option<String>,
    pub timeline: Option<String>,
    pub detection_method: Option<String>,
    pub response_actions: Option<String>,
    pub root_cause: Option<String>,
    pub remediation: Option<String>,
    pub lessons_learned: Option<String>,
    pub status: Option<IncidentStatus>,
}
