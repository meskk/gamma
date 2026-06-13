//! Tests for the users domain against a real Postgres. `#[sqlx::test]` gives each
//! test an isolated, migrated database. One test exercises the repository layer
//! directly; the other drives the full HTTP stack in-process via `oneshot`.

use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt; // for `oneshot`

#[sqlx::test(migrations = "../../migrations")]
async fn repository_create_then_get(pool: PgPool) {
    let repo = UserRepository::new(pool);

    let created = repo
        .create(&NewUser {
            declared_categories: vec!["tech".into()],
            bot_gate_v: false,
        })
        .await
        .expect("create should succeed");

    let fetched = repo
        .get(created.id)
        .await
        .expect("get should run")
        .expect("user should exist");

    assert_eq!(created.id, fetched.id);
    assert_eq!(fetched.declared_categories, vec!["tech".to_string()]);
    assert!(!fetched.bot_gate_v);

    // A missing id yields None, not an error.
    assert!(repo.get(999_999).await.expect("get should run").is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_create_then_get_roundtrip(pool: PgPool) {
    let router = app(AppState::new(pool));

    // POST /users — categories should be normalised by the service layer.
    let body = serde_json::json!({
        "declared_categories": ["Tech", "tech ", ""],
        "bot_gate_v": true
    });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/users")
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
    let id = created["id"].as_i64().expect("id should be an integer");
    assert_eq!(
        created["declared_categories"],
        serde_json::json!(["tech"]),
        "service should normalise + dedupe categories"
    );

    // GET /users/:id — round-trips the created user.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/users/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // GET a missing id → 404.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/users/999999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
