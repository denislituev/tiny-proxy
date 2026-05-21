use hyper::Request;

#[cfg(feature = "logging")]
use std::time::Instant;
#[cfg(feature = "logging")]
use tracing::info;

/// Generate or extract a request ID for tracing.
///
/// If the incoming request already has an `X-Request-ID` header, reuse it.
/// Otherwise, generate a new UUID v4 and inject it into the request.
pub fn ensure_request_id<B>(req: &mut Request<B>) -> String {
    use hyper::header::HeaderValue;

    if let Some(existing) = req.headers().get("X-Request-ID").cloned() {
        if let Ok(id) = existing.to_str() {
            return id.to_string();
        }
    }

    // Generate new UUID v4
    let id = uuid::Uuid::new_v4().to_string();
    if let Ok(val) = HeaderValue::from_str(&id) {
        req.headers_mut().insert("X-Request-ID", val);
    }
    id
}

/// Read the final request ID from headers after directive processing.
///
/// This resolves the conflict where a `header X-Request-ID {uuid}` directive
/// may have overwritten the initially generated ID.
pub fn final_request_id<B>(req: &Request<B>, fallback: &str) -> String {
    req.headers()
        .get("X-Request-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(fallback)
        .to_string()
}

/// Write a structured access log entry via tracing.
///
/// Produces a structured log line with all request metadata:
/// ```text
/// access_log req_id=a1b2c3 remote=127.0.0.1 method=GET path=/api/users host=localhost:8080 status=200 duration_ms=1.23 bytes_sent=1234
/// ```
/// # Semantics
///
/// - **`duration_ms`**: Measures time from request entry to response headers ready
///   (i.e., TTFB — Time To First Byte). For reverse proxy, this includes the
///   backend round-trip but **not** the streaming of the response body to the client.
///   For error responses and direct responses, it effectively covers the full handling time.
///   End-to-end connection duration (including full body transfer) is not currently tracked.
///
/// - **`bytes_sent`**: `None` (`"-"` in log) for streaming responses (reverse proxy),
///   `Some(n)` for direct responses where the body size is known exactly.
#[cfg(feature = "logging")]
#[allow(clippy::too_many_arguments)]
pub fn log_access(
    request_id: &str,
    remote_addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    host: &str,
    status: u16,
    duration_ms: f64,
    bytes_sent: Option<usize>,
) {
    info!(
        req_id = %request_id,
        remote = %remote_addr.ip(),
        method = %method,
        path = %path,
        host = %host,
        status = status,
        duration_ms = format_args!("{:.2}", duration_ms),
        bytes_sent = bytes_sent.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string()),
        "access_log"
    );
}

/// RAII guard for access logging.
///
/// Captures request metadata on creation and writes the access log on drop.
/// If `finish()` was called, uses the provided status. Otherwise logs 500
/// (covers panics and unexpected error paths).
///
/// Only available when the `logging` feature is enabled.
///
/// # Usage
/// ```ignore
/// let mut guard = AccessLogGuard::new(request_id, remote_addr, method, path, host);
/// // ... handle request ...
/// guard.finish(200);
/// guard.set_bytes_sent(body.len());
/// // log is written when guard is dropped
/// ```
#[cfg(feature = "logging")]
pub struct AccessLogGuard {
    request_id: String,
    remote_addr: std::net::SocketAddr,
    method: String,
    path: String,
    host: String,
    start: Instant,
    status: Option<u16>,
    bytes_sent: Option<usize>,
}

#[cfg(feature = "logging")]
impl AccessLogGuard {
    pub fn new(
        request_id: String,
        remote_addr: std::net::SocketAddr,
        method: String,
        path: String,
        host: String,
    ) -> Self {
        Self {
            request_id,
            remote_addr,
            method,
            path,
            host,
            start: Instant::now(),
            status: None,
            bytes_sent: None,
        }
    }

    /// Set the final HTTP status code. Will be used in the access log.
    pub fn finish(&mut self, status: u16) {
        self.status = Some(status);
    }

    /// Set the number of bytes sent in the response body.
    /// Use for direct responses (Respond, Redirect, errors) where body size is known.
    /// For streaming proxy responses, leave as None (logged as `-`).
    pub fn set_bytes_sent(&mut self, bytes: usize) {
        self.bytes_sent = Some(bytes);
    }

    /// Get the current request ID.
    ///
    /// Note: handler.rs tracks request_id as a local variable for use in responses,
    /// so this accessor is not called internally. Kept for external API completeness.
    #[allow(dead_code)]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Update the request ID (e.g., after directive processing changed X-Request-ID).
    pub fn set_request_id(&mut self, id: String) {
        self.request_id = id;
    }
}

#[cfg(feature = "logging")]
impl Drop for AccessLogGuard {
    fn drop(&mut self) {
        let status = self.status.unwrap_or(500);
        let duration_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        log_access(
            &self.request_id,
            self.remote_addr,
            &self.method,
            &self.path,
            &self.host,
            status,
            duration_ms,
            self.bytes_sent,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Empty;

    fn make_request() -> Request<Empty<Bytes>> {
        Request::builder()
            .method("GET")
            .uri("/test")
            .body(Empty::new())
            .unwrap()
    }

    #[test]
    fn test_ensure_request_id_generates_when_missing() {
        let mut req = make_request();
        assert!(req.headers().get("X-Request-ID").is_none());

        let id = ensure_request_id(&mut req);

        assert!(!id.is_empty());
        assert_eq!(id.len(), 36, "Should be a UUID");
        assert!(req.headers().get("X-Request-ID").is_some());
    }

    #[test]
    fn test_ensure_request_id_reuses_existing() {
        let mut req = Request::builder()
            .header("X-Request-ID", "my-custom-id")
            .body(Empty::<Bytes>::new())
            .unwrap();

        let id = ensure_request_id(&mut req);

        assert_eq!(id, "my-custom-id");
    }

    #[test]
    fn test_final_request_id_after_directive_change() {
        let mut req = make_request();
        let original_id = ensure_request_id(&mut req);

        // Simulate a directive changing the header
        req.headers_mut().insert(
            "X-Request-ID",
            hyper::header::HeaderValue::from_static("directive-id"),
        );

        let final_id = final_request_id(&req, &original_id);
        assert_eq!(final_id, "directive-id");
    }

    #[test]
    fn test_final_request_id_fallback_when_no_header() {
        let req = make_request();
        let final_id = final_request_id(&req, "fallback-id");
        assert_eq!(final_id, "fallback-id");
    }

    #[cfg(feature = "logging")]
    mod logging_tests {
        use super::*;

        #[test]
        fn test_log_access_does_not_panic() {
            let addr: std::net::SocketAddr = "127.0.0.1:54321".parse().unwrap();
            log_access(
                "abc123",
                addr,
                "GET",
                "/api/users",
                "localhost:8080",
                200,
                1.23,
                Some(1234),
            );
        }

        #[test]
        fn test_log_access_streaming_response() {
            let addr: std::net::SocketAddr = "127.0.0.1:54321".parse().unwrap();
            // bytes_sent = None for streaming (reverse proxy)
            log_access(
                "abc123",
                addr,
                "GET",
                "/stream",
                "localhost:8080",
                200,
                50.5,
                None,
            );
        }

        #[test]
        fn test_log_access_error_status() {
            let addr: std::net::SocketAddr = "10.0.0.1:12345".parse().unwrap();
            log_access(
                "def456",
                addr,
                "POST",
                "/api/orders",
                "api.example.com",
                502,
                30001.5,
                Some(0),
            );
        }

        #[test]
        fn test_access_log_guard_finish() {
            let addr: std::net::SocketAddr = "127.0.0.1:12345".parse().unwrap();
            let mut guard = AccessLogGuard::new(
                "test-id".to_string(),
                addr,
                "GET".to_string(),
                "/test".to_string(),
                "localhost".to_string(),
            );
            guard.finish(200);
            guard.set_bytes_sent(1024);
            // Drop will log — just verify no panic
        }

        #[test]
        fn test_access_log_guard_default_500() {
            let addr: std::net::SocketAddr = "127.0.0.1:12345".parse().unwrap();
            let guard = AccessLogGuard::new(
                "test-id".to_string(),
                addr,
                "GET".to_string(),
                "/test".to_string(),
                "localhost".to_string(),
            );
            // No finish() called — drop logs 500 with bytes_sent=None
            drop(guard);
        }

        #[test]
        fn test_access_log_guard_set_request_id() {
            let addr: std::net::SocketAddr = "127.0.0.1:12345".parse().unwrap();
            let mut guard = AccessLogGuard::new(
                "old-id".to_string(),
                addr,
                "GET".to_string(),
                "/test".to_string(),
                "localhost".to_string(),
            );
            assert_eq!(guard.request_id(), "old-id");
            guard.set_request_id("new-id".to_string());
            assert_eq!(guard.request_id(), "new-id");
            guard.finish(200);
        }

        #[test]
        fn test_access_log_guard_bytes_sent() {
            let addr: std::net::SocketAddr = "127.0.0.1:12345".parse().unwrap();
            let mut guard = AccessLogGuard::new(
                "test-id".to_string(),
                addr,
                "POST".to_string(),
                "/submit".to_string(),
                "localhost".to_string(),
            );
            guard.finish(201);
            guard.set_bytes_sent(42);
            // Drop logs with bytes_sent=Some(42)
        }
    }
}
