//! Guided postmortems and their corrective actions.
//!
//! A postmortem is a fixed question set (see [`POSTMORTEM_QUESTIONS`]) whose answers each
//! carry a provenance, plus a list of corrective actions with owners, dates and a
//! verification method. Completeness is measured, not assumed: [`completion`] reports
//! exactly which questions are still unanswered.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audit;
use crate::database::{new_id, next_reference, Database};
use crate::error::{Error, Result};
use crate::models::{
    Actor, AnswerValue, AuditAction, CorrectiveAction, CorrectiveActionStatus,
    CorrectiveActionUpdate, NewCorrectiveAction, NewPostmortem, Postmortem, PostmortemStatus,
    Provenance, Timestamp, POSTMORTEM_QUESTIONS,
};

const MAX_TEXT: usize = 50_000;

/// One answer written by the guided postmortem form.
#[derive(Debug, Clone, Deserialize)]
pub struct PostmortemAnswerInput {
    pub question_key: String,
    pub value: String,
    #[serde(default)]
    pub provenance: Provenance,
    #[serde(default)]
    pub ai_request_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PostmortemUpdate {
    pub title: Option<String>,
    pub status: Option<PostmortemStatus>,
    pub incident_id: Option<Option<String>>,
    pub engagement_id: Option<Option<String>>,
    #[serde(default)]
    pub answers: Vec<PostmortemAnswerInput>,
}

/// Progress meter for the guided form.
#[derive(Debug, Clone, Serialize)]
pub struct PostmortemCompletion {
    pub answered: usize,
    pub total: usize,
    pub unanswered: Vec<UnansweredQuestion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnansweredQuestion {
    pub key: String,
    pub prompt: String,
}

/// The question set, for the UI to render.
pub fn questions() -> Vec<UnansweredQuestion> {
    POSTMORTEM_QUESTIONS
        .iter()
        .map(|(key, prompt)| UnansweredQuestion {
            key: (*key).to_string(),
            prompt: (*prompt).to_string(),
        })
        .collect()
}

pub fn create(db: &Database, actor: &Actor, input: &NewPostmortem) -> Result<Postmortem> {
    let title = require(&input.title, "postmortem title", 300)?;
    let id = db.with_tx(|tx| {
        let id = new_id();
        let postmortem_ref = next_reference(tx, "postmortem", "PM")?;
        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO postmortems (id, postmortem_ref, title, incident_id, engagement_id,
                                      status, answers_json, created_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', ?7, ?8, ?8)",
            rusqlite::params![
                id,
                postmortem_ref,
                title,
                input.incident_id,
                input.engagement_id,
                PostmortemStatus::Draft,
                actor.user_id,
                now,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::PostmortemCreated,
            "postmortem",
            &id,
            json!({ "ref": postmortem_ref, "title": title }),
        )?;
        Ok(id)
    })?;
    db.with_conn(|conn| get(conn, &id))
}

pub fn get(conn: &Connection, id: &str) -> Result<Postmortem> {
    let mut postmortem = conn
        .query_row(
            "SELECT id, postmortem_ref, title, incident_id, engagement_id, status, answers_json,
                    created_by, created_at, updated_at
             FROM postmortems WHERE id = ?1",
            [id],
            |row| {
                let answers_raw: String = row.get(6)?;
                Ok(Postmortem {
                    id: row.get(0)?,
                    postmortem_ref: row.get(1)?,
                    title: row.get(2)?,
                    incident_id: row.get(3)?,
                    engagement_id: row.get(4)?,
                    status: row.get(5)?,
                    answers: serde_json::from_str(&answers_raw).unwrap_or_default(),
                    corrective_actions: Vec::new(),
                    created_by: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| Error::not_found("postmortem"))?;
    postmortem.corrective_actions = list_corrective_actions(conn, id)?;
    Ok(postmortem)
}

pub fn list(conn: &Connection, limit: Option<i64>) -> Result<Vec<Postmortem>> {
    let mut stmt = conn.prepare("SELECT id FROM postmortems ORDER BY updated_at DESC LIMIT ?1")?;
    let ids = stmt
        .query_map([limit.unwrap_or(200).clamp(1, 2_000)], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter().map(|id| get(conn, id)).collect()
}

pub fn update(
    db: &Database,
    actor: &Actor,
    id: &str,
    update: &PostmortemUpdate,
) -> Result<Postmortem> {
    db.with_tx(|tx| {
        let mut postmortem = get(tx, id)?;
        if postmortem.status == PostmortemStatus::Complete && update.status.is_none() {
            return Err(Error::Immutable(
                "this postmortem is marked complete; reopen it before editing".to_string(),
            ));
        }

        if let Some(title) = &update.title {
            postmortem.title = require(title, "postmortem title", 300)?;
        }
        if let Some(status) = update.status {
            postmortem.status = status;
        }
        if let Some(incident_id) = &update.incident_id {
            postmortem.incident_id = incident_id.clone();
        }
        if let Some(engagement_id) = &update.engagement_id {
            postmortem.engagement_id = engagement_id.clone();
        }
        apply_answers(&mut postmortem.answers, &update.answers)?;

        tx.execute(
            "UPDATE postmortems SET title = ?1, status = ?2, incident_id = ?3, engagement_id = ?4,
                    answers_json = ?5, updated_at = ?6 WHERE id = ?7",
            rusqlite::params![
                postmortem.title,
                postmortem.status,
                postmortem.incident_id,
                postmortem.engagement_id,
                serde_json::to_string(&postmortem.answers)?,
                Timestamp::now(),
                id,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::PostmortemUpdated,
            "postmortem",
            id,
            json!({
                "ref": postmortem.postmortem_ref,
                "answers_written": update.answers.len(),
                "status": postmortem.status.as_str(),
            }),
        )?;
        get(tx, id)
    })
}

/// Write answers, rejecting keys that are not part of the question set.
///
/// An unknown key is a bug in the caller, not user error; accepting it would create an
/// answer nothing ever renders.
fn apply_answers(
    answers: &mut BTreeMap<String, AnswerValue>,
    inputs: &[PostmortemAnswerInput],
) -> Result<()> {
    for input in inputs {
        if !POSTMORTEM_QUESTIONS
            .iter()
            .any(|(key, _)| *key == input.question_key)
        {
            return Err(Error::validation(format!(
                "unknown question: {}",
                input.question_key
            )));
        }
        answers.insert(
            input.question_key.clone(),
            AnswerValue {
                question_key: input.question_key.clone(),
                value: clamp(&input.value, MAX_TEXT),
                provenance: input.provenance,
                ai_request_id: input.ai_request_id.clone(),
                updated_at: Timestamp::now(),
            },
        );
    }
    Ok(())
}

pub fn completion(postmortem: &Postmortem) -> PostmortemCompletion {
    let unanswered: Vec<UnansweredQuestion> = POSTMORTEM_QUESTIONS
        .iter()
        .filter(|(key, _)| match postmortem.answers.get(*key) {
            Some(answer) => answer.is_blank(),
            None => true,
        })
        .map(|(key, prompt)| UnansweredQuestion {
            key: (*key).to_string(),
            prompt: (*prompt).to_string(),
        })
        .collect();
    PostmortemCompletion {
        answered: POSTMORTEM_QUESTIONS.len() - unanswered.len(),
        total: POSTMORTEM_QUESTIONS.len(),
        unanswered,
    }
}

pub fn add_corrective_action(
    db: &Database,
    actor: &Actor,
    input: &NewCorrectiveAction,
) -> Result<CorrectiveAction> {
    let description = require(
        &input.description,
        "corrective action description",
        MAX_TEXT,
    )?;
    if input.postmortem_id.is_none() && input.incident_id.is_none() && input.finding_id.is_none() {
        return Err(Error::validation(
            "a corrective action must belong to a postmortem, incident or finding",
        ));
    }
    let due_date = match &input.due_date {
        Some(raw) if !raw.trim().is_empty() => Some(crate::models::validate_date(raw, "due date")?),
        _ => None,
    };

    let id = db.with_tx(|tx| {
        let id = new_id();
        let action_ref = next_reference(tx, "corrective_action", "CA")?;
        let now = Timestamp::now();
        tx.execute(
            "INSERT INTO corrective_actions (id, action_ref, postmortem_id, incident_id,
                                             finding_id, description, owner, due_date, status,
                                             verification_method, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            rusqlite::params![
                id,
                action_ref,
                input.postmortem_id,
                input.incident_id,
                input.finding_id,
                description,
                clamp(&input.owner, 300),
                due_date,
                CorrectiveActionStatus::Open,
                clamp(&input.verification_method, MAX_TEXT),
                now,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::PostmortemUpdated,
            "corrective_action",
            &id,
            json!({ "ref": action_ref, "owner": input.owner }),
        )?;
        Ok(id)
    })?;
    db.with_conn(|conn| get_corrective_action(conn, &id))
}

pub fn update_corrective_action(
    db: &Database,
    actor: &Actor,
    id: &str,
    update: &CorrectiveActionUpdate,
) -> Result<CorrectiveAction> {
    db.with_tx(|tx| {
        let mut action = get_corrective_action(tx, id)?;
        if let Some(description) = &update.description {
            action.description = require(description, "description", MAX_TEXT)?;
        }
        if let Some(owner) = &update.owner {
            action.owner = clamp(owner, 300);
        }
        if let Some(due_date) = &update.due_date {
            action.due_date = match due_date {
                Some(raw) if !raw.trim().is_empty() => {
                    Some(crate::models::validate_date(raw, "due date")?)
                }
                _ => None,
            };
        }
        if let Some(status) = update.status {
            action.status = status;
        }
        if let Some(method) = &update.verification_method {
            action.verification_method = clamp(method, MAX_TEXT);
        }
        if action.status == CorrectiveActionStatus::Verified
            && action.verification_method.trim().is_empty()
        {
            return Err(Error::validation(
                "record how the fix was verified before marking the action verified",
            ));
        }

        tx.execute(
            "UPDATE corrective_actions SET description = ?1, owner = ?2, due_date = ?3,
                    status = ?4, verification_method = ?5, updated_at = ?6 WHERE id = ?7",
            rusqlite::params![
                action.description,
                action.owner,
                action.due_date,
                action.status,
                action.verification_method,
                Timestamp::now(),
                id,
            ],
        )?;
        audit::success(
            tx,
            actor,
            AuditAction::PostmortemUpdated,
            "corrective_action",
            id,
            json!({ "status": action.status.as_str() }),
        )?;
        get_corrective_action(tx, id)
    })
}

pub fn get_corrective_action(conn: &Connection, id: &str) -> Result<CorrectiveAction> {
    conn.query_row(
        "SELECT id, action_ref, postmortem_id, incident_id, finding_id, description, owner,
                due_date, status, verification_method, created_at, updated_at
         FROM corrective_actions WHERE id = ?1",
        [id],
        row_to_action,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("corrective action"))
}

pub fn list_corrective_actions(
    conn: &Connection,
    postmortem_id: &str,
) -> Result<Vec<CorrectiveAction>> {
    let mut stmt = conn.prepare(
        "SELECT id, action_ref, postmortem_id, incident_id, finding_id, description, owner,
                due_date, status, verification_method, created_at, updated_at
         FROM corrective_actions WHERE postmortem_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map([postmortem_id], row_to_action)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn row_to_action(row: &rusqlite::Row<'_>) -> rusqlite::Result<CorrectiveAction> {
    Ok(CorrectiveAction {
        id: row.get(0)?,
        action_ref: row.get(1)?,
        postmortem_id: row.get(2)?,
        incident_id: row.get(3)?,
        finding_id: row.get(4)?,
        description: row.get(5)?,
        owner: row.get(6)?,
        due_date: row.get(7)?,
        status: row.get(8)?,
        verification_method: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
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

    fn new_postmortem() -> NewPostmortem {
        NewPostmortem {
            title: "Jump host compromise".to_string(),
            incident_id: None,
            engagement_id: None,
        }
    }

    fn answer(key: &str, value: &str) -> PostmortemAnswerInput {
        PostmortemAnswerInput {
            question_key: key.to_string(),
            value: value.to_string(),
            provenance: Provenance::UserProvided,
            ai_request_id: None,
        }
    }

    #[test]
    fn the_question_set_matches_the_specified_prompts() {
        let keys: Vec<&str> = POSTMORTEM_QUESTIONS.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys.len(), 15);
        for expected in [
            "what_happened",
            "when_did_it_happen",
            "how_detected",
            "systems_affected",
            "impact",
            "timeline",
            "root_cause",
            "actions_taken",
            "what_went_well",
            "what_went_badly",
            "improvements",
            "corrective_actions",
            "action_owners",
            "deadlines",
            "verification",
        ] {
            assert!(keys.contains(&expected), "missing question {expected}");
        }
    }

    #[test]
    fn a_new_postmortem_is_entirely_unanswered() {
        let (db, actor) = seeded();
        let postmortem = create(&db, &actor, &new_postmortem()).unwrap();
        assert!(postmortem.postmortem_ref.starts_with("PM-"));
        let progress = completion(&postmortem);
        assert_eq!(progress.answered, 0);
        assert_eq!(progress.total, 15);
        assert_eq!(progress.unanswered.len(), 15);
    }

    #[test]
    fn answers_are_stored_with_provenance_and_counted() {
        let (db, actor) = seeded();
        let postmortem = create(&db, &actor, &new_postmortem()).unwrap();
        let updated = update(
            &db,
            &actor,
            &postmortem.id,
            &PostmortemUpdate {
                answers: vec![
                    answer(
                        "what_happened",
                        "A session opened from an unexpected address.",
                    ),
                    PostmortemAnswerInput {
                        question_key: "impact".to_string(),
                        value: "Phrased by the assistant from the author's notes.".to_string(),
                        provenance: Provenance::AiWording,
                        ai_request_id: Some("req-1".to_string()),
                    },
                ],
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(completion(&updated).answered, 2);
        assert_eq!(
            updated.answers["what_happened"].provenance,
            Provenance::UserProvided
        );
        assert_eq!(updated.answers["impact"].provenance, Provenance::AiWording);
        assert_eq!(
            updated.answers["impact"].ai_request_id.as_deref(),
            Some("req-1")
        );
    }

    #[test]
    fn unknown_question_keys_are_rejected() {
        let (db, actor) = seeded();
        let postmortem = create(&db, &actor, &new_postmortem()).unwrap();
        let err = update(
            &db,
            &actor,
            &postmortem.id,
            &PostmortemUpdate {
                answers: vec![answer("what_the_model_invented", "...")],
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown question"), "got {err}");
    }

    #[test]
    fn a_completed_postmortem_refuses_edits_until_reopened() {
        let (db, actor) = seeded();
        let postmortem = create(&db, &actor, &new_postmortem()).unwrap();
        update(
            &db,
            &actor,
            &postmortem.id,
            &PostmortemUpdate {
                status: Some(PostmortemStatus::Complete),
                ..Default::default()
            },
        )
        .unwrap();

        let err = update(
            &db,
            &actor,
            &postmortem.id,
            &PostmortemUpdate {
                answers: vec![answer("impact", "late edit")],
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.kind(), "immutable");

        // Reopening in the same call is allowed, since it is an explicit status change.
        update(
            &db,
            &actor,
            &postmortem.id,
            &PostmortemUpdate {
                status: Some(PostmortemStatus::Review),
                answers: vec![answer("impact", "reopened edit")],
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn corrective_actions_need_an_owner_record_and_a_parent() {
        let (db, actor) = seeded();
        let postmortem = create(&db, &actor, &new_postmortem()).unwrap();

        let orphan = add_corrective_action(
            &db,
            &actor,
            &NewCorrectiveAction {
                description: "Rotate credentials".to_string(),
                ..Default::default()
            },
        );
        assert!(orphan.is_err(), "an action with no parent must be refused");

        let action = add_corrective_action(
            &db,
            &actor,
            &NewCorrectiveAction {
                description: "Rotate all jump host credentials".to_string(),
                owner: "Platform team".to_string(),
                due_date: Some("2026-09-30".to_string()),
                postmortem_id: Some(postmortem.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(action.action_ref.starts_with("CA-"));
        assert_eq!(action.status, CorrectiveActionStatus::Open);

        let reloaded = db.with_conn(|conn| get(conn, &postmortem.id)).unwrap();
        assert_eq!(reloaded.corrective_actions.len(), 1);
    }

    #[test]
    fn an_action_cannot_be_marked_verified_without_saying_how() {
        let (db, actor) = seeded();
        let postmortem = create(&db, &actor, &new_postmortem()).unwrap();
        let action = add_corrective_action(
            &db,
            &actor,
            &NewCorrectiveAction {
                description: "Enable MFA".to_string(),
                postmortem_id: Some(postmortem.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        let err = update_corrective_action(
            &db,
            &actor,
            &action.id,
            &CorrectiveActionUpdate {
                status: Some(CorrectiveActionStatus::Verified),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("verified"), "got {err}");

        let verified = update_corrective_action(
            &db,
            &actor,
            &action.id,
            &CorrectiveActionUpdate {
                status: Some(CorrectiveActionStatus::Verified),
                verification_method: Some("Confirmed MFA enforced for all admin roles".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(verified.status, CorrectiveActionStatus::Verified);
    }

    #[test]
    fn invalid_due_dates_are_rejected() {
        let (db, actor) = seeded();
        let postmortem = create(&db, &actor, &new_postmortem()).unwrap();
        let err = add_corrective_action(
            &db,
            &actor,
            &NewCorrectiveAction {
                description: "Something".to_string(),
                due_date: Some("30/09/2026".to_string()),
                postmortem_id: Some(postmortem.id),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("due date"), "got {err}");
    }
}
