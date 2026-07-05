//! Tests for the AI-ingestion signals write-back endpoint: it is operator-only,
//! it round-trips JSONB signals, and a write-back for an unknown post is rejected.

use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::signals::repository::ContentSignalRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

async fn seed_post(pool: &PgPool) -> i64 {
    let author = UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("author")
        .id;
    PostRepository::new(pool.clone())
        .create(&NewPost {
            author_id: author,
            category: None,
            body: "analyse me".into(),
            media_id: None,
        })
        .await
        .expect("post")
        .id
}

async fn put_signals(
    router: &axum::Router,
    post_id: i64,
    token: Option<&str>,
    body: Value,
) -> axum::http::Response<Body> {
    let mut b = Request::builder()
        .method("PUT")
        .uri(format!("/v1/posts/{post_id}/signals"))
        .header("content-type", "application/json");
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

async fn get_signals(
    router: &axum::Router,
    post_id: i64,
    token: Option<&str>,
) -> axum::http::Response<Body> {
    let mut b = Request::builder()
        .method("GET")
        .uri(format!("/v1/posts/{post_id}/signals"));
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn signals_read_back_is_operator_only(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;

    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (user_token, _) = common::register(&router, &[]).await;

    // No signals yet → 404 (even for the operator).
    assert_eq!(
        get_signals(&router, post_id, Some(&op_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // Write some, then read them back.
    put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({ "model_version": "heuristic-v0", "signals": { "word_count": 7 } }),
    )
    .await;

    // Unauthenticated → 401; non-operator → 403.
    assert_eq!(
        get_signals(&router, post_id, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get_signals(&router, post_id, Some(&user_token))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Operator → 200 with the stored row.
    let resp = get_signals(&router, post_id, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["post_id"].as_i64().unwrap(), post_id);
    assert_eq!(v["model_version"], "heuristic-v0");
    assert_eq!(v["signals"]["word_count"].as_i64().unwrap(), 7);
}

#[sqlx::test(migrations = "../../migrations")]
async fn signals_writeback_is_operator_only_and_round_trips(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;

    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (user_token, _) = common::register(&router, &[]).await;

    let payload = json!({ "topic": "rust", "quality": 0.91 });

    // Unauthenticated → 401; authenticated non-operator → 403.
    assert_eq!(
        put_signals(
            &router,
            post_id,
            None,
            json!({ "model_version": "m1", "signals": payload })
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&user_token),
            json!({ "model_version": "m1", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    // Operator → 204, and the JSONB signals round-trip through the repository.
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({ "model_version": "m1", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let stored = ContentSignalRepository::new(pool.clone())
        .get(post_id)
        .await
        .unwrap()
        .expect("signals stored");
    assert_eq!(stored.model_version, "m1");
    assert_eq!(stored.signals, payload);

    // A second write-back supersedes the first (upsert).
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({ "model_version": "m2", "signals": { "topic": "systems" } }),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let stored = ContentSignalRepository::new(pool.clone())
        .get(post_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.model_version, "m2");

    // A write-back for a non-existent post → 400 (unknown_post).
    assert_eq!(
        put_signals(
            &router,
            999_999,
            Some(&op_token),
            json!({ "model_version": "m1", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    // Empty model_version → 400.
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({ "model_version": "  ", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn service_role_writes_signals_but_holds_no_operator_powers(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;

    // A machine identity: registered normally, promoted to 'service' via SQL
    // (the documented provisioning path — there is no escalation endpoint).
    let (svc_token, svc_id) = common::register(&router, &[]).await;
    sqlx::query("UPDATE users SET role = 'service' WHERE id = $1")
        .bind(svc_id)
        .execute(&pool)
        .await
        .unwrap();

    // The service MAY write signals…
    let resp = put_signals(
        &router,
        post_id,
        Some(&svc_token),
        json!({ "model_version": "heuristic-v0", "signals": { "words": 2 } }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // …but NONE of the human-operator powers: settlement, bot gate, takedown.
    let settle = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/epochs/0/settle")
                .header("authorization", format!("Bearer {svc_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(settle.status(), StatusCode::FORBIDDEN);

    let verify = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/users/{svc_id}/verification"))
                .header("authorization", format!("Bearer {svc_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"verified":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(verify.status(), StatusCode::FORBIDDEN);

    let takedown = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/posts/{post_id}/takedown"))
                .header("authorization", format!("Bearer {svc_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(takedown.status(), StatusCode::FORBIDDEN);

    // And a plain user still cannot write signals.
    let (user_token, _uid) = common::register(&router, &[]).await;
    let resp = put_signals(
        &router,
        post_id,
        Some(&user_token),
        json!({ "model_version": "heuristic-v0", "signals": {} }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
