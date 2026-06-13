//! Tests for the posts domain against a real Postgres (`#[sqlx::test]` = isolated
//! migrated DB per test). Covers the repository, the full HTTP stack, and the two
//! validation paths (empty body, unknown author).

use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

async fn seed_author(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: true,
        })
        .await
        .expect("seed author")
        .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn repository_create_get_list(pool: PgPool) {
    let author = seed_author(&pool).await;
    let repo = PostRepository::new(pool);

    let created = repo
        .create(&NewPost {
            author_id: author,
            category: Some("tech".into()),
            body: "hello world".into(),
        })
        .await
        .expect("create");

    let fetched = repo.get(created.id).await.expect("get").expect("exists");
    assert_eq!(fetched.body.as_deref(), Some("hello world"));
    assert_eq!(fetched.author_id, author);

    let recent = repo.list_recent(10).await.expect("list");
    assert_eq!(recent.len(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_create_then_get(pool: PgPool) {
    let author = seed_author(&pool).await;
    let router = app(AppState::new(pool));

    let body = serde_json::json!({ "author_id": author, "category": "Tech", "body": "  hi  " });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
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
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(created["body"], "hi", "service should trim the body");
    assert_eq!(
        created["category"], "tech",
        "service should normalise category"
    );

    let id = created["id"].as_i64().unwrap();
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/posts/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_body_is_rejected(pool: PgPool) {
    let author = seed_author(&pool).await;
    let router = app(AppState::new(pool));

    let body = serde_json::json!({ "author_id": author, "body": "   " });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_author_is_rejected(pool: PgPool) {
    let router = app(AppState::new(pool));

    let body = serde_json::json!({ "author_id": 999999, "body": "orphan post" });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/posts")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    // FK violation mapped to a 400, not a 500.
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
