//! Tests for the posts domain against a real Postgres (`#[sqlx::test]` = isolated
//! migrated DB per test). Covers the repository, the full HTTP stack, and the two
//! validation paths (empty body, unknown author).

use core_api::error::ApiError;
use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::posts::PostService;
use core_api::queue::IngestionQueue;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

async fn seed_author(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: true,
        })
        .await
        .expect("seed author")
        .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn repository_create_get_list(pool: PgPool) {
    let author = seed_author(&pool).await;
    let repo = PostRepository::new(pool);

    let created = repo
        .create(&NewPost {
            author_id: author,
            category: Some("tech".into()),
            body: "hello world".into(),
            media_id: None,
        })
        .await
        .expect("create");

    // Every post is 'public' until the private write path lands (A4g); the area
    // column round-trips through both projections (create RETURNING and get).
    assert_eq!(created.area, "public");
    let fetched = repo.get(created.id).await.expect("get").expect("exists");
    assert_eq!(fetched.body.as_deref(), Some("hello world"));
    assert_eq!(fetched.author_id, author);
    assert_eq!(fetched.area, "public");

    let recent = repo.list(None, 10, 0).await.expect("list");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].area, "public");

    // Author filter: matching author returns it, a different author returns nothing.
    assert_eq!(
        repo.list(Some(author), 10, 0)
            .await
            .expect("by author")
            .len(),
        1
    );
    assert_eq!(
        repo.list(Some(author + 999), 10, 0)
            .await
            .expect("other")
            .len(),
        0
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_create_uses_authenticated_author(pool: PgPool) {
    let router = app(AppState::new(pool));
    let (token, author) = common::register(&router, &[]).await;

    // No author_id in the body — the server takes it from the session.
    let body = serde_json::json!({ "category": "Tech", "body": "  hi  " });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/posts")
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
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(created["author_id"].as_i64().unwrap(), author);
    assert_eq!(created["body"], "hi", "service should trim the body");
    assert_eq!(created["category"], "tech", "category normalised");

    let id = created["id"].as_i64().unwrap();
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/posts/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_post_requires_authentication(pool: PgPool) {
    let router = app(AppState::new(pool));
    // No bearer token → 401, before any body validation.
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/posts")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "body": "hi" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_body_is_rejected(pool: PgPool) {
    let router = app(AppState::new(pool));
    let (token, _) = common::register(&router, &[]).await;

    let body = serde_json::json!({ "body": "   " });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/posts")
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
async fn unknown_author_is_rejected_at_service_level(pool: PgPool) {
    // The HTTP path can't reach this (author comes from a valid session), but the
    // service still maps an FK violation to a 400 for any internal caller.
    let svc = PostService::new(pool);
    let err = svc
        .create(NewPost {
            author_id: 999_999,
            category: None,
            body: "orphan".into(),
            media_id: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Validation("unknown_author")));
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_offers_post_to_ingestion_queue(pool: PgPool) {
    let author = seed_author(&pool).await;
    // Isolated queue key so this test can't see other tests' jobs.
    let key = format!("gamma:ingestion:test:{}", common::unique());
    let queue = IngestionQueue::with_key("redis://localhost:6379", key).unwrap();
    let svc = PostService::with_ingestion(pool, queue.clone());

    assert!(
        queue.dequeue().await.unwrap().is_none(),
        "the queue starts empty"
    );
    let post = svc
        .create(NewPost {
            author_id: author,
            category: None,
            body: "hello pipeline".into(),
            media_id: None,
        })
        .await
        .expect("create");

    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(post.id),
        "the new post id is offered to the ingestion pipeline"
    );
}

/// Insert a ready media asset owned by `owner_id` and return its id.
async fn seed_media(pool: &PgPool, owner_id: i64, object_key: &str) -> i64 {
    sqlx::query_scalar!(
        "INSERT INTO media_assets (owner_id, kind, object_key, content_type) \
         VALUES ($1, 'image', $2, 'image/png') RETURNING id",
        owner_id,
        object_key
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Create a post over HTTP as the bearer of `token`, optionally attaching `media_id`.
async fn create_post_http(
    router: &axum::Router,
    token: &str,
    media_id: Option<i64>,
) -> axum::http::Response<Body> {
    let body = serde_json::json!({ "body": "with media", "media_id": media_id });
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/posts")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_validates_media_ownership(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (token, author) = common::register(&router, &[]).await;
    let (_other_token, other) = common::register(&router, &[]).await;

    // Another user's media → 400 unknown_media (and never misreported as a bad author).
    let other_media = seed_media(&pool, other, "other-key").await;
    let resp = create_post_http(&router, &token, Some(other_media)).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"], "unknown_media");

    // A nonexistent media id → 400 unknown_media.
    let resp = create_post_http(&router, &token, Some(999_999)).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"], "unknown_media");

    // The author's OWN media → 201, with the asset attached.
    let own_media = seed_media(&pool, author, "own-key").await;
    let resp = create_post_http(&router, &token, Some(own_media)).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["media_id"].as_i64().unwrap(), own_media);
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_attaches_media(pool: PgPool) {
    let author = seed_author(&pool).await;
    let media_id: i64 = sqlx::query_scalar!(
        "INSERT INTO media_assets (owner_id, kind, object_key, content_type) \
         VALUES ($1, 'image', $2, 'image/png') RETURNING id",
        author,
        "obj-key-1"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let repo = PostRepository::new(pool.clone());
    let post = repo
        .create(&NewPost {
            author_id: author,
            category: None,
            body: "with media".into(),
            media_id: Some(media_id),
        })
        .await
        .expect("create");
    assert_eq!(post.media_id, Some(media_id));

    // It round-trips through get.
    let fetched = repo.get(post.id).await.unwrap().unwrap();
    assert_eq!(fetched.media_id, Some(media_id));
}
