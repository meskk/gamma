//! Auth flow tests against a real Postgres: register, login, and the bearer-token
//! protected `/auth/me` probe.

use core_api::{app, AppState};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn post_json(router: &axum::Router, uri: &str, body: Value) -> axum::http::Response<Body> {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn json_body(resp: axum::http::Response<Body>) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn register_login_and_authenticated_me(pool: PgPool) {
    let router = app(AppState::new(pool));

    // Register.
    let resp = post_json(
        &router,
        "/v1/auth/register",
        serde_json::json!({ "email": "Alice@example.com", "password": "supersecret" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let reg = json_body(resp).await;
    let token = reg["token"].as_str().unwrap().to_string();
    let user_id = reg["user_id"].as_i64().unwrap();

    // The token authenticates /auth/me.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["user_id"].as_i64().unwrap(), user_id);

    // Login (email is normalised to lowercase) returns a working token too.
    let resp = post_json(
        &router,
        "/v1/auth/login",
        serde_json::json!({ "email": "alice@example.com", "password": "supersecret" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["user_id"].as_i64().unwrap(), user_id);

    // Wrong password → 401.
    let resp = post_json(
        &router,
        "/v1/auth/login",
        serde_json::json!({ "email": "alice@example.com", "password": "wrong" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn logout_revokes_the_session(pool: PgPool) {
    let router = app(AppState::new(pool));

    // Register → a working token.
    let reg = json_body(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "logout@example.com", "password": "supersecret" }),
        )
        .await,
    )
    .await;
    let token = reg["token"].as_str().unwrap().to_string();

    let me = |t: String| {
        let router = router.clone();
        async move {
            router
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/v1/auth/me")
                        .header("authorization", format!("Bearer {t}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
    };

    // Token works, then logout revokes it, then the SAME token is 401.
    assert_eq!(me(token.clone()).await, StatusCode::OK);
    let logout = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/logout")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        me(token).await,
        StatusCode::UNAUTHORIZED,
        "a revoked token must no longer authenticate"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn check_email_reports_existence(pool: PgPool) {
    let router = app(AppState::new(pool));

    // Unknown email → exists: false (normalised the same way login is).
    let resp = post_json(
        &router,
        "/v1/auth/check-email",
        serde_json::json!({ "email": "  New@Example.com " }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["exists"].as_bool(), Some(false));

    // Register it, then the same (differently-cased) email → exists: true.
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "new@example.com", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let resp = post_json(
        &router,
        "/v1/auth/check-email",
        serde_json::json!({ "email": "NEW@example.com" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["exists"].as_bool(), Some(true));
}

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_email_conflicts(pool: PgPool) {
    let router = app(AppState::new(pool));
    let body = serde_json::json!({ "email": "bob@example.com", "password": "supersecret" });

    assert_eq!(
        post_json(&router, "/v1/auth/register", body.clone())
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        post_json(&router, "/v1/auth/register", body).await.status(),
        StatusCode::CONFLICT
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn weak_password_and_bad_email_rejected(pool: PgPool) {
    let router = app(AppState::new(pool));

    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "c@example.com", "password": "short" }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "notanemail", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

/// Log in with (email, password), returning the response.
async fn login(router: &axum::Router, email: &str, password: &str) -> axum::http::Response<Body> {
    post_json(
        router,
        "/v1/auth/login",
        serde_json::json!({ "email": email, "password": password }),
    )
    .await
}

#[sqlx::test(migrations = "../../migrations")]
async fn sixth_attempt_throttled_even_with_correct_password(pool: PgPool) {
    let router = app(AppState::new(pool));
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "throttle@example.com", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    // Failures 1-5 are 401 (the free band is 1-4; the 5th failure SETS the lock
    // but is itself still an ordinary wrong-password rejection).
    for i in 1..=5 {
        assert_eq!(
            login(&router, "throttle@example.com", "wrong-password")
                .await
                .status(),
            StatusCode::UNAUTHORIZED,
            "failure {i} should be 401"
        );
    }

    // Now the email is locked: even the RIGHT password is rejected with 429,
    // a parseable Retry-After, and the shared rate_limited error code.
    let resp = login(&router, "throttle@example.com", "supersecret").await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry: u64 = resp
        .headers()
        .get("retry-after")
        .expect("Retry-After header")
        .to_str()
        .unwrap()
        .parse()
        .expect("Retry-After must be integer seconds");
    assert!(
        (1..=60).contains(&retry),
        "first lock is 60s; Retry-After was {retry}"
    );
    assert_eq!(json_body(resp).await["error"], "rate_limited");
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_email_throttles_identically(pool: PgPool) {
    let router = app(AppState::new(pool));

    // No such account — the observable sequence must match the real-account
    // case exactly (401 ×5, then 429), or the throttle becomes an enumeration
    // oracle.
    for i in 1..=5 {
        assert_eq!(
            login(&router, "ghost@example.com", "wrong-password")
                .await
                .status(),
            StatusCode::UNAUTHORIZED,
            "failure {i} should be 401"
        );
    }
    let resp = login(&router, "ghost@example.com", "wrong-password").await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().get("retry-after").is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn successful_login_resets_the_failure_count(pool: PgPool) {
    let router = app(AppState::new(pool));
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "reset@example.com", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    // 4 failures (the free band), then a success clears the count…
    for _ in 0..4 {
        login(&router, "reset@example.com", "wrong-password").await;
    }
    assert_eq!(
        login(&router, "reset@example.com", "supersecret")
            .await
            .status(),
        StatusCode::OK
    );

    // …so 4 MORE failures still don't lock (a stale count would: 4+4 > 5).
    for _ in 0..4 {
        assert_eq!(
            login(&router, "reset@example.com", "wrong-password")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        login(&router, "reset@example.com", "supersecret")
            .await
            .status(),
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn expired_lock_allows_login_again(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "expired@example.com", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    // Simulate a lock that has already run out.
    sqlx::query(
        "INSERT INTO login_throttle (email, failed_count, last_failed_at, locked_until)
         VALUES ($1, 7, now() - interval '1 hour', now() - interval '1 second')",
    )
    .bind("expired@example.com")
    .execute(&pool)
    .await
    .unwrap();

    // Expired lock no longer blocks, and the success clears the history…
    assert_eq!(
        login(&router, "expired@example.com", "supersecret")
            .await
            .status(),
        StatusCode::OK
    );
    // …so one fresh failure is a plain 401 (count restarted), not a 429.
    assert_eq!(
        login(&router, "expired@example.com", "wrong-password")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn purge_removes_only_expired_sessions(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    // A live session (via register)…
    let reg = json_body(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "purge@example.com", "password": "supersecret" }),
        )
        .await,
    )
    .await;
    let token = reg["token"].as_str().unwrap().to_string();
    let user_id = reg["user_id"].as_i64().unwrap();

    // …plus one already-expired session for the same user.
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at)
         VALUES ('expired-hash', $1, now() - interval '1 day')",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let auth = core_api::auth::AuthService::new(pool);
    assert_eq!(auth.purge_expired_sessions().await.unwrap(), 1);

    // The live token survived the purge.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "../../migrations")]
async fn throttle_sweep_removes_only_stale_rows(pool: PgPool) {
    // One stale row (idle > 24h) and one fresh one.
    sqlx::query(
        "INSERT INTO login_throttle (email, failed_count, last_failed_at)
         VALUES ('stale@example.com', 3, now() - interval '25 hours'),
                ('fresh@example.com', 3, now())",
    )
    .execute(&pool)
    .await
    .unwrap();

    let auth = core_api::auth::AuthService::new(pool.clone());
    assert_eq!(auth.sweep_stale_login_throttle().await.unwrap(), 1);

    let remaining: (String,) = sqlx::query_as("SELECT email FROM login_throttle")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining.0, "fresh@example.com");
}

#[sqlx::test(migrations = "../../migrations")]
async fn register_with_referral_code_freezes_default_terms(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    // The referrer registers; their code comes from GET /auth/me.
    let reg = json_body(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "referrer@example.com", "password": "supersecret" }),
        )
        .await,
    )
    .await;
    let referrer_id = reg["user_id"].as_i64().unwrap();
    let token = reg["token"].as_str().unwrap().to_string();
    let me = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let code = json_body(me).await["referral_code"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(code.len(), 12, "DB-generated 12-hex referral code");

    // A new user registers WITH that code…
    let referred = json_body(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({
                "email": "invited@example.com",
                "password": "supersecret",
                "referral_code": code,
            }),
        )
        .await,
    )
    .await;
    let referred_id = referred["user_id"].as_i64().unwrap();

    // …and the referral row froze the econ defaults (300 bps) with a
    // valid_until in the future.
    let row: (i64, i32, i64) = sqlx::query_as(
        "SELECT referrer_id, bps, valid_until_epoch FROM referrals WHERE referred_id = $1",
    )
    .bind(referred_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, referrer_id);
    assert_eq!(row.1, 300, "econ default referral_bps_default");
    let current_epoch = chrono::Utc::now().timestamp() / 86_400;
    assert_eq!(row.2, current_epoch + 183, "default duration of ~6 months");
}

#[sqlx::test(migrations = "../../migrations")]
async fn register_with_operator_contract_freezes_override_terms(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    let reg = json_body(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "creator@example.com", "password": "supersecret" }),
        )
        .await,
    )
    .await;
    let creator_id = reg["user_id"].as_i64().unwrap();

    // Operator grants the creator a 5% / 30-epoch contract.
    sqlx::query(
        "INSERT INTO referral_terms (referrer_id, bps, duration_epochs, note)
         VALUES ($1, 500, 30, 'launch creator deal')",
    )
    .bind(creator_id)
    .execute(&pool)
    .await
    .unwrap();
    let code: (String,) = sqlx::query_as("SELECT referral_code FROM users WHERE id = $1")
        .bind(creator_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    let referred = json_body(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({
                "email": "fan@example.com",
                "password": "supersecret",
                "referral_code": code.0,
            }),
        )
        .await,
    )
    .await;

    let row: (i32, i64) =
        sqlx::query_as("SELECT bps, valid_until_epoch FROM referrals WHERE referred_id = $1")
            .bind(referred["user_id"].as_i64().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    let current_epoch = chrono::Utc::now().timestamp() / 86_400;
    assert_eq!(row.0, 500, "override bps frozen");
    assert_eq!(row.1, current_epoch + 30, "override duration frozen");
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_referral_code_fails_registration_cleanly(pool: PgPool) {
    let router = app(AppState::new(pool.clone()));

    let resp = post_json(
        &router,
        "/v1/auth/register",
        serde_json::json!({
            "email": "typo@example.com",
            "password": "supersecret",
            "referral_code": "doesnotexist",
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(resp).await["error"], "invalid_referral_code");

    // Nothing half-created: the same email registers fine without the code.
    assert_eq!(
        post_json(
            &router,
            "/v1/auth/register",
            serde_json::json!({ "email": "typo@example.com", "password": "supersecret" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn me_without_or_with_bad_token_is_401(pool: PgPool) {
    let router = app(AppState::new(pool));

    // No token.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Garbage token.
    let resp = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/auth/me")
                .header("authorization", "Bearer deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
