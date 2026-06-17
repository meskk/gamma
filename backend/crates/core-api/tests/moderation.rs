//! Minimal moderation: users report posts; an operator takes them down / restores
//! them; a taken-down post drops out of the feed and public reads.

use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Response<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    router.clone().oneshot(req).await.unwrap()
}

async fn json_of(resp: Response<Body>) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn create_post(router: &axum::Router, token: &str, body: &str) -> i64 {
    let resp = send(
        router,
        "POST",
        "/v1/posts",
        Some(token),
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    json_of(resp).await["id"].as_i64().unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn report_takedown_restore(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    let (author_token, _author) = common::register(&router, &[]).await;
    let post_id = create_post(&router, &author_token, "hello world").await;

    let (reporter_token, _r) = common::register(&router, &[]).await;
    let (op_token, op_id) = common::register(&router, &[]).await;
    sqlx::query!("UPDATE users SET role = 'operator' WHERE id = $1", op_id)
        .execute(&pool)
        .await
        .unwrap();

    let report_uri = format!("/v1/posts/{post_id}/report");

    // Report: unauth → 401; reporter → 204; a repeat is idempotent (still 204).
    assert_eq!(
        send(
            &router,
            "POST",
            &report_uri,
            None,
            Some(json!({"reason": "spam"}))
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &router,
            "POST",
            &report_uri,
            Some(&reporter_token),
            Some(json!({"reason": "spam"}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &router,
            "POST",
            &report_uri,
            Some(&reporter_token),
            Some(json!({"reason": "again"}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    // Empty reason → 400; report on a missing post → 404.
    assert_eq!(
        send(
            &router,
            "POST",
            &report_uri,
            Some(&reporter_token),
            Some(json!({"reason": "  "}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &router,
            "POST",
            "/v1/posts/999999/report",
            Some(&reporter_token),
            Some(json!({"reason": "x"}))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    // Operator review queue shows the post with exactly one report.
    let reports = json_of(send(&router, "GET", "/v1/reports", Some(&op_token), None).await).await;
    let row = reports
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["post_id"].as_i64() == Some(post_id))
        .expect("reported post in the review queue");
    assert_eq!(row["report_count"].as_i64(), Some(1));
    // The review queue is operator-only.
    assert_eq!(
        send(&router, "GET", "/v1/reports", Some(&reporter_token), None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Takedown: non-operator → 403; unauth → 401; operator → 200.
    let takedown_uri = format!("/v1/posts/{post_id}/takedown");
    assert_eq!(
        send(&router, "POST", &takedown_uri, Some(&reporter_token), None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&router, "POST", &takedown_uri, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&router, "POST", &takedown_uri, Some(&op_token), None)
            .await
            .status(),
        StatusCode::OK
    );

    // A taken-down post is gone from public reads and the listing.
    assert_eq!(
        send(&router, "GET", &format!("/v1/posts/{post_id}"), None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let list = json_of(send(&router, "GET", "/v1/posts", None, None).await).await;
    assert!(
        !list
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"].as_i64() == Some(post_id)),
        "hidden post must not appear in the listing"
    );

    // Restore brings it back.
    assert_eq!(
        send(
            &router,
            "POST",
            &format!("/v1/posts/{post_id}/restore"),
            Some(&op_token),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, "GET", &format!("/v1/posts/{post_id}"), None, None)
            .await
            .status(),
        StatusCode::OK
    );
}
