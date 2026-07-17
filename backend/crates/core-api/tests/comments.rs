//! Comments: list (public) + add (authenticated), input validation, and a 404 for a
//! comment on a non-existent post.

use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

async fn seed_post(pool: &PgPool) -> i64 {
    let author = UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("author")
        .id;
    PostRepository::new(pool.clone())
        .create(&NewPost {
            author_id: author,
            category: None,
            body: "post".into(),
            media_id: None,
            area: "public".to_string(),
        })
        .await
        .expect("post")
        .id
}

async fn post_comment(
    router: &Router,
    post_id: i64,
    token: Option<&str>,
    body: Value,
) -> Response<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(format!("/v1/posts/{post_id}/comments"))
        .header("content-type", "application/json");
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

async fn list_comments(router: &Router, post_id: i64) -> Response<Body> {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/posts/{post_id}/comments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn comments_add_list_and_validate(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;
    let (token, _) = common::register(&router, &[]).await;

    // Unauthenticated POST → 401.
    assert_eq!(
        post_comment(&router, post_id, None, json!({ "body": "hi" }))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    // Empty (whitespace) body → 400.
    assert_eq!(
        post_comment(&router, post_id, Some(&token), json!({ "body": "   " }))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    // Comment on a non-existent post → 404.
    assert_eq!(
        post_comment(&router, 999_999, Some(&token), json!({ "body": "hi" }))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    // Valid → 201.
    assert_eq!(
        post_comment(&router, post_id, Some(&token), json!({ "body": "first!" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    // List (public) shows it.
    let resp = list_comments(&router, post_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["body"], "first!");
    assert_eq!(v[0]["post_id"].as_i64().unwrap(), post_id);
}

#[sqlx::test(migrations = "../../migrations")]
async fn comments_respect_post_takedown(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;
    let (token, _) = common::register(&router, &[]).await;

    // A comment lands while the post is visible, and the list shows it.
    assert_eq!(
        post_comment(&router, post_id, Some(&token), json!({ "body": "before" }))
            .await
            .status(),
        StatusCode::CREATED
    );
    let resp = list_comments(&router, post_id).await;
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);

    // Take the post down (operator moderation).
    PostRepository::new(pool.clone())
        .set_hidden(post_id, Some(chrono::Utc::now()))
        .await
        .expect("takedown");

    // Commenting on a taken-down post → 404 (the post is no longer visible).
    assert_eq!(
        post_comment(&router, post_id, Some(&token), json!({ "body": "after" }))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // Listing a taken-down post's comments → empty (the thread is hidden too).
    let resp = list_comments(&router, post_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// ADR 0012: live like aggregates on the Comment read model
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn comment_like_count_and_liked_by_me(pool: PgPool) {
    use core_api::comments::repository::CommentRepository;
    use core_api::interactions::model::{InteractionType, NewInteraction};
    use core_api::interactions::service::InteractionService;

    let post = seed_post(&pool).await;
    let commenter = UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("commenter")
        .id;
    let liker = UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("liker")
        .id;

    let comments = CommentRepository::new(pool.clone());
    let comment = comments
        .create(post, commenter, "like me")
        .await
        .expect("insert")
        .expect("visible");
    assert_eq!(comment.like_count, 0);
    assert!(!comment.liked_by_me);

    let svc = InteractionService::new(pool.clone());
    let like = NewInteraction {
        actor_id: liker,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: None,
        comment_id: Some(comment.id),
    };
    svc.record(like.clone()).await.expect("like");

    let for_liker = comments
        .list_for_post(post, Some(liker), 10, 0)
        .await
        .expect("list");
    assert_eq!(for_liker[0].like_count, 1);
    assert!(for_liker[0].liked_by_me);
    let anon = comments
        .list_for_post(post, None, 10, 0)
        .await
        .expect("list");
    assert_eq!(anon[0].like_count, 1);
    assert!(!anon[0].liked_by_me);

    // Un-like drops it from both projections.
    svc.retract(like).await.expect("unlike");
    let after = comments
        .list_for_post(post, Some(liker), 10, 0)
        .await
        .expect("list");
    assert_eq!(after[0].like_count, 0);
    assert!(!after[0].liked_by_me);
}
