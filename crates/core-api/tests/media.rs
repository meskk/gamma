//! End-to-end media tests: request an upload ticket, upload directly to MinIO,
//! finalize, and read back a playback URL — proving bytes never touch the API.
//! Requires the `postgres` and `minio` services from docker-compose.

use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use storage::{Storage, StorageConfig};
use tower::ServiceExt;

async fn ensure_bucket() {
    Storage::new(StorageConfig::from_env())
        .ensure_bucket()
        .await
        .expect("bucket");
}

async fn verified_user(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: true,
        })
        .await
        .expect("user")
        .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn upload_finalize_playback_roundtrip(pool: PgPool) {
    ensure_bucket().await;
    let owner = verified_user(&pool).await;
    let router = app(AppState::new(pool));

    // 1. Request an upload ticket.
    let body =
        serde_json::json!({ "owner_id": owner, "kind": "video", "content_type": "video/mp4" });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
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
    let ticket: Value = serde_json::from_slice(&bytes).unwrap();
    let asset_id = ticket["asset_id"].as_i64().unwrap();
    let upload_url = ticket["upload_url"].as_str().unwrap();

    // 2. Upload bytes DIRECTLY to the object store (not through the API).
    let payload = b"fake video bytes";
    let put = reqwest::Client::new()
        .put(upload_url)
        .header("content-type", "video/mp4")
        .body(payload.to_vec())
        .send()
        .await
        .unwrap();
    assert!(put.status().is_success(), "upload failed: {}", put.status());

    // 3. Finalize → asset becomes ready with the right size.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/media/{asset_id}/finalize"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let view: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view["status"], "ready");
    assert_eq!(view["size_bytes"].as_i64().unwrap(), payload.len() as i64);

    // 4. GET returns a playback URL that actually serves the bytes.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/media/{asset_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let view: Value = serde_json::from_slice(&bytes).unwrap();
    let playback_url = view["playback_url"]
        .as_str()
        .expect("playback url when ready");

    let got = reqwest::get(playback_url)
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(&got[..], payload);
}

#[sqlx::test(migrations = "../../migrations")]
async fn finalize_without_upload_is_rejected(pool: PgPool) {
    ensure_bucket().await;
    let owner = verified_user(&pool).await;
    let router = app(AppState::new(pool));

    let body =
        serde_json::json!({ "owner_id": owner, "kind": "audio", "content_type": "audio/mpeg" });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let ticket: Value = serde_json::from_slice(&bytes).unwrap();
    let asset_id = ticket["asset_id"].as_i64().unwrap();

    // Finalize without having uploaded → 400 (object not present).
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/media/{asset_id}/finalize"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn content_type_must_match_kind(pool: PgPool) {
    ensure_bucket().await;
    let owner = verified_user(&pool).await;
    let router = app(AppState::new(pool));

    // kind video but an image content-type → 400.
    let body =
        serde_json::json!({ "owner_id": owner, "kind": "video", "content_type": "image/png" });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_owner_is_rejected(pool: PgPool) {
    ensure_bucket().await;
    let router = app(AppState::new(pool));

    let body =
        serde_json::json!({ "owner_id": 999999, "kind": "image", "content_type": "image/png" });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
