//! Tauri command surface.
//!
//! One module per area, mirroring the core's modules. Every command that writes resolves
//! an `Actor` from the session first, so an unauthenticated call fails before it can
//! reach the database.

pub mod ai;
pub mod auth;
pub mod reports;
pub mod settings;
pub mod workspace;
