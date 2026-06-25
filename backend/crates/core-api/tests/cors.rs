//! CORS is enabled for the browser frontend: a cross-origin preflight is answered
//! with the allow-origin header so the Next.js app (a separate origin) can call the API.

use axum::body::Body;
use axum::http::Request;
use core_api::{app, AppState};
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../../migrations")]
async fn preflight_is_answered_with_cors_headers(pool: PgPool) {
    let router = app(AppState::new(pool));

    // A browser preflight for a POST from the dev frontend origin.
    let resp = router
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/v1/auth/login")
                .header("origin", "http://localhost:3000")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "authorization,content-type",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // tower-http's CorsLayer short-circuits preflight with the allow-origin header
    // (the default GAMMA_CORS_ORIGIN is http://localhost:3000).
    assert!(resp.status().is_success());
    assert_eq!(
        resp.headers().get("access-control-allow-origin").unwrap(),
        "http://localhost:3000"
    );
}
