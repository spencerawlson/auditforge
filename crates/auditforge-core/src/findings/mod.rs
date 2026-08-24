//! Findings.
//!
//! One rule dominates this module: **severity is never set implicitly.** A new finding
//! starts at Informational with `severity_confirmed = false`, and the only way to change
//! that is [`set_severity`], which requires an explicit human confirmation flag. Ordinary
//! edits - including edits that apply an accepted AI suggestion - cannot touch it.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::json;

use crate::audit;
use crate::database::{escape_like, new_id, next_reference, Database};
use crate::error::{Error, Result};
use crate::models::{
    Actor, AuditAction, Finding, FindingUpdate, NewFinding, Provenance, Severity, SeverityDecision,
    Timestamp, VerificationStatus,
};

const MAX_TITLE: usize = 300;
const MAX_TEXT: usize = 50_000;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FindingFilter {
    #[serde(default)]
    pub engagement_id: Option<String>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub verification_status: Option<VerificationStatus>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub fn create(db: &Database, actor: &Actor, input: &NewFinding) -> Result<Finding> {
    let title = require(&input.title, "finding title", MAX_TITLE)?;
    if let Some(score) = input.cvss_score {
        validate_cvss(score)?;
    }
    let author_name: String = db.with_conn(|conn| {
        Ok(conn.query_row(
            "SELECT full_name FROM users WHERE id = ?1",
            [&actor.user_id],
            |row| row.get(0),
        )?)
    })?;

    let id = db.with_tx(|tx| {
        let id = new_id();
        let finding_ref = next_reference(tx, "finding", "FND")?;
        let now = Timestamp::now();
        // A severity supplied at creation is recorded but stays unconfirmed until a
        // person confirms it, so it cannot slip into a finalized report unreviewed.
        let severity = input.severity.unwrap_or(Severity::Informational);
        tx.execute(
            "INSERT INTO findings (id, finding_ref, engagement_id, title, severity,
                                   severity_confirmed, category, affected_asset, description,
                                   technical_details, impact, likelihood, risk, root_cause,
                                   remediation, recommendation, verification_status,
                                   reference_links, cvss_vector, cvss_score, provenance_json,
                                   author_user_id, author_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, '{}', ?20, ?21, ?22, ?22)",
            rusqlite::params![
                id,
                finding_ref,
                input.engagement_id,
                title,
                severity,
                clamp(&input.category, MAX_TITLE),
                clamp(&input.affected_asset, MAX_TEXT),
                clamp(&input.description, MAX_TEXT),
                clamp(&input.technical_details, MAX_TEXT),
                clamp(&input.impact, MAX_TEXT),
                clamp(&input.likelihood, MAX_TEXT),
                clamp(&input.risk, MAX_TEXT),
                clamp(&input.root_cause, MAX_TEXT),
                clamp(&input.remediation, MAX_TEXT),
                clamp(&input.recommendation, MAX_TEXT),
                VerificationStatus::Unverified,
                clamp(&input.reference_links, MAX_TEXT),
                clamp(&input.cvss_vector, MAX_TITLE),
                input.cvss_score,
                actor.user_id,
                author_name,
                now,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::FindingCreated,
            "finding",
            &id,
            json!({ "ref": finding_ref, "title": title }),
        )?;
        Ok(id)
    })?;

    db.with_conn(|conn| get(conn, &id))
}

pub fn get(conn: &Connection, id: &str) -> Result<Finding> {
    let mut finding = conn
        .query_row(SELECT_FINDING, [id], row_to_finding)
        .optional()?
        .ok_or_else(|| Error::not_found("finding"))?;
    finding.evidence_ids = linked_evidence(conn, id)?;
    Ok(finding)
}

const SELECT_FINDING: &str = "SELECT id, finding_ref, engagement_id, title, severity,
        severity_confirmed, category, affected_asset, description, technical_details, impact,
        likelihood, risk, root_cause, remediation, recommendation, verification_status,
        reference_links, cvss_vector, cvss_score, provenance_json, author_user_id, author_name,
        created_at, updated_at
    FROM findings WHERE id = ?1";

fn row_to_finding(row: &rusqlite::Row<'_>) -> rusqlite::Result<Finding> {
    let provenance_raw: String = row.get(20)?;
    Ok(Finding {
        id: row.get(0)?,
        finding_ref: row.get(1)?,
        engagement_id: row.get(2)?,
        title: row.get(3)?,
        severity: row.get(4)?,
        severity_confirmed: row.get::<_, i64>(5)? != 0,
        category: row.get(6)?,
        affected_asset: row.get(7)?,
        description: row.get(8)?,
        technical_details: row.get(9)?,
        impact: row.get(10)?,
        likelihood: row.get(11)?,
        risk: row.get(12)?,
        root_cause: row.get(13)?,
        remediation: row.get(14)?,
        recommendation: row.get(15)?,
        verification_status: row.get(16)?,
        reference_links: row.get(17)?,
        cvss_vector: row.get(18)?,
        cvss_score: row.get(19)?,
        provenance: serde_json::from_str(&provenance_raw).unwrap_or_default(),
        author_user_id: row.get(21)?,
        author_name: row.get(22)?,
        evidence_ids: Vec::new(),
        created_at: row.get(23)?,
        updated_at: row.get(24)?,
    })
}

fn linked_evidence(conn: &Connection, finding_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT evidence_id FROM evidence_links
         WHERE target_type = 'finding' AND target_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map([finding_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list(conn: &Connection, filter: &FindingFilter) -> Result<Vec<Finding>> {
    let mut sql = String::from(
        "SELECT id, finding_ref, engagement_id, title, severity, severity_confirmed, category,
                affected_asset, description, technical_details, impact, likelihood, risk,
                root_cause, remediation, recommendation, verification_status, reference_links,
                cvss_vector, cvss_score, provenance_json, author_user_id, author_name,
                created_at, updated_at
         FROM findings WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(engagement_id) = &filter.engagement_id {
        sql.push_str(" AND engagement_id = ?");
        params.push(Box::new(engagement_id.clone()));
    }
    if let Some(severity) = filter.severity {
        sql.push_str(" AND severity = ?");
        params.push(Box::new(severity.as_str().to_string()));
    }
    if let Some(status) = filter.verification_status {
        sql.push_str(" AND verification_status = ?");
        params.push(Box::new(status.as_str().to_string()));
    }
    if let Some(search) = filter.search.as_ref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(
            " AND (title LIKE ? ESCAPE '\\' OR finding_ref LIKE ? ESCAPE '\\'
                   OR affected_asset LIKE ? ESCAPE '\\' OR category LIKE ? ESCAPE '\\')",
        );
        let pattern = format!("%{}%", escape_like(search.trim()));
        for _ in 0..4 {
            params.push(Box::new(pattern.clone()));
        }
    }
    // Severity order is a domain concept, not alphabetical; CASE keeps it in SQL so the
    // limit applies to the right rows.
    sql.push_str(
        " ORDER BY CASE severity
             WHEN 'critical' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2
             WHEN 'low' THEN 3 ELSE 4 END, updated_at DESC LIMIT ?",
    );
    params.push(Box::new(filter.limit.unwrap_or(500).clamp(1, 5_000)));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut findings = stmt
        .query_map(refs.as_slice(), row_to_finding)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for finding in &mut findings {
        finding.evidence_ids = linked_evidence(conn, &finding.id)?;
    }
    Ok(findings)
}

/// Apply an edit. Severity is intentionally absent from `FindingUpdate`.
pub fn update(db: &Database, actor: &Actor, id: &str, update: &FindingUpdate) -> Result<Finding> {
    if let Some(Some(score)) = update.cvss_score {
        validate_cvss(score)?;
    }
    db.with_tx(|tx| {
        let mut finding = get(tx, id)?;

        if let Some(title) = &update.title {
            finding.title = require(title, "finding title", MAX_TITLE)?;
        }
        if let Some(engagement_id) = &update.engagement_id {
            finding.engagement_id = engagement_id.clone();
        }
        for (field, value) in [
            ("category", &update.category),
            ("affected_asset", &update.affected_asset),
            ("description", &update.description),
            ("technical_details", &update.technical_details),
            ("impact", &update.impact),
            ("likelihood", &update.likelihood),
            ("risk", &update.risk),
            ("root_cause", &update.root_cause),
            ("remediation", &update.remediation),
            ("recommendation", &update.recommendation),
        ] {
            if let Some(text) = value {
                finding.set_field(field, clamp(text, MAX_TEXT));
            }
        }
        if let Some(status) = update.verification_status {
            finding.verification_status = status;
        }
        if let Some(links) = &update.reference_links {
            finding.reference_links = clamp(links, MAX_TEXT);
        }
        if let Some(vector) = &update.cvss_vector {
            finding.cvss_vector = clamp(vector, MAX_TITLE);
        }
        if let Some(score) = update.cvss_score {
            finding.cvss_score = score;
        }
        if let Some(provenance) = &update.provenance {
            merge_provenance(&mut finding.provenance, provenance);
        }

        tx.execute(
            "UPDATE findings SET title = ?1, engagement_id = ?2, category = ?3,
                    affected_asset = ?4, description = ?5, technical_details = ?6, impact = ?7,
                    likelihood = ?8, risk = ?9, root_cause = ?10, remediation = ?11,
                    recommendation = ?12, verification_status = ?13, reference_links = ?14,
                    cvss_vector = ?15, cvss_score = ?16, provenance_json = ?17, updated_at = ?18
             WHERE id = ?19",
            rusqlite::params![
                finding.title,
                finding.engagement_id,
                finding.category,
                finding.affected_asset,
                finding.description,
                finding.technical_details,
                finding.impact,
                finding.likelihood,
                finding.risk,
                finding.root_cause,
                finding.remediation,
                finding.recommendation,
                finding.verification_status,
                finding.reference_links,
                finding.cvss_vector,
                finding.cvss_score,
                serde_json::to_string(&finding.provenance)?,
                Timestamp::now(),
                id,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::FindingModified,
            "finding",
            id,
            json!({ "ref": finding.finding_ref }),
        )?;
        get(tx, id)
    })
}

/// Set and confirm severity. This is the only path that can change it.
pub fn set_severity(
    db: &Database,
    actor: &Actor,
    id: &str,
    decision: &SeverityDecision,
) -> Result<Finding> {
    if !decision.confirmed_by_user {
        return Err(Error::validation(
            "severity must be confirmed by a person before it can be set",
        ));
    }
    db.with_tx(|tx| {
        let existing = get(tx, id)?;
        tx.execute(
            "UPDATE findings SET severity = ?1, severity_confirmed = 1, updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![decision.severity, Timestamp::now(), id],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::FindingSeverityConfirmed,
            "finding",
            id,
            json!({
                "ref": existing.finding_ref,
                "from": existing.severity.as_str(),
                "to": decision.severity.as_str(),
                "rationale": decision.rationale,
            }),
        )?;
        get(tx, id)
    })
}

pub fn delete(db: &Database, actor: &Actor, id: &str) -> Result<()> {
    db.with_tx(|tx| {
        let finding_ref: String = tx
            .query_row(
                "SELECT finding_ref FROM findings WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::not_found("finding"))?;

        // A finalized report embeds a copy of the finding, so removing the live record
        // does not alter history; but warn loudly in the audit trail.
        let in_final: i64 = tx.query_row(
            "SELECT COUNT(*) FROM report_findings rf
             JOIN reports r ON r.id = rf.report_id
             WHERE rf.finding_id = ?1 AND r.status = 'final'",
            [id],
            |row| row.get(0),
        )?;

        tx.execute("DELETE FROM findings WHERE id = ?1", [id])?;
        audit::success(
            tx,
            actor,
            AuditAction::FindingDeleted,
            "finding",
            id,
            json!({ "ref": finding_ref, "referenced_by_finalized_reports": in_final }),
        )
    })
}

/// Findings that a report may include: only those whose severity a person has confirmed.
pub fn unconfirmed_severity(conn: &Connection, finding_ids: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for id in finding_ids {
        let confirmed: Option<i64> = conn
            .query_row(
                "SELECT severity_confirmed FROM findings WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        match confirmed {
            Some(0) | None => out.push(id.clone()),
            _ => {}
        }
    }
    Ok(out)
}

fn merge_provenance(
    target: &mut BTreeMap<String, Provenance>,
    incoming: &BTreeMap<String, Provenance>,
) {
    for (field, provenance) in incoming {
        target.insert(field.clone(), *provenance);
    }
}

fn validate_cvss(score: f64) -> Result<()> {
    if !(0.0..=10.0).contains(&score) || score.is_nan() {
        return Err(Error::validation("CVSS score must be between 0.0 and 10.0"));
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

    fn sample() -> NewFinding {
        NewFinding {
            title: "TLS 1.0 enabled on the VPN concentrator".to_string(),
            description: "The concentrator negotiates TLS 1.0.".to_string(),
            affected_asset: "vpn.example.internal".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn a_new_finding_is_informational_and_unconfirmed() {
        let (db, actor) = seeded();
        let finding = create(&db, &actor, &sample()).unwrap();
        assert!(finding.finding_ref.starts_with("FND-"));
        assert_eq!(finding.severity, Severity::Informational);
        assert!(!finding.severity_confirmed);
        assert_eq!(finding.verification_status, VerificationStatus::Unverified);
        assert_eq!(finding.author_name, "Spencer Rawlson Spady");
    }

    #[test]
    fn a_severity_supplied_at_creation_is_recorded_but_still_unconfirmed() {
        let (db, actor) = seeded();
        let mut input = sample();
        input.severity = Some(Severity::High);
        let finding = create(&db, &actor, &input).unwrap();
        assert_eq!(finding.severity, Severity::High);
        assert!(
            !finding.severity_confirmed,
            "a severity typed into the create form is not a confirmation"
        );
    }

    #[test]
    fn ordinary_edits_cannot_change_severity() {
        let (db, actor) = seeded();
        let finding = create(&db, &actor, &sample()).unwrap();
        let updated = update(
            &db,
            &actor,
            &finding.id,
            &FindingUpdate {
                description: Some("Rewritten by the author.".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.description, "Rewritten by the author.");
        assert_eq!(updated.severity, Severity::Informational);
        assert!(!updated.severity_confirmed);
    }

    #[test]
    fn severity_requires_an_explicit_confirmation() {
        let (db, actor) = seeded();
        let finding = create(&db, &actor, &sample()).unwrap();

        let err = set_severity(
            &db,
            &actor,
            &finding.id,
            &SeverityDecision {
                severity: Severity::Critical,
                confirmed_by_user: false,
                rationale: String::new(),
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("confirmed by a person"),
            "got {err}"
        );

        let confirmed = set_severity(
            &db,
            &actor,
            &finding.id,
            &SeverityDecision {
                severity: Severity::High,
                confirmed_by_user: true,
                rationale: "Externally reachable and trivially exploitable.".to_string(),
            },
        )
        .unwrap();
        assert_eq!(confirmed.severity, Severity::High);
        assert!(confirmed.severity_confirmed);
    }

    #[test]
    fn severity_confirmation_is_audited_with_the_before_and_after() {
        let (db, actor) = seeded();
        let finding = create(&db, &actor, &sample()).unwrap();
        set_severity(
            &db,
            &actor,
            &finding.id,
            &SeverityDecision {
                severity: Severity::Critical,
                confirmed_by_user: true,
                rationale: "Remote code execution".to_string(),
            },
        )
        .unwrap();

        let entries = db
            .with_conn(|conn| {
                crate::audit::query(
                    conn,
                    &crate::models::AuditQuery {
                        action: Some(AuditAction::FindingSeverityConfirmed),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].detail["from"], "informational");
        assert_eq!(entries[0].detail["to"], "critical");
    }

    #[test]
    fn provenance_is_recorded_per_field() {
        let (db, actor) = seeded();
        let finding = create(&db, &actor, &sample()).unwrap();
        let mut provenance = BTreeMap::new();
        provenance.insert("impact".to_string(), Provenance::AiWording);
        let updated = update(
            &db,
            &actor,
            &finding.id,
            &FindingUpdate {
                impact: Some("Phrased by the assistant from the author's notes.".to_string()),
                provenance: Some(provenance),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.provenance_of("impact"), Provenance::AiWording);
        assert_eq!(
            updated.provenance_of("description"),
            Provenance::UserProvided,
            "untouched fields stay attributed to the author"
        );
    }

    #[test]
    fn cvss_scores_are_range_checked() {
        let (db, actor) = seeded();
        let mut input = sample();
        input.cvss_score = Some(11.0);
        assert!(create(&db, &actor, &input).is_err());

        input.cvss_score = Some(9.8);
        let finding = create(&db, &actor, &input).unwrap();
        assert_eq!(finding.cvss_score, Some(9.8));

        assert!(update(
            &db,
            &actor,
            &finding.id,
            &FindingUpdate {
                cvss_score: Some(Some(-1.0)),
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn listing_orders_by_severity_and_filters() {
        let (db, actor) = seeded();
        for (title, severity) in [
            ("Low issue", Severity::Low),
            ("Critical issue", Severity::Critical),
            ("Medium issue", Severity::Medium),
        ] {
            let finding = create(
                &db,
                &actor,
                &NewFinding {
                    title: title.to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
            set_severity(
                &db,
                &actor,
                &finding.id,
                &SeverityDecision {
                    severity,
                    confirmed_by_user: true,
                    rationale: String::new(),
                },
            )
            .unwrap();
        }

        let all = db
            .with_conn(|conn| list(conn, &FindingFilter::default()))
            .unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].severity, Severity::Critical);
        assert_eq!(all[2].severity, Severity::Low);

        let critical = db
            .with_conn(|conn| {
                list(
                    conn,
                    &FindingFilter {
                        severity: Some(Severity::Critical),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(critical.len(), 1);
    }

    #[test]
    fn unconfirmed_severity_reports_exactly_the_blocking_findings() {
        let (db, actor) = seeded();
        let unconfirmed = create(&db, &actor, &sample()).unwrap();
        let confirmed = create(&db, &actor, &sample()).unwrap();
        set_severity(
            &db,
            &actor,
            &confirmed.id,
            &SeverityDecision {
                severity: Severity::Medium,
                confirmed_by_user: true,
                rationale: String::new(),
            },
        )
        .unwrap();

        let blocking = db
            .with_conn(|conn| {
                unconfirmed_severity(
                    conn,
                    &[
                        unconfirmed.id.clone(),
                        confirmed.id.clone(),
                        "ghost".to_string(),
                    ],
                )
            })
            .unwrap();
        assert!(blocking.contains(&unconfirmed.id));
        assert!(!blocking.contains(&confirmed.id));
        assert!(
            blocking.contains(&"ghost".to_string()),
            "unknown ids must block too"
        );
    }

    #[test]
    fn delete_removes_the_finding() {
        let (db, actor) = seeded();
        let finding = create(&db, &actor, &sample()).unwrap();
        delete(&db, &actor, &finding.id).unwrap();
        assert!(db.with_conn(|conn| get(conn, &finding.id)).is_err());
        assert_eq!(
            delete(&db, &actor, &finding.id).unwrap_err().kind(),
            "not_found"
        );
    }
}
