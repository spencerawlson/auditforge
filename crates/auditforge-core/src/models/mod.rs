//! Domain records.
//!
//! These structs are the contract between the storage layer, the Tauri command layer and
//! the frontend: every one of them is `Serialize`/`Deserialize` and is handed to the UI
//! unchanged. Input structs (`New*`, `*Update`) are separate from stored records so a
//! client can never set server-owned fields such as ids, refs or timestamps.

pub mod ai;
pub mod audit;
pub mod common;
pub mod engagement;
pub mod evidence;
pub mod finding;
pub mod incident;
pub mod postmortem;
pub mod report;
pub mod user;

pub use ai::*;
pub use audit::*;
pub use common::*;
pub use engagement::*;
pub use evidence::*;
pub use finding::*;
pub use incident::*;
pub use postmortem::*;
pub use report::*;
pub use user::*;
