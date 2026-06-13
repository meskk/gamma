//! Shared test helpers. Not a test target itself — included via `mod common;`.
//! Each test file uses a subset, so unused items are allowed.
#![allow(dead_code)]

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

pub fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// Register a fresh user over HTTP, returning (bearer token, user_id).
pub async fn register(router: &Router, categories: &[&str]) -> (String, i64) {
    let email = format!("u{}@example.com", nanos());
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
                .uri("/auth/register")
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
