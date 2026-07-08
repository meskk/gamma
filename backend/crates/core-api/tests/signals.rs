//! Tests for the AI-ingestion signals write-back endpoint: it is operator-only,
//! it round-trips JSONB signals, and a write-back for an unknown post is rejected.

use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::signals::repository::ContentSignalRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
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
            body: "analyse me".into(),
            media_id: None,
        })
        .await
        .expect("post")
        .id
}

async fn put_signals(
    router: &axum::Router,
    post_id: i64,
    token: Option<&str>,
    body: Value,
) -> axum::http::Response<Body> {
    let mut b = Request::builder()
        .method("PUT")
        .uri(format!("/v1/posts/{post_id}/signals"))
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

async fn get_signals(
    router: &axum::Router,
    post_id: i64,
    token: Option<&str>,
) -> axum::http::Response<Body> {
    let mut b = Request::builder()
        .method("GET")
        .uri(format!("/v1/posts/{post_id}/signals"));
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn signals_read_back_is_operator_only(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;

    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (user_token, _) = common::register(&router, &[]).await;

    // No signals yet → 404 (even for the operator).
    assert_eq!(
        get_signals(&router, post_id, Some(&op_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // Write some, then read them back.
    put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({ "model_version": "heuristic-v0", "signals": { "word_count": 7 } }),
    )
    .await;

    // Unauthenticated → 401; non-operator → 403.
    assert_eq!(
        get_signals(&router, post_id, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get_signals(&router, post_id, Some(&user_token))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Operator → 200 with the stored row.
    let resp = get_signals(&router, post_id, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["post_id"].as_i64().unwrap(), post_id);
    assert_eq!(v["model_version"], "heuristic-v0");
    assert_eq!(v["signals"]["word_count"].as_i64().unwrap(), 7);
}

#[sqlx::test(migrations = "../../migrations")]
async fn signals_writeback_is_operator_only_and_round_trips(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;

    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (user_token, _) = common::register(&router, &[]).await;

    let payload = json!({ "topic": "rust", "quality": 0.91 });

    // Unauthenticated → 401; authenticated non-operator → 403.
    assert_eq!(
        put_signals(
            &router,
            post_id,
            None,
            json!({ "model_version": "m1", "signals": payload })
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&user_token),
            json!({ "model_version": "m1", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    // Operator → 204, and the JSONB signals round-trip through the repository.
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({ "model_version": "m1", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let stored = ContentSignalRepository::new(pool.clone())
        .get(post_id)
        .await
        .unwrap()
        .expect("signals stored");
    assert_eq!(stored.model_version, "m1");
    assert_eq!(stored.signals, payload);

    // A second write-back supersedes the first (upsert).
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({ "model_version": "m2", "signals": { "topic": "systems" } }),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let stored = ContentSignalRepository::new(pool.clone())
        .get(post_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.model_version, "m2");

    // A write-back for a non-existent post → 400 (unknown_post).
    assert_eq!(
        put_signals(
            &router,
            999_999,
            Some(&op_token),
            json!({ "model_version": "m1", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    // Empty model_version → 400.
    assert_eq!(
        put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({ "model_version": "  ", "signals": payload }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn service_role_writes_signals_but_holds_no_operator_powers(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;

    // A machine identity: registered normally, promoted to 'service' via SQL
    // (the documented provisioning path — there is no escalation endpoint).
    let (svc_token, svc_id) = common::register(&router, &[]).await;
    sqlx::query("UPDATE users SET role = 'service' WHERE id = $1")
        .bind(svc_id)
        .execute(&pool)
        .await
        .unwrap();

    // The service MAY write signals…
    let resp = put_signals(
        &router,
        post_id,
        Some(&svc_token),
        json!({ "model_version": "heuristic-v0", "signals": { "words": 2 } }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // …but NONE of the human-operator powers: settlement, bot gate, takedown.
    let settle = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/epochs/0/settle")
                .header("authorization", format!("Bearer {svc_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(settle.status(), StatusCode::FORBIDDEN);

    let verify = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/users/{svc_id}/verification"))
                .header("authorization", format!("Bearer {svc_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"verified":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(verify.status(), StatusCode::FORBIDDEN);

    let takedown = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/posts/{post_id}/takedown"))
                .header("authorization", format!("Bearer {svc_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(takedown.status(), StatusCode::FORBIDDEN);

    // And a plain user still cannot write signals.
    let (user_token, _uid) = common::register(&router, &[]).await;
    let resp = put_signals(
        &router,
        post_id,
        Some(&user_token),
        json!({ "model_version": "heuristic-v0", "signals": {} }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// The machine-readable validation code from a 400 body, for pinning the
/// ADR 0009 error contract (writers retry on codes, not prose).
async fn error_code(resp: axum::http::Response<Body>) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    v["error"].as_str().unwrap_or_default().to_string()
}

#[sqlx::test(migrations = "../../migrations")]
async fn schema_v1_validates_the_typed_core(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;
    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();

    // A full, valid v1 core (plus extras annex) → 204, schema_version stored.
    let resp = put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({
            "model_version": "model-v1",
            "schema_version": 1,
            "signals": {
                "quality": 0.8,
                "bot_likelihood": 0.05,
                "nsfw_likelihood": 0.0,
                "topics": ["tech", "rust"],
                "language": "de",
                "extras": { "anything": ["goes", 42] }
            }
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let stored = ContentSignalRepository::new(pool.clone())
        .get(post_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.schema_version, 1);

    // Null counts as absent — Python writers may send None for empty fields.
    let resp = put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({
            "model_version": "model-v1",
            "schema_version": 1,
            "signals": { "quality": null, "language": null, "extras": { "k": 1 } }
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Legacy writers omit schema_version → accepted verbatim as 0 (pre-ADR).
    let resp = put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({ "model_version": "heuristic-v0", "signals": { "word_count": 7 } }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let stored = ContentSignalRepository::new(pool.clone())
        .get(post_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.schema_version, 0);

    // Rejections, each with its stable machine-readable code:
    let cases: Vec<(Value, &str)> = vec![
        // unknown top-level key (additions go through extras or a schema bump)
        (json!({ "word_count": 7 }), "unknown_signal_field"),
        // scores outside [0,1]
        (json!({ "quality": 1.5 }), "invalid_quality"),
        (json!({ "bot_likelihood": -0.1 }), "invalid_bot_likelihood"),
        (
            json!({ "nsfw_likelihood": "high" }),
            "invalid_nsfw_likelihood",
        ),
        // topics must be normalized (trim+lowercase), unique, string-typed,
        // and capped (≤16 entries, ≤64 bytes each)
        (json!({ "topics": ["Tech"] }), "invalid_topics"),
        (json!({ "topics": ["tech", "tech"] }), "invalid_topics"),
        (json!({ "topics": [7] }), "invalid_topics"),
        (
            json!({ "topics": (0..17).map(|i| format!("t{i}")).collect::<Vec<_>>() }),
            "invalid_topics",
        ),
        (json!({ "topics": ["a".repeat(65)] }), "invalid_topics"),
        // language: loose lowercase BCP-47
        (json!({ "language": "DE" }), "invalid_language"),
        (json!({ "language": "x" }), "invalid_language"),
        // extras must be an object
        (json!({ "extras": [1, 2] }), "invalid_extras"),
    ];
    for (signals, want_code) in cases {
        let resp = put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({ "model_version": "model-v1", "schema_version": 1, "signals": signals }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "case {signals}");
        assert_eq!(error_code(resp).await, want_code, "case {signals}");
    }

    // v1 signals must be an object at all.
    let resp = put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({ "model_version": "model-v1", "schema_version": 1, "signals": [1, 2] }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "signals_not_an_object");

    // A schema version newer than this API → fail closed (deploy API first).
    let resp = put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({ "model_version": "model-v9", "schema_version": 2, "signals": {} }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "unknown_schema_version");
}

#[sqlx::test(migrations = "../../migrations")]
async fn embeddings_are_stored_next_door_and_never_read_back(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let post_id = seed_post(&pool).await;
    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();

    // Write signals + embedding in one write-back → both stored atomically.
    let resp = put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({
            "model_version": "model-v1",
            "schema_version": 1,
            "signals": { "quality": 0.5 },
            "embedding": [0.25, -0.5, 0.125]
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let row = sqlx::query!(
        r#"SELECT model_version, dim, embedding AS "embedding!: Vec<f32>"
           FROM post_embeddings WHERE post_id = $1"#,
        post_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.model_version, "model-v1");
    assert_eq!(row.dim, 3);
    assert_eq!(row.embedding, vec![0.25, -0.5, 0.125]);

    // A later write-back supersedes it (one current row, like the signals).
    let resp = put_signals(
        &router,
        post_id,
        Some(&op_token),
        json!({
            "model_version": "model-v2",
            "schema_version": 1,
            "signals": {},
            "embedding": [1.0, 0.0]
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let row = sqlx::query!(
        r#"SELECT model_version, dim FROM post_embeddings WHERE post_id = $1"#,
        post_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((row.model_version.as_str(), row.dim), ("model-v2", 2));

    // The read path never returns embeddings (ADR 0009 §3).
    let resp = get_signals(&router, post_id, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v.get("embedding").is_none());
    assert_eq!(v["schema_version"].as_i64().unwrap(), 1);

    // Rejected embeddings — each rejects the WHOLE write-back: empty, a value
    // that overflows f32 to +inf (serde casts JSON numbers through f64 without
    // erroring), and one over the dimension cap.
    for bad in [json!([]), json!([1e39]), json!(vec![0.0f32; 4097])] {
        let resp = put_signals(
            &router,
            post_id,
            Some(&op_token),
            json!({
                "model_version": "model-v3",
                "schema_version": 1,
                "signals": {},
                "embedding": bad
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(error_code(resp).await, "invalid_embedding");
    }
    let stored = ContentSignalRepository::new(pool.clone())
        .get(post_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.model_version, "model-v2");
}
