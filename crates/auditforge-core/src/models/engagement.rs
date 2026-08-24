//! Engagements: the project container that reports, findings and evidence hang off.

use serde::{Deserialize, Serialize};

use super::common::{EngagementStatus, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub id: String,
    pub name: String,
    pub role: String,
    pub email: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewTeamMember {
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engagement {
    pub id: String,
    /// Human-facing identifier, e.g. `ENG-2026-0007`.
    pub engagement_ref: String,
    pub name: String,
    pub client_name: String,
    pub organization_id: Option<String>,
    pub owner_user_id: String,
    pub status: EngagementStatus,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub objectives: String,
    pub scope: String,
    pub out_of_scope: String,
    /// Reference to the written authorization (ticket, contract, signed ROE).
    pub authorization_ref: String,
    pub rules_of_engagement: String,
    pub notes: String,
    pub team: Vec<TeamMember>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Counts shown on engagement cards without loading the child records.
#[derive(Debug, Clone, Serialize)]
pub struct EngagementSummary {
    #[serde(flatten)]
    pub engagement: Engagement,
    pub report_count: i64,
    pub finding_count: i64,
    pub evidence_count: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewEngagement {
    pub name: String,
    #[serde(default)]
    pub client_name: String,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub objectives: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub out_of_scope: String,
    #[serde(default)]
    pub authorization_ref: String,
    #[serde(default)]
    pub rules_of_engagement: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub team: Vec<NewTeamMember>,
}

/// Partial update. Fields left as `None` are not written.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EngagementUpdate {
    pub name: Option<String>,
    pub client_name: Option<String>,
    pub status: Option<EngagementStatus>,
    pub start_date: Option<Option<String>>,
    pub end_date: Option<Option<String>>,
    pub objectives: Option<String>,
    pub scope: Option<String>,
    pub out_of_scope: Option<String>,
    pub authorization_ref: Option<String>,
    pub rules_of_engagement: Option<String>,
    pub notes: Option<String>,
    pub team: Option<Vec<NewTeamMember>>,
}

/// The engagement facts copied into a report version so the document stays stable even
/// if the engagement is edited afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngagementSnapshot {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub engagement_ref: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub client_name: String,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub team: Vec<String>,
}

impl From<&Engagement> for EngagementSnapshot {
    fn from(engagement: &Engagement) -> Self {
        EngagementSnapshot {
            id: engagement.id.clone(),
            engagement_ref: engagement.engagement_ref.clone(),
            name: engagement.name.clone(),
            client_name: engagement.client_name.clone(),
            start_date: engagement.start_date.clone(),
            end_date: engagement.end_date.clone(),
            team: engagement
                .team
                .iter()
                .map(|member| {
                    if member.role.trim().is_empty() {
                        member.name.clone()
                    } else {
                        format!("{} - {}", member.name, member.role)
                    }
                })
                .collect(),
        }
    }
}
