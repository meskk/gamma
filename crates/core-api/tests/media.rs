//! End-to-end media tests: request an upload ticket, upload directly to MinIO,
//! finalize, and read back a playback URL — proving bytes never touch the API.
//! Requires the `postgres` and `minio` services from docker-compose.

use core_api::media::model::{MediaKind, NewUpload};
use core_api::media::MediaService;
use core_api::queue::TranscodeQueue;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::worker::process_one;
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

/// Generate a tiny real mp4 with ffmpeg so we can exercise the transcoder.
async fn make_test_mp4() -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("gamma-src-{}.mp4", uuid_like()));
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=128x128:rate=10",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .output()
        .await
        .expect("run ffmpeg");
    assert!(status.status.success(), "ffmpeg gen failed");
    let bytes = tokio::fs::read(&path).await.expect("read mp4");
    let _ = tokio::fs::remove_file(&path).await;
    bytes
}

fn uuid_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[sqlx::test(migrations = "../../migrations")]
async fn transcode_produces_hls_in_storage(pool: PgPool) {
    ensure_bucket().await;
    let owner = verified_user(&pool).await;
    let source = make_test_mp4().await;
    let router = app(AppState::new(pool));

    // Upload ticket.
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
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let ticket: Value = serde_json::from_slice(&bytes).unwrap();
    let asset_id = ticket["asset_id"].as_i64().unwrap();
    let object_key = ticket["object_key"].as_str().unwrap().to_string();
    let upload_url = ticket["upload_url"].as_str().unwrap();

    // Upload + finalize.
    reqwest::Client::new()
        .put(upload_url)
        .header("content-type", "video/mp4")
        .body(source)
        .send()
        .await
        .unwrap();
    router
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

    // Transcode.
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/media/{asset_id}/transcode"))
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
    assert_eq!(view["hls_ready"], true);

    // The manifest and at least one segment exist in the object store.
    let store = Storage::new(StorageConfig::from_env());
    assert!(
        store
            .head(&format!("{object_key}/hls/index.m3u8"))
            .await
            .unwrap()
            .is_some(),
        "HLS manifest should exist"
    );
    assert!(
        store
            .head(&format!("{object_key}/hls/seg_000.ts"))
            .await
            .unwrap()
            .is_some(),
        "at least one HLS segment should exist"
    );
}

const REDIS_URL: &str = "redis://localhost:6379";

fn unique_queue_key() -> String {
    format!("gamma:transcode:test:{}", uuid_like())
}

#[tokio::test]
async fn queue_enqueue_dequeue_is_fifo() {
    let queue = TranscodeQueue::with_key(REDIS_URL, unique_queue_key()).unwrap();

    assert!(queue.dequeue().await.unwrap().is_none());
    queue.enqueue(42).await.unwrap();
    queue.enqueue(43).await.unwrap();
    assert_eq!(queue.dequeue().await.unwrap(), Some(42));
    assert_eq!(queue.dequeue().await.unwrap(), Some(43));
    assert!(queue.dequeue().await.unwrap().is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn worker_transcodes_an_enqueued_asset(pool: PgPool) {
    ensure_bucket().await;
    let owner = verified_user(&pool).await;
    let source = make_test_mp4().await;

    // Isolated queue key so this test can't see other tests' jobs.
    let queue = TranscodeQueue::with_key(REDIS_URL, unique_queue_key()).unwrap();
    let media = MediaService::new(
        pool.clone(),
        Storage::new(StorageConfig::from_env()),
        queue.clone(),
    );

    // Upload + finalize → finalize enqueues a transcode job.
    let ticket = media
        .create_upload(NewUpload {
            owner_id: owner,
            kind: MediaKind::Video,
            content_type: "video/mp4".into(),
        })
        .await
        .unwrap();
    reqwest::Client::new()
        .put(&ticket.upload_url)
        .header("content-type", "video/mp4")
        .body(source)
        .send()
        .await
        .unwrap();
    media.finalize(ticket.asset_id).await.unwrap();

    // The worker picks up the job and transcodes it.
    let processed = process_one(&media, &queue).await;
    assert_eq!(processed, Some(ticket.asset_id));

    let view = media.get(ticket.asset_id).await.unwrap();
    assert!(
        view.hls_ready,
        "asset should be HLS-ready after the worker runs"
    );

    // Queue is now empty.
    assert!(process_one(&media, &queue).await.is_none());
}
