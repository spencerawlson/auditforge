//! Incident records.
//!
//! Severity follows the same rule as findings: an incident carries a severity, but it is
//! only ever set through [`set_severity`] with an explicit human confirmation.

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::json;

use crate::audit;
use crate::database::{escape_like, new_id, next_reference, Database};
use crate::error::{Error, Result};
use crate::models::{
    Actor, AuditAction, Incident, IncidentStatus, IncidentUpdate, NewIncident, Severity,
    SeverityDecision, Timestamp,
};

const MAX_TITLE: usize = 300;
const MAX_TEXT: usize = 50_000;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct IncidentFilter {
    #[serde(default)]
    pub engagement_id: Option<String>,
    #[serde(default)]
    pub status: Option<IncidentStatus>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub fn create(db: &Database, actor: &Actor, input: &NewIncident) -> Result<Incident> {
    let title = require(&input.title, "incident title", MAX_TITLE)?;
    let id = db.with_tx(|tx| {
        let id = new_id();
        let incident_ref = next_reference(tx, "incident", "INC")?;
        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO incidents (id, incident_ref, engagement_id, title, detected_at, reporter,
                                    severity, severity_confirmed, affected_systems, description,
                                    impact, detection_method, status, created_by, created_at,
                                    updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
            rusqlite::params![
                id,
                incident_ref,
                input.engagement_id,
                title,
                input.detected_at,
                clamp(&input.reporter, MAX_TITLE),
                Severity::Informational,
                clamp(&input.affected_systems, MAX_TEXT),
                clamp(&input.description, MAX_TEXT),
                clamp(&input.impact, MAX_TEXT),
                clamp(&input.detection_method, MAX_TEXT),
                IncidentStatus::Open,
                actor.user_id,
                now,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::IncidentCreated,
            "incident",
            &id,
            json!({ "ref": incident_ref, "title": title }),
        )?;
        Ok(id)
    })?;
    db.with_conn(|conn| get(conn, &id))
}

const COLUMNS: &str = "id, incident_ref, engagement_id, title, detected_at, resolved_at, reporter,
        severity, severity_confirmed, affected_systems, description, impact, timeline,
        detection_method, response_actions, root_cause, remediation, lessons_learned, status,
        created_by, created_at, updated_at";

fn row_to_incident(row: &rusqlite::Row<'_>) -> rusqlite::Result<Incident> {
    Ok(Incident {
        id: row.get(0)?,
        incident_ref: row.get(1)?,
        engagement_id: row.get(2)?,
        title: row.get(3)?,
        detected_at: row.get(4)?,
        resolved_at: row.get(5)?,
        reporter: row.get(6)?,
        severity: row.get(7)?,
        severity_confirmed: row.get::<_, i64>(8)? != 0,
        affected_systems: row.get(9)?,
        description: row.get(10)?,
        impact: row.get(11)?,
        timeline: row.get(12)?,
        detection_method: row.get(13)?,
        response_actions: row.get(14)?,
        root_cause: row.get(15)?,
        remediation: row.get(16)?,
        lessons_learned: row.get(17)?,
        status: row.get(18)?,
        created_by: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Incident> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM incidents WHERE id = ?1"),
        [id],
        row_to_incident,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("incident"))
}

pub fn list(conn: &Connection, filter: &IncidentFilter) -> Result<Vec<Incident>> {
    let mut sql = format!("SELECT {COLUMNS} FROM incidents WHERE 1 = 1");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(engagement_id) = &filter.engagement_id {
        sql.push_str(" AND engagement_id = ?");
        params.push(Box::new(engagement_id.clone()));
    }
    if let Some(status) = filter.status {
        sql.push_str(" AND status = ?");
        params.push(Box::new(status.as_str().to_string()));
    }
    if let Some(severity) = filter.severity {
        sql.push_str(" AND severity = ?");
        params.push(Box::new(severity.as_str().to_string()));
    }
    if let Some(search) = filter.search.as_ref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(
            " AND (title LIKE ? ESCAPE '\\' OR incident_ref LIKE ? ESCAPE '\\'
                   OR affected_systems LIKE ? ESCAPE '\\')",
        );
        let pattern = format!("%{}%", escape_like(search.trim()));
        for _ in 0..3 {
            params.push(Box::new(pattern.clone()));
        }
    }
    sql.push_str(" ORDER BY COALESCE(detected_at, created_at) DESC LIMIT ?");
    params.push(Box::new(filter.limit.unwrap_or(200).clamp(1, 2_000)));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), row_to_incident)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn update(db: &Database, actor: &Actor, id: &str, update: &IncidentUpdate) -> Result<Incident> {
    db.with_tx(|tx| {
        let mut incident = get(tx, id)?;
        if let Some(title) = &update.title {
            incident.title = require(title, "incident title", MAX_TITLE)?;
        }
        if let Some(engagement_id) = &update.engagement_id {
            incident.engagement_id = engagement_id.clone();
        }
        if let Some(detected_at) = update.detected_at {
            incident.detected_at = detected_at;
        }
        if let Some(resolved_at) = update.resolved_at {
            incident.resolved_at = resolved_at;
        }
        if let (Some(detected), Some(resolved)) = (incident.detected_at, incident.resolved_at) {
            if resolved.is_before(detected) {
                return Err(Error::validation(
                    "the resolution time cannot be before the detection time",
                ));
            }
        }
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = &update.$field {
                    incident.$field = clamp(value, MAX_TEXT);
                }
            };
        }
        apply!(reporter);
        apply!(affected_systems);
        apply!(description);
        apply!(impact);
        apply!(timeline);
        apply!(detection_method);
        apply!(response_actions);
        apply!(root_cause);
        apply!(remediation);
        apply!(lessons_learned);
        if let Some(status) = update.status {
            incident.status = status;
        }

        tx.execute(
            "UPDATE incidents SET title = ?1, engagement_id = ?2, detected_at = ?3,
                    resolved_at = ?4, reporter = ?5, affected_systems = ?6, description = ?7,
                    impact = ?8, timeline = ?9, detection_method = ?10, response_actions = ?11,
                    root_cause = ?12, remediation = ?13, lessons_learned = ?14, status = ?15,
                    updated_at = ?16
             WHERE id = ?17",
            rusqlite::params![
                incident.title,
                incident.engagement_id,
                incident.detected_at,
                incident.resolved_at,
                incident.reporter,
                incident.affected_systems,
                incident.description,
                incident.impact,
                incident.timeline,
                incident.detection_method,
                incident.response_actions,
                incident.root_cause,
                incident.remediation,
                incident.lessons_learned,
                incident.status,
                Timestamp::now(),
                id,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::IncidentUpdated,
            "incident",
            id,
            json!({ "ref": incident.incident_ref, "status": incident.status.as_str() }),
        )?;
        get(tx, id)
    })
}

pub fn set_severity(
    db: &Database,
    actor: &Actor,
    id: &str,
    decision: &SeverityDecision,
) -> Result<Incident> {
    if !decision.confirmed_by_user {
        return Err(Error::validation(
            "severity must be confirmed by a person before it can be set",
        ));
    }
    db.with_tx(|tx| {
        let existing = get(tx, id)?;
        tx.execute(
            "UPDATE incidents SET severity = ?1, severity_confirmed = 1, updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![decision.severity, Timestamp::now(), id],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::IncidentUpdated,
            "incident",
            id,
            json!({
                "ref": existing.incident_ref,
                "severity_from": existing.severity.as_str(),
                "severity_to": decision.severity.as_str(),
            }),
        )?;
        get(tx, id)
    })
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

    fn sample() -> NewIncident {
        NewIncident {
            title: "Unauthorized access to the jump host".to_string(),
            detected_at: Some(Timestamp::now()),
            reporter: "SOC".to_string(),
            affected_systems: "jump01".to_string(),
            description: "An unexpected session was observed.".to_string(),
            detection_method: "EDR alert".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn create_assigns_a_reference_and_opens_the_incident() {
        let (db, actor) = seeded();
        let incident = create(&db, &actor, &sample()).unwrap();
        assert!(incident.incident_ref.starts_with("INC-"));
        assert_eq!(incident.status, IncidentStatus::Open);
        assert_eq!(incident.severity, Severity::Informational);
        assert!(!incident.severity_confirmed);
    }

    #[test]
    fn severity_needs_confirmation_here_too() {
        let (db, actor) = seeded();
        let incident = create(&db, &actor, &sample()).unwrap();
        assert!(set_severity(
            &db,
            &actor,
            &incident.id,
            &SeverityDecision {
                severity: Severity::High,
                confirmed_by_user: false,
                rationale: String::new()
            }
        )
        .is_err());

        let confirmed = set_severity(
            &db,
            &actor,
            &incident.id,
            &SeverityDecision {
                severity: Severity::High,
                confirmed_by_user: true,
                rationale: "Confirmed lateral movement".to_string(),
            },
        )
        .unwrap();
        assert!(confirmed.severity_confirmed);
    }

    #[test]
    fn resolution_cannot_precede_detection() {
        let (db, actor) = seeded();
        let incident = create(&db, &actor, &sample()).unwrap();
        let err = update(
            &db,
            &actor,
            &incident.id,
            &IncidentUpdate {
                resolved_at: Some(Some(Timestamp::now().plus_seconds(-86_400))),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("before the detection time"),
            "got {err}"
        );
    }

    #[test]
    fn updates_apply_and_list_filters_work() {
        let (db, actor) = seeded();
        let incident = create(&db, &actor, &sample()).unwrap();
        let updated = update(
            &db,
            &actor,
            &incident.id,
            &IncidentUpdate {
                status: Some(IncidentStatus::Contained),
                root_cause: Some("Reused credentials from a prior breach.".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.status, IncidentStatus::Contained);
        assert!(updated.root_cause.contains("Reused credentials"));
        assert_eq!(updated.title, incident.title);

        let contained = db
            .with_conn(|conn| {
                list(
                    conn,
                    &IncidentFilter {
                        status: Some(IncidentStatus::Contained),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(contained.len(), 1);

        let none = db
            .with_conn(|conn| {
                list(
                    conn,
                    &IncidentFilter {
                        search: Some("%".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert!(none.is_empty(), "wildcards in search must be literal");
    }
}
