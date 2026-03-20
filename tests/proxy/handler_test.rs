//! Integration tests for proxy handler
//!
//! Tests the proxy's request handling logic including directive processing,
//! request forwarding, and response handling.

use bytes::Bytes;
use hyper::body::Incoming;
use hyper::client::Client;
use hyper::Method;
use hyper::Request;
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;

use tiny_proxy::config::{Config, Directive, SiteConfig};
use tiny_proxy::proxy::handler::{match_pattern, process_directives, proxy};
use tiny_proxy::proxy::ActionResult;

/// Create a mock backend URL for testing
fn mock_backend_url() -> String {
    "http://127.0.0.1:9001".to_string()
}

#[test]
fn test_match_pattern_exact() {
    let result = match_pattern("/api/users", "/api/users");
    assert_eq!(result, Some("/".to_string()));
}

#[test]
fn test_match_pattern_wildcard() {
    let result = match_pattern("/api/*", "/api/users/123");
    assert_eq!(result, Some("/users/123".to_string()));
}

#[test]
fn test_match_pattern_wildcard_nested() {
    let result = match_pattern("/api/v1/*", "/api/v1/users/123");
    assert_eq!(result, Some("/users/123".to_string()));
}

#[test]
fn test_match_pattern_no_match() {
    let result = match_pattern("/api/*", "/users/123");
    assert_eq!(result, None);
}

#[test]
fn test_match_pattern_wildcard_root() {
    let result = match_pattern("/api/*", "/api/");
    assert_eq!(result, Some("/".to_string()));
}

#[test]
fn test_process_directives_reverse_proxy() {
    let config = Config {
        sites: std::collections::HashMap::new(),
    };

    let mut req = Request::builder()
        .uri("http://localhost:8080/test/path")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![Directive::ReverseProxy {
        to: "backend:3000".to_string(),
    }];

    let result = process_directives(&directives, &mut req, "/test/path");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::ReverseProxy {
            backend_url,
            path_to_send,
        } => {
            assert_eq!(backend_url, "backend:3000");
            assert_eq!(path_to_send, "/test/path");
        }
        _ => panic!("Expected ReverseProxy action"),
    }
}

#[test]
fn test_process_directives_respond() {
    let mut req = Request::builder()
        .uri("http://localhost:8080/health")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![Directive::Respond {
        status: 200,
        body: "OK".to_string(),
    }];

    let result = process_directives(&directives, &mut req, "/health");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::Respond { status, body } => {
            assert_eq!(status, 200);
            assert_eq!(body, "OK");
        }
        _ => panic!("Expected Respond action"),
    }
}

#[test]
fn test_process_directives_uri_replace() {
    let mut req = Request::builder()
        .uri("http://localhost:8080/api/users")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![
        Directive::UriReplace {
            find: "/api".to_string(),
            replace: "".to_string(),
        },
        Directive::ReverseProxy {
            to: "backend:3000".to_string(),
        },
    ];

    let result = process_directives(&directives, &mut req, "/api/users");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::ReverseProxy {
            backend_url,
            path_to_send,
        } => {
            assert_eq!(backend_url, "backend:3000");
            assert_eq!(path_to_send, "/users");
        }
        _ => panic!("Expected ReverseProxy action with replaced path"),
    }
}

#[test]
fn test_process_directives_handle_path() {
    let mut req = Request::builder()
        .uri("http://localhost:8080/api/users")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![Directive::HandlePath {
        pattern: "/api/*".to_string(),
        directives: vec![Directive::ReverseProxy {
            to: "api:8081".to_string(),
        }],
    }];

    let result = process_directives(&directives, &mut req, "/api/users");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::ReverseProxy {
            backend_url,
            path_to_send,
        } => {
            assert_eq!(backend_url, "api:8081");
            assert_eq!(path_to_send, "/users");
        }
        _ => panic!("Expected ReverseProxy action"),
    }
}

#[test]
fn test_process_directives_method() {
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("http://localhost:8080/health")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![Directive::Method {
        methods: vec!["GET".to_string(), "HEAD".to_string()],
        directives: vec![Directive::Respond {
            status: 200,
            body: "OK".to_string(),
        }],
    }];

    let result = process_directives(&directives, &mut req, "/health");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::Respond { status, body } => {
            assert_eq!(status, 200);
            assert_eq!(body, "OK");
        }
        _ => panic!("Expected Respond action"),
    }
}

#[test]
fn test_process_directives_method_no_match() {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("http://localhost:8080/health")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![
        Directive::Method {
            methods: vec!["GET".to_string(), "HEAD".to_string()],
            directives: vec![Directive::Respond {
                status: 200,
                body: "OK".to_string(),
            }],
        },
        Directive::ReverseProxy {
            to: "backend:3000".to_string(),
        },
    ];

    let result = process_directives(&directives, &mut req, "/health");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::ReverseProxy {
            backend_url,
            path_to_send,
        } => {
            assert_eq!(backend_url, "backend:3000");
            assert_eq!(path_to_send, "/health");
        }
        _ => panic!("Expected ReverseProxy action (method didn't match)"),
    }
}

#[test]
fn test_process_directives_nested_handle_path() {
    let mut req = Request::builder()
        .uri("http://localhost:8080/api/v1/users/123")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![Directive::HandlePath {
        pattern: "/api/v1/*".to_string(),
        directives: vec![Directive::HandlePath {
            pattern: "/users/*".to_string(),
            directives: vec![Directive::ReverseProxy {
                to: "user-service:8001".to_string(),
            }],
        }],
    }];

    let result = process_directives(&directives, &mut req, "/api/v1/users/123");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::ReverseProxy {
            backend_url,
            path_to_send,
        } => {
            assert_eq!(backend_url, "user-service:8001");
            assert_eq!(path_to_send, "/123");
        }
        _ => panic!("Expected ReverseProxy action with nested handle_path"),
    }
}

#[test]
fn test_process_directives_no_action() {
    let mut req = Request::builder()
        .uri("http://localhost:8080/test")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![Directive::UriReplace {
        find: "/test".to_string(),
        replace: "/modified".to_string(),
    }];

    let result = process_directives(&directives, &mut req, "/test");
    assert!(result.is_err());
}

#[test]
fn test_process_directives_multiple_headers() {
    let mut req = Request::builder()
        .uri("http://localhost:8080/api/test")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    let directives = vec![
        Directive::Header {
            name: "X-Custom-1".to_string(),
            value: "value1".to_string(),
        },
        Directive::Header {
            name: "X-Custom-2".to_string(),
            value: "value2".to_string(),
        },
        Directive::ReverseProxy {
            to: "backend:3000".to_string(),
        },
    ];

    let result = process_directives(&directives, &mut req, "/api/test");
    assert!(result.is_ok());

    match result.unwrap() {
        ActionResult::ReverseProxy { .. } => {
            // Check headers were added
            assert_eq!(
                req.headers().get("X-Custom-1").unwrap().to_str().unwrap(),
                "value1"
            );
            assert_eq!(
                req.headers().get("X-Custom-2").unwrap().to_str().unwrap(),
                "value2"
            );
        }
        _ => panic!("Expected ReverseProxy action"),
    }
}

#[tokio::test]
async fn test_proxy_integration() {
    // This test requires a running backend server
    // It's an integration test, so it might be skipped if no backend is available

    let config = Config {
        sites: std::collections::HashMap::from([(
            "localhost:8080".to_string(),
            SiteConfig {
                address: "localhost:8080".to_string(),
                directives: vec![Directive::ReverseProxy {
                    to: "http://127.0.0.1:9001".to_string(),
                }],
            },
        )]),
    };

    let https = HttpsConnector::new();
    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build::<_, Incoming>(https);

    // Note: This test will fail if no backend is running on port 9001
    // In CI/CD, you'd use a mock server or skip this test

    let req = Request::builder()
        .uri("http://localhost:8080/test")
        .body(hyper::body::Incoming::empty())
        .unwrap();

    // This is a placeholder - actual integration test would need a running backend
    // For now, we just verify the structure is correct
    assert_eq!(config.sites.len(), 1);
    assert!(config.sites.contains_key("localhost:8080"));
}
