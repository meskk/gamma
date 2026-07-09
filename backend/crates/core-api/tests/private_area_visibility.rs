//! P-4/A4 read-gate matrix: a private post (and, later, its media) must be
//! invisible to non-entitled viewers across EVERY read path. A4b covers the post
//! reads — GET /v1/posts/:id and GET /v1/posts (list + profile). Later sub-steps
//! extend this file (feed A4c, comments A4d, media A4e, write oracles A4f).
//!
//! Setup uses the repositories directly (no private write API exists until A4g)
//! and the reads go through the real HTTP stack so the OptionalAuthUser extractor
//! and the whole handler→service→repo gate are exercised.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use chrono::{Duration, Utc};
use core_api::media::repository::MediaRepository;
use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::private_area::model::{AccessModel, EntitlementSource};
use core_api::private_area::repository::PrivateAreaRepository;
use core_api::{app, AppState};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

/// Create a post by `author` and flip it private (no API write path yet — A4g).
async fn private_post(pool: &PgPool, author: i64, body: &str) -> i64 {
    let id = PostRepository::new(pool.clone())
        .create(&NewPost {
            author_id: author,
            category: None,
            body: body.into(),
            media_id: None,
        })
        .await
        .expect("create")
        .id;
    sqlx::query!("UPDATE posts SET area = 'private' WHERE id = $1", id)
        .execute(pool)
        .await
        .expect("set private");
    id
}

async fn public_post(pool: &PgPool, author: i64, body: &str) -> i64 {
    PostRepository::new(pool.clone())
        .create(&NewPost {
            author_id: author,
            category: None,
            body: body.into(),
            media_id: None,
        })
        .await
        .expect("create")
        .id
}

/// GET /v1/posts/:id — returns the HTTP status (200 visible, 404 hidden/missing).
async fn get_status(router: &Router, id: i64, token: Option<&str>) -> StatusCode {
    let mut b = Request::builder()
        .method("GET")
        .uri(format!("/v1/posts/{id}"));
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

/// GET /v1/posts?author_id=… — returns the set of post ids the viewer sees.
async fn list_ids(router: &Router, author_id: Option<i64>, token: Option<&str>) -> Vec<i64> {
    let uri = match author_id {
        Some(a) => format!("/v1/posts?author_id={a}"),
        None => "/v1/posts".to_string(),
    };
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let resp = router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let posts: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
    posts.iter().map(|p| p["id"].as_i64().unwrap()).collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn private_post_is_invisible_to_anonymous_and_strangers(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (creator_token, creator) = common::register(&router, &[]).await;
    let (stranger_token, _stranger) = common::register(&router, &[]).await;

    let pub_id = public_post(&pool, creator, "public").await;
    let priv_id = private_post(&pool, creator, "secret").await;

    // Single read: the private post is 404 to anonymous and to a stranger
    // (indistinguishable from a missing id — no existence oracle), 200 to the
    // creator; the public post is visible to all.
    assert_eq!(
        get_status(&router, priv_id, None).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_status(&router, priv_id, Some(&stranger_token)).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_status(&router, priv_id, Some(&creator_token)).await,
        StatusCode::OK
    );
    assert_eq!(get_status(&router, pub_id, None).await, StatusCode::OK);

    // List / profile: strangers and anonymous see only the public post; the
    // creator sees both of their own.
    assert_eq!(list_ids(&router, None, None).await, vec![pub_id]);
    assert_eq!(
        list_ids(&router, Some(creator), Some(&stranger_token)).await,
        vec![pub_id]
    );
    let mut own = list_ids(&router, Some(creator), Some(&creator_token)).await;
    own.sort();
    let mut expected = vec![pub_id, priv_id];
    expected.sort();
    assert_eq!(own, expected);
}

#[sqlx::test(migrations = "../../migrations")]
async fn entitlement_grants_access_and_expiry_revokes_it(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (_creator_token, creator) = common::register(&router, &[]).await;
    let (viewer_token, viewer) = common::register(&router, &[]).await;
    // A second viewer whose entitlement is already expired (register up front so
    // its token matches the id we grant to).
    let (expired_token, expired_viewer) = common::register(&router, &[]).await;
    let priv_id = private_post(&pool, creator, "secret").await;
    let areas = PrivateAreaRepository::new(pool.clone());

    // No entitlement → 404.
    assert_eq!(
        get_status(&router, priv_id, Some(&viewer_token)).await,
        StatusCode::NOT_FOUND
    );

    // A live entitlement → visible in both single read and profile list.
    areas
        .grant_entitlement(viewer, creator, EntitlementSource::Purchase, None)
        .await
        .unwrap();
    assert_eq!(
        get_status(&router, priv_id, Some(&viewer_token)).await,
        StatusCode::OK
    );
    assert_eq!(
        list_ids(&router, Some(creator), Some(&viewer_token)).await,
        vec![priv_id]
    );

    // An already-expired entitlement never grants access (the row exists but its
    // expiry is past — revocation by lapse, no cron).
    areas
        .grant_entitlement(
            expired_viewer,
            creator,
            EntitlementSource::Subscription,
            Some(Utc::now() - Duration::hours(1)),
        )
        .await
        .unwrap();
    assert_eq!(
        get_status(&router, priv_id, Some(&expired_token)).await,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn free_area_is_members_only_login_required(pool: PgPool) {
    // Owner-decided (2026-07-09): a 'free' area is visible to logged-in members,
    // NOT to logged-out visitors — fail-closed, login required.
    let router = app(AppState::new(pool.clone()));
    let (_creator_token, creator) = common::register(&router, &[]).await;
    let (member_token, _member) = common::register(&router, &[]).await;
    PrivateAreaRepository::new(pool.clone())
        .upsert_area(creator, AccessModel::Free, 0, "offen für Mitglieder")
        .await
        .unwrap();
    let priv_id = private_post(&pool, creator, "free-but-members").await;

    // Any logged-in member sees it (no entitlement row needed for a free area)...
    assert_eq!(
        get_status(&router, priv_id, Some(&member_token)).await,
        StatusCode::OK
    );
    // ...but an anonymous visitor does not (login required).
    assert_eq!(
        get_status(&router, priv_id, None).await,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn stale_token_is_anonymous_not_401(pool: PgPool) {
    // A garbage/expired bearer must NOT break an otherwise-public read (no 401):
    // it degrades to anonymous. A private post stays 404 for it.
    let router = app(AppState::new(pool.clone()));
    let (_creator_token, creator) = common::register(&router, &[]).await;
    let pub_id = public_post(&pool, creator, "public").await;
    let priv_id = private_post(&pool, creator, "secret").await;

    assert_eq!(
        get_status(&router, pub_id, Some("not-a-real-token")).await,
        StatusCode::OK,
        "a stale bearer must not 401 a public read"
    );
    assert_eq!(
        get_status(&router, priv_id, Some("not-a-real-token")).await,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn expired_session_token_degrades_to_anonymous(pool: PgPool) {
    // The EXPIRED-SESSION branch (a real, once-valid token whose session lapsed),
    // distinct from the garbage-token branch above: it must also degrade to
    // anonymous, not 401 — otherwise a lapsed login would break public reads.
    let router = app(AppState::new(pool.clone()));
    let (_creator_token, creator) = common::register(&router, &[]).await;
    let (stale_token, viewer) = common::register(&router, &[]).await;
    let pub_id = public_post(&pool, creator, "public").await;
    let priv_id = private_post(&pool, creator, "secret").await;

    // Expire the viewer's (only) session in place — a genuine lapsed token.
    sqlx::query!(
        "UPDATE sessions SET expires_at = now() - interval '1 hour' WHERE user_id = $1",
        viewer
    )
    .execute(&pool)
    .await
    .expect("expire session");

    assert_eq!(
        get_status(&router, pub_id, Some(&stale_token)).await,
        StatusCode::OK,
        "a lapsed session must not 401 a public read"
    );
    assert_eq!(
        get_status(&router, priv_id, Some(&stale_token)).await,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn paid_area_is_not_visible_to_logged_in_non_entitled_viewers(pool: PgPool) {
    // Pins that the free arm discriminates on access_model='free'. A creator with
    // a PAID area (one_time) has a private_areas row too — but a logged-in,
    // non-entitled member must NOT see its private posts. If the
    // `access_model = 'free'` condition were ever dropped from the free arm, EVERY
    // paid area would leak to every logged-in user and this test would catch it.
    let router = app(AppState::new(pool.clone()));
    let (creator_token, creator) = common::register(&router, &[]).await;
    let (member_token, _member) = common::register(&router, &[]).await;
    PrivateAreaRepository::new(pool.clone())
        .upsert_area(creator, AccessModel::OneTime, 500, "bezahlt")
        .await
        .unwrap();
    let priv_id = private_post(&pool, creator, "paid-secret").await;

    // A logged-in but non-entitled member cannot see a PAID area's private post...
    assert_eq!(
        get_status(&router, priv_id, Some(&member_token)).await,
        StatusCode::NOT_FOUND
    );
    // ...while the creator still sees their own (the author arm).
    assert_eq!(
        get_status(&router, priv_id, Some(&creator_token)).await,
        StatusCode::OK
    );
}

/// POST /v1/posts/:id/comments — returns the HTTP status.
async fn post_comment(router: &Router, post_id: i64, token: &str, body: &str) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/posts/{post_id}/comments"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({ "body": body }).to_string()))
        .unwrap();
    router.clone().oneshot(req).await.unwrap().status()
}

/// GET /v1/posts/:id/comments — returns (status, number of comments seen).
async fn comment_count(router: &Router, post_id: i64, token: Option<&str>) -> (StatusCode, usize) {
    let mut b = Request::builder()
        .method("GET")
        .uri(format!("/v1/posts/{post_id}/comments"));
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let resp = router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let arr: Vec<Value> = serde_json::from_slice(&bytes).unwrap_or_default();
    (status, arr.len())
}

/// Create a pending media asset owned by `owner` (no storage — the object never
/// has to exist for the area gate, which reads only the DB).
async fn make_asset(pool: &PgPool, owner: i64, price: i64) -> i64 {
    MediaRepository::new(pool.clone())
        .create(
            owner,
            "image",
            &format!("media/test/{}", common::unique()),
            "image/png",
            price,
        )
        .await
        .expect("asset")
        .id
}

async fn attach(pool: &PgPool, asset_id: i64, post_ids: &[i64]) {
    sqlx::query!(
        "UPDATE posts SET media_id = $1 WHERE id = ANY($2)",
        asset_id,
        post_ids
    )
    .execute(pool)
    .await
    .expect("attach media");
}

#[sqlx::test(migrations = "../../migrations")]
async fn media_area_allows_gates_by_the_owning_post(pool: PgPool) {
    // A4e core: the media rail decides access by joining posts.media_id -> the
    // owning post's area. This is the hole ADR 0011 §5 calls out — a price-0 asset
    // of a private post must not be reachable, even though media entitlement is
    // per-asset and knows no posts.
    let router = app(AppState::new(pool.clone()));
    let (_ct, creator) = common::register(&router, &[]).await;
    let (_st, stranger) = common::register(&router, &[]).await;
    let (_vt, viewer) = common::register(&router, &[]).await;
    let media = MediaRepository::new(pool.clone());
    let areas = PrivateAreaRepository::new(pool.clone());

    // Unattached asset → allowed for anyone (out of P-4 scope, unchanged).
    let unattached = make_asset(&pool, creator, 0).await;
    assert!(media.media_area_allows(unattached, stranger).await.unwrap());

    // Attached to a PUBLIC post → allowed for anyone.
    let pub_post = public_post(&pool, creator, "pub").await;
    let pub_asset = make_asset(&pool, creator, 0).await;
    attach(&pool, pub_asset, &[pub_post]).await;
    assert!(media.media_area_allows(pub_asset, stranger).await.unwrap());

    // Attached to a PRIVATE post → denied to a stranger (the closed hole), allowed
    // to the owner (author arm) and to an entitled viewer.
    let priv_post = private_post(&pool, creator, "priv").await;
    let priv_asset = make_asset(&pool, creator, 0).await;
    attach(&pool, priv_asset, &[priv_post]).await;
    assert!(!media.media_area_allows(priv_asset, stranger).await.unwrap());
    assert!(media.media_area_allows(priv_asset, creator).await.unwrap());
    areas
        .grant_entitlement(viewer, creator, EntitlementSource::Purchase, None)
        .await
        .unwrap();
    assert!(media.media_area_allows(priv_asset, viewer).await.unwrap());

    // PUBLIC RESCUE: the SAME asset on both a private and a public post is allowed
    // for a stranger — its bytes are already reachable via the public post, so
    // serving them is not a leak (media_id is non-unique).
    let shared = make_asset(&pool, creator, 0).await;
    let shared_priv = private_post(&pool, creator, "shared-priv").await;
    let shared_pub = public_post(&pool, creator, "shared-pub").await;
    attach(&pool, shared, &[shared_priv, shared_pub]).await;
    assert!(
        media.media_area_allows(shared, stranger).await.unwrap(),
        "a shared asset is served via its public post (public rescue)"
    );

    // FREE area: a free-area private post's asset is allowed for a logged-in viewer.
    let (_ft, free_creator) = common::register(&router, &[]).await;
    areas
        .upsert_area(free_creator, AccessModel::Free, 0, "")
        .await
        .unwrap();
    let free_post = private_post(&pool, free_creator, "free-priv").await;
    let free_asset = make_asset(&pool, free_creator, 0).await;
    attach(&pool, free_asset, &[free_post]).await;
    assert!(media.media_area_allows(free_asset, stranger).await.unwrap());

    // EXPIRED entitlement: media_area_allows has its OWN copy of the expiry
    // predicate — pin that a lapsed row does not admit the asset.
    let (_et, expired_viewer) = common::register(&router, &[]).await;
    areas
        .grant_entitlement(
            expired_viewer,
            creator,
            EntitlementSource::Subscription,
            Some(Utc::now() - Duration::hours(1)),
        )
        .await
        .unwrap();
    assert!(!media
        .media_area_allows(priv_asset, expired_viewer)
        .await
        .unwrap());

    // TAKEN-DOWN post: a moderator takedown of the PUBLIC post that rescued an
    // asset stops it streaming to a stranger (hidden_at on the rescue arm). The
    // asset stays "attached" (so it can't fall through to unattached=allowed).
    let hidden_asset = make_asset(&pool, creator, 0).await;
    let hidden_post = public_post(&pool, creator, "to-be-removed").await;
    attach(&pool, hidden_asset, &[hidden_post]).await;
    assert!(media
        .media_area_allows(hidden_asset, stranger)
        .await
        .unwrap());
    sqlx::query!(
        "UPDATE posts SET hidden_at = now() WHERE id = $1",
        hidden_post
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        !media
            .media_area_allows(hidden_asset, stranger)
            .await
            .unwrap(),
        "a taken-down post must not keep rescuing its media"
    );
}

/// GET /v1/media/:id — status only.
async fn media_get_status(router: &Router, id: i64, token: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/media/{id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn media_unlock_status(router: &Router, id: i64, token: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/media/{id}/unlock"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn media_manifest_status(router: &Router, id: i64, token: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/media/{id}/manifest"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[sqlx::test(migrations = "../../migrations")]
async fn media_endpoints_404_on_a_private_posts_asset_for_strangers(pool: PgPool) {
    // The service wires the gate: get/unlock/manifest all 404 for a non-entitled
    // viewer of a private post's asset — before any not_ready/free_content/
    // transcode-state tell — so the status code is never an existence oracle. The
    // asset stays 'pending', so no storage/MinIO is touched on any path here.
    let router = app(AppState::new(pool.clone()));
    let (creator_token, creator) = common::register(&router, &[]).await;
    let (stranger_token, _stranger) = common::register(&router, &[]).await;

    let priv_post = private_post(&pool, creator, "priv").await;
    let asset = make_asset(&pool, creator, 0).await; // price 0 = the hole
    attach(&pool, asset, &[priv_post]).await;

    for probe in [
        media_get_status(&router, asset, &stranger_token).await,
        media_unlock_status(&router, asset, &stranger_token).await,
        media_manifest_status(&router, asset, &stranger_token).await,
    ] {
        assert_eq!(
            probe,
            StatusCode::NOT_FOUND,
            "a private post's asset must 404 for a stranger on every media endpoint"
        );
    }
    // The owner still reaches their own asset's metadata (200, pending → no URL).
    assert_eq!(
        media_get_status(&router, asset, &creator_token).await,
        StatusCode::OK
    );

    // Mark the asset ready + transcoded: the stranger's 404 must be driven by the
    // AREA gate, not by the pending status — get stays 404, and manifest 404s
    // (area) BEFORE it could 402/not_transcoded, so status is no oracle.
    let media = MediaRepository::new(pool.clone());
    media.mark_ready(asset, 123).await.unwrap();
    media
        .set_hls(asset, &format!("{}/hls/index.m3u8", common::unique()))
        .await
        .unwrap();
    assert_eq!(
        media_get_status(&router, asset, &stranger_token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        media_manifest_status(&router, asset, &stranger_token).await,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn comments_on_a_private_post_are_gated(pool: PgPool) {
    // A4d: a private post's thread must be invisible to non-entitled viewers, and
    // a non-entitled user must not be able to comment on (and thereby confirm) it.
    let router = app(AppState::new(pool.clone()));
    let (creator_token, creator) = common::register(&router, &[]).await;
    let (stranger_token, _stranger) = common::register(&router, &[]).await;
    let (viewer_token, viewer) = common::register(&router, &[]).await;
    let priv_id = private_post(&pool, creator, "secret").await;

    // The creator can comment on their own private post (author arm).
    assert_eq!(
        post_comment(&router, priv_id, &creator_token, "mine").await,
        StatusCode::CREATED
    );
    PrivateAreaRepository::new(pool.clone())
        .grant_entitlement(viewer, creator, EntitlementSource::Purchase, None)
        .await
        .unwrap();

    // Reading the thread: anonymous and stranger get an EMPTY list (200, NOT 404 —
    // it must not diverge from a public post with zero comments); the creator and
    // an entitled viewer see the comment.
    assert_eq!(
        comment_count(&router, priv_id, None).await,
        (StatusCode::OK, 0)
    );
    assert_eq!(
        comment_count(&router, priv_id, Some(&stranger_token)).await,
        (StatusCode::OK, 0)
    );
    assert_eq!(
        comment_count(&router, priv_id, Some(&creator_token)).await,
        (StatusCode::OK, 1)
    );
    assert_eq!(
        comment_count(&router, priv_id, Some(&viewer_token)).await,
        (StatusCode::OK, 1)
    );

    // Writing: a non-entitled stranger commenting on a private post → 404
    // (indistinguishable from commenting on a nonexistent id); entitled → 201.
    assert_eq!(
        post_comment(&router, priv_id, &stranger_token, "sneaky").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        post_comment(&router, priv_id, &viewer_token, "nice").await,
        StatusCode::CREATED
    );
}

async fn make_operator(pool: &PgPool, user_id: i64) {
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", user_id)
        .execute(pool)
        .await
        .expect("make operator");
}

async fn media_moderate(router: &Router, id: i64, action: &str, token: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/media/{id}/{action}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[sqlx::test(migrations = "../../migrations")]
async fn asset_takedown_blocks_everyone_including_the_owner(pool: PgPool) {
    // Asset-level operator takedown (migration 0022): unlike a private-area gate, it
    // has NO owner exception. Uses a PUBLIC post so the area gate is a no-op and the
    // 404 is unambiguously the asset takedown.
    let router = app(AppState::new(pool.clone()));
    let (creator_token, creator) = common::register(&router, &[]).await;
    let (stranger_token, _stranger) = common::register(&router, &[]).await;
    let (op_token, op) = common::register(&router, &[]).await;
    make_operator(&pool, op).await;

    let pub_post = public_post(&pool, creator, "pub").await;
    let asset = make_asset(&pool, creator, 0).await;
    attach(&pool, asset, &[pub_post]).await;

    // Before takedown: owner and stranger both reach the public asset's metadata.
    assert_eq!(
        media_get_status(&router, asset, &creator_token).await,
        StatusCode::OK
    );
    assert_eq!(
        media_get_status(&router, asset, &stranger_token).await,
        StatusCode::OK
    );

    // Non-operator cannot take an asset down; the operator can.
    assert_eq!(
        media_moderate(&router, asset, "takedown", &creator_token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        media_moderate(&router, asset, "takedown", &op_token).await,
        StatusCode::OK
    );

    // Taken down: every content path 404s — for the OWNER too.
    assert_eq!(
        media_get_status(&router, asset, &creator_token).await,
        StatusCode::NOT_FOUND
    );
    for token in [&creator_token, &stranger_token] {
        assert_eq!(
            media_get_status(&router, asset, token).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            media_unlock_status(&router, asset, token).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            media_manifest_status(&router, asset, token).await,
            StatusCode::NOT_FOUND
        );
    }
    // The owner-only WRITE paths must not re-mint a raw URL either: a taken-down
    // asset is frozen, so re-finalize / re-transcode 404 for the owner too.
    assert_eq!(
        media_moderate(&router, asset, "finalize", &creator_token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        media_moderate(&router, asset, "transcode", &creator_token).await,
        StatusCode::NOT_FOUND
    );

    // Restore brings it back.
    assert_eq!(
        media_moderate(&router, asset, "restore", &op_token).await,
        StatusCode::OK
    );
    assert_eq!(
        media_get_status(&router, asset, &creator_token).await,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn owner_cannot_reach_media_of_their_own_taken_down_post(pool: PgPool) {
    // Residual #1 closed: a POST takedown now hides its media from the OWNER too
    // (no owner short-circuit) — consistent with the text rail, where the author
    // cannot read their own taken-down post.
    let router = app(AppState::new(pool.clone()));
    let (creator_token, creator) = common::register(&router, &[]).await;

    let post = public_post(&pool, creator, "pub-with-media").await;
    let asset = make_asset(&pool, creator, 0).await;
    attach(&pool, asset, &[post]).await;

    // Visible post: the owner reaches their own media.
    assert_eq!(
        media_get_status(&router, asset, &creator_token).await,
        StatusCode::OK
    );

    // Take the post down (moderation): the owner can no longer reach its media.
    sqlx::query!("UPDATE posts SET hidden_at = now() WHERE id = $1", post)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        media_get_status(&router, asset, &creator_token).await,
        StatusCode::NOT_FOUND
    );
}
