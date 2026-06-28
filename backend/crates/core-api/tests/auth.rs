//! Auth flow tests against a real Postgres: register, login, and the bearer-token
//! protected `/auth/me` probe.

use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn post_json(router: &axum::Router, uri: &str, body: Value) -> axum::http::Response<Body> {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn json_body(resp: axum::http::Response<Body>) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn register_login_and_authenticated_me(pool: PgPool) {
    let router = app(AppState::new(pool));

    // Register.
    let resp = post_json(
        &router,
        "/v1/auth/register",
        serde_json::json!({ "email": "Alice@example.com", "password": "supersecret" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let reg = json_body(resp).await;
    let token = reg["token"].as_str().unwrap().to_string();
    let user_id = reg["user_id"].as_i64().unwrap();

    // The token authenticates /auth/me.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["user_id"].as_i64().unwrap(), user_id);

    // Login (email is normalised to lowercase) returns a working token too.
    let resp = post_json(
        &router,
        "/v1/auth/login",
        serde_json::json!({ "email": "alice@example.com", "password": "supersecret" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["user_id"].as_i64().unwrap(), user_id);

    // Wrong password → 401.
    let resp = post_json(
        &router,
        "/v1/auth/login",
        serde_json::json!({ "email": "alice@example.com", "password": "wrong" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn check_email_reports_existence(pool: PgPool) {
    let router = app(AppState::new(pool));

    // Unknown email → exists: false (normalised the same way login is).
    let resp = post_json(
        &router,
        "/v1/auth/check-email",
        serde_json::json!({ "email": "  New@Example.com " }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["exists"].as_bool(), Some(false));

    // Register it, then the same (differently-cased) email → exists: true.
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "new@example.com", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let resp = post_json(
        &router,
        "/v1/auth/check-email",
        serde_json::json!({ "email": "NEW@example.com" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["exists"].as_bool(), Some(true));
}

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_email_conflicts(pool: PgPool) {
    let router = app(AppState::new(pool));
    let body = serde_json::json!({ "email": "bob@example.com", "password": "supersecret" });

    assert_eq!(
        post_json(&router, "/v1/auth/register", body.clone())
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        post_json(&router, "/v1/auth/register", body).await.status(),
        StatusCode::CONFLICT
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn weak_password_and_bad_email_rejected(pool: PgPool) {
    let router = app(AppState::new(pool));

    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "c@example.com", "password": "short" }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "notanemail", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn me_without_or_with_bad_token_is_401(pool: PgPool) {
    let router = app(AppState::new(pool));

    // No token.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Garbage token.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .header("authorization", "Bearer deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
