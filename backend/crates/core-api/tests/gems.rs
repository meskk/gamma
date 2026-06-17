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

/// Register a credentialed account via the HTTP API; returns (user_id, token).
async fn register(router: &axum::Router, email: &str) -> (i64, String) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "email": email, "password": "supersecret" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (
        v["user_id"].as_i64().unwrap(),
        v["token"].as_str().unwrap().to_string(),
    )
}

/// POST a settle request, optionally bearing a session token.
async fn settle_request(
    router: &axum::Router,
    epoch_k: i64,
    token: Option<&str>,
) -> axum::http::Response<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/v1/epochs/{epoch_k}/settle"));
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
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
async fn settlement_is_resumable_without_double_mint(pool: PgPool) {
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
    let svc = SettlementService::new(pool.clone());

    // First settlement mints and records the marker.
    let first = svc.settle(epoch_k).await.unwrap();
    assert!(!first.already_settled);
    assert!(first.emission > 0);
    let bal_a = svc.gem_balance(a).await.unwrap().balance;
    assert!(bal_a > 0);

    // Simulate a crash where the mints committed but the marker did NOT: drop it.
    sqlx::query!("DELETE FROM epoch_settlements WHERE epoch_k = $1", epoch_k)
        .execute(&pool)
        .await
        .unwrap();

    // Re-running re-mints idempotently (no double-credit) and restores the marker.
    let second = svc.settle(epoch_k).await.unwrap();
    assert!(
        !second.already_settled,
        "marker was gone, so this run re-records it"
    );
    assert_eq!(
        svc.gem_balance(a).await.unwrap().balance,
        bal_a,
        "a resumed settlement must not double-mint"
    );

    // With the marker back, a third run is a no-op.
    let third = svc.settle(epoch_k).await.unwrap();
    assert!(third.already_settled);
    assert_eq!(svc.gem_balance(a).await.unwrap().balance, bal_a);
}

#[sqlx::test(migrations = "../../migrations")]
async fn scheduler_settles_closed_epochs_idempotently(pool: PgPool) {
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
    let svc = SettlementService::new(pool.clone());

    // As of "now = epoch_k + 1", epoch_k is closed and gets settled by the window.
    let summaries = svc.settle_closed_epochs(epoch_k + 1, 2).await.unwrap();
    assert!(
        summaries
            .iter()
            .any(|s| s.epoch_k == epoch_k && !s.already_settled && s.emission > 0),
        "the just-closed epoch was settled"
    );
    let bal = svc.gem_balance(a).await.unwrap().balance;
    assert!(bal > 0);

    // Re-running the same window is a no-op (idempotent) — no double-mint.
    let again = svc.settle_closed_epochs(epoch_k + 1, 2).await.unwrap();
    assert!(again.iter().all(|s| s.already_settled));
    assert_eq!(svc.gem_balance(a).await.unwrap().balance, bal);
}

/// Insert a Like (ω=1) directly with a controlled `created_at`, so a test can set
/// the interaction's age within an epoch (the normal record() stamps "now").
async fn insert_like_at(pool: &PgPool, actor: i64, target: i64, epoch_k: i64, created_secs: i64) {
    sqlx::query(
        "INSERT INTO interaction_events (actor_id, target_id, post_id, type, weight, epoch_k, created_at)
         VALUES ($1, $2, NULL, 0, 1.0, $3, to_timestamp($4))",
    )
    .bind(actor)
    .bind(target)
    .bind(epoch_k as i32)
    .bind(created_secs as f64)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn older_interactions_decay_and_earn_less(pool: PgPool) {
    let a = verified_user(&pool).await; // actor
    let b = verified_user(&pool).await; // engaged early in the day → older → more decay
    let c = verified_user(&pool).await; // engaged late in the day → newer → less decay
    let epoch_k = current_epoch();
    let day_start = epoch_k * 86_400;

    // Two identical likes from A this epoch — only the timing differs.
    insert_like_at(&pool, a, b, epoch_k, day_start + 3_600).await; // ~start of day
    insert_like_at(&pool, a, c, epoch_k, day_start + 82_800).await; // ~end of day

    let svc = SettlementService::new(pool.clone());
    let summary = svc.settle(epoch_k).await.unwrap();
    assert!(summary.emission > 0);

    let bal_b = svc.gem_balance(b).await.unwrap().balance;
    let bal_c = svc.gem_balance(c).await.unwrap().balance;
    assert!(bal_b > 0 && bal_c > 0);
    assert!(
        bal_c > bal_b,
        "the more recent interaction's target earns more (less time-decay): C={bal_c} B={bal_b}"
    );
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
async fn http_settle_is_operator_only_then_reads_balance(pool: PgPool) {
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
    let router = app(AppState::new(pool.clone()));

    // A normal account and an operator account (promoted directly in the DB —
    // there is no admin promotion endpoint in Phase 1a).
    let (_user_id, user_token) = register(&router, "user@example.com").await;
    let (op_id, op_token) = register(&router, "op@example.com").await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();

    // Unauthenticated → 401, authenticated non-operator → 403. Neither settles.
    assert_eq!(
        settle_request(&router, epoch_k, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        settle_request(&router, epoch_k, Some(&user_token))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Operator → 200 and the epoch mints.
    let resp = settle_request(&router, epoch_k, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let summary: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(summary["emission"].as_i64().unwrap() > 0);

    // A balance is self-scoped: the operator may read any user's.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{a}/gems"))
                .header("authorization", format!("Bearer {op_token}"))
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

#[sqlx::test(migrations = "../../migrations")]
async fn gem_balance_is_self_or_operator(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (uid, token) = register(&router, "owner@example.com").await;
    let (_other, other_token) = register(&router, "other@example.com").await;

    let get = |auth: Option<String>| {
        let router = router.clone();
        async move {
            let mut b = Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{uid}/gems"));
            if let Some(t) = auth {
                b = b.header("authorization", t);
            }
            router
                .oneshot(b.body(Body::empty()).unwrap())
                .await
                .unwrap()
        }
    };

    // No token → 401; another user → 403; the owner → 200.
    assert_eq!(get(None).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        get(Some(format!("Bearer {other_token}"))).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(Some(format!("Bearer {token}"))).await.status(),
        StatusCode::OK
    );
}
