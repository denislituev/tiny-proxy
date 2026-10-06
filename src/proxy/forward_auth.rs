//! Forward-auth middleware: authorization subrequests before a request may
//! continue to its backend.
//!
//! `forward_auth` is a middleware, not a terminal handler. When the directive
//! pipeline reaches a [`crate::config::Directive::ForwardAuth`] entry, the proxy
//! sends a `GET` subrequest (no client body) to the configured auth endpoint.
//! The original request continues only on a 2xx decision; explicit 401/403
//! denials are returned to the client; infrastructure failures (connect
//! error, timeout, 5xx, malformed response) respect the configured
//! [`FailureMode`](crate::config::FailureMode).
//!
//! # Decision vs infrastructure failure
//!
//! ```text
//! AUTH DECISION (never bypassed by failure_mode open)
//!   2xx          -> allow (only with a fully drained response body)
//!   401 / 403    -> deny (status + WWW-Authenticate preserved)
//!   other 4xx    -> deny (auth service status preserved)
//!
//! AUTH INFRASTRUCTURE FAILURE (subject to failure_mode)
//!   connect error / timeout / DNS / 5xx / malformed /
//!   2xx whose body stream broke mid-flight
//!     closed -> 503 to the client, backend NOT contacted
//!     open   -> continue WITHOUT identity headers
//! ```
//!
//! # Identity-header spoofing protection
//!
//! Configured `response_headers` are scrubbed from the client request **before**
//! the subrequest runs, so a client can never inject trusted identity headers —
//! not even when `failure_mode open` lets a request through without auth.

use std::net::SocketAddr;
use std::time::Duration;

use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::{HeaderValue, WWW_AUTHENTICATE};
use hyper::{HeaderMap, Method, Request, StatusCode, Version};
use hyper_util::client::legacy::Error as ClientError;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::config::ForwardAuthConfig;
use crate::metrics::record_auth;
use crate::proxy::types::{ProxyClient, ProxyRequestBody};

/// Per-request context needed to execute a forward-auth subrequest.
pub struct AuthCtx<'a> {
    /// Shared pooled client — the same instance used for backend requests.
    pub client: &'a ProxyClient,
    /// The client's original request target (path + query) as received by
    /// tiny-proxy, captured before any path rewriting (`handle_path`,
    /// `strip_prefix`, `uri_replace`). Sent as `X-Original-URI`.
    pub original_uri: &'a str,
    /// Client socket address, used for `X-Forwarded-For` on the subrequest.
    pub remote_addr: SocketAddr,
    /// Whether the client connected over TLS, used for `X-Forwarded-Proto`.
    pub is_tls: bool,
}

/// Outcome of a forward-auth subrequest.
#[derive(Debug)]
pub enum AuthResult {
    /// 2xx from the auth service. `identity` carries the configured
    /// response headers extracted from the auth response (trusted values).
    Allowed { identity: HeaderMap },
    /// Explicit denial (401/403/other 4xx). The status is preserved from the
    /// auth service decision.
    Denied {
        status: StatusCode,
        www_authenticate: Option<HeaderValue>,
    },
    /// Infrastructure failure — the auth service produced no valid decision.
    Failed { reason: AuthFailure },
}

/// Classification of forward-auth infrastructure failures.
#[derive(Debug)]
pub enum AuthFailure {
    Connect(String),
    Timeout,
    ServerError(StatusCode),
    Malformed(String),
}

/// Remove client-supplied values for the configured identity headers.
///
/// Runs **before** the subrequest so that spoofed values can never reach the
/// backend — including when `failure_mode open` continues without auth or when
/// the auth response omits a configured header.
pub fn scrub_identity_headers<B>(req: &mut Request<B>, cfg: &ForwardAuthConfig) {
    for name in &cfg.response_headers {
        if req.headers_mut().remove(name).is_some() {
            debug!("forward_auth: scrubbed client-supplied identity header {name}");
        }
    }
}

/// Copy trusted identity headers from the auth response onto the request
/// sent to the backend. Only headers explicitly listed in `response_headers`
/// are ever present in `identity`.
pub fn apply_identity_headers<B>(req: &mut Request<B>, identity: &HeaderMap) {
    for (name, value) in identity {
        req.headers_mut().insert(name.clone(), value.clone());
        debug!("forward_auth: applied identity header {name}");
    }
}

/// Empty request body for the auth subrequest. The client's request body is
/// never consumed by forward auth.
pub(crate) fn empty_request_body() -> ProxyRequestBody {
    use http_body_util::{BodyExt, Empty};
    // Empty's error type is Infallible; convert to the unified boxed error type.
    Empty::<bytes::Bytes>::new().map_err(|e| match e {}).boxed()
}

/// Box the client's streaming request body into the unified client body type.
pub(crate) fn box_incoming(body: Incoming) -> ProxyRequestBody {
    use http_body_util::BodyExt;
    body.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .boxed()
}

/// Build the authorization subrequest.
///
/// The subrequest is `GET <endpoint>` with:
/// - the whitelisted `request_headers` copied from the client request;
/// - `X-Original-URI` / `X-Original-Method` describing the client request;
/// - `X-Forwarded-For` / `X-Forwarded-Host` / `X-Forwarded-Proto` matching the
///   forwarding conventions used for backend requests (`X-Forwarded-For` is the
///   immediate peer's IP, not a forwarded client chain);
/// - the request's `X-Request-ID` propagated for tracing correlation;
/// - no body (the client's body is left untouched for the backend).
fn build_auth_request<B>(
    original_uri: &str,
    remote_addr: SocketAddr,
    is_tls: bool,
    req: &Request<B>,
    cfg: &ForwardAuthConfig,
) -> Result<Request<ProxyRequestBody>, String> {
    let mut headers = HeaderMap::new();

    // Whitelisted client headers only — never a blind copy.
    for name in &cfg.request_headers {
        if let Some(value) = req.headers().get(name) {
            headers.insert(name.clone(), value.clone());
        }
    }

    // Host comes from the (validated) endpoint authority. It is a system
    // header: inserted after the whitelist copy so the endpoint always wins
    // over a client-supplied Host.
    if let Some(authority) = cfg.endpoint.authority() {
        if let Ok(host) = HeaderValue::from_str(authority.as_str()) {
            headers.insert(hyper::header::HOST, host);
        }
    }

    let put = |headers: &mut HeaderMap, name: &'static str, value: &str| {
        if let Ok(v) = HeaderValue::from_str(value) {
            headers.insert(name, v);
        } else {
            debug!("forward_auth: skipping invalid {name} value");
        }
    };
    put(&mut headers, "x-original-uri", original_uri);
    put(&mut headers, "x-original-method", req.method().as_str());
    put(
        &mut headers,
        "x-forwarded-for",
        &remote_addr.ip().to_string(),
    );
    if let Some(host) = req.headers().get(hyper::header::HOST) {
        headers.insert("x-forwarded-host", host.clone());
    }
    put(
        &mut headers,
        "x-forwarded-proto",
        if is_tls { "https" } else { "http" },
    );
    if let Some(id) = req.headers().get("x-request-id") {
        headers.insert("x-request-id", id.clone());
    }

    let mut auth_req = Request::builder()
        .method(Method::GET)
        .uri(cfg.endpoint.clone())
        .version(Version::HTTP_11)
        .body(empty_request_body())
        .map_err(|e| format!("failed to build auth request: {e}"))?;
    *auth_req.headers_mut() = headers;

    Ok(auth_req)
}

/// Execute the forward-auth subrequest and classify its outcome.
///
/// This function is responsible for authorization only — it never proxies the
/// request and never mutates `req` (the caller scrubs/applies identity headers
/// around it).
pub async fn resolve_auth(
    ctx: &AuthCtx<'_>,
    req: &Request<Incoming>,
    cfg: &ForwardAuthConfig,
) -> AuthResult {
    let started = std::time::Instant::now();

    let auth_req = match build_auth_request(ctx.original_uri, ctx.remote_addr, ctx.is_tls, req, cfg)
    {
        Ok(r) => r,
        Err(e) => {
            // Invalid metadata header values (e.g. non-ASCII original URI) —
            // fail closed/open per configuration, never spoof.
            record_auth("error", started.elapsed());
            return AuthResult::Failed {
                reason: AuthFailure::Malformed(e),
            };
        }
    };

    info!(
        "   forward_auth: GET {} (timeout: {}s, failure_mode: {:?})",
        cfg.endpoint, cfg.timeout_secs, cfg.failure_mode
    );

    let fut = async {
        let response = ctx.client.request(auth_req).await?;
        let status = response.status();
        let www_authenticate = response.headers().get(WWW_AUTHENTICATE).cloned();

        let mut identity = HeaderMap::new();
        if status.is_success() {
            for name in &cfg.response_headers {
                if let Some(value) = response.headers().get(name) {
                    identity.insert(name.clone(), value.clone());
                }
            }
        }

        // Drain the (auth-service-controlled, typically empty) body frame by
        // frame — no buffering — so the pooled connection can be reused;
        // bounded by the overall timeout. A body stream that breaks mid-flight
        // is captured here and classified below.
        let mut body_err: Option<String> = None;
        let mut body = response.into_body();
        loop {
            match body.frame().await {
                Some(Ok(_)) => continue,
                Some(Err(e)) => {
                    body_err = Some(e.to_string());
                    break;
                }
                None => break,
            }
        }

        Ok::<_, ClientError>((status, www_authenticate, identity, body_err))
    };

    let outcome = match timeout(Duration::from_secs(cfg.timeout_secs), fut).await {
        Ok(Ok((status, www_authenticate, identity, body_err))) => {
            if status.is_success() {
                // A 2xx whose body stream broke is not a reliable decision —
                // treat it as an infrastructure failure (failure_mode applies).
                if let Some(e) = body_err {
                    AuthResult::Failed {
                        reason: AuthFailure::Malformed(e),
                    }
                } else {
                    AuthResult::Allowed { identity }
                }
            } else if status.is_client_error() {
                AuthResult::Denied {
                    status,
                    www_authenticate,
                }
            } else {
                AuthResult::Failed {
                    reason: AuthFailure::ServerError(status),
                }
            }
        }
        Ok(Err(e)) => {
            let reason = if e.is_connect() {
                AuthFailure::Connect(e.to_string())
            } else {
                AuthFailure::Malformed(e.to_string())
            };
            AuthResult::Failed { reason }
        }
        Err(_elapsed) => AuthResult::Failed {
            reason: AuthFailure::Timeout,
        },
    };

    match &outcome {
        AuthResult::Allowed { .. } => {
            record_auth("allowed", started.elapsed());
            info!("   forward_auth: allowed");
        }
        AuthResult::Denied { status, .. } => {
            record_auth("denied", started.elapsed());
            info!("   forward_auth: denied with {}", status.as_u16());
        }
        AuthResult::Failed { reason } => {
            record_auth("error", started.elapsed());
            warn!("   forward_auth: auth service failure ({:?})", reason);
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Empty;
    use hyper::Request;

    fn remote_addr() -> SocketAddr {
        "203.0.113.7:42000".parse().unwrap()
    }

    fn make_config() -> ForwardAuthConfig {
        ForwardAuthConfig {
            endpoint: "http://auth.internal:8080/verify".parse().unwrap(),
            request_headers: vec!["authorization".parse().unwrap()],
            response_headers: vec!["x-user".parse().unwrap()],
            failure_mode: crate::config::FailureMode::Closed,
            timeout_secs: 5,
        }
    }

    #[test]
    fn test_build_auth_request_metadata() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/users?id=123")
            .header("Authorization", "Bearer tok")
            .header("X-Request-ID", "req-42")
            .header("Host", "example.com:8080")
            .header("X-Internal-Secret", "leak-me")
            .body(Empty::<Bytes>::new())
            .unwrap();

        let cfg = make_config();

        let auth_req =
            build_auth_request("/api/users?id=123", remote_addr(), false, &req, &cfg).unwrap();

        assert_eq!(*auth_req.method(), Method::GET);
        assert_eq!(auth_req.uri(), &cfg.endpoint);
        assert_eq!(
            auth_req.headers().get("x-original-uri").unwrap(),
            "/api/users?id=123"
        );
        assert_eq!(auth_req.headers().get("x-original-method").unwrap(), "POST");
        assert_eq!(
            auth_req.headers().get("authorization").unwrap(),
            "Bearer tok"
        );
        assert_eq!(auth_req.headers().get("x-request-id").unwrap(), "req-42");
        assert_eq!(
            auth_req.headers().get("host").unwrap(),
            "auth.internal:8080"
        );
        assert_eq!(
            auth_req.headers().get("x-forwarded-host").unwrap(),
            "example.com:8080"
        );
        assert_eq!(
            auth_req.headers().get("x-forwarded-for").unwrap(),
            "203.0.113.7"
        );
        assert_eq!(auth_req.headers().get("x-forwarded-proto").unwrap(), "http");

        // Non-whitelisted client headers must NOT reach the auth service.
        assert!(auth_req.headers().get("x-internal-secret").is_none());
    }

    #[test]
    fn test_build_auth_request_case_insensitive_copy() {
        // Config header name is lowercase; client sends mixed case.
        let req = Request::builder()
            .method("GET")
            .uri("/")
            .header("aUtHoRiZaTiOn", "Bearer mixed-case")
            .body(Empty::<Bytes>::new())
            .unwrap();

        let auth_req = build_auth_request("/", remote_addr(), false, &req, &make_config()).unwrap();

        assert_eq!(
            auth_req.headers().get("Authorization").unwrap(),
            "Bearer mixed-case"
        );
    }

    #[test]
    fn test_build_auth_request_missing_headers_ok() {
        let req = Request::builder()
            .method("GET")
            .uri("/")
            .body(Empty::<Bytes>::new())
            .unwrap();

        let auth_req = build_auth_request("/", remote_addr(), false, &req, &make_config()).unwrap();

        // Whitelisted header absent from the client request: simply not copied.
        assert!(auth_req.headers().get("authorization").is_none());
        assert!(auth_req.headers().get("x-forwarded-host").is_none());
    }

    #[test]
    fn test_scrub_and_apply_identity_headers() {
        let cfg = make_config();
        let mut req = Request::builder()
            .header("x-user", "attacker")
            .header("x-keep", "kept")
            .body(Empty::<Bytes>::new())
            .unwrap();

        scrub_identity_headers(&mut req, &cfg);
        assert!(req.headers().get("x-user").is_none());
        assert_eq!(req.headers().get("x-keep").unwrap(), "kept");

        let mut identity = HeaderMap::new();
        identity.insert("x-user", HeaderValue::from_static("real-user"));
        apply_identity_headers(&mut req, &identity);
        assert_eq!(req.headers().get("x-user").unwrap(), "real-user");
    }

    #[test]
    fn test_host_header_is_system_controlled() {
        // Even if `request_headers` whitelists Host and the client sends one,
        // the auth request's Host must be the endpoint authority — a client
        // must not be able to influence where the subrequest appears to come from.
        let mut cfg = make_config();
        cfg.request_headers.push("host".parse().unwrap());

        let req = Request::builder()
            .method("GET")
            .uri("/")
            .header("Host", "evil.example.com")
            .body(Empty::<Bytes>::new())
            .unwrap();

        let auth_req = build_auth_request("/", remote_addr(), false, &req, &cfg).unwrap();

        assert_eq!(
            auth_req.headers().get("host").unwrap(),
            "auth.internal:8080"
        );
    }

    #[test]
    fn test_scrub_identity_case_insensitive() {
        // Client sends the identity header with different casing than config.
        let cfg = make_config();
        let mut req = Request::builder()
            .header("X-USER", "attacker")
            .body(Empty::<Bytes>::new())
            .unwrap();

        scrub_identity_headers(&mut req, &cfg);
        assert!(req.headers().get("x-user").is_none());
        assert!(req.headers().get("X-USER").is_none());
    }
}
