//! Binary bootstrap: load env, open the pool, apply migrations, serve.
//!
//! All logic lives in the library (`lib.rs`) so integration tests can drive the
//! same router in-process. This file only wires the process to the outside world.

use std::net::SocketAddr;
use std::sync::Arc;

use core_api::{app, AppState};
use tower_governor::key_extractor::SmartIpKeyExtractor;
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

/// Parse an env var as `T`, or fall back to `default` if it is unset. A PRESENT but
/// unparseable value is a misconfiguration and panics at startup — better than
/// silently falling back to a default the operator didn't intend.
fn env_parsed<T: std::str::FromStr>(name: &str, default: T) -> T {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .unwrap_or_else(|_| panic!("{name} is set but not a valid value: {v:?}")),
        Err(_) => default,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present; real environments set these vars directly.
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Install the Prometheus recorder so `/metrics` has data to render. Done in the
    // binary (not in `app()`) so the in-process test router never tries to install a
    // second global recorder.
    core_api::install_prometheus();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set (see .env.example)"))?;

    let pool_size: u32 = env_parsed("GAMMA_DB_POOL_SIZE", 10);
    let pool = db::connect(&database_url, pool_size).await?;
    db::run_migrations(&pool).await?;
    tracing::info!("database connected and migrations applied");

    let state = AppState::new(pool);
    state.storage.ensure_bucket().await?;
    tracing::info!("object storage bucket ready");

    // Per-IP rate limit, applied at the server edge only (not inside `app()`, so the
    // in-process test router is unaffected). Blunts login/argon2 brute-force and
    // request flooding; keyed on the peer IP (why we serve with connect-info below).
    //
    // Env-configurable: the typed SPA fires several requests per page (doubled again
    // by React StrictMode in dev), so a tight global limit throttles normal use —
    // set GAMMA_RATE_LIMIT_DISABLED=true locally. NOTE: this is a coarse GLOBAL limit;
    // per-route limits (tight on /auth, loose on reads) are the proper follow-up, at
    // which point the default here can be raised for production.
    let mut service = app(state);
    if std::env::var("GAMMA_RATE_LIMIT_DISABLED").as_deref() == Ok("true") {
        tracing::warn!("per-IP rate limit DISABLED (GAMMA_RATE_LIMIT_DISABLED=true)");
    } else {
        // A present-but-invalid value now fails loudly (env_parsed) instead of
        // silently reverting to the default.
        let per_second: u64 = env_parsed("GAMMA_RATE_LIMIT_PER_SECOND", 10);
        let burst: u32 = env_parsed("GAMMA_RATE_LIMIT_BURST", 50);
        // Behind a reverse proxy every request's PEER ip is the proxy's, so the
        // default PeerIpKeyExtractor collapses the per-IP limit into a single global
        // bucket. Set GAMMA_TRUST_PROXY=true ONLY when a trusted proxy sets
        // X-Forwarded-For (otherwise a client can spoof it to dodge the limit), and
        // the SmartIpKeyExtractor keys on the forwarded client IP instead.
        if std::env::var("GAMMA_TRUST_PROXY").as_deref() == Ok("true") {
            let governor = Arc::new(
                GovernorConfigBuilder::default()
                    .per_second(per_second)
                    .burst_size(burst)
                    .key_extractor(SmartIpKeyExtractor)
                    .finish()
                    .expect("valid rate-limit config"),
            );
            service = service.layer(GovernorLayer { config: governor });
            tracing::info!(
                per_second,
                burst,
                "per-IP rate limit enabled (trusting X-Forwarded-For)"
            );
        } else {
            let governor = Arc::new(
                GovernorConfigBuilder::default()
                    .per_second(per_second)
                    .burst_size(burst)
                    .finish()
                    .expect("valid rate-limit config"),
            );
            service = service.layer(GovernorLayer { config: governor });
            tracing::info!(per_second, burst, "per-IP rate limit enabled (peer IP)");
        }
    }

    let bind = std::env::var("CORE_API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("core-api listening on http://{bind}");
    axum::serve(
        listener,
        service.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
