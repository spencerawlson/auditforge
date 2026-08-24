//! The evidence vault.
//!
//! Files are copied into a managed directory under the application data folder, never
//! referenced in place: a report that cites evidence must still resolve after the user
//! tidies their Downloads folder. Inside the vault:
//!
//! * the stored name is sanitized and prefixed with the evidence reference, so two files
//!   called `screenshot.png` cannot collide;
//! * every path is proved to land inside the vault root before anything is written;
//! * the SHA-256 of the original bytes is recorded so a reader can verify the file;
//! * files are written owner-only and never with an executable bit;
//! * bytes can be encrypted at rest with the vault key.
//!
//! Nothing in AuditForge executes evidence or hands it to the shell. Reading it back is
//! an explicit call, and image rendering in reports is limited to formats the renderer
//! decodes itself.

pub mod validation;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::audit;
use crate::database::{escape_like, new_id, next_reference, Database};
use crate::error::{Error, Result};
use crate::models::{
    Actor, AuditAction, EntityType, Evidence, EvidenceKind, EvidenceLink, EvidenceLinkRequest,
    EvidenceUpdate, NewEvidence, Timestamp,
};
use crate::security::crypto::{self, VaultKey};
use crate::security::paths;
use validation::MAX_EVIDENCE_BYTES;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EvidenceFilter {
    #[serde(default)]
    pub engagement_id: Option<String>,
    #[serde(default)]
    pub kind: Option<EvidenceKind>,
    #[serde(default)]
    pub target_type: Option<EntityType>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Import a file into the vault.
pub fn import(
    db: &Database,
    actor: &Actor,
    root: &Path,
    key: Option<&VaultKey>,
    input: &NewEvidence,
) -> Result<Evidence> {
    let title = require(&input.title, "evidence title", 300)?;
    let source = paths::validate_import_source(&input.source_path, MAX_EVIDENCE_BYTES)?;
    let original_filename = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("evidence")
        .to_string();

    let bytes = std::fs::read(&source)?;
    if bytes.len() as u64 > MAX_EVIDENCE_BYTES {
        return Err(Error::validation(
            "that file is larger than the evidence size limit",
        ));
    }

    let detected = validation::detect(&original_filename, &bytes);
    validation::check_policy(&detected, &original_filename, input.acknowledge_executable)?;

    if input.encrypt && key.is_none() {
        return Err(Error::validation(
            "evidence encryption is unavailable because no vault key could be opened",
        ));
    }

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = format!("{:x}", hasher.finalize());
    let stored_filename = paths::sanitize_filename(&original_filename);
    let year = Timestamp::now().inner().format("%Y").to_string();

    let id = db.with_tx(|tx| {
        let id = new_id();
        let evidence_ref = next_reference(tx, "evidence", "EVD")?;
        let relative_path = format!("{year}/{evidence_ref}_{stored_filename}");
        let destination = paths::resolve_within(root, &relative_path)?;

        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let payload = match (input.encrypt, key) {
            (true, Some(key)) => crypto::encrypt(key, &bytes, evidence_ref.as_bytes())?,
            _ => bytes.clone(),
        };
        write_private(&destination, &payload)?;

        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO evidence (id, evidence_ref, engagement_id, title, description,
                                   original_filename, stored_filename, relative_path, mime_type,
                                   kind, byte_size, sha256, is_encrypted, captured_at, created_by,
                                   created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
            rusqlite::params![
                id,
                evidence_ref,
                input.engagement_id,
                title,
                clamp(&input.description, 5_000),
                original_filename,
                stored_filename,
                relative_path,
                detected.mime_type,
                detected.kind,
                bytes.len() as i64,
                sha256,
                i64::from(input.encrypt),
                input.captured_at,
                actor.user_id,
                now,
            ],
        )?;

        if let Some(link) = &input.link_target {
            insert_link(tx, &id, link)?;
        }

        audit::success(
            tx,
            actor,
            AuditAction::EvidenceAdded,
            "evidence",
            &id,
            json!({
                "ref": evidence_ref,
                "filename": original_filename,
                "mime_type": detected.mime_type,
                "bytes": bytes.len(),
                "sha256": sha256,
                "encrypted": input.encrypt,
                "executable_acknowledged": input.acknowledge_executable && detected.risky,
                "extension_mismatch": detected.extension_mismatch,
            }),
        )?;
        Ok(id)
    })?;

    db.with_conn(|conn| get(conn, &id))
}

pub fn get(conn: &Connection, id: &str) -> Result<Evidence> {
    let mut evidence = conn
        .query_row(SELECT_EVIDENCE, [id], row_to_evidence)
        .optional()?
        .ok_or_else(|| Error::not_found("evidence"))?;
    evidence.links = load_links(conn, id)?;
    Ok(evidence)
}

const SELECT_EVIDENCE: &str = "SELECT id, evidence_ref, engagement_id, title, description,
        original_filename, stored_filename, relative_path, mime_type, kind, byte_size, sha256,
        is_encrypted, captured_at, created_by, created_at, updated_at
    FROM evidence WHERE id = ?1";

fn row_to_evidence(row: &rusqlite::Row<'_>) -> rusqlite::Result<Evidence> {
    Ok(Evidence {
        id: row.get(0)?,
        evidence_ref: row.get(1)?,
        engagement_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        original_filename: row.get(5)?,
        stored_filename: row.get(6)?,
        relative_path: row.get(7)?,
        mime_type: row.get(8)?,
        kind: row.get(9)?,
        byte_size: row.get(10)?,
        sha256: row.get(11)?,
        is_encrypted: row.get::<_, i64>(12)? != 0,
        captured_at: row.get(13)?,
        created_by: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        links: Vec::new(),
    })
}

fn load_links(conn: &Connection, evidence_id: &str) -> Result<Vec<EvidenceLink>> {
    let mut stmt = conn.prepare(
        "SELECT id, target_type, target_id, created_at FROM evidence_links
         WHERE evidence_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map([evidence_id], |row| {
            Ok(EvidenceLink {
                id: row.get(0)?,
                target_type: row.get(1)?,
                target_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list(conn: &Connection, filter: &EvidenceFilter) -> Result<Vec<Evidence>> {
    let mut sql = String::from(
        "SELECT e.id, e.evidence_ref, e.engagement_id, e.title, e.description,
                e.original_filename, e.stored_filename, e.relative_path, e.mime_type, e.kind,
                e.byte_size, e.sha256, e.is_encrypted, e.captured_at, e.created_by, e.created_at,
                e.updated_at
         FROM evidence e WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(engagement_id) = &filter.engagement_id {
        sql.push_str(" AND e.engagement_id = ?");
        params.push(Box::new(engagement_id.clone()));
    }
    if let Some(kind) = filter.kind {
        sql.push_str(" AND e.kind = ?");
        params.push(Box::new(kind.as_str().to_string()));
    }
    if let (Some(target_type), Some(target_id)) = (filter.target_type, &filter.target_id) {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM evidence_links l
                          WHERE l.evidence_id = e.id AND l.target_type = ? AND l.target_id = ?)",
        );
        params.push(Box::new(target_type.as_str().to_string()));
        params.push(Box::new(target_id.clone()));
    }
    if let Some(search) = filter.search.as_ref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(
            " AND (e.title LIKE ? ESCAPE '\\' OR e.evidence_ref LIKE ? ESCAPE '\\'
                   OR e.original_filename LIKE ? ESCAPE '\\')",
        );
        let pattern = format!("%{}%", escape_like(search.trim()));
        for _ in 0..3 {
            params.push(Box::new(pattern.clone()));
        }
    }
    sql.push_str(" ORDER BY e.created_at DESC LIMIT ?");
    params.push(Box::new(filter.limit.unwrap_or(500).clamp(1, 5_000)));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut items = stmt
        .query_map(refs.as_slice(), row_to_evidence)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for item in &mut items {
        item.links = load_links(conn, &item.id)?;
    }
    Ok(items)
}

pub fn update(db: &Database, actor: &Actor, id: &str, update: &EvidenceUpdate) -> Result<Evidence> {
    db.with_tx(|tx| {
        let mut evidence = get(tx, id)?;
        if let Some(title) = &update.title {
            evidence.title = require(title, "evidence title", 300)?;
        }
        if let Some(description) = &update.description {
            evidence.description = clamp(description, 5_000);
        }
        if let Some(engagement_id) = &update.engagement_id {
            evidence.engagement_id = engagement_id.clone();
        }
        if let Some(captured_at) = update.captured_at {
            evidence.captured_at = captured_at;
        }
        tx.execute(
            "UPDATE evidence SET title = ?1, description = ?2, engagement_id = ?3,
                    captured_at = ?4, updated_at = ?5 WHERE id = ?6",
            rusqlite::params![
                evidence.title,
                evidence.description,
                evidence.engagement_id,
                evidence.captured_at,
                Timestamp::now(),
                id,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::EvidenceUpdated,
            "evidence",
            id,
            json!({ "ref": evidence.evidence_ref }),
        )?;
        get(tx, id)
    })
}

pub fn link(
    db: &Database,
    actor: &Actor,
    evidence_id: &str,
    request: &EvidenceLinkRequest,
) -> Result<Evidence> {
    db.with_tx(|tx| {
        get(tx, evidence_id)?;
        insert_link(tx, evidence_id, request)?;
        audit::success(
            tx,
            actor,
            AuditAction::EvidenceLinked,
            "evidence",
            evidence_id,
            json!({ "target_type": request.target_type.as_str(), "target_id": request.target_id }),
        )?;
        get(tx, evidence_id)
    })
}

pub fn unlink(
    db: &Database,
    actor: &Actor,
    evidence_id: &str,
    request: &EvidenceLinkRequest,
) -> Result<Evidence> {
    db.with_tx(|tx| {
        tx.execute(
            "DELETE FROM evidence_links WHERE evidence_id = ?1 AND target_type = ?2
             AND target_id = ?3",
            rusqlite::params![evidence_id, request.target_type, request.target_id],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::EvidenceLinked,
            "evidence",
            evidence_id,
            json!({ "unlinked": request.target_id }),
        )?;
        get(tx, evidence_id)
    })
}

fn insert_link(conn: &Connection, evidence_id: &str, request: &EvidenceLinkRequest) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO evidence_links (id, evidence_id, target_type, target_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            new_id(),
            evidence_id,
            request.target_type,
            request.target_id,
            Timestamp::now()
        ],
    )?;
    Ok(())
}

/// Read the stored bytes back, decrypting if necessary, and verify the digest.
///
/// The digest check is the point: it proves the file in the vault is the file that was
/// imported, which is what makes evidence citable in a report.
pub fn read_bytes(
    conn: &Connection,
    root: &Path,
    key: Option<&VaultKey>,
    id: &str,
) -> Result<Vec<u8>> {
    let evidence = get(conn, id)?;
    let path = paths::resolve_within(root, &evidence.relative_path)?;
    let stored = std::fs::read(&path).map_err(|_| {
        Error::Io(format!(
            "evidence {} is missing from the vault",
            evidence.evidence_ref
        ))
    })?;

    let plaintext = if evidence.is_encrypted {
        let key = key.ok_or_else(|| {
            Error::validation("this evidence is encrypted and the vault key is unavailable")
        })?;
        crypto::decrypt(key, &stored, evidence.evidence_ref.as_bytes())?
    } else {
        stored
    };

    let mut hasher = Sha256::new();
    hasher.update(&plaintext);
    let digest = format!("{:x}", hasher.finalize());
    if digest != evidence.sha256 {
        return Err(Error::validation(format!(
            "evidence {} does not match its recorded SHA-256; the file has changed",
            evidence.evidence_ref
        )));
    }
    Ok(plaintext)
}

/// Absolute path of a stored file, for callers that need to show where it lives.
/// Resolving through `resolve_within` means a tampered `relative_path` cannot point out
/// of the vault.
pub fn absolute_path(conn: &Connection, root: &Path, id: &str) -> Result<PathBuf> {
    let evidence = get(conn, id)?;
    paths::resolve_within(root, &evidence.relative_path)
}

pub fn delete(db: &Database, actor: &Actor, root: &Path, id: &str) -> Result<()> {
    db.with_tx(|tx| {
        let evidence = get(tx, id)?;
        if let Ok(path) = paths::resolve_within(root, &evidence.relative_path) {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        tx.execute("DELETE FROM evidence WHERE id = ?1", [id])?;
        audit::success(
            tx,
            actor,
            AuditAction::EvidenceDeleted,
            "evidence",
            id,
            json!({ "ref": evidence.evidence_ref, "sha256": evidence.sha256 }),
        )
    })
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 0600: owner read/write, and never executable.
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn require(raw: &str, field: &str, max: usize) -> Result<String> {
    let value = raw.trim().to_string();
    if value.is_empty() {
        return Err(Error::validation(format!("{field} is required")));
    }
    if value.chars().count() > max {
        return Err(Error::validation(format!("{field} is too long")));
    }
    Ok(value)
}

fn clamp(raw: &str, max: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    trimmed.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::seeded;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDRfake-image-bytes";

    struct Fixture {
        db: Database,
        actor: Actor,
        vault: tempfile::TempDir,
        source: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let (db, actor) = seeded();
            Fixture {
                db,
                actor,
                vault: tempfile::tempdir().unwrap(),
                source: tempfile::tempdir().unwrap(),
            }
        }

        fn write_source(&self, name: &str, bytes: &[u8]) -> String {
            let path = self.source.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path.to_str().unwrap().to_string()
        }

        fn root(&self) -> &Path {
            self.vault.path()
        }

        fn import_named(&self, name: &str, bytes: &[u8]) -> Result<Evidence> {
            import(
                &self.db,
                &self.actor,
                self.root(),
                None,
                &NewEvidence {
                    source_path: self.write_source(name, bytes),
                    title: format!("Evidence {name}"),
                    description: String::new(),
                    engagement_id: None,
                    captured_at: None,
                    encrypt: false,
                    acknowledge_executable: false,
                    link_target: None,
                },
            )
        }
    }

    #[test]
    fn import_copies_the_file_and_records_its_digest() {
        let fixture = Fixture::new();
        let evidence = fixture.import_named("screenshot.png", PNG).unwrap();

        assert!(evidence.evidence_ref.starts_with("EVD-"));
        assert_eq!(evidence.mime_type, "image/png");
        assert_eq!(evidence.kind, EvidenceKind::Image);
        assert_eq!(evidence.byte_size, PNG.len() as i64);

        let stored = fixture.root().join(&evidence.relative_path);
        assert!(stored.exists(), "the file must be copied into the vault");
        assert_eq!(std::fs::read(&stored).unwrap(), PNG);

        let read_back = fixture
            .db
            .with_conn(|conn| read_bytes(conn, fixture.root(), None, &evidence.id))
            .unwrap();
        assert_eq!(read_back, PNG);
    }

    #[test]
    fn the_original_file_is_left_untouched() {
        let fixture = Fixture::new();
        let source_path = fixture.source.path().join("original.log");
        let evidence = import(
            &fixture.db,
            &fixture.actor,
            fixture.root(),
            None,
            &NewEvidence {
                source_path: {
                    std::fs::write(&source_path, b"log line").unwrap();
                    source_path.to_str().unwrap().to_string()
                },
                title: "Original".to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: false,
                acknowledge_executable: false,
                link_target: None,
            },
        )
        .unwrap();
        assert!(source_path.exists(), "import must copy, not move");
        assert_eq!(evidence.original_filename, "original.log");
    }

    #[test]
    fn hostile_filenames_cannot_escape_the_vault() {
        let fixture = Fixture::new();
        // The picker hands us a real path; the dangerous part is the name we derive from
        // it, so import a file whose name is itself a traversal attempt.
        let evidence = fixture.import_named("..evil..log", b"content").unwrap();
        assert!(
            !evidence.relative_path.contains(".."),
            "got {}",
            evidence.relative_path
        );

        let stored = fixture.root().join(&evidence.relative_path);
        assert!(stored.starts_with(fixture.root()));
        assert!(stored.exists());
    }

    #[test]
    fn stored_paths_are_relative_and_prefixed_with_the_reference() {
        let fixture = Fixture::new();
        let first = fixture.import_named("screenshot.png", PNG).unwrap();
        let second = fixture.import_named("screenshot.png", PNG).unwrap();

        assert!(!Path::new(&first.relative_path).is_absolute());
        assert!(first.relative_path.contains(&first.evidence_ref));
        assert_ne!(
            first.relative_path, second.relative_path,
            "identically named files must not collide"
        );
    }

    #[test]
    fn executables_are_refused_unless_acknowledged() {
        let fixture = Fixture::new();
        let err = fixture
            .import_named("implant.exe", b"MZ\x90\x00")
            .unwrap_err();
        assert!(err.to_string().contains("executable"), "got {err}");

        let acknowledged = import(
            &fixture.db,
            &fixture.actor,
            fixture.root(),
            None,
            &NewEvidence {
                source_path: fixture.write_source("implant2.exe", b"MZ\x90\x00"),
                title: "Recovered sample".to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: false,
                acknowledge_executable: true,
                link_target: None,
            },
        )
        .unwrap();
        assert_eq!(acknowledged.kind, EvidenceKind::Other);
    }

    #[test]
    fn a_renamed_executable_is_still_refused() {
        let fixture = Fixture::new();
        let err = fixture
            .import_named("screenshot.png", b"MZ\x90\x00\x03")
            .unwrap_err();
        assert!(err.to_string().contains("executable"), "got {err}");
    }

    #[test]
    fn oversized_and_missing_files_are_refused() {
        let fixture = Fixture::new();
        let err = import(
            &fixture.db,
            &fixture.actor,
            fixture.root(),
            None,
            &NewEvidence {
                source_path: fixture
                    .source
                    .path()
                    .join("nope.txt")
                    .to_str()
                    .unwrap()
                    .to_string(),
                title: "Missing".to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: false,
                acknowledge_executable: false,
                link_target: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.kind(), "validation");

        // A directory is not a regular file.
        let err = import(
            &fixture.db,
            &fixture.actor,
            fixture.root(),
            None,
            &NewEvidence {
                source_path: fixture.source.path().to_str().unwrap().to_string(),
                title: "A directory".to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: false,
                acknowledge_executable: false,
                link_target: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.kind(), "validation");
    }

    #[test]
    fn encrypted_evidence_is_unreadable_on_disk_but_round_trips() {
        let fixture = Fixture::new();
        let key = VaultKey::generate();
        let evidence = import(
            &fixture.db,
            &fixture.actor,
            fixture.root(),
            Some(&key),
            &NewEvidence {
                source_path: fixture.write_source("creds.txt", b"admin:hunter2-canary"),
                title: "Recovered credentials".to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: true,
                acknowledge_executable: false,
                link_target: None,
            },
        )
        .unwrap();

        assert!(evidence.is_encrypted);
        let raw = std::fs::read(fixture.root().join(&evidence.relative_path)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("hunter2-canary"));

        let plaintext = fixture
            .db
            .with_conn(|conn| read_bytes(conn, fixture.root(), Some(&key), &evidence.id))
            .unwrap();
        assert_eq!(plaintext, b"admin:hunter2-canary");

        // Without the key it stays unreadable.
        assert!(fixture
            .db
            .with_conn(|conn| read_bytes(conn, fixture.root(), None, &evidence.id))
            .is_err());
    }

    #[test]
    fn tampering_with_a_stored_file_is_detected_on_read() {
        let fixture = Fixture::new();
        let evidence = fixture.import_named("scan.log", b"clean output").unwrap();
        std::fs::write(
            fixture.root().join(&evidence.relative_path),
            b"tampered output",
        )
        .unwrap();

        let err = fixture
            .db
            .with_conn(|conn| read_bytes(conn, fixture.root(), None, &evidence.id))
            .unwrap_err();
        assert!(err.to_string().contains("SHA-256"), "got {err}");
    }

    #[test]
    fn evidence_links_to_targets_and_can_be_filtered_by_them() {
        let fixture = Fixture::new();
        let evidence = fixture.import_named("proof.png", PNG).unwrap();
        let request = EvidenceLinkRequest {
            target_type: EntityType::Finding,
            target_id: "f-1".to_string(),
        };

        let linked = link(&fixture.db, &fixture.actor, &evidence.id, &request).unwrap();
        assert_eq!(linked.links.len(), 1);
        // Linking twice is idempotent rather than an error.
        let again = link(&fixture.db, &fixture.actor, &evidence.id, &request).unwrap();
        assert_eq!(again.links.len(), 1);

        let found = fixture
            .db
            .with_conn(|conn| {
                list(
                    conn,
                    &EvidenceFilter {
                        target_type: Some(EntityType::Finding),
                        target_id: Some("f-1".to_string()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(found.len(), 1);

        let unlinked = unlink(&fixture.db, &fixture.actor, &evidence.id, &request).unwrap();
        assert!(unlinked.links.is_empty());
    }

    #[test]
    fn delete_removes_the_row_and_the_file() {
        let fixture = Fixture::new();
        let evidence = fixture.import_named("scan.log", b"content").unwrap();
        let stored = fixture.root().join(&evidence.relative_path);
        assert!(stored.exists());

        delete(&fixture.db, &fixture.actor, fixture.root(), &evidence.id).unwrap();
        assert!(!stored.exists());
        assert!(fixture
            .db
            .with_conn(|conn| get(conn, &evidence.id))
            .is_err());
    }

    #[test]
    fn a_tampered_relative_path_cannot_read_outside_the_vault() {
        let fixture = Fixture::new();
        let evidence = fixture.import_named("scan.log", b"content").unwrap();
        fixture
            .db
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE evidence SET relative_path = '../../../../etc/passwd' WHERE id = ?1",
                    [&evidence.id],
                )?;
                Ok(())
            })
            .unwrap();

        let err = fixture
            .db
            .with_conn(|conn| read_bytes(conn, fixture.root(), None, &evidence.id))
            .unwrap_err();
        assert!(err.to_string().contains("escapes the vault"), "got {err}");
        assert!(fixture
            .db
            .with_conn(|conn| absolute_path(conn, fixture.root(), &evidence.id))
            .is_err());
    }

    #[test]
    fn import_is_audited_with_the_digest() {
        let fixture = Fixture::new();
        let evidence = fixture.import_named("scan.log", b"content").unwrap();
        let entries = fixture
            .db
            .with_conn(|conn| {
                audit::query(
                    conn,
                    &crate::models::AuditQuery {
                        action: Some(AuditAction::EvidenceAdded),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].detail["sha256"], evidence.sha256);
    }
}
