//! Tests for the follows domain against a real Postgres. Covers the repository,
//! idempotency, the HTTP routes, and the two rejection paths (self-follow,
//! unknown user).

use core_api::follows::repository::FollowRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

async fn seed_user(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: true,
        })
        .await
        .expect("seed user")
        .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn follow_is_idempotent_and_listed(pool: PgPool) {
    let a = seed_user(&pool).await;
    let b = seed_user(&pool).await;
    let repo = FollowRepository::new(pool);

    repo.follow(a, b).await.expect("follow");
    repo.follow(a, b).await.expect("follow again is a no-op");

    let following = repo.list_following(a).await.expect("list");
    assert_eq!(following.len(), 1);
    assert_eq!(following[0].followee_id, b);

    repo.unfollow(a, b).await.expect("unfollow");
    assert!(repo.list_following(a).await.expect("list").is_empty());

    // Unfollowing a non-edge is fine.
    repo.unfollow(a, b).await.expect("unfollow no-op");
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_follow_unfollow_roundtrip(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (token, a) = common::register(&router, &[]).await;
    let b = seed_user(&pool).await; // the followee just needs to exist

    // Follow as the authenticated user via /me/following/:followee.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/me/following/{b}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // The follow list of any user is public to read.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{a}/following"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    let resp = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/me/following/{b}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn follow_requires_authentication(pool: PgPool) {
    let router = app(AppState::new(pool));
    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/me/following/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn self_follow_is_rejected(pool: PgPool) {
    let router = app(AppState::new(pool));
    let (token, a) = common::register(&router, &[]).await;

    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/me/following/{a}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn following_unknown_user_is_rejected(pool: PgPool) {
    let router = app(AppState::new(pool));
    let (token, _) = common::register(&router, &[]).await;

    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/me/following/999999")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
