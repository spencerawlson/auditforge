//! The application facade.
//!
//! Everything a front end needs is a method on [`App`]: the Tauri shell is a thin
//! translation layer over this type, and a command-line front end would be another. Where
//! an operation spans several modules - rendering a PDF needs the report, its template,
//! settings and the evidence vault - the wiring lives here rather than in the caller.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ai::AiService;
use crate::auth::session::SessionPolicy;
use crate::database::Database;
use crate::error::{Error, Result};
use crate::export::{self, ExportFormat};
use crate::models::{
    Actor, AiSettings, EngagementSummary, EvidenceSnapshot, Finding, Postmortem, ReportStatus,
    ReportSummary, ReportVersion,
};
use crate::pdf::{self, RenderOptions};
use crate::security::crypto::VaultKey;
use crate::security::secrets::{self, SecretStore};
use crate::settings::{self, AppSettings};
use crate::{engagements, evidence, findings, postmortems, reports, templates};

/// Where everything lives on disk.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    /// Managed evidence vault. Files are copied in; nothing is referenced in place.
    pub evidence_dir: PathBuf,
    /// User-supplied report templates, loaded on top of the built-in ones.
    pub templates_dir: PathBuf,
    pub exports_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl AppPaths {
    /// The per-user application data directory for the current platform.
    ///
    /// Resolved here rather than through a crate so the layout is explicit and identical
    /// on the two target distributions.
    pub fn resolve() -> Result<Self> {
        let base = if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or_else(|| Error::internal("APPDATA is not set"))?
                .join("AuditForge")
        } else if cfg!(target_os = "macos") {
            home_dir()?.join("Library/Application Support/AuditForge")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or(home_dir()?.join(".local/share"))
                .join("auditforge")
        };
        Ok(AppPaths::with_base(base))
    }

    pub fn with_base(base: impl Into<PathBuf>) -> Self {
        let data_dir: PathBuf = base.into();
        AppPaths {
            database_path: data_dir.join("auditforge.db"),
            evidence_dir: data_dir.join("evidence"),
            templates_dir: data_dir.join("templates"),
            exports_dir: data_dir.join("exports"),
            logs_dir: data_dir.join("logs"),
            data_dir,
        }
    }

    pub fn ensure(&self) -> Result<()> {
        for directory in [
            &self.data_dir,
            &self.evidence_dir,
            &self.templates_dir,
            &self.exports_dir,
            &self.logs_dir,
        ] {
            std::fs::create_dir_all(directory)?;
        }
        restrict(&self.data_dir)?;
        restrict(&self.evidence_dir)?;
        Ok(())
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::internal("HOME is not set"))
}

/// Owner-only on Unix. The evidence vault and the database both hold client material.
#[cfg(unix)]
fn restrict(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<()> {
    Ok(())
}

pub struct App {
    pub db: Database,
    pub paths: AppPaths,
    secrets: Box<dyn SecretStore>,
}

impl App {
    /// Open the database, run migrations, load templates and open the secret store.
    pub fn bootstrap(paths: AppPaths) -> Result<Self> {
        paths.ensure()?;
        let db = Database::open(&paths.database_path)?;
        db.with_conn(templates::sync_builtins)?;
        db.with_conn(|conn| templates::import_user_templates(conn, &paths.templates_dir))?;
        let secrets = secrets::open_default(&paths.data_dir);
        Ok(App { db, paths, secrets })
    }

    /// An entirely in-memory instance. Used by tests and by `--check`.
    pub fn in_memory() -> Result<Self> {
        let db = Database::open_in_memory()?;
        db.with_conn(templates::sync_builtins)?;
        Ok(App {
            db,
            paths: AppPaths::with_base(std::env::temp_dir().join("auditforge-ephemeral")),
            secrets: Box::new(secrets::MemorySecretStore::default()),
        })
    }

    /// Replace the secret store. Used by tests to avoid touching the real keychain.
    pub fn with_secret_store(mut self, store: Box<dyn SecretStore>) -> Self {
        self.secrets = store;
        self
    }

    pub fn secret_store(&self) -> &dyn SecretStore {
        self.secrets.as_ref()
    }

    // -------------------------------------------------------------- settings

    pub fn settings(&self, user_id: &str) -> Result<AppSettings> {
        let mut settings = self.db.with_conn(|conn| settings::load(conn, user_id))?;
        // The key itself never leaves the keychain; the UI only needs to know one exists.
        settings.ai.has_api_key = self.secrets.get(secrets::KEY_OPENAI_API)?.is_some();
        Ok(settings)
    }

    /// Session timeouts for a user, falling back to defaults before anyone has signed in.
    pub fn session_policy(&self, user_id: Option<&str>) -> SessionPolicy {
        let Some(user_id) = user_id else {
            return SessionPolicy::default();
        };
        self.db
            .with_conn(|conn| {
                settings::load_section::<settings::SecuritySettings>(
                    conn,
                    user_id,
                    settings::SECTION_SECURITY,
                )
            })
            .map(|security| security.session_policy())
            .unwrap_or_default()
    }

    pub fn store_api_key(&self, actor: &Actor, key: &str) -> Result<()> {
        let key = key.trim();
        if key.is_empty() {
            return Err(Error::validation("the API key is empty"));
        }
        self.secrets.set(secrets::KEY_OPENAI_API, key)?;
        self.db.with_tx(|tx| {
            crate::audit::success(
                tx,
                actor,
                crate::models::AuditAction::SecretStored,
                "secret",
                secrets::KEY_OPENAI_API,
                serde_json::json!({ "backend": self.secrets.backend_name() }),
            )
        })
    }

    pub fn clear_api_key(&self, actor: &Actor) -> Result<()> {
        self.secrets.delete(secrets::KEY_OPENAI_API)?;
        self.db.with_tx(|tx| {
            crate::audit::success(
                tx,
                actor,
                crate::models::AuditAction::SecretCleared,
                "secret",
                secrets::KEY_OPENAI_API,
                serde_json::json!({}),
            )
        })
    }

    // -------------------------------------------------------------- AI

    /// Build the AI service for a user from their settings and stored key.
    pub fn ai_service(&self, user_id: &str) -> Result<AiService> {
        let ai: AiSettings = self
            .db
            .with_conn(|conn| settings::load_section(conn, user_id, settings::SECTION_AI))?;
        let api_key = self.secrets.get(secrets::KEY_OPENAI_API).unwrap_or(None);
        Ok(AiService::from_settings(&ai, api_key))
    }

    // -------------------------------------------------------------- evidence

    /// The evidence vault key, created on first use.
    ///
    /// Returns `None` when no secret store is available, in which case encryption is
    /// offered as unavailable rather than silently writing evidence in the clear under a
    /// key nobody can reproduce.
    pub fn vault_key(&self) -> Option<VaultKey> {
        match self.secrets.get(secrets::KEY_EVIDENCE_VAULT) {
            Ok(Some(encoded)) => VaultKey::from_base64(&encoded).ok(),
            Ok(None) => {
                let key = VaultKey::generate();
                match self
                    .secrets
                    .set(secrets::KEY_EVIDENCE_VAULT, &key.to_base64())
                {
                    Ok(()) => Some(key),
                    Err(error) => {
                        tracing::warn!(%error, "could not store the evidence vault key");
                        None
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "could not read the evidence vault key");
                None
            }
        }
    }

    // -------------------------------------------------------------- reports

    /// Load a version: the current one, or a specific one from the history.
    pub fn report_version(
        &self,
        report_id: &str,
        version_id: Option<&str>,
    ) -> Result<ReportVersion> {
        self.db.with_conn(|conn| match version_id {
            Some(version_id) => {
                let version = reports::load_version(conn, version_id)?;
                if version.report_id != report_id {
                    return Err(Error::not_found("report version"));
                }
                Ok(version)
            }
            None => reports::current_version(conn, report_id),
        })
    }

    /// Render a report version to PDF.
    ///
    /// Draft versions can be previewed; only a finalized version produces the document a
    /// reader is meant to receive, and the two are distinguishable because the version
    /// label is printed on every page.
    pub fn render_pdf(
        &self,
        actor: &Actor,
        report_id: &str,
        version_id: Option<&str>,
    ) -> Result<Vec<u8>> {
        let version = self.report_version(report_id, version_id)?;
        let user_settings = self.settings(&actor.user_id)?;
        let template = self
            .db
            .with_conn(|conn| templates::get(conn, &version.content.template_key))
            .ok();

        let vault_key = self.vault_key();
        let load_evidence = |snapshot: &EvidenceSnapshot| -> Option<Vec<u8>> {
            self.db
                .with_conn(|conn| {
                    let id: String = conn.query_row(
                        "SELECT id FROM evidence WHERE evidence_ref = ?1",
                        [&snapshot.evidence_ref],
                        |row| row.get(0),
                    )?;
                    evidence::read_bytes(conn, &self.paths.evidence_dir, vault_key.as_ref(), &id)
                })
                .ok()
        };

        let options = RenderOptions {
            settings: user_settings.reports.pdf.clone(),
            footer_note: user_settings.reports.pdf.footer_note.clone(),
        };
        let bytes = pdf::render(
            &version.content,
            template.as_ref(),
            &options,
            &load_evidence,
        )?;

        self.db.with_tx(|tx| {
            crate::audit::success(
                tx,
                actor,
                crate::models::AuditAction::PdfGenerated,
                "report",
                report_id,
                serde_json::json!({
                    "version": version.version_label,
                    "bytes": bytes.len(),
                    "finalized": version.status == ReportStatus::Final,
                }),
            )
        })?;
        Ok(bytes)
    }

    /// Export a report version in `format`, returning the bytes and a suggested filename.
    pub fn export(
        &self,
        actor: &Actor,
        report_id: &str,
        version_id: Option<&str>,
        format: ExportFormat,
    ) -> Result<ExportedFile> {
        let version = self.report_version(report_id, version_id)?;
        let template = self
            .db
            .with_conn(|conn| templates::get(conn, &version.content.template_key))
            .ok();

        let bytes = match format {
            ExportFormat::Pdf => self.render_pdf(actor, report_id, version_id)?,
            ExportFormat::Html => export::to_html(&version.content, template.as_ref()).into_bytes(),
            ExportFormat::Json => export::to_json(&version.content)?.into_bytes(),
            ExportFormat::Markdown => {
                export::to_markdown(&version.content, template.as_ref()).into_bytes()
            }
            ExportFormat::Csv => export::findings_to_csv(&version.content.findings).into_bytes(),
        };

        self.db.with_tx(|tx| {
            crate::audit::success(
                tx,
                actor,
                crate::models::AuditAction::ReportExported,
                "report",
                report_id,
                serde_json::json!({
                    "format": format.extension(),
                    "version": version.version_label,
                    "bytes": bytes.len(),
                }),
            )
        })?;

        Ok(ExportedFile {
            filename: export::suggested_filename(&version.content, format),
            bytes,
            format,
        })
    }

    /// Write an export into the exports directory and return the path.
    pub fn export_to_disk(
        &self,
        actor: &Actor,
        report_id: &str,
        version_id: Option<&str>,
        format: ExportFormat,
    ) -> Result<PathBuf> {
        let exported = self.export(actor, report_id, version_id, format)?;
        std::fs::create_dir_all(&self.paths.exports_dir)?;
        let path = self.paths.exports_dir.join(&exported.filename);
        std::fs::write(&path, &exported.bytes)?;
        Ok(path)
    }

    // -------------------------------------------------------------- dashboard

    pub fn dashboard(&self, actor: &Actor) -> Result<Dashboard> {
        self.db.with_conn(|conn| {
            let recent_reports = reports::list(
                conn,
                &reports::ReportFilter {
                    limit: Some(8),
                    ..Default::default()
                },
            )?;
            let drafts = reports::list(
                conn,
                &reports::ReportFilter {
                    status: Some(ReportStatus::Draft),
                    limit: Some(8),
                    ..Default::default()
                },
            )?;
            let awaiting_review = reports::list(
                conn,
                &reports::ReportFilter {
                    status: Some(ReportStatus::Review),
                    limit: Some(8),
                    ..Default::default()
                },
            )?;
            let ready_to_export = reports::list(
                conn,
                &reports::ReportFilter {
                    status: Some(ReportStatus::Final),
                    limit: Some(8),
                    ..Default::default()
                },
            )?;
            let active_engagements = engagements::list(
                conn,
                &engagements::EngagementFilter {
                    status: Some(crate::models::EngagementStatus::Active),
                    limit: Some(8),
                    ..Default::default()
                },
            )?;
            let recent_findings = findings::list(
                conn,
                &findings::FindingFilter {
                    limit: Some(8),
                    ..Default::default()
                },
            )?;
            let recent_postmortems = postmortems::list(conn, Some(5))?;

            let counts = Counts {
                engagements: count(conn, "engagements")?,
                reports: count(conn, "reports")?,
                findings: count(conn, "findings")?,
                evidence: count(conn, "evidence")?,
                incidents: count(conn, "incidents")?,
                postmortems: count(conn, "postmortems")?,
                unconfirmed_severities: conn.query_row(
                    "SELECT COUNT(*) FROM findings WHERE severity_confirmed = 0",
                    [],
                    |row| row.get(0),
                )?,
            };

            Ok(Dashboard {
                user_id: actor.user_id.clone(),
                counts,
                recent_reports,
                drafts,
                awaiting_review,
                ready_to_export,
                active_engagements,
                recent_findings,
                recent_postmortems,
            })
        })
    }
}

fn count(conn: &rusqlite::Connection, table: &str) -> Result<i64> {
    // `table` is one of the fixed literals above, never user input.
    Ok(
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?,
    )
}

pub struct ExportedFile {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub format: ExportFormat,
}

#[derive(Debug, Clone, Serialize)]
pub struct Counts {
    pub engagements: i64,
    pub reports: i64,
    pub findings: i64,
    pub evidence: i64,
    pub incidents: i64,
    pub postmortems: i64,
    /// Findings whose severity no human has confirmed. These block finalization.
    pub unconfirmed_severities: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dashboard {
    pub user_id: String,
    pub counts: Counts,
    pub recent_reports: Vec<ReportSummary>,
    pub drafts: Vec<ReportSummary>,
    pub awaiting_review: Vec<ReportSummary>,
    pub ready_to_export: Vec<ReportSummary>,
    pub active_engagements: Vec<EngagementSummary>,
    pub recent_findings: Vec<Finding>,
    pub recent_postmortems: Vec<Postmortem>,
}

#[cfg(test)]
mod tests;
