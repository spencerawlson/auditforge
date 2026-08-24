//! Engagements: create, read, update, delete, and the summary counts the UI shows.

use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::Deserialize;
use serde_json::json;

use crate::audit;
use crate::database::{escape_like, new_id, next_reference, Database};
use crate::error::{Error, Result};
use crate::models::{
    Actor, AuditAction, Engagement, EngagementStatus, EngagementSummary, EngagementUpdate,
    NewEngagement, NewTeamMember, TeamMember, Timestamp,
};

const MAX_NAME: usize = 200;
const MAX_TEXT: usize = 20_000;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EngagementFilter {
    #[serde(default)]
    pub status: Option<EngagementStatus>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub fn create(db: &Database, actor: &Actor, input: &NewEngagement) -> Result<Engagement> {
    let name = require(&input.name, "engagement name", MAX_NAME)?;
    let start_date = optional_date(&input.start_date, "start date")?;
    let end_date = optional_date(&input.end_date, "end date")?;
    check_date_order(&start_date, &end_date)?;

    let id = db.with_tx(|tx| {
        let id = new_id();
        let engagement_ref = next_reference(tx, "engagement", "ENG")?;
        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO engagements (id, engagement_ref, name, client_name, owner_user_id,
                                      status, start_date, end_date, objectives, scope,
                                      out_of_scope, authorization_ref, rules_of_engagement,
                                      notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
            rusqlite::params![
                id,
                engagement_ref,
                name,
                clamp(&input.client_name, MAX_NAME),
                actor.user_id,
                EngagementStatus::Draft,
                start_date,
                end_date,
                clamp(&input.objectives, MAX_TEXT),
                clamp(&input.scope, MAX_TEXT),
                clamp(&input.out_of_scope, MAX_TEXT),
                clamp(&input.authorization_ref, MAX_NAME),
                clamp(&input.rules_of_engagement, MAX_TEXT),
                clamp(&input.notes, MAX_TEXT),
                now,
            ],
        )?;
        replace_team(tx, &id, &input.team)?;
        audit::success(
            tx,
            actor,
            AuditAction::EngagementCreated,
            "engagement",
            &id,
            json!({ "ref": engagement_ref, "name": name }),
        )?;
        Ok(id)
    })?;

    db.with_conn(|conn| get(conn, &id))
}

pub fn get(conn: &Connection, id: &str) -> Result<Engagement> {
    let mut engagement = conn
        .query_row(
            "SELECT id, engagement_ref, name, client_name, organization_id, owner_user_id,
                    status, start_date, end_date, objectives, scope, out_of_scope,
                    authorization_ref, rules_of_engagement, notes, created_at, updated_at
             FROM engagements WHERE id = ?1",
            [id],
            row_to_engagement,
        )
        .optional()?
        .ok_or_else(|| Error::not_found("engagement"))?;
    engagement.team = load_team(conn, id)?;
    Ok(engagement)
}

fn row_to_engagement(row: &rusqlite::Row<'_>) -> rusqlite::Result<Engagement> {
    Ok(Engagement {
        id: row.get(0)?,
        engagement_ref: row.get(1)?,
        name: row.get(2)?,
        client_name: row.get(3)?,
        organization_id: row.get(4)?,
        owner_user_id: row.get(5)?,
        status: row.get(6)?,
        start_date: row.get(7)?,
        end_date: row.get(8)?,
        objectives: row.get(9)?,
        scope: row.get(10)?,
        out_of_scope: row.get(11)?,
        authorization_ref: row.get(12)?,
        rules_of_engagement: row.get(13)?,
        notes: row.get(14)?,
        team: Vec::new(),
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn load_team(conn: &Connection, engagement_id: &str) -> Result<Vec<TeamMember>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, role, email, sort_order FROM engagement_team_members
         WHERE engagement_id = ?1 ORDER BY sort_order, name",
    )?;
    let rows = stmt
        .query_map([engagement_id], |row| {
            Ok(TeamMember {
                id: row.get(0)?,
                name: row.get(1)?,
                role: row.get(2)?,
                email: row.get(3)?,
                sort_order: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn replace_team(tx: &Transaction<'_>, engagement_id: &str, team: &[NewTeamMember]) -> Result<()> {
    tx.execute(
        "DELETE FROM engagement_team_members WHERE engagement_id = ?1",
        [engagement_id],
    )?;
    for (index, member) in team.iter().enumerate() {
        let name = member.name.trim();
        if name.is_empty() {
            continue;
        }
        tx.execute(
            "INSERT INTO engagement_team_members (id, engagement_id, name, role, email, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                new_id(),
                engagement_id,
                clamp(name, MAX_NAME),
                clamp(&member.role, MAX_NAME),
                clamp(&member.email, MAX_NAME),
                index as i64,
            ],
        )?;
    }
    Ok(())
}

pub fn list(conn: &Connection, filter: &EngagementFilter) -> Result<Vec<EngagementSummary>> {
    let mut sql = String::from(
        "SELECT e.id, e.engagement_ref, e.name, e.client_name, e.organization_id, e.owner_user_id,
                e.status, e.start_date, e.end_date, e.objectives, e.scope, e.out_of_scope,
                e.authorization_ref, e.rules_of_engagement, e.notes, e.created_at, e.updated_at,
                (SELECT COUNT(*) FROM reports r WHERE r.engagement_id = e.id),
                (SELECT COUNT(*) FROM findings f WHERE f.engagement_id = e.id),
                (SELECT COUNT(*) FROM evidence v WHERE v.engagement_id = e.id)
         FROM engagements e WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(status) = filter.status {
        sql.push_str(" AND e.status = ?");
        params.push(Box::new(status.as_str().to_string()));
    }
    if let Some(search) = filter.search.as_ref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(
            " AND (e.name LIKE ? ESCAPE '\\' OR e.client_name LIKE ? ESCAPE '\\'
                   OR e.engagement_ref LIKE ? ESCAPE '\\')",
        );
        let pattern = format!("%{}%", escape_like(search.trim()));
        params.push(Box::new(pattern.clone()));
        params.push(Box::new(pattern.clone()));
        params.push(Box::new(pattern));
    }
    sql.push_str(" ORDER BY e.updated_at DESC LIMIT ?");
    params.push(Box::new(filter.limit.unwrap_or(200).clamp(1, 1_000)));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok((
                row_to_engagement(row)?,
                row.get::<_, i64>(17)?,
                row.get::<_, i64>(18)?,
                row.get::<_, i64>(19)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    rows.into_iter()
        .map(
            |(mut engagement, report_count, finding_count, evidence_count)| {
                engagement.team = load_team(conn, &engagement.id)?;
                Ok(EngagementSummary {
                    engagement,
                    report_count,
                    finding_count,
                    evidence_count,
                })
            },
        )
        .collect()
}

pub fn update(
    db: &Database,
    actor: &Actor,
    id: &str,
    update: &EngagementUpdate,
) -> Result<Engagement> {
    db.with_tx(|tx| {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM engagements WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Err(Error::not_found("engagement"));
        }

        if let Some(name) = &update.name {
            let value = require(name, "engagement name", MAX_NAME)?;
            set_field(tx, id, "name", &value)?;
        }
        if let Some(client_name) = &update.client_name {
            set_field(tx, id, "client_name", &clamp(client_name, MAX_NAME))?;
        }
        if let Some(status) = update.status {
            tx.execute(
                "UPDATE engagements SET status = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![status, Timestamp::now(), id],
            )?;
        }
        if let Some(start_date) = &update.start_date {
            let value = optional_date(start_date, "start date")?;
            tx.execute(
                "UPDATE engagements SET start_date = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![value, Timestamp::now(), id],
            )?;
        }
        if let Some(end_date) = &update.end_date {
            let value = optional_date(end_date, "end date")?;
            tx.execute(
                "UPDATE engagements SET end_date = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![value, Timestamp::now(), id],
            )?;
        }
        for (field, value) in [
            ("objectives", &update.objectives),
            ("scope", &update.scope),
            ("out_of_scope", &update.out_of_scope),
            ("rules_of_engagement", &update.rules_of_engagement),
            ("notes", &update.notes),
        ] {
            if let Some(text) = value {
                set_field(tx, id, field, &clamp(text, MAX_TEXT))?;
            }
        }
        if let Some(authorization_ref) = &update.authorization_ref {
            set_field(
                tx,
                id,
                "authorization_ref",
                &clamp(authorization_ref, MAX_NAME),
            )?;
        }
        if let Some(team) = &update.team {
            replace_team(tx, id, team)?;
        }

        // Dates may have been changed one at a time; check the pair afterwards.
        let (start, end): (Option<String>, Option<String>) = tx.query_row(
            "SELECT start_date, end_date FROM engagements WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        check_date_order(&start, &end)?;

        audit::success(
            tx,
            actor,
            AuditAction::EngagementUpdated,
            "engagement",
            id,
            json!({ "status": update.status.map(|s| s.as_str()) }),
        )?;
        get(tx, id)
    })
}

/// Only the enumerated field names above reach this function; the value is always bound.
fn set_field(tx: &Transaction<'_>, id: &str, column: &str, value: &str) -> Result<()> {
    let sql = format!("UPDATE engagements SET {column} = ?1, updated_at = ?2 WHERE id = ?3");
    tx.execute(sql.as_str(), rusqlite::params![value, Timestamp::now(), id])?;
    Ok(())
}

/// Delete an engagement.
///
/// Refused while a finalized report references it: deleting the container would leave a
/// signed-off document pointing at nothing. Reports must be archived first.
pub fn delete(db: &Database, actor: &Actor, id: &str) -> Result<()> {
    db.with_tx(|tx| {
        let finalized: i64 = tx.query_row(
            "SELECT COUNT(*) FROM reports WHERE engagement_id = ?1 AND status = 'final'",
            [id],
            |row| row.get(0),
        )?;
        if finalized > 0 {
            return Err(Error::Immutable(format!(
                "this engagement has {finalized} finalized report(s); archive them first"
            )));
        }
        let engagement_ref: String = tx
            .query_row(
                "SELECT engagement_ref FROM engagements WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::not_found("engagement"))?;

        tx.execute("DELETE FROM engagements WHERE id = ?1", [id])?;
        audit::success(
            tx,
            actor,
            AuditAction::EngagementDeleted,
            "engagement",
            id,
            json!({ "ref": engagement_ref }),
        )
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

fn optional_date(value: &Option<String>, field: &str) -> Result<Option<String>> {
    match value {
        Some(raw) if !raw.trim().is_empty() => Ok(Some(crate::models::validate_date(raw, field)?)),
        _ => Ok(None),
    }
}

fn check_date_order(start: &Option<String>, end: &Option<String>) -> Result<()> {
    if let (Some(start), Some(end)) = (start, end) {
        if end < start {
            return Err(Error::validation(
                "the end date cannot be before the start date",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::seeded;

    fn sample() -> NewEngagement {
        NewEngagement {
            name: "Q3 External Penetration Test".to_string(),
            client_name: "Northwind Traders".to_string(),
            start_date: Some("2026-09-01".to_string()),
            end_date: Some("2026-09-12".to_string()),
            objectives: "Determine whether the perimeter can be breached.".to_string(),
            scope: "203.0.113.0/24".to_string(),
            team: vec![NewTeamMember {
                name: "A. Tester".to_string(),
                role: "Lead".to_string(),
                email: String::new(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn create_assigns_a_reference_and_stores_the_team() {
        let (db, actor) = seeded();
        let engagement = create(&db, &actor, &sample()).unwrap();
        assert!(engagement.engagement_ref.starts_with("ENG-"));
        assert_eq!(engagement.status, EngagementStatus::Draft);
        assert_eq!(engagement.team.len(), 1);
        assert_eq!(engagement.team[0].name, "A. Tester");
        assert_eq!(engagement.owner_user_id, actor.user_id);
    }

    #[test]
    fn references_are_sequential() {
        let (db, actor) = seeded();
        let first = create(&db, &actor, &sample()).unwrap();
        let second = create(&db, &actor, &sample()).unwrap();
        assert_ne!(first.engagement_ref, second.engagement_ref);
    }

    #[test]
    fn validation_rejects_empty_names_and_reversed_dates() {
        let (db, actor) = seeded();
        let mut bad = sample();
        bad.name = "   ".to_string();
        assert!(create(&db, &actor, &bad).is_err());

        let mut bad = sample();
        bad.start_date = Some("2026-09-12".to_string());
        bad.end_date = Some("2026-09-01".to_string());
        assert!(create(&db, &actor, &bad).is_err());

        let mut bad = sample();
        bad.start_date = Some("not-a-date".to_string());
        assert!(create(&db, &actor, &bad).is_err());
    }

    #[test]
    fn update_changes_only_the_supplied_fields() {
        let (db, actor) = seeded();
        let engagement = create(&db, &actor, &sample()).unwrap();
        let updated = update(
            &db,
            &actor,
            &engagement.id,
            &EngagementUpdate {
                status: Some(EngagementStatus::Active),
                notes: Some("Kickoff completed.".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.status, EngagementStatus::Active);
        assert_eq!(updated.notes, "Kickoff completed.");
        assert_eq!(
            updated.name, engagement.name,
            "untouched fields must be preserved"
        );
        assert_eq!(
            updated.team.len(),
            1,
            "team must survive an update that omits it"
        );
    }

    #[test]
    fn update_rejects_a_date_change_that_inverts_the_range() {
        let (db, actor) = seeded();
        let engagement = create(&db, &actor, &sample()).unwrap();
        let err = update(
            &db,
            &actor,
            &engagement.id,
            &EngagementUpdate {
                end_date: Some(Some("2026-08-01".to_string())),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("end date"), "got {err}");

        // The rejected transaction must not have partially applied.
        let unchanged = db.with_conn(|conn| get(conn, &engagement.id)).unwrap();
        assert_eq!(unchanged.end_date.as_deref(), Some("2026-09-12"));
    }

    #[test]
    fn listing_filters_and_counts() {
        let (db, actor) = seeded();
        let first = create(&db, &actor, &sample()).unwrap();
        let mut other = sample();
        other.name = "Cloud Review".to_string();
        other.client_name = "Contoso".to_string();
        create(&db, &actor, &other).unwrap();

        let all = db
            .with_conn(|conn| list(conn, &EngagementFilter::default()))
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].report_count, 0);

        let searched = db
            .with_conn(|conn| {
                list(
                    conn,
                    &EngagementFilter {
                        search: Some("contoso".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].engagement.client_name, "Contoso");

        update(
            &db,
            &actor,
            &first.id,
            &EngagementUpdate {
                status: Some(EngagementStatus::Completed),
                ..Default::default()
            },
        )
        .unwrap();
        let completed = db
            .with_conn(|conn| {
                list(
                    conn,
                    &EngagementFilter {
                        status: Some(EngagementStatus::Completed),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(completed.len(), 1);
    }

    #[test]
    fn search_treats_sql_metacharacters_as_text() {
        let (db, actor) = seeded();
        create(&db, &actor, &sample()).unwrap();
        for hostile in ["' OR 1=1 --", "%", "_", "'; DROP TABLE engagements; --"] {
            let found = db
                .with_conn(|conn| {
                    list(
                        conn,
                        &EngagementFilter {
                            search: Some(hostile.to_string()),
                            ..Default::default()
                        },
                    )
                })
                .unwrap();
            assert!(found.is_empty(), "{hostile} should match nothing");
        }
        // And the table is still there.
        assert_eq!(
            db.with_conn(|conn| list(conn, &EngagementFilter::default()))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn delete_removes_the_engagement_and_its_team() {
        let (db, actor) = seeded();
        let engagement = create(&db, &actor, &sample()).unwrap();
        delete(&db, &actor, &engagement.id).unwrap();
        assert!(db.with_conn(|conn| get(conn, &engagement.id)).is_err());
        let members: i64 = db
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM engagement_team_members WHERE engagement_id = ?1",
                    [&engagement.id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(members, 0);
    }

    #[test]
    fn missing_engagements_report_not_found() {
        let (db, actor) = seeded();
        assert_eq!(
            db.with_conn(|conn| get(conn, "nope")).unwrap_err().kind(),
            "not_found"
        );
        assert_eq!(delete(&db, &actor, "nope").unwrap_err().kind(), "not_found");
        assert_eq!(
            update(&db, &actor, "nope", &EngagementUpdate::default())
                .unwrap_err()
                .kind(),
            "not_found"
        );
    }
}
