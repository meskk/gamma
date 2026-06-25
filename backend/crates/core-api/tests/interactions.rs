//! Tests for interaction-graph capture against a real Postgres. Verifies events
//! are stamped with the current epoch and the type's weight, that they can be
//! read back per epoch, and that the HTTP endpoint returns a typed view.

use core_api::interactions::model::{InteractionType, NewInteraction};
use core_api::interactions::repository::InteractionRepository;
use core_api::interactions::service::InteractionService;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use domain::Epoch;
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

#[sqlx::test(migrations = "../../migrations")]
async fn record_stamps_epoch_and_weight(pool: PgPool) {
    let service = InteractionService::new(pool.clone());

    let event = service
        .record(NewInteraction {
            actor_id: 1,
            r#type: InteractionType::Comment,
            target_id: Some(2),
            post_id: Some(10),
        })
        .await
        .expect("record");

    let expected_epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    assert_eq!(event.epoch_k, expected_epoch);
    assert_eq!(event.type_code, InteractionType::Comment.code());
    assert_eq!(event.weight, InteractionType::Comment.weight());

    // Readable back per epoch (the graph-build input).
    let in_epoch = InteractionRepository::new(pool)
        .list_by_epoch(expected_epoch)
        .await
        .expect("list");
    assert_eq!(in_epoch.len(), 1);
    assert_eq!(in_epoch[0].id, event.id);
}

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_interaction_in_epoch_is_deduped(pool: PgPool) {
    let svc = InteractionService::new(pool.clone());
    let like = NewInteraction {
        actor_id: 1,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: Some(10),
    };

    // The same like, twice, is idempotent — same row, no extra weight.
    let first = svc.record(like.clone()).await.expect("first");
    let again = svc.record(like).await.expect("repeat");
    assert_eq!(
        first.id, again.id,
        "a repeated identical interaction is a no-op"
    );

    // A DIFFERENT type on the same post is a distinct edge.
    let comment = svc
        .record(NewInteraction {
            actor_id: 1,
            r#type: InteractionType::Comment,
            target_id: None,
            post_id: Some(10),
        })
        .await
        .expect("distinct type");
    assert_ne!(first.id, comment.id);

    // Only two rows exist this epoch: the duplicate collapsed.
    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    let all = InteractionRepository::new(pool)
        .list_by_epoch(epoch)
        .await
        .expect("list");
    assert_eq!(all.len(), 2, "the duplicate like did not add a second edge");
}

#[sqlx::test(migrations = "../../migrations")]
async fn edges_exclude_interactions_on_taken_down_posts(pool: PgPool) {
    use core_api::posts::model::NewPost;
    use core_api::posts::repository::PostRepository;

    let router = app(AppState::new(pool.clone()));
    let (_t_author, author) = common::register(&router, &[]).await;
    let (_t_actor, actor) = common::register(&router, &[]).await;

    // A visible post by `author`, liked by `actor`.
    let posts = PostRepository::new(pool.clone());
    let post = posts
        .create(&NewPost {
            author_id: author,
            category: None,
            body: "hello".to_string(),
            media_id: None,
        })
        .await
        .expect("create post");
    InteractionService::new(pool.clone())
        .record(NewInteraction {
            actor_id: actor,
            r#type: InteractionType::Like,
            target_id: None,
            post_id: Some(post.id),
        })
        .await
        .expect("record like");

    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    let repo = InteractionRepository::new(pool.clone());

    // While visible: the like resolves to one actor→author edge.
    let edges = repo.edges_for_epoch(epoch).await.expect("edges");
    assert_eq!(edges.len(), 1, "a like on a visible post confers one edge");
    assert_eq!(edges[0].actor_id, actor);
    assert_eq!(edges[0].target_id, author);

    // Take the post down → its like must no longer confer social weight, even
    // though the interaction row (recorded while visible) still exists.
    posts
        .set_hidden(post.id, Some(Utc::now()))
        .await
        .expect("hide");
    let edges_after = repo.edges_for_epoch(epoch).await.expect("edges after");
    assert!(
        edges_after.is_empty(),
        "a taken-down post confers no weight at settlement"
    );
}

#[test]
fn weights_order_like_below_comment_below_share() {
    // Pure check on the ω_type ordering the graph relies on — no DB needed.
    assert!(InteractionType::Like.weight() < InteractionType::Comment.weight());
    assert!(InteractionType::Comment.weight() < InteractionType::Share.weight());
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_record_returns_typed_view(pool: PgPool) {
    let router = app(AppState::new(pool));
    let (token, actor) = common::register(&router, &[]).await;

    // No actor_id in the body — taken from the session.
    let body = serde_json::json!({ "type": "share", "post_id": 42 });
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/interactions")
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
    let view: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view["type"], "share");
    assert_eq!(view["actor_id"].as_i64().unwrap(), actor);
    assert_eq!(view["weight"], InteractionType::Share.weight());
    assert!(view["epoch_k"].as_i64().unwrap() > 0);
}
