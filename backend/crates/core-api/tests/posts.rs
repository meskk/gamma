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
            area: "public".to_string(),
        })
        .await
        .expect("create");

    // Every post is 'public' until the private write path lands (A4g); the area
    // column round-trips through both projections (create RETURNING and get).
    // Public posts are visible to an anonymous viewer (None).
    assert_eq!(created.area, "public");
    let fetched = repo
        .get(created.id, None)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(fetched.body.as_deref(), Some("hello world"));
    assert_eq!(fetched.author_id, author);
    assert_eq!(fetched.area, "public");

    let recent = repo.list(None, None, 10, 0).await.expect("list");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].area, "public");

    // Author filter: matching author returns it, a different author returns nothing.
    assert_eq!(
        repo.list(Some(author), None, 10, 0)
            .await
            .expect("by author")
            .len(),
        1
    );
    assert_eq!(
        repo.list(Some(author + 999), None, 10, 0)
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
            area: "public".to_string(),
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
            area: "public".to_string(),
        })
        .await
        .expect("create");

    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(post.id),
        "the new post id is offered to the ingestion pipeline"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn private_post_is_never_offered_to_ingestion(pool: PgPool) {
    // A4g: a PRIVATE post is never analysed (ADR 0011 §5), so create must not
    // enqueue it — even though a public post is.
    let author = seed_author(&pool).await;
    let key = format!("gamma:ingestion:test:{}", common::unique());
    let queue = IngestionQueue::with_key("redis://localhost:6379", key).unwrap();
    let svc = PostService::with_ingestion(pool, queue.clone());

    svc.create(NewPost {
        author_id: author,
        category: None,
        body: "private, not for the AI".into(),
        media_id: None,
        area: "private".to_string(),
    })
    .await
    .expect("create private");
    assert!(
        queue.dequeue().await.unwrap().is_none(),
        "a private post must not be offered to the ingestion pipeline"
    );

    // A subsequent public post IS offered — proving the skip is area-specific.
    let public = svc
        .create(NewPost {
            author_id: author,
            category: None,
            body: "public".into(),
            media_id: None,
            area: "public".to_string(),
        })
        .await
        .expect("create public");
    assert_eq!(queue.dequeue().await.unwrap(), Some(public.id));
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_rejects_an_unknown_area(pool: PgPool) {
    let author = seed_author(&pool).await;
    let err = PostService::new(pool)
        .create(NewPost {
            author_id: author,
            category: None,
            body: "hello".into(),
            media_id: None,
            area: "secret".to_string(),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Validation("invalid_area")));
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
            area: "public".to_string(),
        })
        .await
        .expect("create");
    assert_eq!(post.media_id, Some(media_id));

    // It round-trips through get.
    let fetched = repo.get(post.id, None).await.unwrap().unwrap();
    assert_eq!(fetched.media_id, Some(media_id));
}

// ---------------------------------------------------------------------------
// ADR 0012: live like aggregates on the Post read model
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn like_count_and_liked_by_me_reflect_the_journal(pool: PgPool) {
    use core_api::interactions::model::{InteractionType, NewInteraction};
    use core_api::interactions::service::InteractionService;

    let author = seed_author(&pool).await;
    let liker = seed_author(&pool).await;
    let other = seed_author(&pool).await;
    let repo = PostRepository::new(pool.clone());
    let post = repo
        .create(&NewPost {
            author_id: author,
            category: None,
            body: "count me".into(),
            media_id: None,
            area: "public".to_string(),
        })
        .await
        .expect("create");
    assert_eq!(post.like_count, 0, "a fresh post starts unliked");
    assert!(!post.liked_by_me);

    let svc = InteractionService::new(pool.clone());
    let like = |actor: i64| NewInteraction {
        actor_id: actor,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: Some(post.id),
        comment_id: None,
    };
    svc.record(like(liker)).await.expect("like 1");
    svc.record(like(other)).await.expect("like 2");

    // The count is viewer-independent; the flag is the viewer's own state.
    let for_liker = repo.get(post.id, Some(liker)).await.expect("get").unwrap();
    assert_eq!(for_liker.like_count, 2);
    assert!(for_liker.liked_by_me);
    let for_author = repo.get(post.id, Some(author)).await.expect("get").unwrap();
    assert_eq!(for_author.like_count, 2);
    assert!(!for_author.liked_by_me);
    let anon = repo.get(post.id, None).await.expect("get").unwrap();
    assert_eq!(anon.like_count, 2);
    assert!(!anon.liked_by_me, "anonymous is never 'me'");

    // Un-like: the voided row leaves both projections immediately.
    svc.retract(like(liker)).await.expect("unlike");
    let after = repo.get(post.id, Some(liker)).await.expect("get").unwrap();
    assert_eq!(after.like_count, 1);
    assert!(!after.liked_by_me);

    // The list projection carries the same fields.
    let listed = repo
        .list(Some(author), Some(other), 10, 0)
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].like_count, 1);
    assert!(listed[0].liked_by_me, "`other` still holds an active like");

    // Cross-epoch rows for the SAME actor must not inflate the counter: the
    // journal legitimately holds one weighted row per (actor, post, epoch) —
    // daily re-engagement edges — but the DISPLAY semantics are per-user
    // boolean, i.e. distinct likers (ADR 0012 §2). Simulate yesterday's like
    // by inserting the prior-epoch row directly.
    let epoch = domain::Epoch::from_unix_seconds(chrono::Utc::now().timestamp()).0 as i32;
    sqlx::query!(
        "INSERT INTO interaction_events (actor_id, target_id, post_id, type, weight, epoch_k)
         VALUES ($1, NULL, $2, $3, 1.0, $4)",
        other,
        post.id,
        InteractionType::Like.code(),
        epoch - 1
    )
    .execute(&pool)
    .await
    .expect("prior-epoch like");
    let still = repo.get(post.id, Some(other)).await.expect("get").unwrap();
    assert_eq!(
        still.like_count, 1,
        "the same liker across epochs counts ONCE"
    );

    // One unlike voids BOTH epochs' rows: the counter drops exactly to 0.
    svc.retract(like(other)).await.expect("unlike other");
    let gone = repo.get(post.id, Some(other)).await.expect("get").unwrap();
    assert_eq!(gone.like_count, 0);
    assert!(!gone.liked_by_me);
}
