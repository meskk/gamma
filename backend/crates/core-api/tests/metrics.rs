//! The Prometheus `/metrics` endpoint: once the recorder is installed, a request
//! through the router is counted and rendered in the scrape output.

use core_api::{app, install_prometheus, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../../migrations")]
async fn metrics_endpoint_exposes_request_counters(pool: PgPool) {
    // Install the global recorder for this test process (idempotent).
    install_prometheus();
    let router = app(AppState::new(pool));

    // Drive one request so a counter series exists.
    let health = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    // Scrape /metrics — it must render the request counter in Prometheus text form.
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        text.contains("http_requests_total"),
        "metrics output should include the request counter:\n{text}"
    );
    assert!(
        text.contains("http_request_duration_seconds"),
        "metrics output should include the latency histogram:\n{text}"
    );
}
