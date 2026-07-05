//! Per-route rate limiting: the tight `/v1/auth/*` bucket 429s independently
//! of read routes, and buckets are per client IP. The governor keys on
//! `ConnectInfo<SocketAddr>`, which real serving injects via
//! `into_make_service_with_connect_info`; here we insert it as a request
//! extension by hand.

use core_api::rate_limit::AuthRateLimit;
use core_api::{app_with_limits, AppState};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use std::net::SocketAddr;
use tower::ServiceExt;

/// A tiny bucket so tests exhaust it quickly: 3 quick attempts, then one per
/// minute — i.e. the 4th request within the test window must be 429.
fn tight() -> Option<AuthRateLimit> {
    Some(AuthRateLimit {
        burst: 3,
        refill_secs: 60,
        trust_proxy: false,
    })
}

fn check_email_from(ip: [u8; 4]) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/auth/check-email")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "email": "who@example.com" }).to_string(),
        ))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((ip, 44_000))));
    req
}

#[sqlx::test(migrations = "../../migrations")]
async fn auth_bucket_exhausts_while_reads_stay_open(pool: PgPool) {
    let router = app_with_limits(AppState::new(pool), tight());

    // The burst passes…
    for i in 1..=3 {
        let status = router
            .clone()
            .oneshot(check_email_from([10, 0, 0, 1]))
            .await
            .unwrap()
            .status();
        assert_ne!(
            status,
            StatusCode::TOO_MANY_REQUESTS,
            "request {i} is inside the burst"
        );
    }

    // …the 4th auth request from the same IP is 429 in the shared JSON shape.
    let resp = router
        .clone()
        .oneshot(check_email_from([10, 0, 0, 1]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        resp.headers().get("retry-after").is_some(),
        "governor 429 must carry Retry-After"
    );
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "rate_limited");

    // Read routes are OUTSIDE the auth bucket: same IP, still served.
    let mut read = Request::builder()
        .method("GET")
        .uri("/v1/posts")
        .body(Body::empty())
        .unwrap();
    read.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 44_000))));
    let status = router.oneshot(read).await.unwrap().status();
    assert_eq!(
        status,
        StatusCode::OK,
        "reads must not share the auth bucket"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn buckets_are_per_ip(pool: PgPool) {
    let router = app_with_limits(AppState::new(pool), tight());

    // Exhaust the bucket for one IP…
    for _ in 0..4 {
        router
            .clone()
            .oneshot(check_email_from([10, 0, 0, 2]))
            .await
            .unwrap();
    }
    assert_eq!(
        router
            .clone()
            .oneshot(check_email_from([10, 0, 0, 2]))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    // …a different IP still has its own budget.
    assert_ne!(
        router
            .oneshot(check_email_from([10, 0, 0, 3]))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS,
        "a fresh IP must not inherit another IP's exhausted bucket"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn no_limit_config_means_no_throttling(pool: PgPool) {
    // The default test router (app()) passes None — hammering auth stays open.
    let router = app_with_limits(AppState::new(pool), None);
    for _ in 0..20 {
        let status = router
            .clone()
            .oneshot(check_email_from([10, 0, 0, 4]))
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::OK);
    }
}
