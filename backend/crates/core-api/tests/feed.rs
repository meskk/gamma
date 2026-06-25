//! Tests for the feed domain against a real Postgres. Verifies the candidate set
//! unions all three sources and that the cold-start ranker boosts category matches.

use core_api::feed::repository::FeedRepository;
use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

async fn new_user(pool: &PgPool, categories: Vec<String>) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: categories,
            bot_gate_v: true,
        })
        .await
        .expect("user")
        .id
}

async fn new_post(pool: &PgPool, author: i64, category: Option<&str>, body: &str) -> i64 {
    PostRepository::new(pool.clone())
        .create(&NewPost {
            author_id: author,
            category: category.map(str::to_string),
            body: body.into(),
            media_id: None,
        })
        .await
        .expect("post")
        .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn candidate_set_unions_all_three_sources(pool: PgPool) {
    let viewer = new_user(&pool, vec!["tech".into()]).await;
    let followed = new_user(&pool, vec![]).await;
    let stranger = new_user(&pool, vec![]).await;

    // Edge: viewer follows `followed`.
    sqlx::query("INSERT INTO follows (follower_id, followee_id) VALUES ($1, $2)")
        .bind(viewer)
        .bind(followed)
        .execute(&pool)
        .await
        .unwrap();

    let p_follow = new_post(&pool, followed, None, "from a followed author").await;
    let p_category = new_post(&pool, stranger, Some("tech"), "a tech post").await;
    let p_popular = new_post(&pool, stranger, None, "globally visible").await;

    let candidates = FeedRepository::new(pool)
        .candidates(viewer, &["tech".to_string()])
        .await
        .expect("candidates");

    let ids: Vec<i64> = candidates.iter().map(|p| p.id).collect();
    assert!(ids.contains(&p_follow), "follow source missing");
    assert!(ids.contains(&p_category), "category source missing");
    assert!(ids.contains(&p_popular), "popularity source missing");
}

#[sqlx::test(migrations = "../../migrations")]
async fn http_feed_boosts_category_match(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    // The viewer reads their OWN feed, so they need a session; register with the
    // "tech" interest the ranker should boost.
    let (token, viewer) = common::register(&router, &["tech"]).await;
    let author = new_user(&pool, vec![]).await;

    // Both posts are equally recent; only the category match differs.
    new_post(&pool, author, None, "plain post").await;
    new_post(&pool, author, Some("tech"), "tech post").await;

    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{viewer}/feed"))
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
    let feed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let bodies: Vec<&str> = feed
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["body"].as_str().unwrap())
        .collect();

    let tech_pos = bodies.iter().position(|b| *b == "tech post").unwrap();
    let plain_pos = bodies.iter().position(|b| *b == "plain post").unwrap();
    assert!(
        tech_pos < plain_pos,
        "category-matched post should rank above the plain one: {bodies:?}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn feed_is_self_or_operator(pool: PgPool) {
    // A personalized feed is self-scoped: it can only be read with the owner's
    // session (or an operator's). (Reading an unknown user's feed is no longer
    // reachable over HTTP — you can only ask for your own, and you exist.)
    let router = app(AppState::new(pool));
    let (token, viewer) = common::register(&router, &[]).await;
    let (other_token, _other) = common::register(&router, &[]).await;

    let get = |auth: Option<String>| {
        let router = router.clone();
        async move {
            let mut b = Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{viewer}/feed"));
            if let Some(t) = auth {
                b = b.header("authorization", t);
            }
            router
                .oneshot(b.body(Body::empty()).unwrap())
                .await
                .unwrap()
        }
    };

    assert_eq!(get(None).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        get(Some(format!("Bearer {other_token}"))).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(Some(format!("Bearer {token}"))).await.status(),
        StatusCode::OK
    );
}
