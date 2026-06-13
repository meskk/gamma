//! Tests for interaction-graph capture against a real Postgres. Verifies events
//! are stamped with the current epoch and the type's weight, that they can be
//! read back per epoch, and that the HTTP endpoint returns a typed view.

use core_api::interactions::model::{InteractionType, NewInteraction};
use core_api::interactions::repository::InteractionRepository;
use core_api::interactions::service::InteractionService;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use domain::Epoch;
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../../migrations")]
async fn record_stamps_epoch_and_weight(pool: PgPool) {
    let service = InteractionService::new(pool.clone());

    let event = service
        .record(NewInteraction {
            actor_id: 1,
            r#type: InteractionType::Comment,
            target_id: Some(2),
            post_id: Some(10),
        })
        .await
        .expect("record");

    let expected_epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    assert_eq!(event.epoch_k, expected_epoch);
    assert_eq!(event.type_code, InteractionType::Comment.code());
    assert_eq!(event.weight, InteractionType::Comment.weight());

    // Readable back per epoch (the graph-build input).
    let in_epoch = InteractionRepository::new(pool)
        .list_by_epoch(expected_epoch)
        .await
        .expect("list");
    assert_eq!(in_epoch.len(), 1);
    assert_eq!(in_epoch[0].id, event.id);
}

#[test]
fn weights_order_like_below_comment_below_share() {
    // Pure check on the ω_type ordering the graph relies on — no DB needed.
    assert!(InteractionType::Like.weight() < InteractionType::Comment.weight());
    assert!(InteractionType::Comment.weight() < InteractionType::Share.weight());
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_record_returns_typed_view(pool: PgPool) {
    let router = app(AppState::new(pool));

    let body = serde_json::json!({
        "actor_id": 7,
        "type": "share",
        "post_id": 42
    });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/interactions")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let view: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view["type"], "share");
    assert_eq!(view["actor_id"], 7);
    assert_eq!(view["weight"], InteractionType::Share.weight());
    assert!(view["epoch_k"].as_i64().unwrap() > 0);
}
