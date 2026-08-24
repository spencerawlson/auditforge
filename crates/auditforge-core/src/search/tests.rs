use super::*;
use crate::database::Database;
use crate::models::{
    Actor, NewEngagement, NewEvidence, NewFinding, NewIncident, NewPostmortem, NewReport,
    SeverityDecision,
};
use crate::test_support::seeded;
use crate::{engagements, evidence as evidence_repo, findings, incidents, postmortems, reports};

struct World {
    db: Database,
    actor: Actor,
    _vault: tempfile::TempDir,
    _source: tempfile::TempDir,
}

fn world() -> World {
    let (db, actor) = seeded();
    let vault = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();

    engagements::create(
        &db,
        &actor,
        &NewEngagement {
            name: "Northwind Perimeter Test".to_string(),
            client_name: "Northwind Traders".to_string(),
            scope: "203.0.113.0/24".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    reports::create(
        &db,
        &actor,
        &NewReport {
            title: "Q3 External Penetration Test".to_string(),
            template_key: "penetration_test".to_string(),
            engagement_id: None,
            classification: None,
        },
    )
    .unwrap();

    let critical = findings::create(
        &db,
        &actor,
        &NewFinding {
            title: "Unauthenticated remote code execution".to_string(),
            affected_asset: "edge.example.internal".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    findings::set_severity(
        &db,
        &actor,
        &critical.id,
        &SeverityDecision {
            severity: Severity::Critical,
            confirmed_by_user: true,
            rationale: String::new(),
        },
    )
    .unwrap();

    let low = findings::create(
        &db,
        &actor,
        &NewFinding {
            title: "Missing security header".to_string(),
            affected_asset: "www.example.test".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    findings::set_severity(
        &db,
        &actor,
        &low.id,
        &SeverityDecision {
            severity: Severity::Low,
            confirmed_by_user: true,
            rationale: String::new(),
        },
    )
    .unwrap();

    incidents::create(
        &db,
        &actor,
        &NewIncident {
            title: "Unauthorized access to the jump host".to_string(),
            affected_systems: "jump01".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    postmortems::create(
        &db,
        &actor,
        &NewPostmortem {
            title: "Jump host compromise".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    let file = source.path().join("handshake.png");
    std::fs::write(&file, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDRdata").unwrap();
    evidence_repo::import(
        &db,
        &actor,
        vault.path(),
        None,
        &NewEvidence {
            source_path: file.to_str().unwrap().to_string(),
            title: "TLS handshake capture".to_string(),
            description: String::new(),
            engagement_id: None,
            captured_at: None,
            encrypt: false,
            acknowledge_executable: false,
            link_target: None,
        },
    )
    .unwrap();

    World {
        db,
        actor,
        _vault: vault,
        _source: source,
    }
}

fn run(world: &World, query: SearchQuery) -> Vec<SearchHit> {
    world.db.with_conn(|conn| search(conn, &query)).unwrap()
}

fn text(needle: &str) -> SearchQuery {
    SearchQuery {
        text: needle.to_string(),
        ..Default::default()
    }
}

#[test]
fn one_query_reaches_every_record_type() {
    let world = world();
    let mut found = std::collections::BTreeSet::new();
    for needle in ["Northwind", "Q3", "remote code", "jump", "handshake"] {
        for hit in run(&world, text(needle)) {
            found.insert(hit.kind);
        }
    }
    for kind in HitKind::ALL {
        assert!(
            found.contains(kind),
            "{} was never reachable by search",
            kind.label()
        );
    }
}

#[test]
fn an_exact_reference_ranks_first() {
    let world = world();
    let findings = world
        .db
        .with_conn(|conn| findings::list(conn, &findings::FindingFilter::default()))
        .unwrap();
    let reference = findings[0].finding_ref.clone();

    let hits = run(&world, text(&reference));
    assert!(!hits.is_empty());
    assert_eq!(hits[0].reference, reference);
    assert_eq!(hits[0].kind, HitKind::Finding);
    assert!(hits[0].score >= 100);
}

#[test]
fn titles_rank_above_incidental_matches() {
    let world = world();
    let hits = run(&world, text("Missing security header"));
    assert_eq!(hits[0].title, "Missing security header");
    assert!(hits[0].score >= 60);
}

#[test]
fn searching_by_client_and_author_works() {
    let world = world();
    assert!(run(&world, text("Northwind Traders"))
        .iter()
        .any(|hit| hit.kind == HitKind::Engagement));
    assert!(run(&world, text("Spencer Rawlson"))
        .iter()
        .any(|hit| hit.kind == HitKind::Report));
}

#[test]
fn searching_by_asset_and_filename_works() {
    let world = world();
    assert!(run(&world, text("edge.example.internal"))
        .iter()
        .any(|hit| hit.kind == HitKind::Finding));
    assert!(run(&world, text("handshake.png"))
        .iter()
        .any(|hit| hit.kind == HitKind::Evidence));
}

#[test]
fn severity_narrows_the_result_to_findings_and_incidents() {
    let world = world();
    let hits = run(
        &world,
        SearchQuery {
            text: "e".to_string(),
            severity: Some(Severity::Critical),
            ..Default::default()
        },
    );
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .all(|hit| hit.severity == Some(Severity::Critical)));
    assert!(hits
        .iter()
        .all(|hit| matches!(hit.kind, HitKind::Finding | HitKind::Incident)));
}

#[test]
fn the_kind_filter_is_respected() {
    let world = world();
    let hits = run(
        &world,
        SearchQuery {
            text: "e".to_string(),
            kinds: vec![HitKind::Evidence],
            ..Default::default()
        },
    );
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| hit.kind == HitKind::Evidence));
}

#[test]
fn a_date_window_filters_by_last_update() {
    let world = world();
    let today = Timestamp::now().date_string();

    let inside = run(
        &world,
        SearchQuery {
            text: "Northwind".to_string(),
            updated_from: Some(today.clone()),
            ..Default::default()
        },
    );
    assert!(!inside.is_empty());

    let outside = run(
        &world,
        SearchQuery {
            text: "Northwind".to_string(),
            updated_to: Some("2020-01-01".to_string()),
            ..Default::default()
        },
    );
    assert!(outside.is_empty());
}

#[test]
fn tags_can_be_attached_and_used_as_a_filter() {
    let world = world();
    let engagement = world
        .db
        .with_conn(|conn| engagements::list(conn, &engagements::EngagementFilter::default()))
        .unwrap()
        .remove(0);

    world
        .db
        .with_conn(|conn| tag_entity(conn, "engagement", &engagement.engagement.id, "AWS"))
        .unwrap();

    let tags = world
        .db
        .with_conn(|conn| tags_for(conn, "engagement", &engagement.engagement.id))
        .unwrap();
    assert_eq!(tags, vec!["AWS".to_string()]);

    let hits = run(
        &world,
        SearchQuery {
            text: "Northwind".to_string(),
            tag: Some("aws".to_string()),
            ..Default::default()
        },
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].kind, HitKind::Engagement);

    let none = run(
        &world,
        SearchQuery {
            text: "Northwind".to_string(),
            tag: Some("Azure".to_string()),
            ..Default::default()
        },
    );
    assert!(none.is_empty());

    world
        .db
        .with_conn(|conn| untag_entity(conn, "engagement", &engagement.engagement.id, "AWS"))
        .unwrap();
    assert!(world
        .db
        .with_conn(|conn| tags_for(conn, "engagement", &engagement.engagement.id))
        .unwrap()
        .is_empty());
}

#[test]
fn the_seeded_tag_vocabulary_is_available() {
    let world = world();
    let tags = world.db.with_conn(all_tags).unwrap();
    for expected in [
        "AWS",
        "Azure",
        "GCP",
        "Linux",
        "Windows",
        "Compliance",
        "Postmortem",
    ] {
        assert!(
            tags.contains(&expected.to_string()),
            "missing seeded tag {expected}"
        );
    }
}

#[test]
fn an_empty_query_returns_nothing_rather_than_everything() {
    let world = world();
    assert!(run(&world, SearchQuery::default()).is_empty());
    assert!(run(&world, text("   ")).is_empty());
}

#[test]
fn sql_metacharacters_are_treated_as_literal_text() {
    let world = world();
    for hostile in [
        "%",
        "_",
        "' OR 1=1 --",
        "'; DROP TABLE findings; --",
        "\\",
        "%' UNION SELECT password_hash FROM users --",
    ] {
        let hits = run(&world, text(hostile));
        assert!(
            hits.is_empty(),
            "{hostile:?} matched {} records",
            hits.len()
        );
    }

    // Everything still works afterwards.
    assert!(!run(&world, text("Northwind")).is_empty());
}

#[test]
fn results_are_capped() {
    let world = world();
    let hits = run(
        &world,
        SearchQuery {
            text: "e".to_string(),
            limit: Some(2),
            ..Default::default()
        },
    );
    assert!(hits.len() <= 2);

    let clamped = run(
        &world,
        SearchQuery {
            text: "e".to_string(),
            limit: Some(-10),
            ..Default::default()
        },
    );
    assert_eq!(
        clamped.len(),
        1,
        "a negative limit must clamp to one, not to unlimited"
    );
}

#[test]
fn tagging_requires_a_name() {
    let world = world();
    assert!(world
        .db
        .with_conn(|conn| tag_entity(conn, "report", "r-1", "   "))
        .is_err());
    let _ = &world.actor;
}
