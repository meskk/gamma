//! Tests for the users domain against a real Postgres. `#[sqlx::test]` gives each
//! test an isolated, migrated database. One test exercises the repository layer
//! directly; the other drives the operator-only verification flow over HTTP.

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

/// Register a credentialed account via HTTP; returns (user_id, token).
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

async fn get_user(router: &axum::Router, id: i64) -> axum::http::Response<Body> {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn set_verification(
    router: &axum::Router,
    id: i64,
    verified: bool,
    token: Option<&str>,
) -> axum::http::Response<Body> {
    let mut builder = Request::builder()
        .method("PUT")
        .uri(format!("/v1/users/{id}/verification"))
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    serde_json::json!({ "verified": verified }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn bot_gate_is_operator_only(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    // Operator + a normal account; promote the operator directly (no admin UI).
    let (op_id, op_token) = register(&router, "op@example.com").await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (target_id, _t) = register(&router, "target@example.com").await;
    let (_u, user_token) = register(&router, "user@example.com").await;

    // A freshly registered user is UNVERIFIED — registration never self-asserts the gate.
    let resp = get_user(&router, target_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let u: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(u["bot_gate_v"], serde_json::json!(false));

    // Verification is operator-only: 401 unauth, 403 non-operator.
    assert_eq!(
        set_verification(&router, target_id, true, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        set_verification(&router, target_id, true, Some(&user_token))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Operator can flip the gate; the change is reflected on read.
    let resp = set_verification(&router, target_id, true, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let u: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(u["bot_gate_v"], serde_json::json!(true));

    // Verifying a missing user → 404.
    assert_eq!(
        set_verification(&router, 999_999, true, Some(&op_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
