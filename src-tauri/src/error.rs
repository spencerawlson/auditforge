//! The error type commands return.
//!
//! Wraps the core error in the serializable payload the frontend understands, so every
//! failure arrives with a stable `kind` to branch on and a message safe to display.

use auditforge_core::{Error, ErrorPayload};

#[derive(Debug)]
pub struct CommandError(ErrorPayload);

impl From<Error> for CommandError {
    fn from(error: Error) -> Self {
        // Log the full error; hand the frontend only the redacted payload.
        tracing::debug!(kind = error.kind(), %error, "command failed");
        CommandError(ErrorPayload::from(&error))
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        CommandError::from(Error::from(error))
    }
}

impl serde::Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CommandError {}

pub type CommandResult<T> = std::result::Result<T, CommandError>;

/// Run blocking work on a worker thread so a long database write, a PDF render or a slow
/// model call cannot freeze the window.
pub async fn blocking<T, F>(work: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> auditforge_core::Result<T> + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(result) => result.map_err(CommandError::from),
        Err(error) => {
            tracing::error!(%error, "background task failed");
            Err(CommandError::from(Error::internal(
                "the operation could not be completed",
            )))
        }
    }
}
