//! Tests for the operator-only ingestion admin endpoints: the backfill sweep
//! (enqueue posts with no `content_signals` row, excluding analysed + taken-down
//! posts, id-cursor-paged) and the read-only status snapshot (analysed vs not,
//! broken down by model version). Both are operator-gated.

use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::signals::repository::ContentSignalRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use axum::Router;
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

async fn new_author(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("author")
        .id
}

async fn new_post(pool: &PgPool, author: i64, body: &str) -> i64 {
    PostRepository::new(pool.clone())
        .create(&NewPost {
            author_id: author,
            category: None,
            body: body.into(),
            media_id: None,
            area: "public".to_string(),
        })
        .await
        .expect("post")
        .id
}

/// The repository query is the correctness core — exercise it directly (no Redis):
/// it must exclude analysed and hidden posts, order by id, and honour cursor + limit.
#[sqlx::test(migrations = "../../migrations")]
async fn unanalyzed_query_excludes_analysed_and_hidden_and_paginates(pool: PgPool) {
    let posts = PostRepository::new(pool.clone());
    let signals = ContentSignalRepository::new(pool.clone());
    let author = new_author(&pool).await;

    let p1 = new_post(&pool, author, "a").await;
    let p2 = new_post(&pool, author, "b").await;
    let p3 = new_post(&pool, author, "c").await;
    let p4 = new_post(&pool, author, "d").await;

    // p2 already analysed (has a signals row); p3 taken down.
    signals
        .upsert(p2, "heuristic-v0", 0, &json!({"x": 1}), None)
        .await
        .unwrap();
    posts.set_hidden(p3, Some(Utc::now())).await.unwrap();

    // Only the unanalysed + visible posts, id-ordered.
    assert_eq!(
        posts.unanalyzed_post_ids(0, 100).await.unwrap(),
        vec![p1, p4]
    );
    // Limit is respected.
    assert_eq!(posts.unanalyzed_post_ids(0, 1).await.unwrap(), vec![p1]);
    // Cursor is respected (strictly greater than `after`).
    assert_eq!(posts.unanalyzed_post_ids(p1, 100).await.unwrap(), vec![p4]);
    // Past the last id → drained.
    assert!(posts.unanalyzed_post_ids(p4, 100).await.unwrap().is_empty());
}

/// P-4/A4: private posts leave the ingestion rail entirely (ADR 0011 §5 — never
/// analysed). The producer and the status counts must both exclude them, so the
/// public partition (analysed + unanalysed) stays consistent.
#[sqlx::test(migrations = "../../migrations")]
async fn backfill_and_status_exclude_private_posts(pool: PgPool) {
    let posts = PostRepository::new(pool.clone());
    let signals = ContentSignalRepository::new(pool.clone());
    let author = new_author(&pool).await;

    let public = new_post(&pool, author, "public").await;
    let private_unanalyzed = new_post(&pool, author, "private-1").await;
    let private_analyzed = new_post(&pool, author, "private-2").await;
    sqlx::query!(
        "UPDATE posts SET area = 'private' WHERE id = ANY($1)",
        &[private_unanalyzed, private_analyzed][..]
    )
    .execute(&pool)
    .await
    .expect("set private");
    // Give the analysed private post a signals row — it must still be excluded
    // from the model-version count (partition consistency).
    signals
        .upsert(private_analyzed, "heuristic-v0", 0, &json!({"x": 1}), None)
        .await
        .unwrap();

    // The producer offers ONLY the public unanalysed post.
    assert_eq!(
        posts.unanalyzed_post_ids(0, 100).await.unwrap(),
        vec![public]
    );
    assert_eq!(posts.count_unanalyzed_posts().await.unwrap(), 1);
    // The analysed private post does not show up in the by-model-version count.
    assert!(
        posts
            .signals_count_by_model_version()
            .await
            .unwrap()
            .is_empty(),
        "a signals row on a private post is not counted"
    );
}

async fn backfill(router: &Router, token: Option<&str>, query: &str) -> Response<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(format!("/v1/admin/ingestion/backfill{query}"));
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn json_body(resp: Response<Body>) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// The HTTP endpoint is operator-only and reports the enqueue count + resume cursor.
#[sqlx::test(migrations = "../../migrations")]
async fn backfill_endpoint_is_operator_only_and_reports_counts(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (user_token, _) = common::register(&router, &[]).await;

    let author = new_author(&pool).await;
    let p1 = new_post(&pool, author, "a").await;
    let p2 = new_post(&pool, author, "b").await;
    // p2 analysed → excluded from the sweep.
    ContentSignalRepository::new(pool.clone())
        .upsert(p2, "heuristic-v0", 0, &json!({"x": 1}), None)
        .await
        .unwrap();

    // Unauthenticated → 401; authenticated non-operator → 403.
    assert_eq!(
        backfill(&router, None, "").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        backfill(&router, Some(&user_token), "").await.status(),
        StatusCode::FORBIDDEN
    );

    // Operator → 200, enqueues only the unanalysed post (p1), cursor = p1.
    let resp = backfill(&router, Some(&op_token), "").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["enqueued"].as_i64().unwrap(), 1);
    assert_eq!(body["last_id"].as_i64().unwrap(), p1);

    // Resuming past the last id drains the sweep (0 enqueued).
    let body = json_body(backfill(&router, Some(&op_token), &format!("?after={p1}")).await).await;
    assert_eq!(body["enqueued"].as_i64().unwrap(), 0);
}

async fn status(router: &Router, token: Option<&str>) -> Response<Body> {
    let mut b = Request::builder()
        .method("GET")
        .uri("/v1/admin/ingestion/status");
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// The status endpoint is operator-only and reports analysed/unanalysed counts and
/// a per-model-version breakdown.
#[sqlx::test(migrations = "../../migrations")]
async fn status_reports_counts_and_model_version_breakdown(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (user_token, _) = common::register(&router, &[]).await;

    let author = new_author(&pool).await;
    let p1 = new_post(&pool, author, "a").await;
    let p2 = new_post(&pool, author, "b").await;
    let _p3 = new_post(&pool, author, "c").await; // left unanalysed
    let p4 = new_post(&pool, author, "d").await; // analysed + embedded, then hidden

    let signals = ContentSignalRepository::new(pool.clone());
    signals
        .upsert(p1, "heuristic-v0", 0, &json!({}), None)
        .await
        .unwrap();
    // p2 carries an embedding → the ONE the embeddings count should report.
    signals
        .upsert(p2, "real-model-v1", 1, &json!({}), Some(&[0.1, 0.2]))
        .await
        .unwrap();
    // p4 also has an embedding but gets taken down — hidden posts leave every
    // status count, embeddings included.
    signals
        .upsert(p4, "real-model-v1", 1, &json!({}), Some(&[0.3]))
        .await
        .unwrap();
    PostRepository::new(pool.clone())
        .set_hidden(p4, Some(chrono::Utc::now()))
        .await
        .unwrap();

    // Unauthenticated → 401; authenticated non-operator → 403.
    assert_eq!(
        status(&router, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(&router, Some(&user_token)).await.status(),
        StatusCode::FORBIDDEN
    );

    // Operator → 200 with the progress snapshot.
    let resp = status(&router, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["analyzed"].as_i64().unwrap(), 2);
    assert_eq!(body["unanalyzed"].as_i64().unwrap(), 1);
    assert_eq!(
        body["by_model_version"]["heuristic-v0"].as_i64().unwrap(),
        1
    );
    assert_eq!(
        body["by_model_version"]["real-model-v1"].as_i64().unwrap(),
        1
    );
    // Exactly p2's embedding: p4's is hidden with its post, p1 has none.
    assert_eq!(body["embeddings"].as_i64().unwrap(), 1);
}
