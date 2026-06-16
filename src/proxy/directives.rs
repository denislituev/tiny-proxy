use hyper::body::Incoming;
use hyper::Request;
use tracing::info;

use crate::auth::{process_header_substitution, process_upstream_substitution};
use crate::config::HeaderDirective;

use crate::proxy::ActionResult;

/// Handle reverse_proxy directive
pub fn handle_reverse_proxy(
    to: &str,
    path: &str,
    connect_timeout: Option<u64>,
    read_timeout: Option<u64>,
    header_up: Vec<HeaderDirective>,
) -> ActionResult {
    info!(
        "   Proxying to: {} (connect_timeout: {:?}, read_timeout: {:?}, header_up: {} ops)",
        to,
        connect_timeout,
        read_timeout,
        header_up.len()
    );
    ActionResult::ReverseProxy {
        backend_url: to.to_string(),
        path_to_send: path.to_string(),
        connect_timeout,
        read_timeout,
        header_up,
    }
}

/// Handle respond directive
pub fn handle_respond(status: &u16, body: &str) -> ActionResult {
    info!("   Returning direct response: {}", status);
    ActionResult::Respond {
        status: *status,
        body: body.to_string(),
    }
}

/// Handle redirect directive - return redirect response with Location header
/// Supported status codes: 301 (permanent), 302 (temporary), 307, 308
pub fn handle_redirect(status: &u16, url: &str) -> ActionResult {
    info!("   Redirecting ({}) to: {}", status, url);
    ActionResult::Redirect {
        status: *status,
        url: url.to_string(),
    }
}

/// Handle header directive - add, replace, or remove header in request
/// - `value = Some("...")`: set header with placeholder substitution ({uuid}, {header.Name}, {env.VAR})
/// - `value = None`: remove header (syntax: `header -Name`)
pub fn handle_header<B>(
    name: &str,
    value: Option<&str>,
    req: &mut Request<B>,
) -> anyhow::Result<()> {
    use hyper::header::{HeaderName, HeaderValue};

    let header_name = HeaderName::from_bytes(name.as_bytes())?;

    match value {
        Some(val) => {
            // Process placeholders like {uuid}, {header.Name}, {env.VAR}
            let processed_value = process_header_substitution(val, req)?;

            let header_value = HeaderValue::from_str(&processed_value)?;

            req.headers_mut().insert(header_name, header_value);
            info!("   Applied header: {} = {}", name, processed_value);
        }
        None => {
            req.headers_mut().remove(&header_name);
            info!("   Removed header: {}", name);
        }
    }

    Ok(())
}

/// Apply `header_up` directives to the outbound (upstream) request.
///
/// Runs after default Host / X-Forwarded-* headers so explicit `header_up` can override them.
pub fn apply_header_up<B>(
    directives: &[HeaderDirective],
    req: &mut Request<B>,
    upstream_host: &str,
    request_uri: &str,
    remote_ip: &str,
) {
    use hyper::header::{HeaderName, HeaderValue};

    for directive in directives {
        match HeaderName::from_bytes(directive.name.as_bytes()) {
            Ok(header_name) => match &directive.value {
                Some(val) => {
                    match process_upstream_substitution(
                        val,
                        req,
                        upstream_host,
                        request_uri,
                        remote_ip,
                    ) {
                        Ok(processed) => match HeaderValue::from_str(&processed) {
                            Ok(header_value) => {
                                req.headers_mut().insert(header_name, header_value);
                                info!("   Applied header_up: {} = {}", directive.name, processed);
                            }
                            Err(e) => {
                                info!(
                                    "   Failed to apply header_up {}: invalid value: {}",
                                    directive.name, e
                                );
                            }
                        },
                        Err(e) => {
                            info!("   Failed to apply header_up {}: {}", directive.name, e);
                        }
                    }
                }
                None => {
                    req.headers_mut().remove(&header_name);
                    info!("   Removed header_up: {}", directive.name);
                }
            },
            Err(e) => {
                info!(
                    "   Failed to apply header_up {}: invalid header name: {}",
                    directive.name, e
                );
            }
        }
    }
}

/// Handle uri_replace directive - replace substring in path
pub fn handle_uri_replace(find: &str, replace: &str, path: &mut String) {
    *path = path.replace(find, replace);
    info!("   Applied uri_replace: {} → {}", find, replace);
}

/// Handle strip_prefix directive - remove a prefix from the URI path
/// Ensures the result always starts with `/`
pub fn handle_strip_prefix(prefix: &str, path: &mut String) {
    if let Some(stripped) = path.strip_prefix(prefix) {
        *path = if stripped.is_empty() || !stripped.starts_with('/') {
            format!("/{}", stripped)
        } else {
            stripped.to_string()
        };
        info!("   Applied strip_prefix: {} → {}", prefix, *path);
    }
}

/// Handle method directive - check if request method matches allowed methods
pub fn handle_method(methods: &[String], req: &Request<Incoming>) -> bool {
    methods
        .iter()
        .any(|m| m.eq_ignore_ascii_case(req.method().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Empty;

    fn make_request() -> Request<Empty<Bytes>> {
        Request::builder()
            .header("Authorization", "Bearer secret-token")
            .body(Empty::new())
            .unwrap()
    }

    #[test]
    fn test_handle_header_static_value() {
        let mut req = make_request();
        handle_header("X-Static", Some("hello-world"), &mut req).unwrap();

        let value = req.headers().get("X-Static").unwrap().to_str().unwrap();
        assert_eq!(value, "hello-world");
    }

    #[test]
    fn test_handle_header_uuid_placeholder() {
        // {uuid} should be replaced with a real UUID like "550e8400-e29b-41d4-..."
        let mut req = make_request();
        handle_header("X-Request-ID", Some("{uuid}"), &mut req).unwrap();

        let value = req.headers().get("X-Request-ID").unwrap().to_str().unwrap();

        assert_ne!(value, "{uuid}", "Should not be literal placeholder");
        assert!(value.contains("-"), "UUID should contain dashes");
        assert_eq!(value.len(), 36, "UUID should be 36 characters");
    }

    #[test]
    fn test_handle_header_header_placeholder() {
        // {header.Authorization} should be replaced with "Bearer secret-token"
        let mut req = make_request();
        handle_header("X-Client-Auth", Some("{header.Authorization}"), &mut req).unwrap();

        let value = req
            .headers()
            .get("X-Client-Auth")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(
            value, "Bearer secret-token",
            "Should extract value from Authorization header"
        );
    }

    #[test]
    fn test_handle_header_env_placeholder() {
        std::env::set_var("TEST_PROXY_VAR", "test-value-123");

        let mut req = make_request();
        handle_header("X-Env-Test", Some("{env.TEST_PROXY_VAR}"), &mut req).unwrap();

        let value = req.headers().get("X-Env-Test").unwrap().to_str().unwrap();

        assert_eq!(
            value, "test-value-123",
            "Should substitute environment variable"
        );

        std::env::remove_var("TEST_PROXY_VAR");
    }

    #[test]
    fn test_handle_header_remove() {
        let mut req = make_request();
        assert!(
            req.headers().get("Authorization").is_some(),
            "Authorization header should exist before removal"
        );

        handle_header("Authorization", None, &mut req).unwrap();

        assert!(
            req.headers().get("Authorization").is_none(),
            "Authorization header should be removed"
        );
    }

    #[test]
    fn test_handle_header_remove_nonexistent() {
        let mut req = make_request();
        // Removing a header that doesn't exist should not error
        handle_header("X-Nonexistent", None, &mut req).unwrap();
    }

    #[test]
    fn test_handle_strip_prefix_basic() {
        let mut path = "/api/users/123".to_string();
        handle_strip_prefix("/api", &mut path);
        assert_eq!(path, "/users/123");
    }

    #[test]
    fn test_handle_strip_prefix_exact_match() {
        let mut path = "/api".to_string();
        handle_strip_prefix("/api", &mut path);
        assert_eq!(path, "/", "Exact match should result in root path");
    }

    #[test]
    fn test_handle_strip_prefix_no_match() {
        let mut path = "/users/123".to_string();
        handle_strip_prefix("/api", &mut path);
        assert_eq!(
            path, "/users/123",
            "Should remain unchanged when prefix doesn't match"
        );
    }

    #[test]
    fn test_handle_strip_prefix_trailing_slash() {
        let mut path = "/api/v2/users".to_string();
        handle_strip_prefix("/api/v2", &mut path);
        assert_eq!(path, "/users");
    }

    #[test]
    fn test_handle_redirect_301() {
        let result = handle_redirect(&301, "https://example.com/new");
        match result {
            ActionResult::Redirect { status, url } => {
                assert_eq!(status, 301);
                assert_eq!(url, "https://example.com/new");
            }
            _ => panic!("Expected Redirect action"),
        }
    }

    #[test]
    fn test_handle_redirect_302() {
        let result = handle_redirect(&302, "/temporary");
        match result {
            ActionResult::Redirect { status, url } => {
                assert_eq!(status, 302);
                assert_eq!(url, "/temporary");
            }
            _ => panic!("Expected Redirect action"),
        }
    }

    #[test]
    fn test_apply_header_up_set_and_remove() {
        use bytes::Bytes;
        use http_body_util::Empty;

        let mut req = Request::builder()
            .header("Accept-Encoding", "gzip")
            .body(Empty::<Bytes>::new())
            .unwrap();

        let directives = vec![
            HeaderDirective {
                name: "Host".to_string(),
                value: Some("{upstream_host}".to_string()),
            },
            HeaderDirective {
                name: "X-Original-Uri".to_string(),
                value: Some("{request.uri}".to_string()),
            },
            HeaderDirective {
                name: "Accept-Encoding".to_string(),
                value: None,
            },
        ];

        apply_header_up(
            &directives,
            &mut req,
            "api.example.com:443",
            "/api/test?q=1",
            "10.0.0.1",
        );

        assert_eq!(
            req.headers().get("Host").unwrap().to_str().unwrap(),
            "api.example.com:443"
        );
        assert_eq!(
            req.headers()
                .get("X-Original-Uri")
                .unwrap()
                .to_str()
                .unwrap(),
            "/api/test?q=1"
        );
        assert!(req.headers().get("Accept-Encoding").is_none());
    }
}
