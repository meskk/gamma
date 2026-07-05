//! Per-IP rate limiting: env-driven config, shared IP-key extraction, and the
//! two buckets — a TIGHT one on `/v1/auth/*` (brute-force pacing) and a LOOSE
//! edge backstop over everything (volumetric abuse). Both live here so their
//! proxy semantics and their 429 shape can never diverge.
//!
//! Knobs (all optional):
//! - `GAMMA_RATE_LIMIT_DISABLED=true`  — disables BOTH buckets (local dev).
//! - `GAMMA_RATE_LIMIT_PER_SECOND`     — edge sustained rate, requests/second
//!   (default 20). NOTE: earlier code passed this straight to governor's
//!   `per_second`, which is a PERIOD in seconds — so "10" meant one request
//!   per ten seconds, not ten per second. Fixed here.
//! - `GAMMA_RATE_LIMIT_BURST`          — edge burst size (default 100).
//! - `GAMMA_RATE_LIMIT_AUTH_BURST`     — auth bucket burst (default 5).
//! - `GAMMA_RATE_LIMIT_AUTH_REFILL_SECS` — auth bucket refill period in seconds
//!   (default 2: one attempt per 2s sustained).
//! - `GAMMA_TRUST_PROXY=true`          — key on X-Forwarded-For (ONLY behind a
//!   trusted proxy; otherwise clients could spoof their way past the limit).

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::Router;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::SmartIpKeyExtractor;
use tower_governor::{GovernorError, GovernorLayer};

use crate::error::ApiError;
use crate::util::env_parsed;

/// Whether rate limiting is globally disabled (local dev / in-process tests).
pub fn disabled() -> bool {
    std::env::var("GAMMA_RATE_LIMIT_DISABLED").as_deref() == Ok("true")
}

/// Whether to key on `X-Forwarded-For` (trusted reverse proxy) instead of the
/// peer socket IP. One shared switch, so the two buckets can never disagree
/// about who "the client" is.
fn trust_proxy() -> bool {
    std::env::var("GAMMA_TRUST_PROXY").as_deref() == Ok("true")
}

/// Config for the tight `/v1/auth/*` bucket.
#[derive(Clone, Copy, Debug)]
pub struct AuthRateLimit {
    pub burst: u32,
    pub refill_secs: u64,
    pub trust_proxy: bool,
}

impl AuthRateLimit {
    /// Read the auth-bucket config from the environment, or `None` when rate
    /// limiting is disabled.
    pub fn from_env() -> Option<Self> {
        if disabled() {
            return None;
        }
        Some(Self {
            burst: env_parsed("GAMMA_RATE_LIMIT_AUTH_BURST", 5),
            refill_secs: env_parsed("GAMMA_RATE_LIMIT_AUTH_REFILL_SECS", 2),
            trust_proxy: trust_proxy(),
        })
    }
}

/// Map governor rejections to the SAME JSON shape as `ApiError`, so a client
/// sees one 429 format (`{"error":"rate_limited"}` + `Retry-After`) whether the
/// bucket or the login throttle fired.
fn governor_error(err: GovernorError) -> Response {
    match err {
        GovernorError::TooManyRequests { wait_time, .. } => ApiError::TooManyRequests {
            retry_after_secs: wait_time,
        }
        .into_response(),
        GovernorError::UnableToExtractKey => {
            ApiError::Internal("rate limiter could not extract a client key".into()).into_response()
        }
        GovernorError::Other { msg, .. } => {
            ApiError::Internal(format!("rate limiter failure: {msg:?}")).into_response()
        }
    }
}

/// Wrap a sub-router in the tight auth bucket. `route_layer` (not `layer`) so
/// unmatched paths don't consume budget.
pub fn auth_layer<S>(router: Router<S>, cfg: AuthRateLimit) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    if cfg.trust_proxy {
        let governor = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(cfg.refill_secs)
                .burst_size(cfg.burst)
                .key_extractor(SmartIpKeyExtractor)
                .error_handler(governor_error)
                .finish()
                .expect("valid auth rate-limit config"),
        );
        router.route_layer(GovernorLayer { config: governor })
    } else {
        let governor = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(cfg.refill_secs)
                .burst_size(cfg.burst)
                .error_handler(governor_error)
                .finish()
                .expect("valid auth rate-limit config"),
        );
        router.route_layer(GovernorLayer { config: governor })
    }
}

/// Wrap the WHOLE service in the loose edge backstop (applied in the binary,
/// after `app()`, so the in-process test router stays un-limited). Sustained
/// rate is requests/second (converted to governor's period-in-milliseconds),
/// clamped to at least 1ms per token.
pub fn edge_layer(service: Router, trust_proxy_flag: bool) -> Router {
    let per_second: u64 = env_parsed("GAMMA_RATE_LIMIT_PER_SECOND", 20);
    let burst: u32 = env_parsed("GAMMA_RATE_LIMIT_BURST", 100);
    assert!(
        per_second > 0,
        "GAMMA_RATE_LIMIT_PER_SECOND must be at least 1"
    );
    let period_ms = (1000 / per_second).max(1);

    if trust_proxy_flag {
        let governor = Arc::new(
            GovernorConfigBuilder::default()
                .per_millisecond(period_ms)
                .burst_size(burst)
                .key_extractor(SmartIpKeyExtractor)
                .error_handler(governor_error)
                .finish()
                .expect("valid edge rate-limit config"),
        );
        tracing::info!(
            per_second,
            burst,
            "edge per-IP rate limit enabled (trusting X-Forwarded-For)"
        );
        service.layer(GovernorLayer { config: governor })
    } else {
        let governor = Arc::new(
            GovernorConfigBuilder::default()
                .per_millisecond(period_ms)
                .burst_size(burst)
                .error_handler(governor_error)
                .finish()
                .expect("valid edge rate-limit config"),
        );
        tracing::info!(
            per_second,
            burst,
            "edge per-IP rate limit enabled (peer IP)"
        );
        service.layer(GovernorLayer { config: governor })
    }
}

/// Apply the edge backstop unless disabled. The binary's single entry point.
pub fn edge_from_env(service: Router) -> Router {
    if disabled() {
        tracing::warn!("per-IP rate limit DISABLED (GAMMA_RATE_LIMIT_DISABLED=true)");
        service
    } else {
        edge_layer(service, trust_proxy())
    }
}
