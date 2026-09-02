//! Reports and their version history.
//!
//! # Version model
//!
//! A report always has exactly one *current* version. Draft edits are written to that
//! version in place, which is what makes the guided form feel like a document rather than
//! a commit log. History is created at explicit transitions:
//!
//! | Action | Effect |
//! |---|---|
//! | `create` | `v0.1 Draft` |
//! | `save_draft` | writes into the current version |
//! | `snapshot` | seals the current version, opens `v0.<n+1>` |
//! | `submit_for_review` | seals the current version, opens `v0.<n+1> Review` |
//! | `finalize` | seals everything and writes `v<major+1>.0 Final`, immutable |
//! | `create_revision` | opens `v<major>.<n+1> Revised` from the finalized content |
//!
//! Sealed versions set `is_immutable`, which the application enforces here *and* the
//! database enforces with triggers, so no future code path can quietly rewrite a
//! finalized report.
//!
//! # Self-contained versions
//!
//! Each version stores the whole document: the author identity as it was at the time, the
//! engagement facts, and copies of every finding and evidence record it cites. Editing a
//! job title, renaming an engagement or deleting a finding tomorrow cannot alter a report
//! finalized today.

use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::audit;
use crate::database::{escape_like, new_id, next_reference, Database};
use crate::error::{Error, Result};
use crate::models::{
    version_label, Actor, AnswerInput, AnswerValue, AuditAction, AuthorSnapshot, Classification,
    EngagementSnapshot, EvidenceSnapshot, FindingSnapshot, IssueSeverity, NewReport, Provenance,
    Report, ReportContent, ReportDraftUpdate, ReportReadiness, ReportStatus, ReportSummary,
    ReportVersion, ReportVersionSummary, Timestamp, ValidationIssue,
};
use crate::templates::{self, SectionKind};
use crate::{auth, engagements, evidence, findings};

const MAX_TITLE: usize = 300;
const MAX_ANSWER: usize = 100_000;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReportFilter {
    #[serde(default)]
    pub engagement_id: Option<String>,
    #[serde(default)]
    pub status: Option<ReportStatus>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

// ---------------------------------------------------------------------- creation

pub fn create(db: &Database, actor: &Actor, input: &NewReport) -> Result<Report> {
    let title = require(&input.title, "report title", MAX_TITLE)?;

    let id = db.with_tx(|tx| {
        let template = templates::get(tx, &input.template_key)?;
        let user = auth::load_user(tx, &actor.user_id)?;
        let author = user.author_snapshot();

        let engagement_snapshot = match &input.engagement_id {
            Some(engagement_id) => EngagementSnapshot::from(&engagements::get(tx, engagement_id)?),
            None => EngagementSnapshot::default(),
        };

        let classification = input
            .classification
            .unwrap_or(template.default_classification);
        let report_id = new_id();
        let report_ref = next_reference(tx, "report", "RPT")?;
        let now = Timestamp::now();

        let mut content =
            template.instantiate(&title, classification, author.clone(), engagement_snapshot);
        content.report_ref = report_ref.clone();
        content.version_label = version_label(0, 1, ReportStatus::Draft);

        tx.execute(
            "INSERT INTO reports (id, report_ref, engagement_id, template_key, report_type, title,
                                  classification, status, created_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            rusqlite::params![
                report_id,
                report_ref,
                input.engagement_id,
                template.key,
                template.report_type,
                title,
                classification,
                ReportStatus::Draft,
                actor.user_id,
                now,
            ],
        )?;

        let version_id = insert_version(
            tx,
            &report_id,
            0,
            1,
            ReportStatus::Draft,
            &content,
            &author,
            "",
            false,
        )?;
        tx.execute(
            "UPDATE reports SET current_version_id = ?1 WHERE id = ?2",
            rusqlite::params![version_id, report_id],
        )?;

        audit::success(
            tx,
            actor,
            AuditAction::ReportCreated,
            "report",
            &report_id,
            json!({ "ref": report_ref, "template": template.key, "title": title }),
        )?;
        Ok(report_id)
    })?;

    db.with_conn(|conn| get(conn, &id))
}

#[allow(clippy::too_many_arguments)]
fn insert_version(
    tx: &Transaction<'_>,
    report_id: &str,
    major: i64,
    minor: i64,
    status: ReportStatus,
    content: &ReportContent,
    author: &AuthorSnapshot,
    notes: &str,
    immutable: bool,
) -> Result<String> {
    let id = new_id();
    let json_body = serde_json::to_string(content)?;
    let mut hasher = Sha256::new();
    hasher.update(json_body.as_bytes());
    let content_hash = format!("{:x}", hasher.finalize());
    let now = Timestamp::now();

    tx.execute(
        "INSERT INTO report_versions (id, report_id, version_major, version_minor, version_label,
                                      status, content_json, author_snapshot_json, content_hash,
                                      notes, is_immutable, created_by, created_at, finalized_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        rusqlite::params![
            id,
            report_id,
            major,
            minor,
            version_label(major, minor, status),
            status,
            json_body,
            serde_json::to_string(author)?,
            content_hash,
            notes,
            i64::from(immutable),
            author.user_id,
            now,
            (status == ReportStatus::Final).then_some(now),
        ],
    )?;
    Ok(id)
}

// ---------------------------------------------------------------------- reads

pub fn get(conn: &Connection, id: &str) -> Result<Report> {
    conn.query_row(
        "SELECT id, report_ref, engagement_id, template_key, report_type, title, classification,
                status, current_version_id, created_by, created_at, updated_at, finalized_at
         FROM reports WHERE id = ?1",
        [id],
        row_to_report,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("report"))
}

fn row_to_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<Report> {
    Ok(Report {
        id: row.get(0)?,
        report_ref: row.get(1)?,
        engagement_id: row.get(2)?,
        template_key: row.get(3)?,
        report_type: row.get(4)?,
        title: row.get(5)?,
        classification: row.get(6)?,
        status: row.get(7)?,
        current_version_id: row.get(8)?,
        created_by: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        finalized_at: row.get(12)?,
    })
}

/// The current version of a report, content included.
pub fn current_version(conn: &Connection, report_id: &str) -> Result<ReportVersion> {
    let report = get(conn, report_id)?;
    let version_id = report
        .current_version_id
        .ok_or_else(|| Error::internal("report has no current version"))?;
    load_version(conn, &version_id)
}

pub fn load_version(conn: &Connection, version_id: &str) -> Result<ReportVersion> {
    conn.query_row(
        "SELECT id, report_id, version_major, version_minor, version_label, status, content_json,
                author_snapshot_json, content_hash, notes, is_immutable, created_by, created_at,
                finalized_at
         FROM report_versions WHERE id = ?1",
        [version_id],
        |row| {
            let content_raw: String = row.get(6)?;
            let author_raw: String = row.get(7)?;
            Ok((row_to_version_shell(row)?, content_raw, author_raw))
        },
    )
    .optional()?
    .ok_or_else(|| Error::not_found("report version"))
    .and_then(|(mut version, content_raw, author_raw)| {
        version.content = serde_json::from_str(&content_raw)?;
        version.author = serde_json::from_str(&author_raw)?;
        Ok(version)
    })
}

fn row_to_version_shell(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReportVersion> {
    Ok(ReportVersion {
        id: row.get(0)?,
        report_id: row.get(1)?,
        version_major: row.get(2)?,
        version_minor: row.get(3)?,
        version_label: row.get(4)?,
        status: row.get(5)?,
        // Filled in by the caller once the JSON columns are parsed.
        content: placeholder_content(),
        author: placeholder_author(),
        content_hash: row.get(8)?,
        notes: row.get(9)?,
        is_immutable: row.get::<_, i64>(10)? != 0,
        created_by: row.get(11)?,
        created_at: row.get(12)?,
        finalized_at: row.get(13)?,
    })
}

fn placeholder_author() -> AuthorSnapshot {
    AuthorSnapshot {
        user_id: String::new(),
        username: String::new(),
        full_name: String::new(),
        organization: String::new(),
        job_title: String::new(),
        captured_at: Timestamp::now(),
    }
}

fn placeholder_content() -> ReportContent {
    ReportContent {
        schema_version: crate::models::CONTENT_SCHEMA_VERSION,
        template_key: String::new(),
        report_type: crate::models::ReportType::Custom,
        title: String::new(),
        classification: Classification::Confidential,
        author: placeholder_author(),
        engagement: EngagementSnapshot::default(),
        sections: Vec::new(),
        findings: Vec::new(),
        evidence: Vec::new(),
        report_ref: String::new(),
        version_label: String::new(),
        generated_at: Timestamp::now(),
    }
}

pub fn versions(conn: &Connection, report_id: &str) -> Result<Vec<ReportVersionSummary>> {
    let mut stmt = conn.prepare(
        "SELECT v.id, v.report_id, v.version_label, v.status, v.is_immutable, v.notes,
                v.content_hash, v.created_at, v.finalized_at, v.author_snapshot_json
         FROM report_versions v WHERE v.report_id = ?1
         ORDER BY v.version_major DESC, v.version_minor DESC",
    )?;
    let rows = stmt
        .query_map([report_id], |row| {
            let author_raw: String = row.get(9)?;
            let author_name = serde_json::from_str::<AuthorSnapshot>(&author_raw)
                .map(|a| a.full_name)
                .unwrap_or_default();
            Ok(ReportVersionSummary {
                id: row.get(0)?,
                report_id: row.get(1)?,
                version_label: row.get(2)?,
                status: row.get(3)?,
                is_immutable: row.get::<_, i64>(4)? != 0,
                notes: row.get(5)?,
                author_name,
                content_hash: row.get(6)?,
                created_at: row.get(7)?,
                finalized_at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list(conn: &Connection, filter: &ReportFilter) -> Result<Vec<ReportSummary>> {
    let mut sql = String::from(
        "SELECT r.id, r.report_ref, r.engagement_id, r.template_key, r.report_type, r.title,
                r.classification, r.status, r.current_version_id, r.created_by, r.created_at,
                r.updated_at, r.finalized_at,
                (SELECT e.name FROM engagements e WHERE e.id = r.engagement_id),
                (SELECT v.version_label FROM report_versions v WHERE v.id = r.current_version_id),
                (SELECT u.full_name FROM users u WHERE u.id = r.created_by),
                (SELECT COUNT(*) FROM report_findings rf WHERE rf.report_id = r.id)
         FROM reports r WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(engagement_id) = &filter.engagement_id {
        sql.push_str(" AND r.engagement_id = ?");
        params.push(Box::new(engagement_id.clone()));
    }
    if let Some(status) = filter.status {
        sql.push_str(" AND r.status = ?");
        params.push(Box::new(status.as_str().to_string()));
    }
    if let Some(search) = filter.search.as_ref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(" AND (r.title LIKE ? ESCAPE '\\' OR r.report_ref LIKE ? ESCAPE '\\')");
        let pattern = format!("%{}%", escape_like(search.trim()));
        params.push(Box::new(pattern.clone()));
        params.push(Box::new(pattern));
    }
    sql.push_str(" ORDER BY r.updated_at DESC LIMIT ?");
    params.push(Box::new(filter.limit.unwrap_or(200).clamp(1, 2_000)));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(ReportSummary {
                report: row_to_report(row)?,
                engagement_name: row.get(13)?,
                current_version_label: row.get(14)?,
                author_name: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
                finding_count: row.get(16)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------- editing

/// Write guided answers, section inclusion and linked records into the current version.
pub fn save_draft(
    db: &Database,
    actor: &Actor,
    report_id: &str,
    update: &ReportDraftUpdate,
) -> Result<ReportVersion> {
    db.with_tx(|tx| {
        let report = get(tx, report_id)?;
        let mut version = current_version(tx, report_id)?;
        ensure_editable(&report, &version)?;

        let template = templates::get(tx, &report.template_key)?;
        template.reconcile(&mut version.content);

        if let Some(title) = &update.title {
            let title = require(title, "report title", MAX_TITLE)?;
            version.content.title = title.clone();
            tx.execute(
                "UPDATE reports SET title = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![title, Timestamp::now(), report_id],
            )?;
        }
        if let Some(classification) = update.classification {
            version.content.classification = classification;
            tx.execute(
                "UPDATE reports SET classification = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![classification, Timestamp::now(), report_id],
            )?;
        }

        apply_answers(&template, &mut version.content, &update.answers)?;

        if let Some(included) = &update.included_sections {
            for section in &mut version.content.sections {
                section.included = included.iter().any(|key| key == &section.key);
            }
        }
        if let Some(finding_ids) = &update.finding_ids {
            set_findings(tx, report_id, finding_ids)?;
        }
        if let Some(evidence_ids) = &update.evidence_ids {
            set_evidence(tx, report_id, evidence_ids)?;
        }

        refresh_linked_snapshots(tx, report_id, &mut version.content)?;
        write_version_content(tx, &version)?;
        tx.execute(
            "UPDATE reports SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![Timestamp::now(), report_id],
        )?;

        audit::success(
            tx,
            actor,
            AuditAction::ReportEdited,
            "report",
            report_id,
            json!({
                "ref": report.report_ref,
                "version": version.version_label,
                "answers_written": update.answers.len(),
            }),
        )?;
        current_version(tx, report_id)
    })
}

/// Reject an edit to a sealed version with an actionable message.
fn ensure_editable(report: &Report, version: &ReportVersion) -> Result<()> {
    if version.is_immutable || version.status == ReportStatus::Final {
        return Err(Error::Immutable(format!(
            "{} is finalized ({}). Create a new revision to make changes.",
            report.report_ref, version.version_label
        )));
    }
    Ok(())
}

fn apply_answers(
    template: &templates::ReportTemplate,
    content: &mut ReportContent,
    answers: &[AnswerInput],
) -> Result<()> {
    for input in answers {
        let question = template
            .question(&input.section_key, &input.question_key)
            .ok_or_else(|| {
                Error::validation(format!(
                    "unknown question {} in section {}",
                    input.question_key, input.section_key
                ))
            })?;
        let mut value = clamp(&input.value, MAX_ANSWER);
        if question.input == templates::InputKind::Date && !value.trim().is_empty() {
            crate::models::validate_date(&value, &question.label)?;
        }
        // Canonicalize a table here rather than trusting the client: blank rows dropped,
        // cells trimmed, and a table with nothing in it collapsed to the empty string so
        // `is_blank` — which the readiness gate and every renderer use — reports it as
        // unanswered instead of as a filled-in question holding "[]".
        if question.is_table() {
            value = templates::tables::normalize(&value);
        }

        let section = content
            .section_mut(&input.section_key)
            .ok_or_else(|| Error::validation("unknown section"))?;
        let now = Timestamp::now();
        match section
            .answers
            .iter_mut()
            .find(|a| a.question_key == input.question_key)
        {
            Some(existing) => {
                existing.value = value;
                existing.provenance = input.provenance;
                existing.ai_request_id = input.ai_request_id.clone();
                existing.updated_at = now;
            }
            None => section.answers.push(AnswerValue {
                question_key: input.question_key.clone(),
                value,
                provenance: input.provenance,
                ai_request_id: input.ai_request_id.clone(),
                updated_at: now,
            }),
        }
    }
    Ok(())
}

fn set_findings(tx: &Transaction<'_>, report_id: &str, finding_ids: &[String]) -> Result<()> {
    tx.execute(
        "DELETE FROM report_findings WHERE report_id = ?1",
        [report_id],
    )?;
    for (index, finding_id) in finding_ids.iter().enumerate() {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM findings WHERE id = ?1",
            [finding_id],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Err(Error::validation(format!(
                "finding {finding_id} does not exist"
            )));
        }
        tx.execute(
            "INSERT INTO report_findings (report_id, finding_id, sort_order) VALUES (?1, ?2, ?3)",
            rusqlite::params![report_id, finding_id, index as i64],
        )?;
    }
    Ok(())
}

fn set_evidence(tx: &Transaction<'_>, report_id: &str, evidence_ids: &[String]) -> Result<()> {
    tx.execute(
        "DELETE FROM evidence_links WHERE target_type = 'report' AND target_id = ?1",
        [report_id],
    )?;
    for evidence_id in evidence_ids {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM evidence WHERE id = ?1",
            [evidence_id],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Err(Error::validation(format!(
                "evidence {evidence_id} does not exist"
            )));
        }
        tx.execute(
            "INSERT OR IGNORE INTO evidence_links (id, evidence_id, target_type, target_id,
                                                   created_at)
             VALUES (?1, ?2, 'report', ?3, ?4)",
            rusqlite::params![new_id(), evidence_id, report_id, Timestamp::now()],
        )?;
    }
    Ok(())
}

/// Copy the current state of every linked finding and evidence record into the document.
fn refresh_linked_snapshots(
    conn: &Connection,
    report_id: &str,
    content: &mut ReportContent,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT finding_id FROM report_findings WHERE report_id = ?1 ORDER BY sort_order",
    )?;
    let finding_ids = stmt
        .query_map([report_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut finding_snapshots = Vec::new();
    for finding_id in &finding_ids {
        let finding = findings::get(conn, finding_id)?;
        let mut snapshot = FindingSnapshot::from(&finding);
        snapshot.evidence_refs = finding
            .evidence_ids
            .iter()
            .filter_map(|id| evidence::get(conn, id).ok().map(|e| e.evidence_ref))
            .collect();
        finding_snapshots.push(snapshot);
    }
    // Highest severity first, so the document reads in priority order.
    finding_snapshots.sort_by_key(|snapshot| snapshot.severity.rank());
    content.findings = finding_snapshots;

    let mut stmt = conn.prepare(
        "SELECT evidence_id FROM evidence_links WHERE target_type = 'report' AND target_id = ?1
         ORDER BY created_at",
    )?;
    let evidence_ids = stmt
        .query_map([report_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    content.evidence = evidence_ids
        .iter()
        .filter_map(|id| evidence::get(conn, id).ok())
        .map(|item| EvidenceSnapshot::from(&item))
        .collect();

    Ok(())
}

fn write_version_content(tx: &Transaction<'_>, version: &ReportVersion) -> Result<()> {
    let json_body = serde_json::to_string(&version.content)?;
    let mut hasher = Sha256::new();
    hasher.update(json_body.as_bytes());
    let content_hash = format!("{:x}", hasher.finalize());
    tx.execute(
        "UPDATE report_versions SET content_json = ?1, content_hash = ?2 WHERE id = ?3",
        rusqlite::params![json_body, content_hash, version.id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------- transitions

/// Seal the current version and open the next one with the given status.
fn advance(
    tx: &Transaction<'_>,
    report_id: &str,
    status: ReportStatus,
    notes: &str,
) -> Result<String> {
    let current = current_version(tx, report_id)?;
    tx.execute(
        "UPDATE report_versions SET is_immutable = 1 WHERE id = ?1",
        [&current.id],
    )?;
    let mut content = current.content.clone();
    let (major, minor) = (current.version_major, current.version_minor + 1);
    content.version_label = version_label(major, minor, status);
    let version_id = insert_version(
        tx,
        report_id,
        major,
        minor,
        status,
        &content,
        &current.author,
        notes,
        false,
    )?;
    tx.execute(
        "UPDATE reports SET current_version_id = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![version_id, status, Timestamp::now(), report_id],
    )?;
    Ok(version_id)
}

/// Seal the current version without changing status - "keep a copy of this".
pub fn snapshot(
    db: &Database,
    actor: &Actor,
    report_id: &str,
    notes: &str,
) -> Result<ReportVersion> {
    db.with_tx(|tx| {
        let report = get(tx, report_id)?;
        let current = current_version(tx, report_id)?;
        ensure_editable(&report, &current)?;
        advance(tx, report_id, current.status, notes)?;
        audit::success(
            tx,
            actor,
            AuditAction::ReportEdited,
            "report",
            report_id,
            json!({ "snapshot_of": current.version_label, "notes": notes }),
        )?;
        current_version(tx, report_id)
    })
}

pub fn submit_for_review(db: &Database, actor: &Actor, report_id: &str) -> Result<ReportVersion> {
    db.with_tx(|tx| {
        let report = get(tx, report_id)?;
        let current = current_version(tx, report_id)?;
        ensure_editable(&report, &current)?;
        advance(tx, report_id, ReportStatus::Review, "Submitted for review")?;
        audit::success(
            tx,
            actor,
            AuditAction::ReportSubmittedForReview,
            "report",
            report_id,
            json!({ "ref": report.report_ref }),
        )?;
        current_version(tx, report_id)
    })
}

/// What still stands between this report and being finalized.
pub fn readiness(conn: &Connection, report_id: &str) -> Result<ReportReadiness> {
    let report = get(conn, report_id)?;
    let version = current_version(conn, report_id)?;
    let template = templates::get(conn, &report.template_key)?;
    let mut issues = Vec::new();

    if version.content.title.trim().is_empty() {
        issues.push(blocker(
            "title_missing",
            "The report has no title.",
            None,
            None,
        ));
    }
    if version.content.author.full_name.trim().is_empty() {
        issues.push(blocker(
            "author_missing",
            "The report has no author. Complete your profile in Settings.",
            None,
            None,
        ));
    }

    for section in template.sections.iter() {
        let Some(content_section) = version.content.section(&section.key) else {
            continue;
        };
        if !content_section.included {
            continue;
        }
        match section.kind {
            SectionKind::Guided => {
                for question in &section.questions {
                    let answer = content_section.answer(&question.key);
                    let blank = answer.map(|a| a.is_blank()).unwrap_or(true);
                    if blank && question.required {
                        issues.push(blocker(
                            "required_answer_missing",
                            &format!(
                                "{} - {} has not been answered.",
                                section.title, question.label
                            ),
                            Some(&section.key),
                            Some(&question.key),
                        ));
                    } else if blank {
                        issues.push(warning(
                            "answer_missing",
                            &format!("{} - {} is empty.", section.title, question.label),
                            Some(&section.key),
                            Some(&question.key),
                        ));
                    }
                    if let Some(answer) = answer {
                        if answer.provenance == Provenance::AiInference && !answer.is_blank() {
                            issues.push(blocker(
                                "unconfirmed_ai_inference",
                                &format!(
                                    "{} - {} contains an unconfirmed AI inference. Confirm it as \
                                     your own, edit it, or remove it.",
                                    section.title, question.label
                                ),
                                Some(&section.key),
                                Some(&question.key),
                            ));
                        }
                        if answer.provenance == Provenance::MissingInformation && !answer.is_blank()
                        {
                            issues.push(warning(
                                "marked_missing_information",
                                &format!(
                                    "{} - {} is still marked as missing information.",
                                    section.title, question.label
                                ),
                                Some(&section.key),
                                Some(&question.key),
                            ));
                        }
                    }
                }
            }
            SectionKind::Findings => {
                if version.content.findings.is_empty() {
                    issues.push(warning(
                        "no_findings",
                        "No findings are linked to this report.",
                        Some(&section.key),
                        None,
                    ));
                }
            }
            SectionKind::Evidence => {
                if version.content.evidence.is_empty() {
                    issues.push(warning(
                        "no_evidence",
                        "No evidence is attached to this report.",
                        Some(&section.key),
                        None,
                    ));
                }
            }
        }
    }

    // Severity must be a human decision on every finding the report carries.
    let mut stmt = conn.prepare("SELECT finding_id FROM report_findings WHERE report_id = ?1")?;
    let linked = stmt
        .query_map([report_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for finding_id in findings::unconfirmed_severity(conn, &linked)? {
        let title = findings::get(conn, &finding_id)
            .map(|f| format!("{} ({})", f.title, f.finding_ref))
            .unwrap_or_else(|_| finding_id.clone());
        issues.push(blocker(
            "severity_unconfirmed",
            &format!("Severity has not been confirmed for {title}."),
            None,
            None,
        ));
    }

    for snapshot in &version.content.findings {
        if snapshot.remediation.trim().is_empty() {
            issues.push(warning(
                "remediation_missing",
                &format!("{} has no remediation.", snapshot.finding_ref),
                None,
                None,
            ));
        }
    }

    let can_finalize = !issues
        .iter()
        .any(|issue| issue.severity == IssueSeverity::Blocker);

    // Completeness is advisory: it tells the author how much of the required structure is
    // filled in and whether the document meets the professional length target for its kind.
    // It never changes `can_finalize` - a short-but-complete report is flagged for review,
    // not refused (see `reports::standards`).
    let estimated_pages = crate::pdf::estimate_pages(
        &version.content,
        Some(&template),
        &crate::pdf::RenderOptions::default(),
    );
    let completeness = standards::evaluate(&version.content, Some(&template), estimated_pages);

    Ok(ReportReadiness {
        can_finalize,
        issues,
        completeness,
    })
}

/// Finalize the report. `confirmed` must be true: finalization is a deliberate act, and
/// the PDF is only produced from a finalized version.
pub fn finalize(
    db: &Database,
    actor: &Actor,
    report_id: &str,
    confirmed: bool,
    notes: &str,
) -> Result<ReportVersion> {
    if !confirmed {
        return Err(Error::validation(
            "finalizing a report requires explicit confirmation from the author",
        ));
    }
    db.with_tx(|tx| {
        let report = get(tx, report_id)?;
        let current = current_version(tx, report_id)?;
        ensure_editable(&report, &current)?;

        let readiness = readiness(tx, report_id)?;
        if !readiness.can_finalize {
            let blockers: Vec<&str> = readiness
                .issues
                .iter()
                .filter(|issue| issue.severity == IssueSeverity::Blocker)
                .map(|issue| issue.message.as_str())
                .collect();
            return Err(Error::validation(format!(
                "this report is not ready to finalize:\n- {}",
                blockers.join("\n- ")
            )));
        }

        tx.execute(
            "UPDATE report_versions SET is_immutable = 1 WHERE id = ?1",
            [&current.id],
        )?;

        let major = current.version_major + 1;
        let mut content = current.content.clone();
        content.version_label = version_label(major, 0, ReportStatus::Final);
        content.generated_at = Timestamp::now();
        let version_id = insert_version(
            tx,
            report_id,
            major,
            0,
            ReportStatus::Final,
            &content,
            &current.author,
            notes,
            true,
        )?;

        let now = Timestamp::now();
        tx.execute(
            "UPDATE reports SET current_version_id = ?1, status = ?2, finalized_at = ?3,
                    updated_at = ?3 WHERE id = ?4",
            rusqlite::params![version_id, ReportStatus::Final, now, report_id],
        )?;

        audit::success(
            tx,
            actor,
            AuditAction::ReportFinalized,
            "report",
            report_id,
            json!({
                "ref": report.report_ref,
                "version": content.version_label,
                "warnings": readiness.issues.len(),
            }),
        )?;
        current_version(tx, report_id)
    })
}

/// Open a new editable revision of a finalized report. The finalized version is left
/// exactly as it was.
pub fn create_revision(db: &Database, actor: &Actor, report_id: &str) -> Result<ReportVersion> {
    db.with_tx(|tx| {
        let report = get(tx, report_id)?;
        let current = current_version(tx, report_id)?;
        if current.status != ReportStatus::Final {
            return Err(Error::validation(
                "only a finalized report needs a revision; this one is still editable",
            ));
        }

        // The revision is authored by whoever opens it, with their identity as it is now.
        let user = auth::load_user(tx, &actor.user_id)?;
        let author = user.author_snapshot();
        let mut content = current.content.clone();
        let (major, minor) = (current.version_major, current.version_minor + 1);
        content.version_label = version_label(major, minor, ReportStatus::Draft);
        content.author = author.clone();

        let version_id = insert_version(
            tx,
            report_id,
            major,
            minor,
            ReportStatus::Draft,
            &content,
            &author,
            "Revision of a finalized report",
            false,
        )?;
        tx.execute(
            "UPDATE reports SET current_version_id = ?1, status = ?2, updated_at = ?3
             WHERE id = ?4",
            rusqlite::params![version_id, ReportStatus::Draft, Timestamp::now(), report_id],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::ReportRevisionCreated,
            "report",
            report_id,
            json!({ "ref": report.report_ref, "from": current.version_label }),
        )?;
        current_version(tx, report_id)
    })
}

pub fn delete(db: &Database, actor: &Actor, report_id: &str) -> Result<()> {
    db.with_tx(|tx| {
        let report = get(tx, report_id)?;
        let finalized: i64 = tx.query_row(
            "SELECT COUNT(*) FROM report_versions WHERE report_id = ?1 AND is_immutable = 1
             AND status = 'final'",
            [report_id],
            |row| row.get(0),
        )?;
        if finalized > 0 {
            return Err(Error::Immutable(
                "this report has finalized versions and cannot be deleted".to_string(),
            ));
        }
        tx.execute("DELETE FROM reports WHERE id = ?1", [report_id])?;
        audit::success(
            tx,
            actor,
            AuditAction::ReportDeleted,
            "report",
            report_id,
            json!({ "ref": report.report_ref }),
        )
    })
}

fn blocker(
    code: &str,
    message: &str,
    section_key: Option<&str>,
    question_key: Option<&str>,
) -> ValidationIssue {
    ValidationIssue {
        severity: IssueSeverity::Blocker,
        code: code.to_string(),
        message: message.to_string(),
        section_key: section_key.map(str::to_string),
        question_key: question_key.map(str::to_string),
    }
}

fn warning(
    code: &str,
    message: &str,
    section_key: Option<&str>,
    question_key: Option<&str>,
) -> ValidationIssue {
    ValidationIssue {
        severity: IssueSeverity::Warning,
        code: code.to_string(),
        message: message.to_string(),
        section_key: section_key.map(str::to_string),
        question_key: question_key.map(str::to_string),
    }
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
    if raw.chars().count() <= max {
        return raw.to_string();
    }
    raw.chars().take(max).collect()
}

pub mod standards;

#[cfg(test)]
mod tests;
