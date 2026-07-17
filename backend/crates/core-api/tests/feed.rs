//! Tests for the feed domain against a real Postgres. Verifies the candidate set
//! unions all three sources and that the cold-start ranker boosts category matches.

use core_api::feed::repository::FeedRepository;
use core_api::posts::model::NewPost;
use core_api::posts::repository::PostRepository;
use core_api::private_area::model::{AccessModel, EntitlementSource};
use core_api::private_area::repository::PrivateAreaRepository;
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
            area: "public".to_string(),
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
async fn candidate_set_excludes_private_posts(pool: PgPool) {
    // P-4/A4c twin of the taken-down test: a private post from a FOLLOWED author
    // must not reach a non-entitled follower's feed; an entitlement — or the
    // creator choosing a `free` area — admits it.
    let viewer = new_user(&pool, vec![]).await;
    let paid_creator = new_user(&pool, vec![]).await;
    let free_creator = new_user(&pool, vec![]).await;
    for followee in [paid_creator, free_creator] {
        sqlx::query("INSERT INTO follows (follower_id, followee_id) VALUES ($1, $2)")
            .bind(viewer)
            .bind(followee)
            .execute(&pool)
            .await
            .unwrap();
    }

    let pub_id = new_post(&pool, paid_creator, None, "public").await;
    let priv_id = new_post(&pool, paid_creator, None, "secret").await;
    let free_priv_id = new_post(&pool, free_creator, None, "free-but-members").await;
    sqlx::query("UPDATE posts SET area = 'private' WHERE id = ANY($1)")
        .bind(vec![priv_id, free_priv_id])
        .execute(&pool)
        .await
        .unwrap();
    let areas = PrivateAreaRepository::new(pool.clone());
    areas
        .upsert_area(free_creator, AccessModel::Free, 0, "")
        .await
        .unwrap();

    let feed = FeedRepository::new(pool.clone());
    let got: Vec<i64> = feed
        .candidates(viewer, &[])
        .await
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    assert!(got.contains(&pub_id), "public post should be a candidate");
    assert!(
        !got.contains(&priv_id),
        "a paid creator's private post leaked into a non-entitled follower's feed"
    );
    assert!(
        got.contains(&free_priv_id),
        "a free area's private post should reach a logged-in follower's feed"
    );

    // Granting the viewer an entitlement admits the paid creator's private post.
    areas
        .grant_entitlement(viewer, paid_creator, EntitlementSource::Purchase, None)
        .await
        .unwrap();
    let after: Vec<i64> = feed
        .candidates(viewer, &[])
        .await
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    assert!(
        after.contains(&priv_id),
        "an entitlement should admit the private post to the feed"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn candidate_set_excludes_taken_down_posts(pool: PgPool) {
    // Moderation invariant: a taken-down post must drop out of the feed (it is a
    // user-facing read). See the post-visibility invariant in posts::repository.
    let viewer = new_user(&pool, vec![]).await;
    let author = new_user(&pool, vec![]).await;
    let post_id = new_post(&pool, author, None, "globally visible").await;

    let repo = FeedRepository::new(pool.clone());
    let before = repo.candidates(viewer, &[]).await.expect("candidates");
    assert!(
        before.iter().any(|p| p.id == post_id),
        "a visible post should be a feed candidate"
    );

    // Take it down → it must no longer be a candidate from any source.
    PostRepository::new(pool.clone())
        .set_hidden(post_id, Some(chrono::Utc::now()))
        .await
        .expect("hide");
    let after = repo
        .candidates(viewer, &[])
        .await
        .expect("candidates after");
    assert!(
        !after.iter().any(|p| p.id == post_id),
        "a taken-down post must not appear in the feed"
    );
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
    let bodies: Vec<&str> = feed["items"]
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

async fn fetch_feed(
    router: &axum::Router,
    viewer: i64,
    token: &str,
    query: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/users/{viewer}/feed{query}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[sqlx::test(migrations = "../../migrations")]
async fn pages_concatenate_to_the_single_shot_ranking(pool: PgPool) {
    use core_api::interactions::model::{InteractionType, NewInteraction};
    use core_api::interactions::service::InteractionService;

    let router = app(AppState::new(pool.clone()));
    let (token, viewer) = common::register(&router, &["tech"]).await;
    let author = new_user(&pool, vec![]).await;
    let mut ids = Vec::new();
    for i in 0..7 {
        let cat = if i % 2 == 0 { Some("tech") } else { None };
        ids.push(new_post(&pool, author, cat, &format!("post {i}")).await);
    }

    // Give the first posts DISTINCT nonzero like counts (3/2/1/0/…) before any
    // fetch: at like_count = 0 the ln(1+likes) ranking term is identically zero,
    // so the cursor's bit-exact score round-trip would be vacuously untested.
    // Counts are fixed before the walk and nothing mutates during it.
    let svc = InteractionService::new(pool.clone());
    for i in 0..3usize {
        let fan = new_user(&pool, vec![]).await;
        for post in ids.iter().take(3 - i) {
            svc.record(NewInteraction {
                actor_id: fan,
                r#type: InteractionType::Like,
                target_id: None,
                post_id: Some(*post),
                comment_id: None,
            })
            .await
            .expect("like");
        }
    }

    // The single-shot ranking is the reference order.
    let (status, single) = fetch_feed(&router, viewer, &token, "?limit=50").await;
    assert_eq!(status, StatusCode::OK);
    let reference: Vec<i64> = single["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    assert_eq!(reference.len(), 7);

    // Walking with limit=2 must reproduce it exactly: no dupes, no gaps.
    let mut walked: Vec<i64> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let q = match &cursor {
            None => "?limit=2".to_string(),
            Some(c) => format!("?limit=2&cursor={c}"),
        };
        let (status, page) = fetch_feed(&router, viewer, &token, &q).await;
        assert_eq!(status, StatusCode::OK);
        walked.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["id"].as_i64().unwrap()),
        );
        match page["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
        assert!(walked.len() <= 20, "cursor walk must terminate");
    }
    assert_eq!(walked, reference, "paged walk must equal the one-shot list");
}

#[sqlx::test(migrations = "../../migrations")]
async fn invalid_and_stale_cursors_are_rejected(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    let (token, viewer) = common::register(&router, &[]).await;

    let (status, body) = fetch_feed(&router, viewer, &token, "?cursor=garbage").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_cursor");

    // A well-formed cursor whose frozen clock predates the candidate window.
    let old = core_api::feed::cursor::encode(&core_api::feed::cursor::FeedCursor {
        ranked_at: chrono::Utc::now().timestamp() - 49 * 3600,
        score_bits: 0,
        last_id: 1,
    });
    let (status, body) = fetch_feed(&router, viewer, &token, &format!("?cursor={old}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "stale_cursor");
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

// ---------------------------------------------------------------------------
// ADR 0012: likes lift the cold-start ranking and hydrate liked_by_me
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn http_feed_ranks_liked_posts_higher_and_hydrates_liked_by_me(pool: PgPool) {
    use core_api::interactions::model::{InteractionType, NewInteraction};
    use core_api::interactions::service::InteractionService;

    let router = app(AppState::new(pool.clone()));
    let (token, viewer) = common::register(&router, &[]).await;
    let author = new_user(&pool, vec![]).await;
    let fan = new_user(&pool, vec![]).await;

    // Equally recent, same author, no category signal — only the likes differ.
    // "plain post" is created SECOND, so on a pure recency tie-break it would
    // win; the liked post outranking it can only come from the like term.
    let liked_id = new_post(&pool, author, None, "liked post").await;
    new_post(&pool, author, None, "plain post").await;

    let svc = InteractionService::new(pool.clone());
    for actor in [viewer, fan] {
        svc.record(NewInteraction {
            actor_id: actor,
            r#type: InteractionType::Like,
            target_id: None,
            post_id: Some(liked_id),
            comment_id: None,
        })
        .await
        .expect("like");
    }

    let (status, feed) = fetch_feed(&router, viewer, &token, "").await;
    assert_eq!(status, StatusCode::OK);
    let items = feed["items"].as_array().unwrap();
    let pos = |body: &str| {
        items
            .iter()
            .position(|p| p["body"].as_str() == Some(body))
            .unwrap_or_else(|| panic!("{body} missing from feed"))
    };
    assert!(
        pos("liked post") < pos("plain post"),
        "the liked post should outrank the newer unliked one"
    );

    // The feed items hydrate the viewer's own like state and the count.
    let liked_item = &items[pos("liked post")];
    assert_eq!(liked_item["like_count"].as_i64(), Some(2));
    assert_eq!(liked_item["liked_by_me"].as_bool(), Some(true));
    let plain_item = &items[pos("plain post")];
    assert_eq!(plain_item["like_count"].as_i64(), Some(0));
    assert_eq!(plain_item["liked_by_me"].as_bool(), Some(false));
}
