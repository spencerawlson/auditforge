//! Global search.
//!
//! One query runs against every record type and returns a single ranked list. Matching is
//! `LIKE` over the fields a person would actually search by - titles, reference ids,
//! client and organization names, authors, filenames - with every user-supplied value
//! bound as a parameter and every wildcard escaped, so a search box cannot become an
//! injection point or an accidental full-table scan.
//!
//! Evidence is searched by metadata only. The vault is never opened to answer a search:
//! grepping evidence content would mean decrypting files as a side effect of typing.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::database::escape_like;
use crate::error::Result;
use crate::models::{Severity, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    Engagement,
    Report,
    Finding,
    Incident,
    Postmortem,
    Evidence,
}

impl HitKind {
    pub const ALL: &'static [HitKind] = &[
        HitKind::Engagement,
        HitKind::Report,
        HitKind::Finding,
        HitKind::Incident,
        HitKind::Postmortem,
        HitKind::Evidence,
    ];

    /// The `entity_tags.entity_type` value this kind is stored under.
    pub fn as_entity_type(&self) -> &'static str {
        match self {
            HitKind::Engagement => "engagement",
            HitKind::Report => "report",
            HitKind::Finding => "finding",
            HitKind::Incident => "incident",
            HitKind::Postmortem => "postmortem",
            HitKind::Evidence => "evidence",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            HitKind::Engagement => "Engagement",
            HitKind::Report => "Report",
            HitKind::Finding => "Finding",
            HitKind::Incident => "Incident",
            HitKind::Postmortem => "Postmortem",
            HitKind::Evidence => "Evidence",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub text: String,
    /// Restrict to these record types. Empty means all of them.
    #[serde(default)]
    pub kinds: Vec<HitKind>,
    #[serde(default)]
    pub severity: Option<Severity>,
    /// Inclusive lower bound on the record's last update, as `YYYY-MM-DD`.
    #[serde(default)]
    pub updated_from: Option<String>,
    #[serde(default)]
    pub updated_to: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub kind: HitKind,
    pub id: String,
    /// Human-facing reference, e.g. `FND-2026-0042`.
    pub reference: String,
    pub title: String,
    /// Secondary line: client, author, asset or filename, depending on the kind.
    pub subtitle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    pub status: String,
    pub updated_at: Timestamp,
    /// Higher is a better match. Exposed so the UI can group by relevance if it wants.
    pub score: i64,
}

/// Search everything the query allows.
pub fn search(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>> {
    let needle = query.text.trim();
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let wanted = |kind: HitKind| query.kinds.is_empty() || query.kinds.contains(&kind);

    // A query with no text and no filters would return the whole database; that is a
    // browse, not a search, and the list views already do it better.
    if needle.is_empty() && query.severity.is_none() && query.tag.is_none() {
        return Ok(Vec::new());
    }

    let pattern = format!("%{}%", escape_like(needle));
    let mut hits = Vec::new();

    if wanted(HitKind::Engagement) && query.severity.is_none() {
        hits.extend(engagements(conn, &pattern, needle)?);
    }
    if wanted(HitKind::Report) && query.severity.is_none() {
        hits.extend(reports(conn, &pattern, needle)?);
    }
    if wanted(HitKind::Finding) {
        hits.extend(findings(conn, &pattern, needle, query.severity)?);
    }
    if wanted(HitKind::Incident) {
        hits.extend(incidents(conn, &pattern, needle, query.severity)?);
    }
    if wanted(HitKind::Postmortem) && query.severity.is_none() {
        hits.extend(postmortems(conn, &pattern, needle)?);
    }
    if wanted(HitKind::Evidence) && query.severity.is_none() {
        hits.extend(evidence(conn, &pattern, needle)?);
    }

    if let Some(tag) = query.tag.as_ref().filter(|tag| !tag.trim().is_empty()) {
        // Match on the pair: a tag is recorded against a kind as well as an id, and
        // comparing ids alone would let a tag on one record surface another.
        let tagged = tagged_entities(conn, tag)?;
        hits.retain(|hit| {
            tagged
                .iter()
                .any(|(entity_type, id)| id == &hit.id && entity_type == hit.kind.as_entity_type())
        });
    }
    if let Some(from) = query
        .updated_from
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        hits.retain(|hit| hit.updated_at.date_string().as_str() >= from.as_str());
    }
    if let Some(to) = query.updated_to.as_ref().filter(|value| !value.is_empty()) {
        hits.retain(|hit| hit.updated_at.date_string().as_str() <= to.as_str());
    }

    // Best match first, then most recently touched.
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    hits.truncate(limit as usize);
    Ok(hits)
}

/// Score a hit: an exact reference match beats a title match, which beats a match
/// anywhere else.
fn score(needle: &str, reference: &str, title: &str) -> i64 {
    if needle.is_empty() {
        return 10;
    }
    let needle = needle.to_lowercase();
    if reference.to_lowercase() == needle {
        return 100;
    }
    if reference.to_lowercase().contains(&needle) {
        return 80;
    }
    let title = title.to_lowercase();
    if title == needle {
        return 70;
    }
    if title.starts_with(&needle) {
        return 60;
    }
    if title.contains(&needle) {
        return 50;
    }
    20
}

fn engagements(conn: &Connection, pattern: &str, needle: &str) -> Result<Vec<SearchHit>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.engagement_ref, e.name, e.client_name, e.status, e.updated_at,
                COALESCE(u.full_name, '')
         FROM engagements e LEFT JOIN users u ON u.id = e.owner_user_id
         WHERE e.name LIKE ?1 ESCAPE '\\' OR e.engagement_ref LIKE ?1 ESCAPE '\\'
            OR e.client_name LIKE ?1 ESCAPE '\\' OR e.scope LIKE ?1 ESCAPE '\\'
            OR u.full_name LIKE ?1 ESCAPE '\\'",
    )?;
    let rows = stmt
        .query_map([pattern], |row| {
            let reference: String = row.get(1)?;
            let title: String = row.get(2)?;
            let client: String = row.get(3)?;
            let owner: String = row.get(6)?;
            Ok(SearchHit {
                kind: HitKind::Engagement,
                id: row.get(0)?,
                score: score(needle, &reference, &title),
                subtitle: if client.is_empty() { owner } else { client },
                reference,
                title,
                severity: None,
                status: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn reports(conn: &Connection, pattern: &str, needle: &str) -> Result<Vec<SearchHit>> {
    // The engagement is joined in so that searching for a client finds their reports,
    // which is how people actually look for them.
    let mut stmt = conn.prepare(
        "SELECT r.id, r.report_ref, r.title, r.status, r.updated_at, COALESCE(u.full_name, ''),
                COALESCE(o.name, ''), COALESCE(e.name, ''), COALESCE(e.client_name, '')
         FROM reports r
         LEFT JOIN users u ON u.id = r.created_by
         LEFT JOIN organizations o ON o.id = u.organization_id
         LEFT JOIN engagements e ON e.id = r.engagement_id
         WHERE r.title LIKE ?1 ESCAPE '\\' OR r.report_ref LIKE ?1 ESCAPE '\\'
            OR u.full_name LIKE ?1 ESCAPE '\\' OR o.name LIKE ?1 ESCAPE '\\'
            OR e.name LIKE ?1 ESCAPE '\\' OR e.client_name LIKE ?1 ESCAPE '\\'",
    )?;
    let rows = stmt
        .query_map([pattern], |row| {
            let reference: String = row.get(1)?;
            let title: String = row.get(2)?;
            let author: String = row.get(5)?;
            let client: String = row.get(8)?;
            let engagement: String = row.get(7)?;
            let subtitle = match (client.is_empty(), engagement.is_empty()) {
                (false, _) => client,
                (true, false) => engagement,
                (true, true) => author,
            };
            Ok(SearchHit {
                kind: HitKind::Report,
                id: row.get(0)?,
                score: score(needle, &reference, &title),
                subtitle,
                reference,
                title,
                severity: None,
                status: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn findings(
    conn: &Connection,
    pattern: &str,
    needle: &str,
    severity: Option<Severity>,
) -> Result<Vec<SearchHit>> {
    let mut sql = String::from(
        "SELECT id, finding_ref, title, severity, verification_status, updated_at,
                affected_asset, author_name
         FROM findings
         WHERE (title LIKE ?1 ESCAPE '\\' OR finding_ref LIKE ?1 ESCAPE '\\'
                OR affected_asset LIKE ?1 ESCAPE '\\' OR category LIKE ?1 ESCAPE '\\'
                OR author_name LIKE ?1 ESCAPE '\\' OR description LIKE ?1 ESCAPE '\\')",
    );
    if severity.is_some() {
        sql.push_str(" AND severity = ?2");
    }

    let mut stmt = conn.prepare(&sql)?;
    let map = |row: &rusqlite::Row<'_>| {
        let reference: String = row.get(1)?;
        let title: String = row.get(2)?;
        let asset: String = row.get(6)?;
        let author: String = row.get(7)?;
        Ok(SearchHit {
            kind: HitKind::Finding,
            id: row.get(0)?,
            score: score(needle, &reference, &title),
            subtitle: if asset.is_empty() { author } else { asset },
            reference,
            title,
            severity: Some(row.get(3)?),
            status: row.get(4)?,
            updated_at: row.get(5)?,
        })
    };
    let rows = match severity {
        Some(severity) => stmt
            .query_map(rusqlite::params![pattern, severity.as_str()], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => stmt
            .query_map([pattern], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

fn incidents(
    conn: &Connection,
    pattern: &str,
    needle: &str,
    severity: Option<Severity>,
) -> Result<Vec<SearchHit>> {
    let mut sql = String::from(
        "SELECT id, incident_ref, title, severity, status, updated_at, affected_systems, reporter
         FROM incidents
         WHERE (title LIKE ?1 ESCAPE '\\' OR incident_ref LIKE ?1 ESCAPE '\\'
                OR affected_systems LIKE ?1 ESCAPE '\\' OR reporter LIKE ?1 ESCAPE '\\'
                OR description LIKE ?1 ESCAPE '\\')",
    );
    if severity.is_some() {
        sql.push_str(" AND severity = ?2");
    }

    let mut stmt = conn.prepare(&sql)?;
    let map = |row: &rusqlite::Row<'_>| {
        let reference: String = row.get(1)?;
        let title: String = row.get(2)?;
        let systems: String = row.get(6)?;
        let reporter: String = row.get(7)?;
        Ok(SearchHit {
            kind: HitKind::Incident,
            id: row.get(0)?,
            score: score(needle, &reference, &title),
            subtitle: if systems.is_empty() {
                reporter
            } else {
                systems
            },
            reference,
            title,
            severity: Some(row.get(3)?),
            status: row.get(4)?,
            updated_at: row.get(5)?,
        })
    };
    let rows = match severity {
        Some(severity) => stmt
            .query_map(rusqlite::params![pattern, severity.as_str()], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => stmt
            .query_map([pattern], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

fn postmortems(conn: &Connection, pattern: &str, needle: &str) -> Result<Vec<SearchHit>> {
    let mut stmt = conn.prepare(
        "SELECT id, postmortem_ref, title, status, updated_at
         FROM postmortems
         WHERE title LIKE ?1 ESCAPE '\\' OR postmortem_ref LIKE ?1 ESCAPE '\\'
            OR answers_json LIKE ?1 ESCAPE '\\'",
    )?;
    let rows = stmt
        .query_map([pattern], |row| {
            let reference: String = row.get(1)?;
            let title: String = row.get(2)?;
            Ok(SearchHit {
                kind: HitKind::Postmortem,
                id: row.get(0)?,
                score: score(needle, &reference, &title),
                subtitle: String::new(),
                reference,
                title,
                severity: None,
                status: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn evidence(conn: &Connection, pattern: &str, needle: &str) -> Result<Vec<SearchHit>> {
    let mut stmt = conn.prepare(
        "SELECT id, evidence_ref, title, kind, updated_at, original_filename, sha256
         FROM evidence
         WHERE title LIKE ?1 ESCAPE '\\' OR evidence_ref LIKE ?1 ESCAPE '\\'
            OR original_filename LIKE ?1 ESCAPE '\\' OR description LIKE ?1 ESCAPE '\\'
            OR sha256 LIKE ?1 ESCAPE '\\'",
    )?;
    let rows = stmt
        .query_map([pattern], |row| {
            let reference: String = row.get(1)?;
            let title: String = row.get(2)?;
            Ok(SearchHit {
                kind: HitKind::Evidence,
                id: row.get(0)?,
                score: score(needle, &reference, &title),
                subtitle: row.get(5)?,
                reference,
                title,
                severity: None,
                status: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn tagged_entities(conn: &Connection, tag: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT et.entity_type, et.entity_id FROM entity_tags et
         JOIN tags t ON t.id = et.tag_id WHERE t.name = ?1 COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([tag], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Attach a tag to a record, creating the tag if it does not exist yet.
pub fn tag_entity(conn: &Connection, entity_type: &str, entity_id: &str, tag: &str) -> Result<()> {
    let name = tag.trim();
    if name.is_empty() {
        return Err(crate::error::Error::validation("a tag needs a name"));
    }
    conn.execute(
        "INSERT OR IGNORE INTO tags (id, name, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![crate::database::new_id(), name, Timestamp::now()],
    )?;
    let tag_id: String = conn.query_row(
        "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE",
        [name],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO entity_tags (tag_id, entity_type, entity_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![tag_id, entity_type, entity_id],
    )?;
    Ok(())
}

pub fn untag_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    tag: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM entity_tags WHERE entity_type = ?1 AND entity_id = ?2
         AND tag_id IN (SELECT id FROM tags WHERE name = ?3 COLLATE NOCASE)",
        rusqlite::params![entity_type, entity_id, tag.trim()],
    )?;
    Ok(())
}

/// Tags on one record.
pub fn tags_for(conn: &Connection, entity_type: &str, entity_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t JOIN entity_tags et ON et.tag_id = t.id
         WHERE et.entity_type = ?1 AND et.entity_id = ?2 ORDER BY t.name",
    )?;
    let rows = stmt
        .query_map([entity_type, entity_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every known tag, for the picker.
pub fn all_tags(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM tags ORDER BY name")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tag_tests;
#[cfg(test)]
mod tests;
