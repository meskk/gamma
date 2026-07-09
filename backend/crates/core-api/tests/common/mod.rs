//! Shared test helpers. Not a test target itself — included via `mod common;`.
//! Each test file uses a subset, so unused items are allowed.
#![allow(dead_code)]

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

pub fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// Process-wide monotonic counter behind [`unique`], so two calls on the same
/// clock tick still differ.
static SEQ: AtomicU64 = AtomicU64::new(0);

/// A token unique across concurrent tests, across test binaries, and across
/// repeated `cargo test` runs — for isolating shared external namespaces
/// (Redis queue keys, registration emails). The nanosecond clock alone is NOT
/// enough: under parallel load two tests can read the SAME nanos and collide
/// on a Redis key, which made the transcode-queue tests flaky (one test drained
/// another's job). Composition: the atomic counter guarantees uniqueness WITHIN
/// a process regardless of clock resolution; the process id separates the
/// per-binary test processes cargo runs in parallel; the clock separates
/// distinct runs so a stale key from a crashed run never aliases a fresh one.
pub fn unique() -> String {
    let pid = std::process::id();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{pid}-{}-{seq}", nanos())
}

/// Register a fresh user over HTTP, returning (bearer token, user_id).
pub async fn register(router: &Router, categories: &[&str]) -> (String, i64) {
    let email = format!("u{}@example.com", unique());
    let body = serde_json::json!({
        "email": email,
        "password": "supersecret",
        "declared_categories": categories,
    });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    (
        v["token"].as_str().unwrap().to_string(),
        v["user_id"].as_i64().unwrap(),
    )
}
