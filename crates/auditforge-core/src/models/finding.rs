//! Findings: the reusable, structured unit of a security report.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::{Provenance, Severity, Timestamp, VerificationStatus};

/// Narrative field names that may carry AI-assisted text and therefore a provenance
/// marker. Severity is not in this list: severity is never machine-assigned.
pub const NARRATIVE_FIELDS: &[&str] = &[
    "description",
    "technical_details",
    "impact",
    "likelihood",
    "risk",
    "root_cause",
    "remediation",
    "recommendation",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    /// Human-facing identifier, e.g. `FND-2026-0042`.
    pub finding_ref: String,
    pub engagement_id: Option<String>,
    pub title: String,
    pub severity: Severity,
    /// Set only when a human explicitly confirmed the severity. A finding cannot be
    /// included in a finalized report while this is false.
    pub severity_confirmed: bool,
    pub category: String,
    pub affected_asset: String,
    pub description: String,
    pub technical_details: String,
    pub impact: String,
    pub likelihood: String,
    pub risk: String,
    pub root_cause: String,
    pub remediation: String,
    pub recommendation: String,
    pub verification_status: VerificationStatus,
    pub reference_links: String,
    pub cvss_vector: String,
    pub cvss_score: Option<f64>,
    /// Field name -> where that field's current text came from.
    #[serde(default)]
    pub provenance: BTreeMap<String, Provenance>,
    pub author_user_id: String,
    pub author_name: String,
    pub evidence_ids: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Finding {
    pub fn field(&self, name: &str) -> Option<&str> {
        match name {
            "description" => Some(&self.description),
            "technical_details" => Some(&self.technical_details),
            "impact" => Some(&self.impact),
            "likelihood" => Some(&self.likelihood),
            "risk" => Some(&self.risk),
            "root_cause" => Some(&self.root_cause),
            "remediation" => Some(&self.remediation),
            "recommendation" => Some(&self.recommendation),
            "title" => Some(&self.title),
            "affected_asset" => Some(&self.affected_asset),
            "category" => Some(&self.category),
            _ => None,
        }
    }

    pub fn set_field(&mut self, name: &str, value: String) -> bool {
        match name {
            "description" => self.description = value,
            "technical_details" => self.technical_details = value,
            "impact" => self.impact = value,
            "likelihood" => self.likelihood = value,
            "risk" => self.risk = value,
            "root_cause" => self.root_cause = value,
            "remediation" => self.remediation = value,
            "recommendation" => self.recommendation = value,
            "title" => self.title = value,
            "affected_asset" => self.affected_asset = value,
            "category" => self.category = value,
            _ => return false,
        }
        true
    }

    pub fn provenance_of(&self, field: &str) -> Provenance {
        self.provenance
            .get(field)
            .copied()
            .unwrap_or(Provenance::UserProvided)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewFinding {
    pub title: String,
    #[serde(default)]
    pub engagement_id: Option<String>,
    /// Optional at creation. A finding starts as Informational/unconfirmed until the
    /// author explicitly sets and confirms a severity.
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub affected_asset: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub technical_details: String,
    #[serde(default)]
    pub impact: String,
    #[serde(default)]
    pub likelihood: String,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub root_cause: String,
    #[serde(default)]
    pub remediation: String,
    #[serde(default)]
    pub recommendation: String,
    #[serde(default)]
    pub reference_links: String,
    #[serde(default)]
    pub cvss_vector: String,
    #[serde(default)]
    pub cvss_score: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FindingUpdate {
    pub title: Option<String>,
    pub engagement_id: Option<Option<String>>,
    pub category: Option<String>,
    pub affected_asset: Option<String>,
    pub description: Option<String>,
    pub technical_details: Option<String>,
    pub impact: Option<String>,
    pub likelihood: Option<String>,
    pub risk: Option<String>,
    pub root_cause: Option<String>,
    pub remediation: Option<String>,
    pub recommendation: Option<String>,
    pub verification_status: Option<VerificationStatus>,
    pub reference_links: Option<String>,
    pub cvss_vector: Option<String>,
    pub cvss_score: Option<Option<f64>>,
    /// Provenance overrides for fields changed in this update.
    #[serde(default)]
    pub provenance: Option<BTreeMap<String, Provenance>>,
}

/// Severity changes travel on their own command so that "confirmed by a human" is an
/// explicit, auditable act rather than a side effect of an edit.
#[derive(Debug, Clone, Deserialize)]
pub struct SeverityDecision {
    pub severity: Severity,
    /// Must be true. Present so the UI cannot accidentally confirm on the user's behalf.
    pub confirmed_by_user: bool,
    #[serde(default)]
    pub rationale: String,
}

/// The copy of a finding embedded in a report version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingSnapshot {
    pub finding_ref: String,
    pub title: String,
    pub severity: Severity,
    pub category: String,
    pub affected_asset: String,
    pub description: String,
    pub technical_details: String,
    pub impact: String,
    pub likelihood: String,
    pub risk: String,
    pub root_cause: String,
    pub remediation: String,
    pub recommendation: String,
    pub verification_status: VerificationStatus,
    pub reference_links: String,
    pub cvss_vector: String,
    pub cvss_score: Option<f64>,
    #[serde(default)]
    pub provenance: BTreeMap<String, Provenance>,
    pub author_name: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl From<&Finding> for FindingSnapshot {
    fn from(finding: &Finding) -> Self {
        FindingSnapshot {
            finding_ref: finding.finding_ref.clone(),
            title: finding.title.clone(),
            severity: finding.severity,
            category: finding.category.clone(),
            affected_asset: finding.affected_asset.clone(),
            description: finding.description.clone(),
            technical_details: finding.technical_details.clone(),
            impact: finding.impact.clone(),
            likelihood: finding.likelihood.clone(),
            risk: finding.risk.clone(),
            root_cause: finding.root_cause.clone(),
            remediation: finding.remediation.clone(),
            recommendation: finding.recommendation.clone(),
            verification_status: finding.verification_status,
            reference_links: finding.reference_links.clone(),
            cvss_vector: finding.cvss_vector.clone(),
            cvss_score: finding.cvss_score,
            provenance: finding.provenance.clone(),
            author_name: finding.author_name.clone(),
            evidence_refs: Vec::new(),
        }
    }
}
