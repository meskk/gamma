//! End-to-end settlement tests against a real Postgres: capture interactions,
//! settle the epoch, and verify gems are minted by weight, conserved, and
//! idempotent.

use core_api::gems::service::SettlementService;
use core_api::interactions::model::{InteractionType, NewInteraction};
use core_api::interactions::service::InteractionService;
use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use domain::Epoch;
use sqlx::PgPool;
use tower::ServiceExt;

async fn verified_user(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: true,
        })
        .await
        .expect("user")
        .id
}

fn current_epoch() -> i64 {
    Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i64
}

#[sqlx::test(migrations = "../../migrations")]
async fn settles_epoch_mints_by_weight_and_is_idempotent(pool: PgPool) {
    let a = verified_user(&pool).await; // hub (receives engagement)
    let b = verified_user(&pool).await;
    let c = verified_user(&pool).await;

    let post_a = PostRepository::new(pool.clone())
        .create(&NewPost {
            author_id: a,
            category: None,
            body: "a's post".into(),
        })
        .await
        .expect("post")
        .id;

    // b likes and c shares a's post → both edges resolve to author `a`.
    let inter = InteractionService::new(pool.clone());
    inter
        .record(NewInteraction {
            actor_id: b,
            r#type: InteractionType::Like,
            target_id: None,
            post_id: Some(post_a),
        })
        .await
        .unwrap();
    inter
        .record(NewInteraction {
            actor_id: c,
            r#type: InteractionType::Share,
            target_id: None,
            post_id: Some(post_a),
        })
        .await
        .unwrap();

    let epoch_k = current_epoch();
    let svc = SettlementService::new(pool.clone());
    let summary = svc.settle(epoch_k).await.unwrap();

    assert!(!summary.already_settled);
    assert!(summary.emission > 0);
    assert_eq!(summary.user_count, 3, "a, b and c all appear in the graph");

    let bal_a = svc.gem_balance(a).await.unwrap().balance;
    let bal_b = svc.gem_balance(b).await.unwrap().balance;
    let bal_c = svc.gem_balance(c).await.unwrap().balance;

    assert!(bal_a > 0);
    assert!(
        bal_a >= bal_b,
        "the engaged hub should earn at least as much as a pure actor"
    );
    // Conservation: everything minted is exactly the epoch's emission.
    assert_eq!(bal_a + bal_b + bal_c, summary.emission);

    // Idempotent: settling again changes nothing.
    let again = svc.settle(epoch_k).await.unwrap();
    assert!(again.already_settled);
    assert_eq!(svc.gem_balance(a).await.unwrap().balance, bal_a);
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_epoch_mints_nothing(pool: PgPool) {
    let svc = SettlementService::new(pool);
    let summary = svc.settle(123_456).await.unwrap();
    assert_eq!(summary.user_count, 0);
    assert_eq!(summary.emission, 0);
    assert!(!summary.already_settled);
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_settle_then_read_balance(pool: PgPool) {
    let a = verified_user(&pool).await;
    let b = verified_user(&pool).await;
    InteractionService::new(pool.clone())
        .record(NewInteraction {
            actor_id: b,
            r#type: InteractionType::Comment,
            target_id: Some(a),
            post_id: None,
        })
        .await
        .unwrap();
    let epoch_k = current_epoch();
    let router = app(AppState::new(pool));

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/epochs/{epoch_k}/settle"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let summary: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(summary["emission"].as_i64().unwrap() > 0);

    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/users/{a}/gems"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let balance: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(balance["balance"].as_i64().unwrap() > 0);
}
