//! Prometheus metrics for tiny-proxy.
//!
//! Opt-in via feature flag `metrics`. When disabled, all calls are no-ops.
//!
//! # Metrics exposed
//!
//! | Metric | Type | Labels |
//! |--------|------|--------|
//! | `http_requests_total` | counter | `method`, `status`, `site` |
//! | `http_request_duration_seconds` | histogram | `method`, `status` |
//! | `http_active_requests` | gauge | (none) |
//! | `tls_handshakes_total` | counter | `status` (`ok` / `fail`) |
//!
//! # Usage
//!
//! ```bash
//! cargo run --features metrics -- --config config.conf --metrics-addr 127.0.0.1:9090
//! curl http://127.0.0.1:9090/metrics
//! ```

mod recorder;
mod server;

pub use recorder::{record_request, tls_handshake, MetricsGuard};
pub use server::start_metrics_server;
