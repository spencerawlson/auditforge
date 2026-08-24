//! The single error type used across the core.
//!
//! Errors that reach the UI are deliberately coarse: the frontend gets a stable `kind`
//! it can branch on plus a message safe to show a user. Detail that would help an
//! attacker (which half of a credential pair was wrong, absolute filesystem paths,
//! provider response bodies) stays in the log, not in the message.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The caller supplied something the domain rejects. Safe to show verbatim.
    #[error("{0}")]
    Validation(String),

    /// The requested record does not exist.
    #[error("{kind} not found")]
    NotFound { kind: &'static str },

    /// Credentials did not match, or the session is missing/expired.
    #[error("authentication failed")]
    Unauthorized,

    /// Authenticated, but not allowed to touch this record.
    #[error("not permitted")]
    Forbidden,

    /// Too many failed attempts; retry after the given number of seconds.
    #[error("account temporarily locked")]
    Locked { retry_after_secs: i64 },

    /// The operation would modify a finalized (immutable) record.
    #[error("{0}")]
    Immutable(String),

    /// AI is disabled, or the request would violate the configured privacy mode.
    #[error("{0}")]
    AiPolicy(String),

    /// The provider was reachable but returned something unusable.
    #[error("{0}")]
    AiProvider(String),

    /// Local storage failure.
    #[error("database error: {0}")]
    Database(String),

    #[error("io error: {0}")]
    Io(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("{0}")]
    Internal(String),
}

impl Error {
    pub fn validation(msg: impl Into<String>) -> Self {
        Error::Validation(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }

    pub fn not_found(kind: &'static str) -> Self {
        Error::NotFound { kind }
    }

    /// Stable machine-readable discriminant for the frontend.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Validation(_) => "validation",
            Error::NotFound { .. } => "not_found",
            Error::Unauthorized => "unauthorized",
            Error::Forbidden => "forbidden",
            Error::Locked { .. } => "locked",
            Error::Immutable(_) => "immutable",
            Error::AiPolicy(_) => "ai_policy",
            Error::AiProvider(_) => "ai_provider",
            Error::Database(_) => "database",
            Error::Io(_) => "io",
            Error::Serialization(_) => "serialization",
            Error::Internal(_) => "internal",
        }
    }
}

/// Wire shape handed to the frontend. `Error` itself is not `Serialize` on purpose so
/// that every boundary has to go through this conversion and think about redaction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ErrorPayload {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<i64>,
}

impl From<&Error> for ErrorPayload {
    fn from(err: &Error) -> Self {
        ErrorPayload {
            kind: err.kind().to_string(),
            message: err.to_string(),
            retry_after_secs: match err {
                Error::Locked { retry_after_secs } => Some(*retry_after_secs),
                _ => None,
            },
        }
    }
}

impl From<Error> for ErrorPayload {
    fn from(err: Error) -> Self {
        ErrorPayload::from(&err)
    }
}

impl fmt::Display for ErrorPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        // Constraint violations are usually a user-visible conflict (duplicate username,
        // duplicate reference) rather than an internal fault, so they map to Validation.
        match &err {
            rusqlite::Error::SqliteFailure(e, msg)
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                let text = msg.as_deref().unwrap_or("constraint violation");
                if text.contains("immutable") {
                    Error::Immutable(text.to_string())
                } else {
                    Error::Validation(format!("conflicts with an existing record ({text})"))
                }
            }
            rusqlite::Error::QueryReturnedNoRows => Error::NotFound { kind: "record" },
            _ => Error::Database(err.to_string()),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_carries_retry_after_for_lockout() {
        let payload = ErrorPayload::from(Error::Locked {
            retry_after_secs: 90,
        });
        assert_eq!(payload.kind, "locked");
        assert_eq!(payload.retry_after_secs, Some(90));
    }

    #[test]
    fn unauthorized_message_does_not_disclose_which_field_failed() {
        let msg = Error::Unauthorized.to_string();
        assert!(!msg.to_lowercase().contains("password"));
        assert!(!msg.to_lowercase().contains("username"));
    }
}
