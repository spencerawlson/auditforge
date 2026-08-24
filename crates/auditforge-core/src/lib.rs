//! AuditForge core.
//!
//! Everything the application does that is not a window lives here: storage, accounts,
//! engagements, findings, evidence, the guided template engine, the AI provider
//! abstraction, document rendering and export. The Tauri shell is a thin adapter over
//! this crate, which is also why a command-line front end remains possible later without
//! moving any logic.
//!
//! Two rules shape the design:
//!
//! 1. **The human owns the facts.** AI output is a *suggestion* until a person accepts
//!    it, and every stored string carries a [`models::Provenance`] saying where it came
//!    from. Nothing in this crate writes model output into a report on its own.
//! 2. **Finalized means finalized.** A report version marked final is immutable in the
//!    application layer and in the database (see the triggers in migration 0001).
//!    Changing a finalized report means creating a new revision.

#![forbid(unsafe_code)]
#![warn(clippy::all)]

pub mod ai;
pub mod app;
pub mod audit;
pub mod auth;
pub mod database;
pub mod engagements;
pub mod error;
pub mod evidence;
pub mod export;
pub mod findings;
pub mod incidents;
pub mod models;
pub mod pdf;
pub mod postmortems;
pub mod reports;
pub mod search;
pub mod security;
pub mod settings;
pub mod templates;

#[cfg(test)]
pub(crate) mod test_support;

pub use app::{App, AppPaths};
pub use error::{Error, ErrorPayload, Result};

/// Version of the application, taken from the crate manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
