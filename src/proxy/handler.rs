use anyhow::Error;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use tracing::{error, info};

use crate::config::{Config};
use crate::proxy::ActionResult;

use crate::proxy::directives::{
    handle_header, handle_method, handle_respond, handle_reverse_proxy, handle_uri_replace,
};

/// Process directives in order, applying modifications and returning final action
/// Supports recursive handling of handle_path blocks
pub fn process_directives(
    directives: &[crate::config::Directive],
    req: &mut Request<Incoming>,
    current_path: &str,
) -> Result<ActionResult, String> {
    let mut modified_path = current_path.to_string();

    for directive in directives {
        match directive {
            // Apply header modifications using directive handler
            crate::config::Directive::Header { name, value } => {
                if let Err(e) = handle_header(name, value, req) {
                    info!("   Failed to apply header {}: {}", name, e);
                }
            }

            // Apply URI replacements using directive handler
            crate::config::Directive::UriReplace { find, replace } => {
                handle_uri_replace(find, replace, &mut modified_path);
            }

            // Handle path-based routing recursively
            crate::config::Directive::HandlePath {
                pattern,
                directives: nested_directives,
            } => {
                if let Some(remaining_path) = match_pattern(pattern, &modified_path) {
                    info!("   Matched handle_path: {}", pattern);
                    // Recursively process nested directives with remaining path
                    return process_directives(nested_directives, req, &remaining_path);
                }
            }

            // Method-based directives
            crate::config::Directive::Method {
                methods,
                directives: nested_directives,
            } => {
                if handle_method(methods, req) {
                    info!("   Matched method directive");
                    // Process nested directives with same path
                    return process_directives(nested_directives, req, &modified_path);
                }
            }

            // Direct response - return immediately using directive handler
            crate::config::Directive::Respond { status, body } => {
                return Ok(handle_respond(status, body));
            }

            // Reverse proxy - return action using directive handler
            crate::config::Directive::ReverseProxy { to } => {
                return Ok(handle_reverse_proxy(to, &modified_path));
            }
        }
    }

    Err(format!(
        "No action directive (respond or reverse_proxy) found in configuration for path: {}",
        current_path
    ))
}

/// Process a single request through the proxy
pub async fn proxy(
    mut req: Request<Incoming>,
    client: Client<HttpsConnector<HttpConnector>, Incoming>,
    config: Config,
) -> Result<Response<Full<Bytes>>, Error> {
    // Get path from URI
    let path = req.uri().path().to_string();

    // Get host from Host header (includes port, e.g., "localhost:8080")
    let host = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");

    info!("Request: {} from {}", path, host);

    // Find site configuration by host (with port!)
    let site_config = match config.sites.get(host) {
        Some(config) => config,
        None => {
            error!("No configuration found for host: {}", host);
            return Ok(error_response(
                StatusCode::NOT_FOUND,
                &format!("No configuration found for host: {}", host)
            ));
        }
    };

    // Process directives in correct order
    let action_result =
        process_directives(&site_config.directives, &mut req, &path).map_err(anyhow::Error::msg)?;

    // Execute action
    match action_result {
        ActionResult::Respond { status, body } => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            return Ok(Response::builder()
                .status(status_code)
                .body(Full::new(Bytes::from(body)))
                .unwrap());
        }
        ActionResult::ReverseProxy {
            backend_url,
            path_to_send,
        } => {
            // Add protocol if missing
            let backend_with_proto =
                if backend_url.starts_with("http://") || backend_url.starts_with("https://") {
                    backend_url
                } else {
                    format!("http://{}", backend_url)
                };

            let full_url = format!("{}{}", backend_with_proto, path_to_send);

            info!("Proxying to: {}", full_url);
            info!("   Original path: {}", path);
            info!("   Modified path: {}", path_to_send);

            let new_uri = match full_url.parse::<Uri>() {
                Ok(uri) => uri,
                Err(e) => {
                    error!("Invalid URI: {:?}", e);
                    return Ok(error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Invalid proxy URI",
                    ));
                }
            };

            *req.uri_mut() = new_uri.clone();

            // Update Host header for backend
            req.headers_mut().remove(hyper::header::HOST);
            if let Some(authority) = new_uri.authority() {
                if let Ok(host_value) = authority.as_str().parse() {
                    req.headers_mut().insert(hyper::header::HOST, host_value);
                }
            }

            // Forward request to backend
            match client.request(req).await {
                Ok(response) => {
                    // Successfully received response from backend
                    let status = response.status();
                    let headers = response.headers().clone();

                    // Convert Incoming to Full<Bytes>
                    match response.into_body().collect().await {
                        Ok(collected) => {
                            let bytes = collected.to_bytes();
                            let mut builder = Response::builder().status(status);

                            // Copy all headers from backend
                            for (name, value) in headers.iter() {
                                builder = builder.header(name, value);
                            }

                            Ok(builder.body(Full::new(bytes)).unwrap())
                        }
                        Err(e) => {
                            error!("Error reading response body: {:?}", e);
                            Ok(error_response(
                                StatusCode::BAD_GATEWAY,
                                "Error reading backend response",
                            ))
                        }
                    }
                }
                Err(e) => {
                    // Backend unavailable - return 502 Bad Gateway
                    let error_msg = format!("{:?}", e);
                    error!("Backend connection failed: {}", error_msg);

                    // Check error type for more detailed logging
                    if e.is_connect() {
                        error!("   Reason: Connection refused - backend unavailable");
                    } else {
                        error!("   Reason: Other connection error");
                    }

                    Ok(error_response(
                        StatusCode::BAD_GATEWAY,
                        "Backend service unavailable",
                    ))
                }
            }
        }
    }
}

/// Creates HTTP response with error
pub fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    let body = format!(
        r#"<!DOCTYPE html>
        <html>
        <head><title>{} {}</title></head>
        <body>
        <h1>{} {}</h1>
        <p>{}</p>
        <hr>
        <p><em>Rust Proxy Server</em></p>
        </body>
        </html>"#,
        status.as_u16(),
        status.canonical_reason().unwrap_or("Error"),
        status.as_u16(),
        status.canonical_reason().unwrap_or("Error"),
        message
    );

    Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

/// Match path against pattern (supports wildcard *)
/// Returns Some(remaining_path) if match, None otherwise
pub fn match_pattern(pattern: &str, path: &str) -> Option<String> {
    if pattern.ends_with("/*") {
        let prefix = &pattern[..pattern.len() - 2];
        if path.starts_with(prefix) {
            // Remove prefix and return remaining path
            let remaining = path.strip_prefix(prefix).unwrap_or(path);
            Some(remaining.to_string())
        } else {
            None
        }
    } else {
        if pattern == path {
            Some("/".to_string()) // Exact match, send root
        } else {
            None
        }
    }
}
