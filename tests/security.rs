//! Security regression tests.
//!
//! Each test states an attack and asserts the product refuses it. They are grouped by the
//! classes named in the build specification: injection, path traversal, hostile filenames,
//! oversized and mistyped uploads, authentication bypass, unauthorized access, and session
//! handling.

use auditforge_core::app::{App, AppPaths};
use auditforge_core::auth::session::{self, SessionPolicy};
use auditforge_core::auth::{self, password};
use auditforge_core::models::*;
use auditforge_core::search::{self, SearchQuery};
use auditforge_core::security::secrets::MemorySecretStore;
use auditforge_core::{engagements, evidence, findings, reports};

const PASSWORD: &str = "Correct-Horse-9-Battery";

struct Fixture {
    app: App,
    actor: Actor,
    token: String,
    _home: tempfile::TempDir,
    workspace: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let app = App::bootstrap(AppPaths::with_base(home.path().join("auditforge")))
        .unwrap()
        .with_secret_store(Box::new(MemorySecretStore::default()));

    auth::register(
        &app.db,
        &NewUser {
            username: "analyst".to_string(),
            password: PASSWORD.to_string(),
            confirm_password: PASSWORD.to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            email: String::new(),
        },
    )
    .unwrap();
    let session = auth::login(&app.db, "analyst", PASSWORD, SessionPolicy::default()).unwrap();
    let actor = auth::resolve(&app.db, &session.token, SessionPolicy::default()).unwrap();

    Fixture {
        app,
        actor,
        token: session.token,
        _home: home,
        workspace: tempfile::tempdir().unwrap(),
    }
}

// ---------------------------------------------------------------- injection

/// Strings that have historically broken naive query building.
const INJECTION_PAYLOADS: &[&str] = &[
    "' OR '1'='1",
    "'; DROP TABLE reports; --",
    "\" OR 1=1 --",
    "1; DELETE FROM users",
    "%' UNION SELECT password_hash, 1, 1 FROM users --",
    "admin'--",
    "\\'; DROP TABLE findings; --",
    "') OR ('a'='a",
];

#[test]
fn injection_payloads_in_search_match_nothing_and_break_nothing() {
    let fixture = fixture();
    engagements::create(
        &fixture.app.db,
        &fixture.actor,
        &NewEngagement {
            name: "Northwind".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    for payload in INJECTION_PAYLOADS {
        let hits = fixture
            .app
            .db
            .with_conn(|conn| {
                search::search(
                    conn,
                    &SearchQuery {
                        text: payload.to_string(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert!(
            hits.is_empty(),
            "{payload:?} matched {} records",
            hits.len()
        );
    }

    // Every table is still present and the data is intact.
    let survivors = fixture
        .app
        .db
        .with_conn(|conn| engagements::list(conn, &engagements::EngagementFilter::default()))
        .unwrap();
    assert_eq!(survivors.len(), 1);
    assert!(auth::has_any_user(&fixture.app.db).unwrap());
}

#[test]
fn injection_payloads_stored_as_content_are_returned_verbatim() {
    let fixture = fixture();
    // A pentest report legitimately contains payload strings; they must round-trip.
    for payload in INJECTION_PAYLOADS {
        let finding = findings::create(
            &fixture.app.db,
            &fixture.actor,
            &NewFinding {
                title: format!("SQL injection via {payload}"),
                description: payload.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let reloaded = fixture
            .app
            .db
            .with_conn(|conn| findings::get(conn, &finding.id))
            .unwrap();
        assert_eq!(reloaded.description, *payload);
    }
    assert!(auth::has_any_user(&fixture.app.db).unwrap());
}

#[test]
fn injection_payloads_in_filters_and_usernames_are_inert() {
    let fixture = fixture();
    for payload in INJECTION_PAYLOADS {
        let listed = fixture
            .app
            .db
            .with_conn(|conn| {
                findings::list(
                    conn,
                    &findings::FindingFilter {
                        search: Some(payload.to_string()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert!(listed.is_empty());

        // A hostile username is rejected by validation before it reaches SQL.
        assert!(auth::login(&fixture.app.db, payload, PASSWORD, SessionPolicy::default()).is_err());
    }
    assert!(auth::login(
        &fixture.app.db,
        "analyst",
        PASSWORD,
        SessionPolicy::default()
    )
    .is_ok());
}

// ---------------------------------------------------------------- path traversal

#[test]
fn evidence_cannot_be_written_outside_the_vault() {
    let fixture = fixture();
    let outside = fixture.workspace.path().join("outside.txt");
    std::fs::write(&outside, b"secret").unwrap();

    for hostile_name in [
        "../../../../etc/passwd",
        "..\\..\\..\\Windows\\System32\\drivers\\etc\\hosts",
        "....//....//escape.txt",
        "/etc/shadow",
    ] {
        let source = fixture.workspace.path().join("payload.txt");
        std::fs::write(&source, b"content").unwrap();

        let imported = evidence::import(
            &fixture.app.db,
            &fixture.actor,
            &fixture.app.paths.evidence_dir,
            None,
            &NewEvidence {
                source_path: source.to_str().unwrap().to_string(),
                title: hostile_name.to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: false,
                acknowledge_executable: false,
                link_target: None,
            },
        )
        .unwrap();

        let stored = fixture.app.paths.evidence_dir.join(&imported.relative_path);
        let canonical = stored.canonicalize().unwrap();
        let vault = fixture.app.paths.evidence_dir.canonicalize().unwrap();
        assert!(
            canonical.starts_with(&vault),
            "{hostile_name} escaped to {canonical:?}"
        );
    }
}

#[test]
fn a_tampered_stored_path_cannot_read_arbitrary_files() {
    let fixture = fixture();
    let source = fixture.workspace.path().join("note.txt");
    std::fs::write(&source, b"content").unwrap();
    let imported = evidence::import(
        &fixture.app.db,
        &fixture.actor,
        &fixture.app.paths.evidence_dir,
        None,
        &NewEvidence {
            source_path: source.to_str().unwrap().to_string(),
            title: "Note".to_string(),
            description: String::new(),
            engagement_id: None,
            captured_at: None,
            encrypt: false,
            acknowledge_executable: false,
            link_target: None,
        },
    )
    .unwrap();

    for hostile in [
        "../../../../etc/passwd",
        "/etc/passwd",
        "..\\..\\secrets.txt",
    ] {
        fixture
            .app
            .db
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE evidence SET relative_path = ?1 WHERE id = ?2",
                    rusqlite::params![hostile, imported.id],
                )?;
                Ok(())
            })
            .unwrap();

        let result = fixture.app.db.with_conn(|conn| {
            evidence::read_bytes(conn, &fixture.app.paths.evidence_dir, None, &imported.id)
        });
        assert!(result.is_err(), "{hostile} was readable");
    }
}

#[test]
fn import_refuses_anything_that_is_not_a_regular_file() {
    let fixture = fixture();
    let attempt = |path: String| {
        evidence::import(
            &fixture.app.db,
            &fixture.actor,
            &fixture.app.paths.evidence_dir,
            None,
            &NewEvidence {
                source_path: path,
                title: "Attempt".to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: false,
                acknowledge_executable: false,
                link_target: None,
            },
        )
    };

    assert!(attempt(fixture.workspace.path().to_str().unwrap().to_string()).is_err());
    assert!(attempt("relative/path.txt".to_string()).is_err());
    assert!(attempt(
        fixture
            .workspace
            .path()
            .join("missing.txt")
            .to_str()
            .unwrap()
            .to_string()
    )
    .is_err());
}

// ---------------------------------------------------------------- uploads

#[test]
fn oversized_uploads_are_refused() {
    let fixture = fixture();
    let big = fixture.workspace.path().join("big.bin");
    let size = auditforge_core::evidence::validation::MAX_EVIDENCE_BYTES + 1;
    // Sparse-ish write: allocate the length without buffering it all in memory.
    let file = std::fs::File::create(&big).unwrap();
    file.set_len(size).unwrap();
    drop(file);

    let result = evidence::import(
        &fixture.app.db,
        &fixture.actor,
        &fixture.app.paths.evidence_dir,
        None,
        &NewEvidence {
            source_path: big.to_str().unwrap().to_string(),
            title: "Oversized".to_string(),
            description: String::new(),
            engagement_id: None,
            captured_at: None,
            encrypt: false,
            acknowledge_executable: false,
            link_target: None,
        },
    );
    assert!(result.is_err(), "a file over the limit must be refused");
}

#[test]
fn executables_are_refused_by_content_not_by_extension() {
    let fixture = fixture();
    let import = |name: &str, bytes: &[u8], acknowledged: bool| {
        let path = fixture.workspace.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        evidence::import(
            &fixture.app.db,
            &fixture.actor,
            &fixture.app.paths.evidence_dir,
            None,
            &NewEvidence {
                source_path: path.to_str().unwrap().to_string(),
                title: name.to_string(),
                description: String::new(),
                engagement_id: None,
                captured_at: None,
                encrypt: false,
                acknowledge_executable: acknowledged,
                link_target: None,
            },
        )
    };

    // Windows PE, Linux ELF and a shell script, all disguised as a screenshot.
    assert!(import("shot1.png", b"MZ\x90\x00\x03\x00", false).is_err());
    assert!(import("shot2.png", b"\x7FELF\x02\x01\x01", false).is_err());
    assert!(import("shot3.png", b"#!/bin/sh\nrm -rf /\n", false).is_err());
    // A real screenshot is accepted.
    assert!(import("shot4.png", b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR", false).is_ok());
    // And a genuine sample can be kept when the analyst says so explicitly.
    assert!(import("sample.exe", b"MZ\x90\x00\x03\x00", true).is_ok());
}

#[test]
fn stored_evidence_is_never_executable_and_keeps_its_digest() {
    let fixture = fixture();
    let source = fixture.workspace.path().join("scan.log");
    std::fs::write(&source, b"nmap output").unwrap();
    let imported = evidence::import(
        &fixture.app.db,
        &fixture.actor,
        &fixture.app.paths.evidence_dir,
        None,
        &NewEvidence {
            source_path: source.to_str().unwrap().to_string(),
            title: "Scan".to_string(),
            description: String::new(),
            engagement_id: None,
            captured_at: None,
            encrypt: false,
            acknowledge_executable: false,
            link_target: None,
        },
    )
    .unwrap();

    let stored = fixture.app.paths.evidence_dir.join(&imported.relative_path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&stored).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0, "evidence must never be executable");
        assert_eq!(mode & 0o077, 0, "evidence must be owner-only");
    }

    // Modifying the stored copy is detected on read.
    std::fs::write(&stored, b"tampered").unwrap();
    assert!(fixture
        .app
        .db
        .with_conn(|conn| evidence::read_bytes(
            conn,
            &fixture.app.paths.evidence_dir,
            None,
            &imported.id
        ))
        .is_err());
}

// ---------------------------------------------------------------- authentication

#[test]
fn passwords_are_never_stored_in_a_recoverable_form() {
    let fixture = fixture();
    let stored: String = fixture
        .app
        .db
        .with_conn(|conn| {
            Ok(
                conn.query_row("SELECT password_hash FROM users LIMIT 1", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .unwrap();

    assert!(stored.starts_with("$argon2id$"));
    assert!(!stored.contains(PASSWORD));

    // And the raw database file contains no trace of the password either.
    let bytes = std::fs::read(&fixture.app.paths.database_path).unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).contains(PASSWORD),
        "the password must not appear anywhere in the database file"
    );
}

#[test]
fn authentication_cannot_be_bypassed() {
    let fixture = fixture();
    for attempt in [
        "",
        " ",
        PASSWORD.to_uppercase().as_str(),
        "Correct-Horse-9-Batter",
        "' OR '1'='1",
        "%",
    ] {
        assert!(
            auth::login(
                &fixture.app.db,
                "analyst",
                attempt,
                SessionPolicy::default()
            )
            .is_err(),
            "{attempt:?} must not authenticate"
        );
    }
    // A corrupted hash fails closed rather than opening the account.
    fixture
        .app
        .db
        .with_conn(|conn| {
            conn.execute("UPDATE users SET password_hash = 'not-a-hash'", [])?;
            Ok(())
        })
        .unwrap();
    assert!(auth::login(
        &fixture.app.db,
        "analyst",
        PASSWORD,
        SessionPolicy::default()
    )
    .is_err());
    assert!(!password::verify_password(PASSWORD, "not-a-hash"));
}

#[test]
fn brute_force_attempts_lock_the_account() {
    let fixture = fixture();
    let mut locked = false;
    for _ in 0..auth::MAX_FAILED_ATTEMPTS {
        if let Err(error) = auth::login(
            &fixture.app.db,
            "analyst",
            "wrong-guess-1A!",
            SessionPolicy::default(),
        ) {
            locked = error.kind() == "locked";
        }
    }
    assert!(locked, "repeated failures must lock the account");
    assert_eq!(
        auth::login(
            &fixture.app.db,
            "analyst",
            PASSWORD,
            SessionPolicy::default()
        )
        .unwrap_err()
        .kind(),
        "locked",
        "even the correct password is refused while locked"
    );
}

#[test]
fn an_unknown_user_is_indistinguishable_from_a_wrong_password() {
    let fixture = fixture();
    let unknown = auth::login(
        &fixture.app.db,
        "ghost",
        "whatever-1234A!",
        SessionPolicy::default(),
    )
    .unwrap_err();
    let wrong = auth::login(
        &fixture.app.db,
        "analyst",
        "whatever-1234A!",
        SessionPolicy::default(),
    )
    .unwrap_err();
    assert_eq!(unknown.kind(), wrong.kind());
    assert_eq!(unknown.to_string(), wrong.to_string());
}

// ---------------------------------------------------------------- sessions

#[test]
fn session_tokens_are_unguessable_and_only_hashes_are_stored() {
    let fixture = fixture();
    let stored: Vec<String> = fixture
        .app
        .db
        .with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT token_hash FROM sessions")?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap();

    assert!(
        !stored.contains(&fixture.token),
        "the raw token must never be stored"
    );
    assert_eq!(stored[0], session::hash_token(&fixture.token));
    assert!(
        fixture.token.len() >= 40,
        "a 256-bit token base64s to at least 43 characters"
    );
}

#[test]
fn a_forged_or_reused_token_is_rejected() {
    let fixture = fixture();
    for forged in [
        "",
        "not-a-token",
        &fixture.token[..fixture.token.len() - 1],
        &format!("{}x", fixture.token),
        &session::hash_token(&fixture.token),
    ] {
        assert!(
            auth::resolve(&fixture.app.db, forged, SessionPolicy::default()).is_err(),
            "{forged:?} must not resolve"
        );
    }
    assert!(auth::resolve(&fixture.app.db, &fixture.token, SessionPolicy::default()).is_ok());
}

#[test]
fn sessions_expire_and_a_password_change_revokes_them() {
    let fixture = fixture();
    let short = SessionPolicy {
        idle_timeout_secs: 60,
        absolute_timeout_secs: 3_600,
    };

    fixture
        .app
        .db
        .with_conn(|conn| {
            conn.execute(
                "UPDATE sessions SET last_seen_at = ?1",
                rusqlite::params![Timestamp::now().plus_seconds(-600)],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(
        auth::resolve(&fixture.app.db, &fixture.token, short).is_err(),
        "idle timeout"
    );

    let session = auth::login(
        &fixture.app.db,
        "analyst",
        PASSWORD,
        SessionPolicy::default(),
    )
    .unwrap();
    let actor = auth::resolve(&fixture.app.db, &session.token, SessionPolicy::default()).unwrap();
    auth::change_password(
        &fixture.app.db,
        &actor,
        PASSWORD,
        "New-Passphrase-2026!",
        "New-Passphrase-2026!",
    )
    .unwrap();
    assert!(
        auth::resolve(&fixture.app.db, &session.token, SessionPolicy::default()).is_err(),
        "changing the password must end every session"
    );
}

// ---------------------------------------------------------------- integrity

#[test]
fn a_finalized_report_cannot_be_rewritten_through_any_route() {
    let fixture = fixture();
    let report = reports::create(
        &fixture.app.db,
        &fixture.actor,
        &NewReport {
            title: "Board report".to_string(),
            template_key: "executive_summary".to_string(),
            engagement_id: None,
            classification: None,
        },
    )
    .unwrap();

    let answers = fixture
        .app
        .db
        .with_conn(|conn| {
            let template = auditforge_core::templates::get(conn, "executive_summary")?;
            Ok(template
                .sections
                .iter()
                .flat_map(|section| {
                    section
                        .questions
                        .iter()
                        .filter(|question| question.required)
                        .map(|question| AnswerInput {
                            section_key: section.key.clone(),
                            question_key: question.key.clone(),
                            value: "Answered.".to_string(),
                            provenance: Provenance::UserProvided,
                            ai_request_id: None,
                        })
                })
                .collect::<Vec<_>>())
        })
        .unwrap();
    reports::save_draft(
        &fixture.app.db,
        &fixture.actor,
        &report.id,
        &ReportDraftUpdate {
            answers,
            ..Default::default()
        },
    )
    .unwrap();
    let sealed =
        reports::finalize(&fixture.app.db, &fixture.actor, &report.id, true, "Issued").unwrap();

    // Application layer.
    assert_eq!(
        reports::save_draft(
            &fixture.app.db,
            &fixture.actor,
            &report.id,
            &ReportDraftUpdate::default()
        )
        .unwrap_err()
        .kind(),
        "immutable"
    );
    assert!(reports::delete(&fixture.app.db, &fixture.actor, &report.id).is_err());

    // Storage layer: the triggers refuse direct SQL as well.
    assert!(fixture
        .app
        .db
        .with_conn(|conn| {
            conn.execute(
                "UPDATE report_versions SET content_json = '{}' WHERE id = ?1",
                [&sealed.id],
            )?;
            Ok(())
        })
        .is_err());
    assert!(fixture
        .app
        .db
        .with_conn(|conn| {
            conn.execute("DELETE FROM report_versions WHERE id = ?1", [&sealed.id])?;
            Ok(())
        })
        .is_err());

    let reloaded = fixture
        .app
        .report_version(&report.id, Some(&sealed.id))
        .unwrap();
    assert_eq!(reloaded.content_hash, sealed.content_hash);
}

#[test]
fn severity_cannot_be_set_without_a_human_decision() {
    let fixture = fixture();
    let finding = findings::create(
        &fixture.app.db,
        &fixture.actor,
        &NewFinding {
            title: "Something".to_string(),
            severity: Some(Severity::Critical),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!finding.severity_confirmed);

    assert!(findings::set_severity(
        &fixture.app.db,
        &fixture.actor,
        &finding.id,
        &SeverityDecision {
            severity: Severity::Critical,
            confirmed_by_user: false,
            rationale: String::new(),
        },
    )
    .is_err());

    // And an unconfirmed severity blocks the report that includes it.
    let report = reports::create(
        &fixture.app.db,
        &fixture.actor,
        &NewReport {
            title: "Report".to_string(),
            template_key: "penetration_test".to_string(),
            engagement_id: None,
            classification: None,
        },
    )
    .unwrap();
    reports::save_draft(
        &fixture.app.db,
        &fixture.actor,
        &report.id,
        &ReportDraftUpdate {
            finding_ids: Some(vec![finding.id]),
            ..Default::default()
        },
    )
    .unwrap();
    let readiness = fixture
        .app
        .db
        .with_conn(|conn| reports::readiness(conn, &report.id))
        .unwrap();
    assert!(!readiness.can_finalize);
    assert!(readiness
        .issues
        .iter()
        .any(|issue| issue.code == "severity_unconfirmed"));
}

#[test]
fn secrets_never_reach_the_database() {
    let fixture = fixture();
    fixture
        .app
        .store_api_key(&fixture.actor, "sk-canary-value-do-not-store")
        .unwrap();

    let bytes = std::fs::read(&fixture.app.paths.database_path).unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).contains("sk-canary-value-do-not-store"),
        "an API key must never be written to the database"
    );

    let settings = fixture.app.settings(&fixture.actor.user_id).unwrap();
    assert!(settings.ai.has_api_key);
    assert!(!serde_json::to_string(&settings)
        .unwrap()
        .contains("sk-canary"));
}
