//! Tests for interaction-graph capture against a real Postgres. Verifies events
//! are stamped with the current epoch and the type's weight, that they can be
//! read back per epoch, and that the HTTP endpoint returns a typed view.

use core_api::interactions::model::{InteractionType, NewInteraction};
use core_api::interactions::repository::InteractionRepository;
use core_api::interactions::service::InteractionService;
use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use domain::Epoch;
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

/// The ω_type weight for a type under the default econ-params (weights now live in
/// econ-params, not hardcoded — see `InteractionType::weight`).
fn omega(t: InteractionType) -> f64 {
    t.weight(&econ_params::EconParams::default().interaction_weights)
}

// interaction_events now has FKs (migration 0015), so events must reference real
// rows. These seed the minimal actor/target/post a capture test needs.
async fn seed_user(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: true,
        })
        .await
        .expect("user")
        .id
}

async fn seed_post(pool: &PgPool, author: i64) -> i64 {
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

#[sqlx::test(migrations = "../../migrations")]
async fn record_stamps_epoch_and_weight(pool: PgPool) {
    let actor = seed_user(&pool).await;
    let target = seed_user(&pool).await;
    let post = seed_post(&pool, target).await;
    let service = InteractionService::new(pool.clone());

    let event = service
        .record(NewInteraction {
            actor_id: actor,
            r#type: InteractionType::Comment,
            target_id: Some(target),
            post_id: Some(post),
            comment_id: None,
        })
        .await
        .expect("record");

    let expected_epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    assert_eq!(event.epoch_k, expected_epoch);
    assert_eq!(event.type_code, InteractionType::Comment.code());
    assert_eq!(event.weight, omega(InteractionType::Comment));

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
    let actor = seed_user(&pool).await;
    let post = seed_post(&pool, actor).await;
    let svc = InteractionService::new(pool.clone());
    let like = NewInteraction {
        actor_id: actor,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: Some(post),
        comment_id: None,
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
            actor_id: actor,
            r#type: InteractionType::Comment,
            target_id: None,
            post_id: Some(post),
            comment_id: None,
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
            area: "public".to_string(),
        })
        .await
        .expect("create post");
    InteractionService::new(pool.clone())
        .record(NewInteraction {
            actor_id: actor,
            r#type: InteractionType::Like,
            target_id: None,
            post_id: Some(post.id),
            comment_id: None,
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

#[sqlx::test(migrations = "../../migrations")]
async fn edges_exclude_private_posts(pool: PgPool) {
    // Twin of the taken-down test for the P-4/A4 area invariant: engagement on a
    // PRIVATE post must never feed the settlement graph (ADR 0011 §5), but a
    // DIRECT user→user edge (target_id set) survives regardless of post area.
    let router = app(AppState::new(pool.clone()));
    let (_t_author, author) = common::register(&router, &[]).await;
    let (_t_actor, actor) = common::register(&router, &[]).await;

    let posts = PostRepository::new(pool.clone());
    let post = posts
        .create(&NewPost {
            author_id: author,
            category: None,
            body: "hello".to_string(),
            media_id: None,
            area: "public".to_string(),
        })
        .await
        .expect("create post");
    let svc = InteractionService::new(pool.clone());
    // A post-derived edge (Like: no target_id) and a direct edge (Comment: sets
    // target_id), both on the same post.
    svc.record(NewInteraction {
        actor_id: actor,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: Some(post.id),
        comment_id: None,
    })
    .await
    .expect("record like");
    svc.record(NewInteraction {
        actor_id: actor,
        r#type: InteractionType::Comment,
        target_id: Some(author),
        post_id: Some(post.id),
        comment_id: None,
    })
    .await
    .expect("record comment");

    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    let repo = InteractionRepository::new(pool.clone());

    // While public: both resolve to an actor→author edge.
    assert_eq!(
        repo.edges_for_epoch(epoch).await.expect("edges").len(),
        2,
        "public post: like + comment each confer an edge"
    );

    // Flip the post private (no API write path exists yet — A4g; set it directly).
    sqlx::query!("UPDATE posts SET area = 'private' WHERE id = $1", post.id)
        .execute(&pool)
        .await
        .expect("set private");

    // The post-derived Like is dropped; the direct Comment edge (target_id set)
    // survives — a top-level area filter would have wrongly zeroed it too.
    let after = repo.edges_for_epoch(epoch).await.expect("edges after");
    assert_eq!(
        after.len(),
        1,
        "private post: the post-derived like is dropped, the direct edge survives"
    );
    assert_eq!(after[0].actor_id, actor);
    assert_eq!(after[0].target_id, author);
}

#[sqlx::test(migrations = "../../migrations")]
async fn interaction_on_missing_post_is_404(pool: PgPool) {
    // The post FK (migration 0015) rejects an interaction on a non-existent post;
    // the service maps it to 404 (a client error) rather than a 500.
    let router = app(AppState::new(pool));
    let (token, _actor) = common::register(&router, &[]).await;

    let body = serde_json::json!({ "type": "like", "post_id": 999999 });
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
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn target_directed_interactions_dedup_regardless_of_post(pool: PgPool) {
    // Anti-inflation regression: N interactions directed at the SAME user with
    // different post_ids must collapse to ONE edge. `post_id` is cleared when
    // `target_id` is set, so it can no longer multiply the dedup key
    // (actor, type, epoch, target_id, post_id) into N duplicate edges — the bypass
    // that defeated the 0009 unique index.
    let actor = seed_user(&pool).await;
    let target = seed_user(&pool).await;
    let p1 = seed_post(&pool, target).await;
    let p2 = seed_post(&pool, target).await;
    let svc = InteractionService::new(pool.clone());

    let mk = |post| NewInteraction {
        actor_id: actor,
        r#type: InteractionType::Follow,
        target_id: Some(target),
        post_id: Some(post),
        comment_id: None,
    };
    let first = svc.record(mk(p1)).await.expect("first");
    let again = svc.record(mk(p2)).await.expect("second");
    assert_eq!(
        first.id, again.id,
        "a different post_id must not create a second edge to the same target"
    );

    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    let all = InteractionRepository::new(pool)
        .list_by_epoch(epoch)
        .await
        .expect("list");
    assert_eq!(
        all.len(),
        1,
        "target-directed interactions dedup regardless of post_id"
    );
}

#[test]
fn weights_order_like_below_comment_below_share() {
    // Pure check on the ω_type ordering the graph relies on — no DB needed.
    assert!(omega(InteractionType::Like) < omega(InteractionType::Comment));
    assert!(omega(InteractionType::Comment) < omega(InteractionType::Share));
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_record_returns_typed_view(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (token, actor) = common::register(&router, &[]).await;
    let post = seed_post(&pool, actor).await;

    // No actor_id in the body — taken from the session.
    let body = serde_json::json!({ "type": "share", "post_id": post });
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
    assert_eq!(view["weight"], omega(InteractionType::Share));
    assert!(view["epoch_k"].as_i64().unwrap() > 0);
}

// ---------------------------------------------------------------------------
// ADR 0012: un-like (retraction) + comment likes
// ---------------------------------------------------------------------------

async fn seed_comment(pool: &PgPool, post: i64, author: i64) -> i64 {
    core_api::comments::repository::CommentRepository::new(pool.clone())
        .create(post, author, "hi")
        .await
        .expect("comment insert")
        .expect("post visible to commenter")
        .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn unlike_voids_the_edge_and_relike_restores_it(pool: PgPool) {
    let actor = seed_user(&pool).await;
    let author = seed_user(&pool).await;
    let post = seed_post(&pool, author).await;
    let svc = InteractionService::new(pool.clone());
    let like = NewInteraction {
        actor_id: actor,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: Some(post),
        comment_id: None,
    };
    let first = svc.record(like.clone()).await.expect("like");

    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    let repo = InteractionRepository::new(pool.clone());
    assert_eq!(repo.edges_for_epoch(epoch).await.expect("edges").len(), 1);

    // Un-like: the edge disappears from settlement, but the journal keeps the
    // row — voided, not deleted (append-only history).
    svc.retract(like.clone()).await.expect("retract");
    assert!(
        repo.edges_for_epoch(epoch).await.expect("edges").is_empty(),
        "a retracted like confers no edge"
    );
    let journal = repo.list_by_epoch(epoch).await.expect("journal");
    assert_eq!(journal.len(), 1, "the journal keeps the voided row");
    assert!(journal[0].retracted_at.is_some());

    // Re-like within the same epoch: the ORIGINAL row is un-voided — same id,
    // same weight, still exactly one row. Like → un-like → like cycling can
    // never inflate weight past the dedup cap.
    let again = svc.record(like.clone()).await.expect("re-like");
    assert_eq!(again.id, first.id);
    assert_eq!(again.weight, first.weight);
    assert!(again.retracted_at.is_none());
    assert_eq!(repo.edges_for_epoch(epoch).await.expect("edges").len(), 1);
    assert_eq!(repo.list_by_epoch(epoch).await.expect("journal").len(), 1);

    // Retracting twice — and retracting something never liked — is an
    // idempotent no-op, not an error.
    svc.retract(like.clone()).await.expect("retract");
    svc.retract(like).await.expect("retract again");
    assert!(repo.edges_for_epoch(epoch).await.expect("edges").is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn retract_is_like_only(pool: PgPool) {
    // A follow has its own DELETE path, a comment event mirrors a comment row
    // that still exists — only `like` may be retracted.
    let actor = seed_user(&pool).await;
    let target = seed_user(&pool).await;
    let svc = InteractionService::new(pool.clone());
    let err = svc
        .retract(NewInteraction {
            actor_id: actor,
            r#type: InteractionType::Follow,
            target_id: Some(target),
            post_id: None,
            comment_id: None,
        })
        .await
        .expect_err("follow is not retractable");
    assert!(matches!(
        err,
        core_api::error::ApiError::Validation("only_like_retractable")
    ));
}

#[sqlx::test(migrations = "../../migrations")]
async fn comment_like_resolves_to_comment_author(pool: PgPool) {
    let post_author = seed_user(&pool).await;
    let commenter = seed_user(&pool).await;
    let liker = seed_user(&pool).await;
    let post = seed_post(&pool, post_author).await;
    let comment = seed_comment(&pool, post, commenter).await;

    let svc = InteractionService::new(pool.clone());
    let like = NewInteraction {
        actor_id: liker,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: None,
        comment_id: Some(comment),
    };
    let first = svc.record(like.clone()).await.expect("like");
    assert_eq!(first.comment_id, Some(comment));

    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    let repo = InteractionRepository::new(pool.clone());
    let edges = repo.edges_for_epoch(epoch).await.expect("edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].actor_id, liker);
    assert_eq!(
        edges[0].target_id, commenter,
        "the edge flows to the COMMENT author, not the post author"
    );

    // Dedup: the same comment-like repeats into the same row.
    let again = svc.record(like.clone()).await.expect("repeat");
    assert_eq!(first.id, again.id);

    // Un-like removes the edge.
    svc.retract(like).await.expect("retract");
    assert!(repo.edges_for_epoch(epoch).await.expect("edges").is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn comment_like_on_missing_comment_is_404(pool: PgPool) {
    let actor = seed_user(&pool).await;
    let err = InteractionService::new(pool.clone())
        .record(NewInteraction {
            actor_id: actor,
            r#type: InteractionType::Like,
            target_id: None,
            post_id: None,
            comment_id: Some(999_999),
        })
        .await
        .expect_err("missing comment");
    assert!(matches!(err, core_api::error::ApiError::NotFound));
}

#[sqlx::test(migrations = "../../migrations")]
async fn comment_like_gated_by_the_posts_visibility(pool: PgPool) {
    // A4f twin for comment targets: a comment is exactly as visible as its post.
    // Write side: a stranger liking a comment under a private post gets the same
    // 404 as for a missing comment (no existence oracle); the post's author can.
    // Settlement side: the edge is gated by the COMMENT's post visibility.
    let post_author = seed_user(&pool).await;
    let liker = seed_user(&pool).await;
    let post = seed_post(&pool, post_author).await;
    let comment = seed_comment(&pool, post, post_author).await;

    let svc = InteractionService::new(pool.clone());
    let like = |actor: i64| NewInteraction {
        actor_id: actor,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: None,
        comment_id: Some(comment),
    };

    // Flip the post private (no API write path for that yet — set directly).
    sqlx::query!("UPDATE posts SET area = 'private' WHERE id = $1", post)
        .execute(&pool)
        .await
        .expect("set private");

    let err = svc.record(like(liker)).await.expect_err("stranger");
    assert!(matches!(err, core_api::error::ApiError::NotFound));

    // The author can see their own private post, so liking its comment works —
    // but as a self-loop (own comment) it confers no edge, and the private post
    // gates any comment-derived edge out of settlement anyway.
    svc.record(like(post_author)).await.expect("author");
    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    let repo = InteractionRepository::new(pool.clone());
    assert!(repo.edges_for_epoch(epoch).await.expect("edges").is_empty());

    // Back to public, liked by a third user: one real actor→commenter edge.
    sqlx::query!("UPDATE posts SET area = 'public' WHERE id = $1", post)
        .execute(&pool)
        .await
        .expect("set public");
    svc.record(like(liker)).await.expect("liker");
    assert_eq!(repo.edges_for_epoch(epoch).await.expect("edges").len(), 1);

    // Pin the AREA arm of the settlement gate in isolation (hidden_at stays
    // NULL): flipping the comment's post private must drop the third-party
    // edge — this is the `cp.area = 'public'` conjunct, not the takedown one.
    sqlx::query!("UPDATE posts SET area = 'private' WHERE id = $1", post)
        .execute(&pool)
        .await
        .expect("set private again");
    assert!(
        repo.edges_for_epoch(epoch).await.expect("edges").is_empty(),
        "a comment like under a PRIVATE post confers no weight"
    );
    sqlx::query!("UPDATE posts SET area = 'public' WHERE id = $1", post)
        .execute(&pool)
        .await
        .expect("set public again");
    assert_eq!(repo.edges_for_epoch(epoch).await.expect("edges").len(), 1);

    // And the moderation arm: a takedown drops the comment-derived edge too.
    sqlx::query!("UPDATE posts SET hidden_at = now() WHERE id = $1", post)
        .execute(&pool)
        .await
        .expect("take down");
    assert!(
        repo.edges_for_epoch(epoch).await.expect("edges").is_empty(),
        "a comment like on a taken-down post confers no weight"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_unlike_roundtrip(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (_t_author, author) = common::register(&router, &[]).await;
    let (token, _liker) = common::register(&router, &[]).await;
    let post = seed_post(&pool, author).await;

    let like_body = serde_json::json!({ "type": "like", "post_id": post });
    let send = |method: &'static str, auth: Option<String>| {
        let router = router.clone();
        let body = like_body.to_string();
        async move {
            let mut req = Request::builder()
                .method(method)
                .uri("/v1/interactions")
                .header("content-type", "application/json");
            if let Some(token) = auth {
                req = req.header("authorization", format!("Bearer {token}"));
            }
            router
                .oneshot(req.body(Body::from(body)).unwrap())
                .await
                .unwrap()
        }
    };

    // Like → 201; un-like → 204; un-like again → 204 (idempotent toggle).
    assert_eq!(
        send("POST", Some(token.clone())).await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        send("DELETE", Some(token.clone())).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send("DELETE", Some(token.clone())).await.status(),
        StatusCode::NO_CONTENT
    );
    // Unauthenticated → 401, exactly like the POST.
    assert_eq!(
        send("DELETE", None).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let epoch = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i32;
    assert!(InteractionRepository::new(pool)
        .edges_for_epoch(epoch)
        .await
        .expect("edges")
        .is_empty());
}
