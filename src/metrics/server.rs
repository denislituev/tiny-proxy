//! Prometheus HTTP server setup.

use std::net::SocketAddr;

#[cfg(feature = "metrics")]
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder};

/// Latency histogram buckets (seconds). Chosen for proxy workloads:
/// sub-millisecond fast paths up to slow upstream timeouts.
#[cfg(feature = "metrics")]
const LATENCY_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Start the Prometheus metrics HTTP server on the given address.
///
/// Installs a global metrics recorder, registers metric descriptions (`# HELP`),
/// configures latency histogram buckets, and spawns an HTTP listener
/// that serves `/metrics` in Prometheus text format.
#[cfg(feature = "metrics")]
pub fn start_metrics_server(addr: SocketAddr) -> anyhow::Result<()> {
    use metrics::{describe_counter, describe_gauge, describe_histogram};

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_request_duration_seconds".to_string()),
            LATENCY_BUCKETS,
        )?
        .with_http_listener(addr)
        .install()?;

    describe_counter!(
        "http_requests_total",
        "Total number of HTTP requests processed"
    );
    describe_histogram!(
        "http_request_duration_seconds",
        "HTTP request processing duration in seconds"
    );
    describe_gauge!(
        "http_active_requests",
        "Number of HTTP requests currently in flight"
    );
    describe_counter!(
        "tls_handshakes_total",
        "Total number of TLS handshakes (labelled by outcome)"
    );

    tracing::info!("Metrics server listening on http://{}/metrics", addr);
    Ok(())
}

/// No-op when `metrics` feature is disabled.
#[cfg(not(feature = "metrics"))]
pub fn start_metrics_server(_addr: SocketAddr) -> anyhow::Result<()> {
    Ok(())
}
