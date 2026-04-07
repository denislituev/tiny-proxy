use hyper::body::Incoming;
use hyper::Request;
use tracing::info;

use crate::auth::process_header_substitution;

use crate::proxy::ActionResult;

/// Handle reverse_proxy directive
pub fn handle_reverse_proxy(to: &str, path: &str) -> ActionResult {
    info!("   Proxying to: {}", to);
    ActionResult::ReverseProxy {
        backend_url: to.to_string(),
        path_to_send: path.to_string(),
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

/// Handle header directive - add or replace header in request
/// Supports placeholder substitution: {uuid}, {header.Name}, {env.VAR}
pub fn handle_header<B>(name: &str, value: &str, req: &mut Request<B>) -> anyhow::Result<()> {
    use hyper::header::{HeaderName, HeaderValue};

    let header_name = HeaderName::from_bytes(name.as_bytes())?;

    // Process placeholders like {uuid}, {header.Name}, {env.VAR}
    let processed_value = process_header_substitution(value, req)?;

    let header_value = HeaderValue::from_str(&processed_value)?;

    req.headers_mut().insert(header_name, header_value);
    info!("   Applied header: {} = {}", name, processed_value);

    Ok(())
}

/// Handle uri_replace directive - replace substring in path
pub fn handle_uri_replace(find: &str, replace: &str, path: &mut String) {
    *path = path.replace(find, replace);
    info!("   Applied uri_replace: {} → {}", find, replace);
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
        handle_header("X-Static", "hello-world", &mut req).unwrap();

        let value = req.headers().get("X-Static").unwrap().to_str().unwrap();
        assert_eq!(value, "hello-world");
    }

    #[test]
    fn test_handle_header_uuid_placeholder() {
        // {uuid} should be replaced with a real UUID like "550e8400-e29b-41d4-..."
        let mut req = make_request();
        handle_header("X-Request-ID", "{uuid}", &mut req).unwrap();

        let value = req.headers().get("X-Request-ID").unwrap().to_str().unwrap();

        assert_ne!(value, "{uuid}", "Should not be literal placeholder");
        assert!(value.contains("-"), "UUID should contain dashes");
        assert_eq!(value.len(), 36, "UUID should be 36 characters");
    }

    #[test]
    fn test_handle_header_header_placeholder() {
        // {header.Authorization} should be replaced with "Bearer secret-token"
        let mut req = make_request();
        handle_header("X-Client-Auth", "{header.Authorization}", &mut req).unwrap();

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
        handle_header("X-Env-Test", "{env.TEST_PROXY_VAR}", &mut req).unwrap();

        let value = req.headers().get("X-Env-Test").unwrap().to_str().unwrap();

        assert_eq!(
            value, "test-value-123",
            "Should substitute environment variable"
        );

        std::env::remove_var("TEST_PROXY_VAR");
    }
}
