//! Prometheus metrics: a process-global recorder, the `/metrics` scrape endpoint,
//! and a middleware that counts HTTP requests and their latency.
//!
//! The recorder is installed exactly once via a `OnceLock` — the binary calls
//! [`install_prometheus`] at startup, while the in-process test router never does.
//! That matters because a global metrics recorder can only be installed once per
//! process, and the integration tests build many routers in one process; recording
//! macros are no-ops until a recorder exists, so the untouched test path is safe.

use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the global Prometheus recorder once and return the render handle.
/// Idempotent: the first call installs the recorder; later calls reuse it. The
/// binary calls this at startup; tests don't, so they record into the default
/// no-op recorder.
pub fn install_prometheus() -> &'static PrometheusHandle {
    METRICS_HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .set_buckets(&[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0])
            .expect("static bucket list is non-empty")
            .install_recorder()
            .expect("no global metrics recorder installed yet")
    })
}

/// Render the current metrics in Prometheus text format. Empty until
/// [`install_prometheus`] has run (e.g. an in-process test that didn't install one).
pub async fn metrics_handler() -> String {
    METRICS_HANDLE
        .get()
        .map(PrometheusHandle::render)
        .unwrap_or_default()
}

/// Count every HTTP request and record its latency, labelled by method and status.
/// Deliberately NOT labelled by path: the matched route would inflate cardinality,
/// so method × status keeps the series count bounded. A no-op until a recorder is
/// installed (so it's transparent in tests).
pub async fn track_metrics(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().as_str().to_owned();

    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    metrics::counter!("http_requests_total", "method" => method.clone(), "status" => status)
        .increment(1);
    metrics::histogram!("http_request_duration_seconds", "method" => method)
        .record(start.elapsed().as_secs_f64());
    response
}
