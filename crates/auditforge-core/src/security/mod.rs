//! Security primitives shared across the core: path containment, encryption at rest,
//! secret storage and prompt redaction.

pub mod crypto;
pub mod paths;
pub mod redact;
pub mod secrets;

pub use paths::{resolve_within, sanitize_filename, validate_import_source};
pub use redact::{Detection, Redactor, SensitiveKind};
pub use secrets::{SecretStore, KEY_EVIDENCE_VAULT, KEY_OPENAI_API};
