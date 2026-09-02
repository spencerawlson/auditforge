//! Evidence records.
//!
//! The database row is metadata only. Bytes live in the managed evidence vault under the
//! application data directory; `relative_path` is always relative to that root so the
//! vault can be moved or backed up as a unit and no absolute path is ever trusted.

use serde::{Deserialize, Serialize};

use super::common::{EntityType, EvidenceKind, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    /// Human-facing identifier, e.g. `EVD-2026-0113`.
    pub evidence_ref: String,
    pub engagement_id: Option<String>,
    pub title: String,
    pub description: String,
    /// The name as supplied by the user, kept for the report appendix. Never used to
    /// build a path.
    pub original_filename: String,
    /// Sanitized name actually used on disk.
    pub stored_filename: String,
    pub relative_path: String,
    pub mime_type: String,
    pub kind: EvidenceKind,
    pub byte_size: i64,
    /// SHA-256 of the stored bytes, so a reader can prove the file is unmodified.
    pub sha256: String,
    /// SHA-256 of the file exactly as supplied, when it differed from the stored bytes.
    /// Set only when metadata was stripped on import; `None` for anything imported
    /// before scrubbing existed, which is why it is not simply a copy of `sha256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    /// Whether image metadata (EXIF, XMP, IPTC, PNG text chunks) was removed on import.
    #[serde(default)]
    pub metadata_stripped: bool,
    pub is_encrypted: bool,
    pub captured_at: Option<Timestamp>,
    pub created_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default)]
    pub links: Vec<EvidenceLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceLink {
    pub id: String,
    pub target_type: EntityType,
    pub target_id: String,
    pub created_at: Timestamp,
}

/// Import request: a path the user picked plus the metadata they typed.
#[derive(Debug, Clone, Deserialize)]
pub struct NewEvidence {
    /// Absolute path to the source file being imported. Validated before any read.
    pub source_path: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub engagement_id: Option<String>,
    #[serde(default)]
    pub captured_at: Option<Timestamp>,
    /// Encrypt at rest inside the vault.
    #[serde(default)]
    pub encrypt: bool,
    /// Explicit acknowledgement that an executable or script is being imported on
    /// purpose. Without it, such files are refused.
    #[serde(default)]
    pub acknowledge_executable: bool,
    /// Optional link created at import time.
    #[serde(default)]
    pub link_target: Option<EvidenceLinkRequest>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvidenceLinkRequest {
    pub target_type: EntityType,
    pub target_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EvidenceUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub engagement_id: Option<Option<String>>,
    pub captured_at: Option<Option<Timestamp>>,
}

/// The evidence description embedded in a report version. Bytes are referenced, not
/// duplicated: the appendix cites the ref, filename and hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSnapshot {
    pub evidence_ref: String,
    pub title: String,
    pub description: String,
    pub original_filename: String,
    pub relative_path: String,
    pub mime_type: String,
    pub kind: EvidenceKind,
    pub byte_size: i64,
    pub sha256: String,
    pub captured_at: Option<Timestamp>,
}

impl From<&Evidence> for EvidenceSnapshot {
    fn from(evidence: &Evidence) -> Self {
        EvidenceSnapshot {
            evidence_ref: evidence.evidence_ref.clone(),
            title: evidence.title.clone(),
            description: evidence.description.clone(),
            original_filename: evidence.original_filename.clone(),
            relative_path: evidence.relative_path.clone(),
            mime_type: evidence.mime_type.clone(),
            kind: evidence.kind,
            byte_size: evidence.byte_size,
            sha256: evidence.sha256.clone(),
            captured_at: evidence.captured_at,
        }
    }
}
