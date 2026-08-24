//! User and application settings.
//!
//! Settings are stored one JSON document per section, keyed by user. Storing whole
//! sections rather than individual keys keeps a section's fields consistent with each
//! other and means adding a field is a `serde` default rather than a migration.
//!
//! Secrets are never here. API keys live in the OS keychain; this table only records
//! *that* a key exists.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::auth::session::SessionPolicy;
use crate::database::new_id;
use crate::error::{Error, Result};
use crate::models::{AiSettings, Classification, Timestamp};

pub const SECTION_SECURITY: &str = "security";
pub const SECTION_AI: &str = "ai";
pub const SECTION_REPORTS: &str = "reports";
pub const SECTION_UI: &str = "ui";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecuritySettings {
    /// Sign out after this long without activity.
    pub idle_timeout_secs: i64,
    /// Hard cap on a session's lifetime regardless of activity.
    pub absolute_timeout_secs: i64,
    /// Encrypt newly imported evidence at rest by default.
    pub encrypt_evidence_by_default: bool,
    /// Warn before importing evidence types that are commonly executable.
    pub warn_on_risky_evidence: bool,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        let policy = SessionPolicy::default();
        SecuritySettings {
            idle_timeout_secs: policy.idle_timeout_secs,
            absolute_timeout_secs: policy.absolute_timeout_secs,
            encrypt_evidence_by_default: false,
            warn_on_risky_evidence: true,
        }
    }
}

impl SecuritySettings {
    pub fn session_policy(&self) -> SessionPolicy {
        SessionPolicy::validated(self.idle_timeout_secs, self.absolute_timeout_secs)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageSize {
    A4,
    Letter,
}

impl PageSize {
    /// Width and height in PDF points (1/72 inch).
    pub fn dimensions(&self) -> (f32, f32) {
        match self {
            PageSize::A4 => (595.28, 841.89),
            PageSize::Letter => (612.0, 792.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PdfSettings {
    pub page_size: PageSize,
    pub include_table_of_contents: bool,
    pub include_evidence_appendix: bool,
    /// Embed image evidence into the appendix. Off keeps documents small and avoids
    /// re-publishing screenshots that may contain more than intended.
    pub include_evidence_images: bool,
    /// Print a marker next to text that came from an AI suggestion.
    pub show_provenance_markers: bool,
    pub footer_note: String,
}

impl Default for PdfSettings {
    fn default() -> Self {
        PdfSettings {
            page_size: PageSize::A4,
            include_table_of_contents: true,
            include_evidence_appendix: true,
            include_evidence_images: true,
            show_provenance_markers: false,
            footer_note: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportSettings {
    pub default_template_key: String,
    pub default_classification: Classification,
    pub pdf: PdfSettings,
}

impl Default for ReportSettings {
    fn default() -> Self {
        ReportSettings {
            default_template_key: "penetration_test".to_string(),
            default_classification: Classification::Confidential,
            pdf: PdfSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub theme: Theme,
    pub compact_density: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        UiSettings {
            theme: Theme::System,
            compact_density: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub security: SecuritySettings,
    pub ai: AiSettings,
    pub reports: ReportSettings,
    pub ui: UiSettings,
}

pub fn load(conn: &Connection, user_id: &str) -> Result<AppSettings> {
    Ok(AppSettings {
        security: load_section(conn, user_id, SECTION_SECURITY)?,
        ai: load_section(conn, user_id, SECTION_AI)?,
        reports: load_section(conn, user_id, SECTION_REPORTS)?,
        ui: load_section(conn, user_id, SECTION_UI)?,
    })
}

/// Read one section, falling back to defaults when absent or unreadable.
///
/// A settings row that fails to parse (hand-edited, or written by a newer build) must
/// not stop the application from starting, so it degrades to defaults with a warning
/// rather than an error.
pub fn load_section<T: Default + serde::de::DeserializeOwned>(
    conn: &Connection,
    user_id: &str,
    section: &str,
) -> Result<T> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value_json FROM settings WHERE user_id = ?1 AND key = ?2",
            rusqlite::params![user_id, section],
            |row| row.get(0),
        )
        .optional()?;
    match raw {
        Some(json) => Ok(serde_json::from_str(&json).unwrap_or_else(|error| {
            tracing::warn!(section, %error, "settings section unreadable; using defaults");
            T::default()
        })),
        None => Ok(T::default()),
    }
}

pub fn save_section<T: Serialize>(
    conn: &Connection,
    user_id: &str,
    section: &str,
    value: &T,
) -> Result<()> {
    let json = serde_json::to_string(value)?;
    conn.execute(
        "INSERT INTO settings (id, user_id, key, value_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(user_id, key) DO UPDATE SET value_json = ?4, updated_at = ?5",
        rusqlite::params![new_id(), user_id, section, json, Timestamp::now()],
    )?;
    Ok(())
}

/// Validate before saving so an out-of-range timeout cannot lock a user out of their own
/// session or make the idle timeout meaningless.
pub fn validate_security(settings: &SecuritySettings) -> Result<()> {
    SessionPolicy::validated(settings.idle_timeout_secs, settings.absolute_timeout_secs)?;
    Ok(())
}

pub fn validate_ai(settings: &AiSettings) -> Result<()> {
    if !(0.0..=2.0).contains(&settings.temperature) {
        return Err(Error::validation("temperature must be between 0.0 and 2.0"));
    }
    if settings.max_output_tokens == 0 || settings.max_output_tokens > 32_000 {
        return Err(Error::validation(
            "maximum output tokens must be between 1 and 32000",
        ));
    }
    if !(5..=600).contains(&settings.request_timeout_secs) {
        return Err(Error::validation(
            "request timeout must be between 5 and 600 seconds",
        ));
    }
    check_endpoint(&settings.openai_base_url, "OpenAI base URL", true)?;
    check_endpoint(&settings.local_endpoint, "local AI endpoint", false)?;
    Ok(())
}

/// Reject endpoints that are not HTTP(S). The cloud endpoint must additionally be HTTPS:
/// a plaintext cloud endpoint would send report text over the network in the clear.
fn check_endpoint(url: &str, field: &str, require_tls: bool) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(Error::validation(format!("{field} is required")));
    }
    let is_https = trimmed.starts_with("https://");
    let is_http = trimmed.starts_with("http://");
    if !is_https && !is_http {
        return Err(Error::validation(format!(
            "{field} must start with http:// or https://"
        )));
    }
    if require_tls && !is_https {
        return Err(Error::validation(format!("{field} must use https")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::models::AiProviderKind;

    fn db_with_user() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, username, password_hash, full_name, password_changed_at,
                                    created_at, updated_at)
                 VALUES ('u1', 'analyst', 'x', 'Analyst', ?1, ?1, ?1)",
                rusqlite::params![Timestamp::now()],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    #[test]
    fn defaults_are_returned_when_nothing_is_stored() {
        let db = db_with_user();
        let settings = db.with_conn(|conn| load(conn, "u1")).unwrap();
        assert_eq!(settings.ai.provider, AiProviderKind::Disabled);
        assert_eq!(
            settings.reports.default_classification,
            Classification::Confidential
        );
        assert_eq!(settings.ui.theme, Theme::System);
        assert!(settings.reports.pdf.include_table_of_contents);
    }

    #[test]
    fn sections_round_trip() {
        let db = db_with_user();
        let ai = AiSettings {
            provider: AiProviderKind::Local,
            local_model: "qwen2.5:14b".to_string(),
            ..Default::default()
        };
        db.with_conn(|conn| save_section(conn, "u1", SECTION_AI, &ai))
            .unwrap();

        let loaded = db.with_conn(|conn| load(conn, "u1")).unwrap();
        assert_eq!(loaded.ai.provider, AiProviderKind::Local);
        assert_eq!(loaded.ai.local_model, "qwen2.5:14b");
    }

    #[test]
    fn saving_twice_updates_rather_than_duplicating() {
        let db = db_with_user();
        let ui = UiSettings {
            theme: Theme::Dark,
            compact_density: true,
        };
        db.with_conn(|conn| save_section(conn, "u1", SECTION_UI, &ui))
            .unwrap();
        db.with_conn(|conn| save_section(conn, "u1", SECTION_UI, &ui))
            .unwrap();
        let count: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM settings WHERE user_id = 'u1' AND key = 'ui'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn a_corrupt_section_falls_back_to_defaults_instead_of_failing_startup() {
        let db = db_with_user();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO settings (id, user_id, key, value_json, updated_at)
                 VALUES ('s1', 'u1', 'ui', '{not json', ?1)",
                rusqlite::params![Timestamp::now()],
            )?;
            Ok(())
        })
        .unwrap();
        let settings = db.with_conn(|conn| load(conn, "u1")).unwrap();
        assert_eq!(settings.ui.theme, Theme::System);
    }

    #[test]
    fn settings_are_per_user() {
        let db = db_with_user();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, username, password_hash, full_name, password_changed_at,
                                    created_at, updated_at)
                 VALUES ('u2', 'other', 'x', 'Other', ?1, ?1, ?1)",
                rusqlite::params![Timestamp::now()],
            )?;
            Ok(())
        })
        .unwrap();

        let dark = UiSettings {
            theme: Theme::Dark,
            compact_density: false,
        };
        db.with_conn(|conn| save_section(conn, "u1", SECTION_UI, &dark))
            .unwrap();

        assert_eq!(
            db.with_conn(|conn| load(conn, "u1")).unwrap().ui.theme,
            Theme::Dark
        );
        assert_eq!(
            db.with_conn(|conn| load(conn, "u2")).unwrap().ui.theme,
            Theme::System
        );
    }

    #[test]
    fn ai_validation_rejects_bad_values() {
        assert!(validate_ai(&AiSettings {
            temperature: 5.0,
            ..Default::default()
        })
        .is_err());
        assert!(
            validate_ai(&AiSettings {
                openai_base_url: "http://api.openai.com/v1".to_string(),
                ..Default::default()
            })
            .is_err(),
            "cloud endpoint must require TLS"
        );
        assert!(validate_ai(&AiSettings {
            local_endpoint: "ftp://localhost".to_string(),
            ..Default::default()
        })
        .is_err());
        assert!(validate_ai(&AiSettings {
            max_output_tokens: 0,
            ..Default::default()
        })
        .is_err());

        assert!(validate_ai(&AiSettings::default()).is_ok());
    }

    #[test]
    fn security_validation_rejects_impossible_timeouts() {
        assert!(validate_security(&SecuritySettings {
            idle_timeout_secs: 5,
            ..Default::default()
        })
        .is_err());
        assert!(validate_security(&SecuritySettings {
            idle_timeout_secs: 1800,
            absolute_timeout_secs: 60,
            ..Default::default()
        })
        .is_err());

        assert!(validate_security(&SecuritySettings::default()).is_ok());
    }

    #[test]
    fn page_sizes_are_portrait_and_plausible() {
        for size in [PageSize::A4, PageSize::Letter] {
            let (width, height) = size.dimensions();
            assert!(width > 400.0 && width < 700.0);
            assert!(height > width);
        }
    }
}
