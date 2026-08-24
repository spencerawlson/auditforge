//! Post-import touch-ups for the LabNet engagements: mark them completed and render each
//! finalized report to a PDF for review. Idempotent.
//!
//! ```text
//! cargo run -p auditforge-core --example finish_lab_reports -- "<db path>" "<output dir>"
//! ```

use std::path::{Path, PathBuf};

use auditforge_core::app::{App, AppPaths};
use auditforge_core::export::ExportFormat;
use auditforge_core::models::*;
use auditforge_core::security::secrets::MemorySecretStore;
use auditforge_core::{engagements, reports};

fn main() -> auditforge_core::Result<()> {
    let db_path = PathBuf::from(std::env::args().nth(1).expect("usage: <db path> <out dir>"));
    let out_dir = PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| ".".into()));

    // Point the App at the real data directory so evidence and settings resolve, but keep
    // secrets in memory so this tool never touches the keychain.
    let paths = AppPaths::with_base(db_path.parent().unwrap_or(Path::new(".")));
    let app = App::bootstrap(paths)?.with_secret_store(Box::new(MemorySecretStore::default()));

    let actor = app.db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, username FROM users ORDER BY created_at LIMIT 1",
            [],
            |row| {
                Ok(Actor {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    session_id: String::new(),
                })
            },
        )
        .map_err(|_| auditforge_core::Error::validation("no account"))
    })?;

    // Mark the LabNet engagements completed (they hold finalized reports).
    let ids: Vec<(String, String)> = app.db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, engagement_ref FROM engagements WHERE name LIKE 'LabNet Internal Assessment%'
             ORDER BY engagement_ref",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    for (id, reference) in &ids {
        let engagement = app.db.with_conn(|conn| engagements::get(conn, id))?;
        if engagement.status != EngagementStatus::Completed {
            engagements::update(
                &app.db,
                &actor,
                id,
                &EngagementUpdate {
                    status: Some(EngagementStatus::Completed),
                    ..Default::default()
                },
            )?;
            println!("{reference}: marked completed");
        }
    }

    // Render each finalized report to PDF.
    std::fs::create_dir_all(&out_dir)?;
    let reports: Vec<(String, String)> = app.db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, report_ref FROM reports WHERE status = 'final' ORDER BY report_ref",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    for (id, reference) in &reports {
        let exported = app.export(&actor, id, None, ExportFormat::Pdf)?;
        let path = out_dir.join(&exported.filename);
        std::fs::write(&path, &exported.bytes)?;
        println!(
            "{reference}: {} ({} bytes)",
            path.display(),
            exported.bytes.len()
        );
    }
    let _ = reports::get; // keep the import referenced for clarity of intent
    Ok(())
}
