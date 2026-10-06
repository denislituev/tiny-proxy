//! Integration tests for the `forward_auth` middleware.
//!
//! Covers the v0.6.0 spec: decision vs infrastructure failure, identity-header
//! spoofing protection, failure modes, nested `handle_path`, request-body
//! preservation, and `X-Original-URI` semantics.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{HeaderValue, WWW_AUTHENTICATE};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tiny_proxy::config::{Directive, FailureMode, ForwardAuthConfig, SiteConfig};
use tiny_proxy::{Config, Proxy};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

async fn get_random_port_addr() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

/// Scriptable behavior for the mock auth server.
#[derive(Clone)]
struct AuthScript {
    status: u16,
    /// Headers returned by the auth service (identity + WWW-Authenticate...).
    headers: Vec<(String, String)>,
    /// Artificial delay before responding.
    delay: Option<Duration>,
}

impl AuthScript {
    fn ok() -> Self {
        Self {
            status: 200,
            headers: vec![],
            delay: None,
        }
    }

    fn with_headers(mut self, headers: Vec<(&str, &str)>) -> Self {
        self.headers = headers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self
    }

    fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

/// Captured state of one request received by the mock auth server.
#[derive(Debug, Clone, Default)]
struct AuthCapture {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
}

/// Shared state between the mock auth server and test assertions.
#[derive(Clone, Default)]
struct AuthState {
    calls: Arc<AtomicUsize>,
    captures: Arc<Mutex<Vec<AuthCapture>>>,
}

impl AuthState {
    fn new() -> Self {
        Self::default()
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_capture(&self) -> AuthCapture {
        self.captures
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_default()
    }

    fn header_value(&self, name: &str) -> Option<String> {
        self.last_capture()
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
}

/// Start a scriptable mock auth server. Every request gets the scripted
/// status/headers; requests are captured for assertions.
async fn start_mock_auth(script: AuthScript, state: AuthState) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let io = TokioIo::new(stream);
            let script = script.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                    let script = script.clone();
                    let state = state.clone();
                    async move {
                        state.calls.fetch_add(1, Ordering::SeqCst);
                        let (parts, body) = req.into_parts();
                        // Drain auth request body (if any) for connection reuse.
                        let _ = body.collect().await;

                        let mut capture = AuthCapture {
                            method: parts.method.as_str().to_string(),
                            uri: parts.uri.to_string(),
                            headers: vec![],
                        };
                        for (name, value) in parts.headers.iter() {
                            capture.headers.push((
                                name.as_str().to_string(),
                                value.to_str().unwrap_or_default().to_string(),
                            ));
                        }
                        state.captures.lock().unwrap().push(capture);

                        if let Some(delay) = script.delay {
                            tokio::time::sleep(delay).await;
                        }

                        let mut response = Response::new(Full::new(Bytes::new()));
                        *response.status_mut() = StatusCode::from_u16(script.status).unwrap();
                        for (k, v) in &script.headers {
                            if let (Ok(name), Ok(value)) = (
                                hyper::header::HeaderName::from_bytes(k.as_bytes()),
                                HeaderValue::from_str(v),
                            ) {
                                response.headers_mut().insert(name, value);
                            }
                        }
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    addr
}

/// Backend that echoes selected request properties back in the body and
/// counts every request it receives.
#[derive(Clone, Default)]
struct BackendState {
    calls: Arc<AtomicUsize>,
}

impl BackendState {
    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

async fn start_echo_backend(state: BackendState) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let io = TokioIo::new(stream);
            let state = state.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                    let state = state.clone();
                    async move {
                        state.calls.fetch_add(1, Ordering::SeqCst);
                        let (parts, body) = req.into_parts();
                        let body_bytes = body.collect().await.unwrap().to_bytes();

                        let header = |name: &str| {
                            parts
                                .headers
                                .get(name)
                                .and_then(|h| h.to_str().ok())
                                .unwrap_or("<none>")
                                .to_string()
                        };

                        let payload = format!(
                                "method={}|uri={}|query={}|x-user={}|x-email={}|authorization={}|body={}",
                                parts.method.as_str(),
                                parts.uri.path(),
                                parts.uri.query().unwrap_or(""),
                                header("x-user"),
                                header("x-email"),
                                header("authorization"),
                                String::from_utf8_lossy(&body_bytes),
                            );

                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(payload))))
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    addr
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

fn auth_config(endpoint: &str) -> ForwardAuthConfig {
    ForwardAuthConfig {
        endpoint: endpoint.parse().unwrap(),
        request_headers: vec!["authorization".parse().unwrap(), "cookie".parse().unwrap()],
        response_headers: vec!["x-user".parse().unwrap(), "x-email".parse().unwrap()],
        failure_mode: FailureMode::Closed,
        timeout_secs: 5,
    }
}

fn reverse_proxy(backend_addr: std::net::SocketAddr) -> Directive {
    Directive::ReverseProxy {
        to: format!("http://{backend_addr}"),
        connect_timeout: None,
        read_timeout: None,
        header_up: vec![],
    }
}

fn build_config(proxy_host: &str, directives: Vec<Directive>) -> Config {
    let mut sites = HashMap::new();
    sites.insert(
        proxy_host.to_string(),
        SiteConfig {
            address: proxy_host.to_string(),
            directives,
            tls: None,
        },
    );
    Config { sites }
}

/// Start the proxy on a random port with the given directives and return the
/// `host:port` clients should target.
async fn start_proxy(directives: Vec<Directive>) -> String {
    let proxy_addr = get_random_port_addr().await;
    let proxy_host = format!("127.0.0.1:{}", proxy_addr.port());
    let config = build_config(&proxy_host, directives);
    let proxy = Proxy::new(config);
    tokio::spawn(async move {
        proxy.start_with_addr(proxy_addr).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    proxy_host
}

fn test_client() -> hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    Full<Bytes>,
> {
    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build::<_, Full<Bytes>>(hyper_util::client::legacy::connect::HttpConnector::new())
}

async fn body_string(response: Response<hyper::body::Incoming>) -> String {
    String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes()).to_string()
}

fn query_param(payload: &str, key: &str) -> String {
    // payload entries look like `key=value` separated by `|`; values may
    // themselves contain `=` (e.g. base64 bodies) but not `|`.
    payload
        .split('|')
        .find(|part| part.starts_with(key) && part.as_bytes().get(key.len()) == Some(&b'='))
        .map(|part| part[part.find('=').unwrap() + 1..].to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests: decisions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_authorized_request_propagates_identity_and_original_headers() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(
        AuthScript::ok().with_headers(vec![
            ("X-User", "real-user"),
            ("X-Email", "user@example.com"),
            ("X-Roles", "admin"), // not configured in response_headers
        ]),
        auth_state.clone(),
    )
    .await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users?id=123"))
                .header("Authorization", "Bearer valid-token")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_string(response).await;

    assert_eq!(query_param(&payload, "x-user"), "real-user");
    assert_eq!(query_param(&payload, "x-email"), "user@example.com");
    assert_eq!(query_param(&payload, "authorization"), "Bearer valid-token");
    assert_eq!(query_param(&payload, "query"), "id=123");

    assert_eq!(backend_state.call_count(), 1);

    assert_eq!(auth_state.call_count(), 1);
    assert_eq!(auth_state.last_capture().uri, "/verify");
    assert_eq!(
        auth_state.header_value("authorization").as_deref(),
        Some("Bearer valid-token")
    );
    assert_eq!(
        auth_state.header_value("x-original-uri").as_deref(),
        Some("/api/users?id=123")
    );
    assert_eq!(
        auth_state.header_value("x-original-method").as_deref(),
        Some("GET")
    );
    assert_eq!(
        auth_state.header_value("host").as_deref(),
        Some(auth_addr.to_string().as_str())
    );
}

#[tokio::test]
async fn test_unauthorized_401_denies_without_backend_contact() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(
        AuthScript::ok()
            .with_status(401)
            .with_headers(vec![("WWW-Authenticate", "Bearer realm=\"test\"")]),
        auth_state.clone(),
    )
    .await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .header("Authorization", "Bearer invalid")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should return the denial");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get(WWW_AUTHENTICATE).unwrap(),
        "Bearer realm=\"test\""
    );
    assert_eq!(
        backend_state.call_count(),
        0,
        "backend must not be contacted"
    );
    assert_eq!(auth_state.call_count(), 1);
}

#[tokio::test]
async fn test_forbidden_403_denies_without_backend_contact() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok().with_status(403), auth_state.clone()).await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should return the denial");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        backend_state.call_count(),
        0,
        "backend must not be contacted"
    );
}

// ---------------------------------------------------------------------------
// Tests: infrastructure failures + failure modes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_auth_500_closed_fails_closed() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok().with_status(500), auth_state.clone()).await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should return a failure response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        backend_state.call_count(),
        0,
        "fail-closed: no backend contact"
    );
}

#[tokio::test]
async fn test_auth_unreachable_closed_returns_503() {
    // Bind then drop a listener to get a port that is (almost certainly) closed.
    let dead_addr = get_random_port_addr().await;

    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let mut auth = auth_config(&format!("http://{dead_addr}/verify"));
    auth.failure_mode = FailureMode::Closed;
    // Short timeout: connection refused should be classified quickly.
    auth.timeout_secs = 2;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should return a failure response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        backend_state.call_count(),
        0,
        "fail-closed: no backend contact"
    );
}

#[tokio::test]
async fn test_auth_unreachable_open_continues_to_backend() {
    let dead_addr = get_random_port_addr().await;

    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let mut auth = auth_config(&format!("http://{dead_addr}/verify"));
    auth.failure_mode = FailureMode::Open;
    auth.timeout_secs = 2;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should continue despite auth outage");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        backend_state.call_count(),
        1,
        "fail-open: request continues"
    );
}

#[tokio::test]
async fn test_401_with_open_mode_still_denies() {
    // Critical: failure_mode open must NEVER bypass an explicit 401 decision.
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok().with_status(401), auth_state.clone()).await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let mut auth = auth_config(&format!("http://{auth_addr}/verify"));
    auth.failure_mode = FailureMode::Open;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should return the denial");

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "failure_mode open must not bypass an explicit 401"
    );
    assert_eq!(
        backend_state.call_count(),
        0,
        "backend must not be contacted"
    );
}

#[tokio::test]
async fn test_auth_timeout_closed_returns_503() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(
        AuthScript::ok().with_delay(Duration::from_secs(3)),
        auth_state.clone(),
    )
    .await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let mut auth = auth_config(&format!("http://{auth_addr}/verify"));
    auth.timeout_secs = 1;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth),
        reverse_proxy(backend_addr),
    ])
    .await;

    let started = std::time::Instant::now();
    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should return a failure response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "timeout must bound the wait"
    );
    assert_eq!(
        backend_state.call_count(),
        0,
        "fail-closed: no backend contact"
    );
}

/// Raw-TCP auth server that promises a 100-byte body but closes the connection
/// after a few bytes — the client sees a 2xx whose body stream breaks mid-flight.
async fn start_broken_body_auth() -> std::net::SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            tokio::spawn(async move {
                let mut req_buf = [0u8; 4096];
                let _ = stream.read(&mut req_buf).await;

                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\npartial")
                    .await;
                // Drop with 92 of 100 promised bytes missing.
            });
        }
    });

    addr
}

#[tokio::test]
async fn test_broken_auth_body_closed_returns_503() {
    // A 200 response whose body stream breaks is not a reliable decision:
    // fail-closed must treat it as an infrastructure failure.
    let auth_addr = start_broken_body_auth().await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let mut auth = auth_config(&format!("http://{auth_addr}/verify"));
    auth.failure_mode = FailureMode::Closed;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy should return a failure response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        backend_state.call_count(),
        0,
        "fail-closed: no backend contact"
    );
}

#[tokio::test]
async fn test_broken_auth_body_open_continues_to_backend() {
    let auth_addr = start_broken_body_auth().await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let mut auth = auth_config(&format!("http://{auth_addr}/verify"));
    auth.failure_mode = FailureMode::Open;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("fail-open should continue to the backend");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(backend_state.call_count(), 1);

    // Fail-open continues WITHOUT identity headers — a broken auth response
    // must not leak partial identity data to the backend.
    let payload = body_string(response).await;
    assert_eq!(query_param(&payload, "x-user"), "<none>");
}

// ---------------------------------------------------------------------------
// Tests: identity spoofing protection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_identity_header_spoofing_is_rejected() {
    // Client sends a spoofed X-User; auth returns the real one.
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(
        AuthScript::ok().with_headers(vec![("X-User", "real-user")]),
        auth_state.clone(),
    )
    .await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .header("X-User", "attacker") // spoofed identity
                .header("X-Email", "attacker@example.com") // spoofed identity
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_string(response).await;
    assert_eq!(query_param(&payload, "x-user"), "real-user");
    // X-Email configured in response_headers but NOT returned by auth:
    // the spoofed client value must NOT fall back through — it is removed.
    assert_eq!(query_param(&payload, "x-email"), "<none>");
}

#[tokio::test]
async fn test_spoofed_identity_headers_removed_when_auth_omits_them() {
    // Auth returns 200 but none of the configured identity headers.
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok(), auth_state.clone()).await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .header("x-user", "attacker")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_string(response).await;
    assert_eq!(
        query_param(&payload, "x-user"),
        "<none>",
        "absent auth value must not fall back to untrusted client input"
    );
}

// ---------------------------------------------------------------------------
// Tests: request body preservation + metadata
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_post_body_preserved_through_auth() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(
        AuthScript::ok().with_headers(vec![("X-User", "real-user")]),
        auth_state.clone(),
    )
    .await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let body = "{\"name\":\"test\",\"value\":42}";
    let response = client
        .request(
            Request::builder()
                .method("POST")
                .uri(format!("http://{proxy_host}/api/items"))
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body.to_string())))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_string(response).await;

    assert_eq!(query_param(&payload, "body"), body);
    assert_eq!(query_param(&payload, "method"), "POST");

    // The subrequest must not consume the client's body.
    assert_eq!(auth_state.last_capture().method, "GET");
    assert_eq!(
        auth_state.header_value("x-original-method").as_deref(),
        Some("POST")
    );
}

#[tokio::test]
async fn test_query_string_forwarded_in_original_uri() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok(), auth_state.clone()).await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users?page=2&limit=10"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        auth_state.header_value("x-original-uri").as_deref(),
        Some("/api/users?page=2&limit=10"),
        "X-Original-URI must include the query string"
    );
}

#[tokio::test]
async fn test_non_whitelisted_headers_not_forwarded_to_auth() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok(), auth_state.clone()).await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .header("X-Internal-Secret", "backend-only-secret")
                .header("User-Agent", "test-client")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        auth_state.header_value("x-internal-secret").is_none(),
        "non-whitelisted header must not reach the auth service"
    );
    assert!(auth_state.header_value("user-agent").is_none());
}

#[tokio::test]
async fn test_request_id_propagated_to_auth() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok(), auth_state.clone()).await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .header("X-Request-ID", "test-req-id-123")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        auth_state.header_value("x-request-id").as_deref(),
        Some("test-req-id-123")
    );
}

// ---------------------------------------------------------------------------
// Tests: nested handle_path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_forward_auth_inside_handle_path() {
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(
        AuthScript::ok().with_headers(vec![("X-User", "real-user")]),
        auth_state.clone(),
    )
    .await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let api_block = vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ];
    // Public block: no auth.
    let public_block = vec![reverse_proxy(backend_addr)];

    let directives = vec![
        Directive::HandlePath {
            pattern: "/api/*".to_string(),
            directives: api_block,
        },
        Directive::HandlePath {
            pattern: "/public/*".to_string(),
            directives: public_block,
        },
    ];

    let proxy_host = start_proxy(directives).await;

    let client = test_client();

    // Protected path: auth runs, identity applied, path prefix stripped
    // (handle_path semantics) and X-Original-URI keeps the original path.
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users?id=7"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_string(response).await;
    assert_eq!(query_param(&payload, "x-user"), "real-user");
    assert_eq!(auth_state.call_count(), 1);
    assert_eq!(
        auth_state.header_value("x-original-uri").as_deref(),
        Some("/api/users?id=7"),
        "X-Original-URI must be the client's original path even inside handle_path"
    );

    // Public path: no auth call.
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/public/stats"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        auth_state.call_count(),
        1,
        "public path must not trigger auth"
    );
}

// ---------------------------------------------------------------------------
// Tests: header casing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_header_casing_case_insensitive() {
    // Client sends mixed-case headers; config uses lowercase names.
    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(
        AuthScript::ok().with_headers(vec![("x-UsEr", "real-user")]),
        auth_state.clone(),
    )
    .await;
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users"))
                .header("AuThOrIzAtIoN", "Bearer mixed-case")
                .header("X-UsEr", "attacker")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_string(response).await;
    // Whitelist copy is case-insensitive.
    assert_eq!(query_param(&payload, "authorization"), "Bearer mixed-case");
    // Identity scrub/apply is case-insensitive: auth value wins.
    assert_eq!(query_param(&payload, "x-user"), "real-user");

    assert_eq!(
        auth_state.header_value("authorization").as_deref(),
        Some("Bearer mixed-case")
    );
}

#[tokio::test]
async fn test_handle_path_strips_prefix_but_preserves_query() {
    let backend_state = BackendState::default();
    let backend_addr = start_echo_backend(backend_state.clone()).await;

    let proxy_host = start_proxy(vec![Directive::HandlePath {
        pattern: "/api/*".to_string(),
        directives: vec![reverse_proxy(backend_addr)],
    }])
    .await;

    let client = test_client();
    let response = client
        .request(
            Request::builder()
                .method("GET")
                .uri(format!("http://{proxy_host}/api/users?page=2&limit=10"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_string(response).await;
    // Prefix stripped ...
    assert_eq!(query_param(&payload, "uri"), "/users");
    // ... but the query string survives.
    assert_eq!(query_param(&payload, "query"), "page=2&limit=10");
}

// ---------------------------------------------------------------------------
// Tests: streaming (SSE-style) responses pass through unbuffered
// ---------------------------------------------------------------------------

/// Raw-TCP backend that streams a chunked response: headers + first chunk
/// immediately, then (after `delay`) the second chunk and terminator.
async fn start_streaming_backend(delay: Duration) -> std::net::SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let delay = delay;
            tokio::spawn(async move {
                // Read the request head (small) so the backend is not blocked
                // on the client's request buffer.
                let mut req_buf = [0u8; 4096];
                let _ = stream.read(&mut req_buf).await;

                let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
                let _ = stream.write_all(head.as_bytes()).await;
                // `event:a\n\n` is 9 bytes -> chunk size "9".
                let _ = stream.write_all(b"9\r\nevent:a\n\n\r\n").await;

                tokio::time::sleep(delay).await;

                let _ = stream.write_all(b"9\r\nevent:b\n\n\r\n").await;
                let _ = stream.write_all(b"0\r\n\r\n").await;
            });
        }
    });

    addr
}

#[tokio::test]
async fn test_streaming_response_not_buffered_by_forward_auth() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let auth_state = AuthState::new();
    let auth_addr = start_mock_auth(AuthScript::ok(), auth_state.clone()).await;
    let backend_addr = start_streaming_backend(Duration::from_millis(1000)).await;

    let proxy_host = start_proxy(vec![
        Directive::ForwardAuth(auth_config(&format!("http://{auth_addr}/verify"))),
        reverse_proxy(backend_addr),
    ])
    .await;

    // Raw TCP client so chunk arrival timing is observable.
    let mut stream = tokio::net::TcpStream::connect(&proxy_host).await.unwrap();
    stream
        .write_all(
            format!("GET /stream HTTP/1.1\r\nhost: {proxy_host}\r\nconnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();

    // The backend sends `event:a` immediately and `event:b` after 1s. If the
    // proxy buffered the response, nothing would arrive before ~1s; streaming
    // must deliver `event:a` well within 700ms.
    let mut received = Vec::new();
    let mut buf = [0u8; 1024];
    let first_chunk_deadline = tokio::time::Instant::now() + Duration::from_millis(700);
    loop {
        match tokio::time::timeout_at(first_chunk_deadline, stream.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
            Ok(Ok(n)) => {
                received.extend_from_slice(&buf[..n]);
                if find(&received, b"event:a\n\n").is_some() {
                    break;
                }
            }
        }
    }
    assert!(
        find(&received, b"event:a\n\n").is_some(),
        "first streamed chunk must arrive before the backend's second chunk (response is not buffered)"
    );

    // Read the rest of the streamed body.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline, stream.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
            Ok(Ok(n)) => received.extend_from_slice(&buf[..n]),
        }
    }
    assert!(
        find(&received, b"event:b\n\n").is_some(),
        "full streamed body received"
    );
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
