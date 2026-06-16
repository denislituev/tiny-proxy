//! Request/TLS recording helpers and the in-flight request guard.

/// Record a completed HTTP request.
#[cfg(feature = "metrics")]
pub fn record_request(method: &str, status: u16, site: &str, duration: std::time::Duration) {
    use metrics::{counter, histogram};

    counter!(
        "http_requests_total",
        "method" => method.to_owned(),
        "status" => status.to_string(),
        "site" => site.to_owned(),
    )
    .increment(1);

    histogram!(
        "http_request_duration_seconds",
        "method" => method.to_owned(),
        "status" => status.to_string(),
    )
    .record(duration.as_secs_f64());
}

/// No-op when `metrics` feature is disabled.
#[cfg(not(feature = "metrics"))]
pub fn record_request(_method: &str, _status: u16, _site: &str, _duration: std::time::Duration) {}

/// RAII guard that tracks one in-flight request.
///
/// On creation, increments the `http_active_requests` gauge.
/// Call `.record(status)` to record the request counter + latency histogram.
/// On drop, decrements the gauge.
pub struct MetricsGuard {
    #[cfg(feature = "metrics")]
    method: String,
    #[cfg(feature = "metrics")]
    site: String,
    #[cfg(feature = "metrics")]
    start: std::time::Instant,
}

impl MetricsGuard {
    pub fn new(_method: String, _site: String) -> Self {
        #[cfg(feature = "metrics")]
        {
            metrics::gauge!("http_active_requests").increment(1.0);
        }
        Self {
            #[cfg(feature = "metrics")]
            method: _method,
            #[cfg(feature = "metrics")]
            site: _site,
            #[cfg(feature = "metrics")]
            start: std::time::Instant::now(),
        }
    }

    /// Record the request counter + duration histogram for this request.
    pub fn record(&mut self, status: u16) {
        #[cfg(feature = "metrics")]
        {
            record_request(&self.method, status, &self.site, self.start.elapsed());
        }
        #[cfg(not(feature = "metrics"))]
        let _ = status;
    }
}

#[cfg(feature = "metrics")]
impl Drop for MetricsGuard {
    fn drop(&mut self) {
        metrics::gauge!("http_active_requests").decrement(1.0);
    }
}

/// Record a TLS handshake result.
#[cfg(feature = "metrics")]
pub fn tls_handshake(status: &str) {
    metrics::counter!("tls_handshakes_total", "status" => status.to_owned()).increment(1);
}

/// No-op when `metrics` feature is disabled.
#[cfg(not(feature = "metrics"))]
pub fn tls_handshake(_status: &str) {}
