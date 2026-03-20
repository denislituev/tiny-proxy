//! Unit tests for configuration parser
//!
//! Tests the Config parser's ability to parse Caddy-like configuration format
//! and validate the resulting data structures.

use std::collections::HashMap;
use tiny_proxy::config::{Config, Directive, SiteConfig};

#[test]
fn test_parse_simple_reverse_proxy() {
    let config_str = r#"
localhost:8080 {
    reverse_proxy backend:3000
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    assert_eq!(config.sites.len(), 1);

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.address, "localhost:8080");
    assert_eq!(site_config.directives.len(), 1);

    match &site_config.directives[0] {
        Directive::ReverseProxy { to } => {
            assert_eq!(to, "backend:3000");
        }
        _ => panic!("Expected ReverseProxy directive"),
    }
}

#[test]
fn test_parse_handle_path() {
    let config_str = r#"
localhost:8080 {
    handle_path /api/* {
        reverse_proxy api:8081
    }
    reverse_proxy backend:3000
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    assert_eq!(config.sites.len(), 1);

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 2);

    match &site_config.directives[0] {
        Directive::HandlePath {
            pattern,
            directives,
        } => {
            assert_eq!(pattern, "/api/*");
            assert_eq!(directives.len(), 1);

            match &directives[0] {
                Directive::ReverseProxy { to } => {
                    assert_eq!(to, "api:8081");
                }
                _ => panic!("Expected nested ReverseProxy directive"),
            }
        }
        _ => panic!("Expected HandlePath directive"),
    }
}

#[test]
fn test_parse_header_directive() {
    let config_str = r#"
localhost:8080 {
    reverse_proxy backend:3000
    header X-Forwarded-For {remote_ip}
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 2);

    match &site_config.directives[1] {
        Directive::Header { name, value } => {
            assert_eq!(name, "X-Forwarded-For");
            assert_eq!(value, "{remote_ip}");
        }
        _ => panic!("Expected Header directive"),
    }
}

#[test]
fn test_parse_uri_replace() {
    let config_str = r#"
localhost:8080 {
    uri_replace /api /
    reverse_proxy backend:3000
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 2);

    match &site_config.directives[0] {
        Directive::UriReplace { find, replace } => {
            assert_eq!(find, "/api");
            assert_eq!(replace, "/");
        }
        _ => panic!("Expected UriReplace directive"),
    }
}

#[test]
fn test_parse_method_directive() {
    let config_str = r#"
localhost:8080 {
    method GET HEAD {
        respond 200 "Hello World"
    }
    reverse_proxy backend:3000
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 2);

    match &site_config.directives[0] {
        Directive::Method {
            methods,
            directives,
        } => {
            assert_eq!(methods.len(), 2);
            assert!(methods.contains(&"GET".to_string()));
            assert!(methods.contains(&"HEAD".to_string()));

            assert_eq!(directives.len(), 1);

            match &directives[0] {
                Directive::Respond { status, body } => {
                    assert_eq!(*status, 200);
                    assert_eq!(body, "Hello World");
                }
                _ => panic!("Expected nested Respond directive"),
            }
        }
        _ => panic!("Expected Method directive"),
    }
}

#[test]
fn test_parse_respond_directive() {
    let config_str = r#"
localhost:8080 {
    respond 200 "Service Unavailable"
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 1);

    match &site_config.directives[0] {
        Directive::Respond { status, body } => {
            assert_eq!(*status, 200);
            assert_eq!(body, "Service Unavailable");
        }
        _ => panic!("Expected Respond directive"),
    }
}

#[test]
fn test_parse_multiple_sites() {
    let config_str = r#"
localhost:8080 {
    reverse_proxy backend:3000
}

localhost:8081 {
    reverse_proxy backend:3001
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    assert_eq!(config.sites.len(), 2);
    assert!(config.sites.contains_key("localhost:8080"));
    assert!(config.sites.contains_key("localhost:8081"));
}

#[test]
fn test_parse_invalid_config() {
    let config_str = r#"
localhost:8080 {
    invalid_directive
}
"#;

    let result = Config::from_str(config_str);
    assert!(
        result.is_err(),
        "Expected parse error for invalid directive"
    );
}

#[test]
fn test_parse_empty_config() {
    let config_str = "";

    let config = Config::from_str(config_str).expect("Failed to parse empty config");
    assert_eq!(config.sites.len(), 0);
}

#[test]
fn test_parse_nested_handle_path() {
    let config_str = r#"
localhost:8080 {
    handle_path /api/v1/* {
        handle_path /users/* {
            reverse_proxy user-service:8001
        }
        reverse_proxy api-service:8000
    }
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 1);

    match &site_config.directives[0] {
        Directive::HandlePath {
            pattern,
            directives,
        } => {
            assert_eq!(pattern, "/api/v1/*");
            assert_eq!(directives.len(), 2);

            // Check nested handle_path
            match &directives[0] {
                Directive::HandlePath {
                    pattern,
                    directives,
                } => {
                    assert_eq!(pattern, "/users/*");
                    assert_eq!(directives.len(), 1);

                    match &directives[0] {
                        Directive::ReverseProxy { to } => {
                            assert_eq!(to, "user-service:8001");
                        }
                        _ => panic!("Expected nested ReverseProxy directive"),
                    }
                }
                _ => panic!("Expected nested HandlePath directive"),
            }
        }
        _ => panic!("Expected HandlePath directive"),
    }
}

#[test]
fn test_parse_multiple_headers() {
    let config_str = r#"
localhost:8080 {
    header X-Custom-Header custom-value
    header X-Another-Header another-value
    reverse_proxy backend:3000
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 3);

    match &site_config.directives[0] {
        Directive::Header { name, value } => {
            assert_eq!(name, "X-Custom-Header");
            assert_eq!(value, "custom-value");
        }
        _ => panic!("Expected Header directive"),
    }

    match &site_config.directives[1] {
        Directive::Header { name, value } => {
            assert_eq!(name, "X-Another-Header");
            assert_eq!(value, "another-value");
        }
        _ => panic!("Expected Header directive"),
    }
}

#[test]
fn test_parse_with_comments_and_whitespace() {
    let config_str = r#"
# Configuration for main site
localhost:8080 {
    # Forward to backend
    reverse_proxy backend:3000

    # Add custom headers
    header X-Proxy tiny-proxy

    # Replace URI prefix
    uri_replace /old-path /new-path
}
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");

    assert_eq!(config.sites.len(), 1);

    let site_config = config.sites.get("localhost:8080").expect("Site not found");
    assert_eq!(site_config.directives.len(), 3);
}
