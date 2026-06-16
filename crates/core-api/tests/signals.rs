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
        .uri(format!("/posts/{post_id}/signals"))
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
