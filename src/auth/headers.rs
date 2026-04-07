//! Header manipulation utilities for authentication
//!
//! This module provides utilities for processing header value substitutions
//! including request headers, UUIDs, and environment variables.

use hyper::Request;

/// Process header value substitutions
///
/// Replaces placeholders in the header value with actual values:
/// - `{header.Name}` - value of request header with that name
/// - `{uuid}` - generates a random UUID
/// - `{env.VAR}` - value of environment variable VAR
///
/// # Arguments
///
/// * `value` - The header value template with placeholders
/// * `req` - The HTTP request to extract headers from
///
/// # Returns
///
/// The processed header value with all placeholders replaced
///
/// # Example
///
/// ```no_run
/// # use hyper::{Request, body::Incoming};
/// # use tiny_proxy::auth::headers::process_header_substitution;
/// # fn main() -> anyhow::Result<()> {
/// # let req = Request::builder().body(hyper::body::Incoming::empty()).unwrap();
/// let result = process_header_substitution("X-Request-ID: {uuid}", &req)?;
/// assert!(result.contains("X-Request-ID:"));
/// # Ok(())
/// # }
/// ```
pub fn process_header_substitution<B>(value: &str, req: &Request<B>) -> anyhow::Result<String> {
    let mut result = value.to_string();

    // Process {header.Name} substitutions
    while let Some(start) = result.find("{header.") {
        let end = result[start..]
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("Unclosed header substitution at position {}", start))?
            + start;

        let header_name = &result[start + 8..end];

        if let Some(header_value) = req.headers().get(header_name).and_then(|h| h.to_str().ok()) {
            result.replace_range(start..=end, header_value);
        } else {
            // If header doesn't exist, remove the placeholder
            result.replace_range(start..=end, "");
        }
    }

    // Process {env.VAR} substitutions
    while let Some(start) = result.find("{env.") {
        let end = result[start..].find('}').ok_or_else(|| {
            anyhow::anyhow!(
                "Unclosed environment variable substitution at position {}",
                start
            )
        })? + start;

        let var_name = &result[start + 5..end];

        if let Ok(env_value) = std::env::var(var_name) {
            result.replace_range(start..=end, &env_value);
        } else {
            // If env var doesn't exist, remove the placeholder
            result.replace_range(start..=end, "");
        }
    }

    // Process {uuid} substitutions
    result = result.replace("{uuid}", &uuid::Uuid::new_v4().to_string());

    Ok(result)
}

/// Extract remote IP address from request headers
///
/// Looks for the X-Forwarded-For or X-Real-IP headers to determine the
/// original client IP address.
///
/// # Arguments
///
/// * `req` - The HTTP request
///
/// # Returns
///
/// The remote IP address if found, None otherwise
pub fn extract_remote_ip<B>(req: &Request<B>) -> Option<String> {
    // Check X-Forwarded-For header (set by proxies)
    if let Some(xff) = req.headers().get("X-Forwarded-For") {
        if let Ok(xff_str) = xff.to_str() {
            // X-Forwarded-For can contain multiple IPs, take the first one
            let first_ip = xff_str.split(',').next()?.trim();
            return Some(first_ip.to_string());
        }
    }

    // Check X-Real-IP header
    if let Some(xri) = req.headers().get("X-Real-IP") {
        if let Ok(xri_str) = xri.to_str() {
            return Some(xri_str.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Empty;
    use hyper::Request;

    fn make_request() -> Request<Empty<Bytes>> {
        Request::builder().body(Empty::new()).unwrap()
    }

    fn make_request_with_header(name: &str, value: &str) -> Request<Empty<Bytes>> {
        Request::builder()
            .header(name, value)
            .body(Empty::new())
            .unwrap()
    }

    #[test]
    fn test_process_header_substitution_header() {
        let req = make_request_with_header("X-User-ID", "12345");

        let result = process_header_substitution("User: {header.X-User-ID}", &req).unwrap();
        assert_eq!(result, "User: 12345");
    }

    #[test]
    fn test_process_header_substitution_env() {
        std::env::set_var("TEST_VAR", "test-value");
        let req = make_request();

        let result = process_header_substitution("Value: {env.TEST_VAR}", &req).unwrap();
        assert_eq!(result, "Value: test-value");
        std::env::remove_var("TEST_VAR");
    }

    #[test]
    fn test_process_header_substitution_uuid() {
        let req = make_request();

        let result = process_header_substitution("ID: {uuid}", &req).unwrap();
        assert!(result.starts_with("ID: "));
        assert!(result.len() > 5); // UUID should be present
    }

    #[test]
    fn test_process_header_substitution_missing_header() {
        let req = make_request();

        let result = process_header_substitution("Value: {header.Missing}", &req).unwrap();
        assert_eq!(result, "Value: ");
    }

    #[test]
    fn test_extract_remote_ip_xff() {
        let req = make_request_with_header("X-Forwarded-For", "192.168.1.1, 10.0.0.1");

        let ip = extract_remote_ip(&req);
        assert_eq!(ip, Some("192.168.1.1".to_string()));
    }

    #[test]
    fn test_extract_remote_ip_xri() {
        let req = make_request_with_header("X-Real-IP", "192.168.1.2");

        let ip = extract_remote_ip(&req);
        assert_eq!(ip, Some("192.168.1.2".to_string()));
    }

    #[test]
    fn test_extract_remote_ip_none() {
        let req = make_request();

        let ip = extract_remote_ip(&req);
        assert!(ip.is_none());
    }
}
