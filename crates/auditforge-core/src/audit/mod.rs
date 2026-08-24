//! Append-only audit trail.
//!
//! The core exposes writes and reads for `audit_logs` and nothing else - no update, no
//! delete. Recording is best effort at the call site only in the sense that a failure to
//! log is surfaced as an error rather than swallowed; it never silently succeeds.

use rusqlite::{Connection, Transaction};
use serde_json::json;

use crate::database::{escape_like, new_id};
use crate::error::Result;
use crate::models::{Actor, AuditAction, AuditEntry, AuditOutcome, AuditQuery, Timestamp};

/// Record an entry inside an existing transaction, so the audit row commits or rolls
/// back with the change it describes.
pub fn record(
    tx: &Transaction<'_>,
    actor: Option<&Actor>,
    action: AuditAction,
    target_type: &str,
    target_id: &str,
    outcome: AuditOutcome,
    detail: serde_json::Value,
) -> Result<()> {
    insert(tx, actor, action, target_type, target_id, outcome, detail)
}

/// Record an entry outside a transaction. Used for events that have no accompanying
/// write, such as a failed login against an unknown username.
pub fn record_on(
    conn: &Connection,
    actor: Option<&Actor>,
    action: AuditAction,
    target_type: &str,
    target_id: &str,
    outcome: AuditOutcome,
    detail: serde_json::Value,
) -> Result<()> {
    insert(conn, actor, action, target_type, target_id, outcome, detail)
}

fn insert(
    conn: &Connection,
    actor: Option<&Actor>,
    action: AuditAction,
    target_type: &str,
    target_id: &str,
    outcome: AuditOutcome,
    detail: serde_json::Value,
) -> Result<()> {
    let detail = if detail.is_object() {
        detail
    } else {
        json!({ "value": detail })
    };
    conn.execute(
        "INSERT INTO audit_logs
            (id, occurred_at, user_id, username, action, target_type, target_id, outcome, detail_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            new_id(),
            Timestamp::now(),
            actor.map(|a| a.user_id.as_str()),
            actor.map(|a| a.username.as_str()).unwrap_or(""),
            action,
            target_type,
            target_id,
            outcome,
            serde_json::to_string(&detail)?,
        ],
    )?;
    Ok(())
}

/// Convenience for the common "this succeeded" case.
pub fn success(
    tx: &Transaction<'_>,
    actor: &Actor,
    action: AuditAction,
    target_type: &str,
    target_id: &str,
    detail: serde_json::Value,
) -> Result<()> {
    record(
        tx,
        Some(actor),
        action,
        target_type,
        target_id,
        AuditOutcome::Success,
        detail,
    )
}

pub fn query(conn: &Connection, filter: &AuditQuery) -> Result<Vec<AuditEntry>> {
    // Every filter is a bound parameter; the only interpolated text is the fixed
    // fragment list below.
    let mut sql = String::from(
        "SELECT id, occurred_at, user_id, username, action, target_type, target_id, outcome,
                detail_json
         FROM audit_logs WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(action) = filter.action {
        sql.push_str(" AND action = ?");
        params.push(Box::new(action.as_str().to_string()));
    }
    if let Some(user_id) = &filter.user_id {
        sql.push_str(" AND user_id = ?");
        params.push(Box::new(user_id.clone()));
    }
    if let Some(target_type) = &filter.target_type {
        sql.push_str(" AND target_type = ?");
        params.push(Box::new(target_type.clone()));
    }
    if let Some(target_id) = &filter.target_id {
        sql.push_str(" AND target_id LIKE ? ESCAPE '\\'");
        params.push(Box::new(format!("%{}%", escape_like(target_id))));
    }
    if let Some(since) = filter.since {
        sql.push_str(" AND occurred_at >= ?");
        params.push(Box::new(since.to_rfc3339()));
    }
    if let Some(until) = filter.until {
        sql.push_str(" AND occurred_at <= ?");
        params.push(Box::new(until.to_rfc3339()));
    }

    sql.push_str(" ORDER BY occurred_at DESC, rowid DESC LIMIT ?");
    let limit = filter.limit.unwrap_or(200).clamp(1, 5_000);
    params.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            let detail_raw: String = row.get(8)?;
            Ok(AuditEntry {
                id: row.get(0)?,
                occurred_at: row.get(1)?,
                user_id: row.get(2)?,
                username: row.get(3)?,
                action: row.get(4)?,
                target_type: row.get(5)?,
                target_id: row.get(6)?,
                outcome: row.get(7)?,
                detail: serde_json::from_str(&detail_raw).unwrap_or(serde_json::Value::Null),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Count of entries, used by the settings screen.
pub fn count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM audit_logs", [], |row| row.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn actor() -> Actor {
        Actor {
            user_id: "user-1".to_string(),
            username: "analyst".to_string(),
            session_id: "session-1".to_string(),
        }
    }

    fn db_with_user() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, username, password_hash, full_name, password_changed_at,
                                    created_at, updated_at)
                 VALUES ('user-1', 'analyst', 'x', 'Analyst', ?1, ?1, ?1)",
                rusqlite::params![Timestamp::now()],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    #[test]
    fn entries_are_recorded_and_read_back_newest_first() {
        let db = db_with_user();
        db.with_tx(|tx| {
            success(
                tx,
                &actor(),
                AuditAction::ReportCreated,
                "report",
                "r-1",
                json!({"a": 1}),
            )?;
            success(
                tx,
                &actor(),
                AuditAction::ReportFinalized,
                "report",
                "r-1",
                json!({}),
            )
        })
        .unwrap();

        let entries = db
            .with_conn(|conn| query(conn, &AuditQuery::default()))
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action, AuditAction::ReportFinalized);
        assert_eq!(entries[0].username, "analyst");
        assert_eq!(entries[1].detail["a"], 1);
    }

    #[test]
    fn failed_events_can_be_recorded_without_an_actor() {
        let db = db_with_user();
        db.with_conn(|conn| {
            record_on(
                conn,
                None,
                AuditAction::LoginFailed,
                "user",
                "ghost",
                AuditOutcome::Failure,
                json!({"reason": "unknown user"}),
            )
        })
        .unwrap();
        let entries = db
            .with_conn(|conn| query(conn, &AuditQuery::default()))
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].outcome, AuditOutcome::Failure);
        assert!(entries[0].user_id.is_none());
    }

    #[test]
    fn filters_are_applied() {
        let db = db_with_user();
        db.with_tx(|tx| {
            success(
                tx,
                &actor(),
                AuditAction::ReportCreated,
                "report",
                "r-1",
                json!({}),
            )?;
            success(
                tx,
                &actor(),
                AuditAction::FindingCreated,
                "finding",
                "f-1",
                json!({}),
            )
        })
        .unwrap();

        let filtered = db
            .with_conn(|conn| {
                query(
                    conn,
                    &AuditQuery {
                        action: Some(AuditAction::FindingCreated),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].target_id, "f-1");
    }

    #[test]
    fn target_filter_treats_wildcards_as_literal_text() {
        let db = db_with_user();
        db.with_tx(|tx| {
            success(
                tx,
                &actor(),
                AuditAction::ReportCreated,
                "report",
                "r-1",
                json!({}),
            )
        })
        .unwrap();

        let matched = db
            .with_conn(|conn| {
                query(
                    conn,
                    &AuditQuery {
                        target_id: Some("%".to_string()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert!(matched.is_empty(), "a bare % must not match every row");
    }

    #[test]
    fn limit_is_clamped_to_a_sane_range() {
        let db = db_with_user();
        db.with_tx(|tx| {
            for index in 0..5 {
                success(
                    tx,
                    &actor(),
                    AuditAction::ReportEdited,
                    "report",
                    &format!("r-{index}"),
                    json!({}),
                )?;
            }
            Ok(())
        })
        .unwrap();

        let entries = db
            .with_conn(|conn| {
                query(
                    conn,
                    &AuditQuery {
                        limit: Some(-5),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(
            entries.len(),
            1,
            "a negative limit must clamp to 1, not disable the limit"
        );
        assert_eq!(db.with_conn(count).unwrap(), 5);
    }
}
