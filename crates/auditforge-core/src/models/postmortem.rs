//! Postmortems and the corrective actions they produce.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::{AnswerValue, CorrectiveActionStatus, PostmortemStatus, Timestamp};

/// The guided postmortem question set, in asking order.
///
/// Question keys are stable identifiers: renaming a prompt is safe, changing a key is
/// not, because saved answers are addressed by key.
pub const POSTMORTEM_QUESTIONS: &[(&str, &str)] = &[
    ("what_happened", "What happened?"),
    ("when_did_it_happen", "When did it happen?"),
    ("how_detected", "How was it detected?"),
    ("systems_affected", "What systems were affected?"),
    ("impact", "What was the impact?"),
    ("timeline", "What was the timeline?"),
    ("root_cause", "What was the root cause?"),
    ("actions_taken", "What actions were taken?"),
    ("what_went_well", "What went well?"),
    ("what_went_badly", "What did not go well?"),
    ("improvements", "What could be improved?"),
    (
        "corrective_actions",
        "What corrective actions are required?",
    ),
    ("action_owners", "Who owns each corrective action?"),
    ("deadlines", "What is the deadline?"),
    ("verification", "How will remediation be verified?"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Postmortem {
    pub id: String,
    /// Human-facing identifier, e.g. `PM-2026-0002`.
    pub postmortem_ref: String,
    pub title: String,
    pub incident_id: Option<String>,
    pub engagement_id: Option<String>,
    pub status: PostmortemStatus,
    /// question key -> answer
    pub answers: BTreeMap<String, AnswerValue>,
    pub corrective_actions: Vec<CorrectiveAction>,
    pub created_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Postmortem {
    /// Question keys that have not been answered yet. Drives the completion meter and
    /// the pre-finalize check.
    pub fn unanswered(&self) -> Vec<&'static str> {
        POSTMORTEM_QUESTIONS
            .iter()
            .filter(|(key, _)| match self.answers.get(*key) {
                Some(answer) => answer.is_blank(),
                None => true,
            })
            .map(|(key, _)| *key)
            .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewPostmortem {
    pub title: String,
    #[serde(default)]
    pub incident_id: Option<String>,
    #[serde(default)]
    pub engagement_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectiveAction {
    pub id: String,
    pub action_ref: String,
    pub postmortem_id: Option<String>,
    pub incident_id: Option<String>,
    pub finding_id: Option<String>,
    pub description: String,
    pub owner: String,
    pub due_date: Option<String>,
    pub status: CorrectiveActionStatus,
    pub verification_method: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewCorrectiveAction {
    pub description: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub verification_method: String,
    #[serde(default)]
    pub postmortem_id: Option<String>,
    #[serde(default)]
    pub incident_id: Option<String>,
    #[serde(default)]
    pub finding_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CorrectiveActionUpdate {
    pub description: Option<String>,
    pub owner: Option<String>,
    pub due_date: Option<Option<String>>,
    pub status: Option<CorrectiveActionStatus>,
    pub verification_method: Option<String>,
}
