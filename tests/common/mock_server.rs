//! Mock HTTP server for testing proxy functionality
//!
//! This module provides a simple HTTP server that can be used as a backend
//! for testing the proxy. It supports customizing responses and inspecting
//! received requests.

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Response configuration for the mock server
#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub body: String,
    pub headers: HashMap<String, String>,
}

impl Default for MockResponse {
    fn default() -> Self {
        Self {
            status: 200,
            body: "OK".to_string(),
            headers: HashMap::new(),
        }
    }
}

impl MockResponse {
    /// Create a new mock response
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            headers: HashMap::new(),
        }
    }

    /// Add a header to the response
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Create a JSON response
    pub fn json(body: impl serde::Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            status: 200,
            body: serde_json::to_string(&body)?,
            headers: {
                let mut headers = HashMap::new();
                headers.insert("Content-Type".to_string(), "application/json".to_string());
                headers
            },
        })
    }
}

/// Shared state for the mock server
#[derive(Debug, Default)]
struct MockServerState {
    response: MockResponse,
    request_count: Arc<Mutex<usize>>,
    recorded_requests: Arc<Mutex<Vec<RequestRecord>>>,
}

#[derive(Debug, Clone)]
struct RequestRecord {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

/// Mock HTTP server for testing
pub struct MockServer {
    port: u16,
    _handle: JoinHandle<()>,
    request_count: Arc<Mutex<usize>>,
    recorded_requests: Arc<Mutex<Vec<RequestRecord>>>,
}

impl MockServer {
    /// Create a new mock server on the specified port
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tiny_proxy::tests::common::MockServer;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let server = MockServer::new(3000);
    ///
    ///     // Use the server for testing...
    ///
    ///     // Server automatically stops when dropped
    /// }
    /// ```
    pub fn new(port: u16) -> Self {
        Self::with_response(port, MockResponse::default())
    }

    /// Create a new mock server with a custom response
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tiny_proxy::tests::common::{MockServer, MockResponse};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let response = MockResponse::new(200, "Hello, World!")
    ///         .with_header("X-Custom-Header", "test-value");
    ///
    ///     let server = MockServer::with_response(3000, response);
    ///
    ///     // All requests to this server will receive the custom response
    /// }
    /// ```
    pub fn with_response(port: u16, response: MockResponse) -> Self {
        let state = Arc::new(MockServerState {
            response,
            request_count: Arc::new(Mutex::new(0)),
            recorded_requests: Arc::new(Mutex::new(Vec::new())),
        });

        let state_clone = state.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = Self::run_server(port, state_clone).await {
                eprintln!("Mock server error: {}", e);
            }
        });

        // Wait a bit for the server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Self {
            port,
            _handle: handle,
            request_count: state.request_count,
            recorded_requests: state.recorded_requests,
        }
    }

    async fn run_server(port: u16, state: Arc<MockServerState>) -> anyhow::Result<()> {
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
        let listener = TcpListener::bind(&addr).await?;

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let state = state.clone();

            tokio::task::spawn(async move {
                let service = service_fn(move |req| {
                    let state = state.clone();
                    async move {
                        // Record the request
                        {
                            let mut count = state.request_count.lock().unwrap();
                            *count += 1;
                        }

                        {
                            let mut requests = state.recorded_requests.lock().unwrap();
                            requests.push(RequestRecord {
                                method: req.method().to_string(),
                                path: req.uri().path().to_string(),
                                headers: req
                                    .headers()
                                    .iter()
                                    .map(|(name, value)| {
                                        (name.to_string(), value.to_str().unwrap().to_string())
                                    })
                                    .collect(),
                            });
                        }

                        // Build the response
                        let status =
                            StatusCode::from_u16(state.response.status).unwrap_or(StatusCode::OK);

                        let mut builder = Response::builder().status(status);

                        // Add headers
                        for (name, value) in &state.response.headers {
                            if let Ok(name) = hyper::header::HeaderName::from_bytes(name.as_bytes())
                            {
                                if let Ok(value) =
                                    hyper::header::HeaderValue::from_str(value.as_str())
                                {
                                    builder = builder.header(name, value);
                                }
                            }
                        }

                        let body = state.response.body.clone();
                        builder
                            .body(hyper::body::Full::new(hyper::body::Bytes::from(body)))
                            .map_err(|e| anyhow::anyhow!("Failed to build response: {}", e))
                    }
                });

                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    eprintln!("Mock server connection error: {:?}", err);
                }
            });
        }
    }

    /// Get the number of requests received by this server
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tiny_proxy::tests::common::MockServer;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let server = MockServer::new(3000);
    ///
    ///     // Make some requests...
    ///
    ///     let count = server.request_count();
    ///     println!("Received {} requests", count);
    /// }
    /// ```
    pub fn request_count(&self) -> usize {
        *self.request_count.lock().unwrap()
    }

    /// Get a list of all recorded requests
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tiny_proxy::tests::common::MockServer;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let server = MockServer::new(3000);
    ///
    ///     // Make some requests...
    ///
    ///     let requests = server.recorded_requests();
    ///     for req in requests {
    ///         println!("{} {}", req.method, req.path);
    ///     }
    /// }
    /// ```
    pub fn recorded_requests(&self) -> Vec<RequestRecord> {
        self.recorded_requests.lock().unwrap().clone()
    }

    /// Reset the request count and clear recorded requests
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tiny_proxy::tests::common::MockServer;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let server = MockServer::new(3000);
    ///
    ///     // Make some requests...
    ///
    ///     assert!(server.request_count() > 0);
    ///
    ///     // Reset for next test
    ///     server.reset();
    ///
    ///     assert_eq!(server.request_count(), 0);
    /// }
    /// ```
    pub fn reset(&self) {
        *self.request_count.lock().unwrap() = 0;
        self.recorded_requests.lock().unwrap().clear();
    }

    /// Get the base URL for this server
    ///
    /// # Example
    ///
    /// ```no_run
    /// use tiny_proxy::tests::common::MockServer;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let server = MockServer::new(3000);
    ///
    ///     let url = server.url();
    ///     assert_eq!(url, "http://127.0.0.1:3000");
    /// }
    /// ```
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self._handle.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_server_basic() {
        let server = MockServer::new(3999);

        let response = reqwest::get(&server.url()).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), "OK");
    }

    #[tokio::test]
    async fn test_mock_server_custom_response() {
        let response = MockResponse::new(404, "Not Found").with_header("X-Custom", "value");

        let server = MockServer::with_response(3998, response);

        let response = reqwest::get(&server.url()).await.unwrap();
        assert_eq!(response.status(), 404);
        assert_eq!(response.text().await.unwrap(), "Not Found");
        assert_eq!(
            response
                .headers()
                .get("X-Custom")
                .unwrap()
                .to_str()
                .unwrap(),
            "value"
        );
    }

    #[tokio::test]
    async fn test_request_count() {
        let server = MockServer::new(3997);

        assert_eq!(server.request_count(), 0);

        reqwest::get(&server.url()).await.unwrap();
        assert_eq!(server.request_count(), 1);

        reqwest::get(&server.url()).await.unwrap();
        assert_eq!(server.request_count(), 2);

        server.reset();
        assert_eq!(server.request_count(), 0);
    }

    #[tokio::test]
    async fn test_recorded_requests() {
        let server = MockServer::new(3996);

        reqwest::get(&format!("{}/test", server.url()))
            .await
            .unwrap();

        let requests = server.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/test");
    }
}
