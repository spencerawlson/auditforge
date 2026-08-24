//! Regression: a tag is recorded against an entity *kind* as well as an id, and the
//! filter must honour both.

use super::*;
use crate::database::Database;
use crate::models::{Actor, NewEngagement, NewReport};
use crate::test_support::seeded;
use crate::{engagements, reports};

fn world() -> (Database, Actor, String, String) {
    let (db, actor) = seeded();
    let engagement = engagements::create(
        &db,
        &actor,
        &NewEngagement {
            name: "Northwind Perimeter".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let report = reports::create(
        &db,
        &actor,
        &NewReport {
            title: "Northwind Report".to_string(),
            template_key: "penetration_test".to_string(),
            engagement_id: Some(engagement.id.clone()),
            classification: None,
        },
    )
    .unwrap();
    (db, actor, engagement.id, report.id)
}

#[test]
fn a_tag_on_one_kind_does_not_surface_another_kind() {
    let (db, _actor, engagement_id, report_id) = world();

    // Tag only the engagement.
    db.with_conn(|conn| tag_entity(conn, "engagement", &engagement_id, "AWS"))
        .unwrap();

    let hits = db
        .with_conn(|conn| {
            search(
                conn,
                &SearchQuery {
                    text: "Northwind".to_string(),
                    tag: Some("AWS".to_string()),
                    ..Default::default()
                },
            )
        })
        .unwrap();

    assert_eq!(hits.len(), 1, "only the tagged engagement should match");
    assert_eq!(hits[0].kind, HitKind::Engagement);
    assert_eq!(hits[0].id, engagement_id);
    assert!(
        !hits.iter().any(|hit| hit.id == report_id),
        "the untagged report must not be returned"
    );
}

#[test]
fn tagging_both_kinds_returns_both() {
    let (db, _actor, engagement_id, report_id) = world();
    db.with_conn(|conn| {
        tag_entity(conn, "engagement", &engagement_id, "AWS")?;
        tag_entity(conn, "report", &report_id, "AWS")
    })
    .unwrap();

    let hits = db
        .with_conn(|conn| {
            search(
                conn,
                &SearchQuery {
                    text: "Northwind".to_string(),
                    tag: Some("AWS".to_string()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn every_hit_kind_maps_to_the_entity_type_it_is_stored_under() {
    for kind in HitKind::ALL {
        assert!(!kind.as_entity_type().is_empty());
        assert_eq!(kind.as_entity_type(), kind.label().to_lowercase());
    }
}
