//! P-4/A3 HTTP tests: the dark launch (OFF = the routes are unmounted and
//! byte-for-byte indistinguishable from a nonexistent path), creator-scoped
//! configuration with service validation, and the public terms read.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::Router;
use core_api::{app, AppState};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

mod common;

fn enabled_app(pool: PgPool) -> Router {
    app(AppState::new(pool).with_private_area(true))
}

async fn raw(
    router: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    router.clone().oneshot(request).await.unwrap()
}

async fn send(
    router: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let resp = raw(router, method, uri, token, body).await;
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// (status, sorted header name/value pairs, body bytes) — the full observable
/// surface, so a "dark" route can be proven byte-identical to an unrouted one.
/// `x-request-id` is dropped: it is a fresh UUID on EVERY response (including
/// the unrouted reference), so it is never a feature-specific tell.
async fn surface(resp: Response) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let status = resp.status();
    let mut headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .filter(|(k, _)| k.as_str() != "x-request-id")
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<bin>").to_string()))
        .collect();
    headers.sort();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, body)
}

#[sqlx::test(migrations = "../../migrations")]
async fn flag_off_routes_are_indistinguishable_from_nonexistent(pool: PgPool) {
    // Explicitly OFF — never read from (or write to) the process environment.
    let router = app(AppState::new(pool).with_private_area(false));
    let (token, user) = common::register(&router, &[]).await;

    // A genuinely unrouted path under /v1 is the reference: whatever the dark
    // routes answer must match it EXACTLY (status + headers + body), or a probe
    // could fingerprint the feature before legal sign-off (ADR 0011 §6).
    let reference =
        surface(raw(&router, "GET", "/v1/me/does-not-exist", Some(&token), None).await).await;
    assert_eq!(reference.0, StatusCode::NOT_FOUND);

    // Every method/path the feature WOULD expose — including a mismatched method
    // (POST/DELETE), which on a mounted route would leak an `Allow` header, and
    // an unauthenticated / malformed-body call, which on a live route would
    // 401/400.
    let tok = Some(token.as_str());
    let me = "/v1/me/private-area";
    let user_path = format!("/v1/users/{user}/private-area");
    let probes: [(&str, &str, Option<&str>, Option<Value>); 7] = [
        ("GET", me, tok, None),
        (
            "PUT",
            me,
            tok,
            Some(json!({ "access_model": "one_time", "price_cents": 500 })),
        ),
        ("POST", me, tok, None),
        ("DELETE", me, None, None),
        ("GET", &user_path, tok, None),
        ("GET", me, None, None),
        ("PUT", me, None, Some(json!("not an object"))),
    ];
    for (method, uri, probe_tok, body) in probes {
        let got = surface(raw(&router, method, uri, probe_tok, body).await).await;
        assert_eq!(
            got, reference,
            "{method} {uri} is distinguishable from an unrouted path with the flag off"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn creator_configures_and_anyone_reads_the_terms(pool: PgPool) {
    let router = enabled_app(pool);
    let (creator_token, creator) = common::register(&router, &[]).await;
    let (viewer_token, _viewer) = common::register(&router, &[]).await;

    // Nothing configured yet: own read and public read are honest 404s.
    let (status, _) = send(
        &router,
        "GET",
        "/v1/me/private-area",
        Some(&creator_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The creator prices their area. The creator id comes from the session —
    // there is no id field to spoof.
    let config = json!({
        "access_model": "one_time",
        "price_cents": 500,
        "description": "  Mein privater Bereich  "
    });
    let (status, body) = send(
        &router,
        "PUT",
        "/v1/me/private-area",
        Some(&creator_token),
        Some(config),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["creator_id"].as_i64(), Some(creator));
    assert_eq!(body["access_model"], "one_time");
    assert_eq!(body["price_cents"].as_i64(), Some(500));
    assert_eq!(body["currency"], "EUR");
    // Description is trimmed by the service.
    assert_eq!(body["description"], "Mein privater Bereich");

    // Another authenticated user reads the terms — they are the offer.
    let (status, body) = send(
        &router,
        "GET",
        &format!("/v1/users/{creator}/private-area"),
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["price_cents"].as_i64(), Some(500));

    // Reconfiguring replaces the one row (creator changes their mind).
    let config = json!({ "access_model": "free", "price_cents": 0 });
    let (status, body) = send(
        &router,
        "PUT",
        "/v1/me/private-area",
        Some(&creator_token),
        Some(config),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["access_model"], "free");
    assert_eq!(body["price_cents"].as_i64(), Some(0));
    assert_eq!(body["description"], "");

    // Reads require a session (the platform is logged-in-only).
    let (status, _) = send(
        &router,
        "GET",
        &format!("/v1/users/{creator}/private-area"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn validation_rejects_what_the_offer_could_not_honor(pool: PgPool) {
    let router = enabled_app(pool);
    let (token, _creator) = common::register(&router, &[]).await;

    for (case, config, expected) in [
        (
            "unknown model",
            json!({ "access_model": "vip", "price_cents": 100 }),
            "unknown_access_model",
        ),
        (
            "negative price",
            json!({ "access_model": "one_time", "price_cents": -1 }),
            "negative_price",
        ),
        (
            "absurd price",
            json!({ "access_model": "one_time", "price_cents": 1_000_001 }),
            "price_too_high",
        ),
        (
            "paid model without a price",
            json!({ "access_model": "subscription", "price_cents": 0 }),
            "missing_price",
        ),
        (
            "free area with a price",
            json!({ "access_model": "free", "price_cents": 100 }),
            "price_on_unpriced_model",
        ),
        (
            "per-post area with an area price",
            json!({ "access_model": "per_post", "price_cents": 100 }),
            "price_on_unpriced_model",
        ),
        (
            "description too long",
            json!({
                "access_model": "one_time",
                "price_cents": 100,
                "description": "x".repeat(501)
            }),
            "description_too_long",
        ),
    ] {
        let (status, body) = send(
            &router,
            "PUT",
            "/v1/me/private-area",
            Some(&token),
            Some(config),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{case}");
        assert_eq!(body["error"], expected, "{case}");
    }

    // Nothing invalid was stored.
    let (status, _) = send(&router, "GET", "/v1/me/private-area", Some(&token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn every_access_model_accepts_its_valid_price(pool: PgPool) {
    let router = enabled_app(pool);

    // One creator per model — the accept side of the model×price matrix that the
    // DB CHECKs do NOT enforce (price>0 for paid, price==0 for unpriced).
    for (model, price) in [
        ("free", 0),
        ("one_time", 500),
        ("subscription", 999),
        ("per_post", 0),
    ] {
        let (token, _) = common::register(&router, &[]).await;
        let config = json!({ "access_model": model, "price_cents": price });
        let (status, body) = send(
            &router,
            "PUT",
            "/v1/me/private-area",
            Some(&token),
            Some(config),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{model} @ {price} should be accepted"
        );
        assert_eq!(body["access_model"], model);
        assert_eq!(body["price_cents"].as_i64(), Some(price));
    }
}
