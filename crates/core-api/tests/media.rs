//! End-to-end media tests: request an upload ticket, upload directly to MinIO,
//! finalize, and read back a playback URL — proving bytes never touch the API.
//! Requires the `postgres` and `minio` services from docker-compose.

use core_api::error::ApiError;
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

mod common;

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
    let router = app(AppState::new(pool));
    let (token, _owner) = common::register(&router, &[]).await;

    // 1. Request an upload ticket — owner comes from the session.
    let body = serde_json::json!({ "kind": "video", "content_type": "video/mp4" });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
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

    // 3. Finalize → asset becomes ready with the right size (owner-only).
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/media/{asset_id}/finalize"))
                .header("authorization", format!("Bearer {token}"))
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

    // 4. GET (as the entitled owner) returns a playback URL that serves the bytes.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/media/{asset_id}"))
                .header("authorization", format!("Bearer {token}"))
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
    let router = app(AppState::new(pool));
    let (token, _) = common::register(&router, &[]).await;

    let body = serde_json::json!({ "kind": "audio", "content_type": "audio/mpeg" });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
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
                .header("authorization", format!("Bearer {token}"))
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
    let router = app(AppState::new(pool));
    let (token, _) = common::register(&router, &[]).await;

    // kind video but an image content-type → 400.
    let body = serde_json::json!({ "kind": "video", "content_type": "image/png" });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_media_requires_authentication(pool: PgPool) {
    let router = app(AppState::new(pool));
    let body = serde_json::json!({ "kind": "image", "content_type": "image/png" });
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
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_owner_is_rejected_at_service_level(pool: PgPool) {
    // Unreachable over HTTP now (owner comes from a valid session), but the
    // service still maps the FK violation to a 400 for any internal caller.
    let media = media_service(&pool);
    let err = media
        .create_upload(NewUpload {
            owner_id: 999_999,
            kind: MediaKind::Image,
            content_type: "image/png".into(),
            unlock_price: 0,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Validation("unknown_owner")));
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
    let source = make_test_mp4().await;
    let router = app(AppState::new(pool));
    let (token, _owner) = common::register(&router, &[]).await;

    // Upload ticket — owner from session.
    let body = serde_json::json!({ "kind": "video", "content_type": "video/mp4" });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
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
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Transcode (owner-only).
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/media/{asset_id}/transcode"))
                .header("authorization", format!("Bearer {token}"))
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
            unlock_price: 0,
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
    media.finalize(ticket.asset_id, owner).await.unwrap();

    // The worker picks up the job and transcodes it.
    let processed = process_one(&media, &queue).await;
    assert_eq!(processed, Some(ticket.asset_id));

    let view = media.get(ticket.asset_id, owner).await.unwrap();
    assert!(
        view.hls_ready,
        "asset should be HLS-ready after the worker runs"
    );

    // Queue is now empty.
    assert!(process_one(&media, &queue).await.is_none());
}

fn media_service(pool: &PgPool) -> MediaService {
    let queue = TranscodeQueue::with_key(REDIS_URL, unique_queue_key()).unwrap();
    MediaService::new(pool.clone(), Storage::new(StorageConfig::from_env()), queue)
}

async fn give_gems(pool: &PgPool, user: i64, amount: i64) {
    sqlx::query(
        "INSERT INTO gem_balances (user_id, balance) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET balance = gem_balances.balance + EXCLUDED.balance",
    )
    .bind(user)
    .bind(amount)
    .execute(pool)
    .await
    .unwrap();
}

async fn gem_balance(pool: &PgPool, user: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE((SELECT balance FROM gem_balances WHERE user_id = $1), 0)")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Upload + finalize + transcode a paid video, returning its asset id.
async fn make_paid_video(pool: &PgPool, owner: i64, price: i64) -> i64 {
    let media = media_service(pool);
    let source = make_test_mp4().await;
    let ticket = media
        .create_upload(NewUpload {
            owner_id: owner,
            kind: MediaKind::Video,
            content_type: "video/mp4".into(),
            unlock_price: price,
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
    media.finalize(ticket.asset_id, owner).await.unwrap();
    media.transcode(ticket.asset_id).await.unwrap();
    ticket.asset_id
}

#[sqlx::test(migrations = "../../migrations")]
async fn paid_unlock_splits_payment_and_grants_access(pool: PgPool) {
    ensure_bucket().await;
    let creator = verified_user(&pool).await;
    let viewer = verified_user(&pool).await;
    let price = 1000;
    give_gems(&pool, viewer, price).await;
    let asset_id = make_paid_video(&pool, creator, price).await;

    let media = media_service(&pool);

    // Owner is entitled without paying.
    assert!(media.manifest(asset_id, creator).await.is_ok());

    // Viewer is not entitled yet → 402.
    assert!(matches!(
        media.manifest(asset_id, viewer).await.unwrap_err(),
        ApiError::PaymentRequired
    ));

    // Unlock: 2% fee + 2% burn (defaults) → creator 960, fee 20, burn 20.
    let summary = media.unlock(asset_id, viewer).await.unwrap();
    assert!(!summary.already_unlocked);
    assert_eq!(summary.company_fee, 20);
    assert_eq!(summary.burned, 20);
    assert_eq!(summary.creator_received, 960);
    assert_eq!(
        summary.creator_received + summary.company_fee + summary.burned,
        price,
        "the split must conserve the price"
    );

    // Balances moved correctly; the burn left the supply (credited to no one).
    assert_eq!(gem_balance(&pool, viewer).await, 0);
    assert_eq!(gem_balance(&pool, creator).await, 960);
    assert_eq!(
        gem_balance(&pool, 0).await,
        20,
        "company account holds the fee"
    );

    // Now the viewer can fetch a manifest with presigned segment URLs.
    let manifest = media.manifest(asset_id, viewer).await.unwrap();
    assert!(manifest.contains("#EXTM3U"));
    assert!(
        manifest.contains("http"),
        "segments should be presigned URLs"
    );

    // Re-unlock is a no-charge no-op.
    let again = media.unlock(asset_id, viewer).await.unwrap();
    assert!(again.already_unlocked);
    assert_eq!(gem_balance(&pool, viewer).await, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn unlock_without_enough_gems_fails_and_grants_nothing(pool: PgPool) {
    ensure_bucket().await;
    let creator = verified_user(&pool).await;
    let poor = verified_user(&pool).await; // no gems
    let asset_id = make_paid_video(&pool, creator, 500).await;

    let media = media_service(&pool);
    assert!(matches!(
        media.unlock(asset_id, poor).await.unwrap_err(),
        ApiError::Validation("insufficient_gems")
    ));
    // The failed payment rolled back: still no access.
    assert!(matches!(
        media.manifest(asset_id, poor).await.unwrap_err(),
        ApiError::PaymentRequired
    ));
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_manifest_is_402_until_unlocked(pool: PgPool) {
    ensure_bucket().await;
    let creator = verified_user(&pool).await;
    let asset_id = make_paid_video(&pool, creator, 100).await;

    let router = app(AppState::new(pool));
    let (token, _viewer) = common::register(&router, &[]).await;

    // Authenticated viewer who hasn't paid → 402.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/media/{asset_id}/manifest"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);

    // No token at all → 401.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/media/{asset_id}/manifest"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

async fn get_media_json(router: &axum::Router, id: i64, token: &str) -> Value {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/media/{id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn get_media_requires_auth_and_gates_raw_url(pool: PgPool) {
    ensure_bucket().await;
    let router = app(AppState::new(pool.clone()));
    let (creator_token, creator) = common::register(&router, &[]).await;
    let price = 100;
    let asset_id = make_paid_video(&pool, creator, price).await;

    // No token → 401 (the raw file is never reachable anonymously).
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/media/{asset_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // A stranger (authenticated, not owner, not unlocked) sees metadata but NO raw URL.
    let (stranger_token, stranger) = common::register(&router, &[]).await;
    let view = get_media_json(&router, asset_id, &stranger_token).await;
    assert_eq!(view["unlock_price"].as_i64().unwrap(), price);
    assert!(
        view["playback_url"].is_null(),
        "paid content must not hand the raw file to an unentitled viewer"
    );

    // The owner is entitled → raw URL present.
    let view = get_media_json(&router, asset_id, &creator_token).await;
    assert!(view["playback_url"].is_string(), "owner sees the raw URL");

    // After paying to unlock, the stranger becomes entitled and sees the raw URL.
    give_gems(&pool, stranger, price).await;
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/media/{asset_id}/unlock"))
                .header("authorization", format!("Bearer {stranger_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let view = get_media_json(&router, asset_id, &stranger_token).await;
    assert!(
        view["playback_url"].is_string(),
        "an unlocked viewer is entitled to the raw URL"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn finalize_and_transcode_are_owner_only(pool: PgPool) {
    ensure_bucket().await;
    let router = app(AppState::new(pool.clone()));
    let creator = verified_user(&pool).await;
    let asset_id = make_paid_video(&pool, creator, 100).await;

    // A different authenticated user cannot finalize or transcode someone else's asset.
    let (attacker_token, _) = common::register(&router, &[]).await;
    for action in ["finalize", "transcode"] {
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/media/{asset_id}/{action}"))
                    .header("authorization", format!("Bearer {attacker_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{action} must be owner-only"
        );
    }
}
