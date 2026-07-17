//! Tests for the users domain against a real Postgres. `#[sqlx::test]` gives each
//! test an isolated, migrated database. One test exercises the repository layer
//! directly; the other drives the operator-only verification flow over HTTP.

use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt; // for `oneshot`

#[sqlx::test(migrations = "../../migrations")]
async fn repository_create_then_get(pool: PgPool) {
    let repo = UserRepository::new(pool);

    let created = repo
        .create(&NewUser {
            declared_categories: vec!["tech".into()],
            bot_gate_v: false,
        })
        .await
        .expect("create should succeed");

    let fetched = repo
        .get(created.id)
        .await
        .expect("get should run")
        .expect("user should exist");

    assert_eq!(created.id, fetched.id);
    assert_eq!(fetched.declared_categories, vec!["tech".to_string()]);
    assert!(!fetched.bot_gate_v);

    // A missing id yields None, not an error.
    assert!(repo.get(999_999).await.expect("get should run").is_none());
}

/// Register a credentialed account via HTTP; returns (user_id, token).
async fn register(router: &axum::Router, email: &str) -> (i64, String) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "email": email, "password": "supersecret" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (
        v["user_id"].as_i64().unwrap(),
        v["token"].as_str().unwrap().to_string(),
    )
}

async fn get_user(router: &axum::Router, id: i64) -> axum::http::Response<Body> {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn set_verification(
    router: &axum::Router,
    id: i64,
    verified: bool,
    token: Option<&str>,
) -> axum::http::Response<Body> {
    let mut builder = Request::builder()
        .method("PUT")
        .uri(format!("/v1/users/{id}/verification"))
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    serde_json::json!({ "verified": verified }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn bot_gate_is_operator_only(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    // Operator + a normal account; promote the operator directly (no admin UI).
    let (op_id, op_token) = register(&router, "op@example.com").await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (target_id, _t) = register(&router, "target@example.com").await;
    let (_u, user_token) = register(&router, "user@example.com").await;

    // A freshly registered user is UNVERIFIED — registration never self-asserts the gate.
    let resp = get_user(&router, target_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let u: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(u["bot_gate_v"], serde_json::json!(false));

    // Verification is operator-only: 401 unauth, 403 non-operator.
    assert_eq!(
        set_verification(&router, target_id, true, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        set_verification(&router, target_id, true, Some(&user_token))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Operator can flip the gate; the change is reflected on read.
    let resp = set_verification(&router, target_id, true, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let u: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(u["bot_gate_v"], serde_json::json!(true));

    // Verifying a missing user → 404.
    assert_eq!(
        set_verification(&router, 999_999, true, Some(&op_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

async fn put_referral_terms(
    router: &axum::Router,
    id: i64,
    body: serde_json::Value,
    token: Option<&str>,
) -> axum::http::Response<Body> {
    let mut builder = Request::builder()
        .method("PUT")
        .uri(format!("/v1/users/{id}/referral-terms"))
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    router
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn referral_terms_are_operator_only_and_upsert(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    let (op_id, op_token) = register(&router, "op2@example.com").await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();
    let (creator_id, creator_token) = register(&router, "creator2@example.com").await;

    let deal = serde_json::json!({ "bps": 500, "duration_epochs": 30, "note": "launch deal" });

    // Operator-only: 401 unauthenticated, 403 for a normal user (even for
    // their own terms — contracts are granted, not self-served).
    assert_eq!(
        put_referral_terms(&router, creator_id, deal.clone(), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        put_referral_terms(&router, creator_id, deal.clone(), Some(&creator_token))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Operator sets the contract; the row round-trips.
    let resp = put_referral_terms(&router, creator_id, deal, Some(&op_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let t: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(t["bps"], serde_json::json!(500));
    assert_eq!(t["duration_epochs"], serde_json::json!(30));

    // Upsert: a second PUT replaces, not duplicates.
    let resp = put_referral_terms(
        &router,
        creator_id,
        serde_json::json!({ "bps": 400, "duration_epochs": 60 }),
        Some(&op_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let count: (i64,) = sqlx::query_as("SELECT count(*) FROM referral_terms")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);

    // Validation and 404: bps beyond 100% rejected; unknown user rejected.
    assert_eq!(
        put_referral_terms(
            &router,
            creator_id,
            serde_json::json!({ "bps": 10_001, "duration_epochs": 30 }),
            Some(&op_token)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        put_referral_terms(
            &router,
            999_999,
            serde_json::json!({ "bps": 500, "duration_epochs": 30 }),
            Some(&op_token)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// ADR 0012: the public profile stat `likes_received`
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn likes_received_counts_only_active_likes_on_visible_public_posts(pool: PgPool) {
    use core_api::interactions::model::{InteractionType, NewInteraction};
    use core_api::interactions::service::InteractionService;
    use core_api::posts::model::NewPost;
    use core_api::posts::repository::PostRepository;

    let repo = UserRepository::new(pool.clone());
    let creator = repo
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("creator")
        .id;
    let fan = repo
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("fan")
        .id;

    let posts = PostRepository::new(pool.clone());
    let mut mk_post = Vec::new();
    for body in ["kept", "retracted", "hidden", "private"] {
        mk_post.push(
            posts
                .create(&NewPost {
                    author_id: creator,
                    category: None,
                    body: body.into(),
                    media_id: None,
                    area: "public".to_string(),
                })
                .await
                .expect("post")
                .id,
        );
    }
    let svc = InteractionService::new(pool.clone());
    for id in &mk_post {
        svc.record(NewInteraction {
            actor_id: fan,
            r#type: InteractionType::Like,
            target_id: None,
            post_id: Some(*id),
            comment_id: None,
        })
        .await
        .expect("like");
    }

    // 4 active likes on 4 public posts.
    let user = repo.get(creator).await.expect("get").expect("exists");
    assert_eq!(user.likes_received, 4);

    // Retract one; hide one; flip one private. Only "kept" still counts: the
    // public stat must not move for voided likes, and must not leak moderated
    // or paywalled engagement.
    svc.retract(NewInteraction {
        actor_id: fan,
        r#type: InteractionType::Like,
        target_id: None,
        post_id: Some(mk_post[1]),
        comment_id: None,
    })
    .await
    .expect("unlike");
    sqlx::query!(
        "UPDATE posts SET hidden_at = now() WHERE id = $1",
        mk_post[2]
    )
    .execute(&pool)
    .await
    .expect("hide");
    sqlx::query!(
        "UPDATE posts SET area = 'private' WHERE id = $1",
        mk_post[3]
    )
    .execute(&pool)
    .await
    .expect("privatise");

    let user = repo.get(creator).await.expect("get").expect("exists");
    assert_eq!(user.likes_received, 1);

    // Distinct-(liker, post)-pair semantics (ADR 0012 §2): a prior-epoch
    // duplicate of the surviving like must NOT inflate the public stat — one
    // fan on one post counts once, no matter how many epochs the journal spans.
    let epoch = domain::Epoch::from_unix_seconds(chrono::Utc::now().timestamp()).0 as i32;
    sqlx::query!(
        "INSERT INTO interaction_events (actor_id, target_id, post_id, type, weight, epoch_k)
         VALUES ($1, NULL, $2, $3, 1.0, $4)",
        fan,
        mk_post[0],
        InteractionType::Like.code(),
        epoch - 1
    )
    .execute(&pool)
    .await
    .expect("prior-epoch like");
    let user = repo.get(creator).await.expect("get").expect("exists");
    assert_eq!(
        user.likes_received, 1,
        "the same (fan, post) pair across epochs counts once"
    );
}
