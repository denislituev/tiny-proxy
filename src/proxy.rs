use anyhow::Error;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::config::Config;

/// Result of directive processing
#[derive(Debug, Clone)]
pub enum ActionResult {
    Respond {
        status: u16,
        body: String,
    },
    ReverseProxy {
        backend_url: String,
        path_to_send: String,
    },
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
            // Apply header modifications
            crate::config::Directive::Header { name, value } => {
                if let Ok(header_name) = hyper::header::HeaderName::from_bytes(name.as_bytes()) {
                    if let Ok(header_value) = hyper::header::HeaderValue::from_str(value.as_str()) {
                        req.headers_mut().insert(header_name, header_value);
                        info!("   Applied header: {}", name);
                    }
                }
            }

            // Apply URI replacements
            crate::config::Directive::UriReplace { find, replace } => {
                modified_path = modified_path.replace(find, replace);
                info!("   Applied uri_replace: {} → {}", find, replace);
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

            // Direct response - return immediately
            crate::config::Directive::Respond { status, body } => {
                info!("   Returning direct response: {}", status);
                return Ok(ActionResult::Respond {
                    status: *status,
                    body: body.clone(),
                });
            }

            // Reverse proxy - return action with current (possibly modified) path
            crate::config::Directive::ReverseProxy { to } => {
                info!("   Proxying to: {}", to);
                return Ok(ActionResult::ReverseProxy {
                    backend_url: to.clone(),
                    path_to_send: modified_path,
                });
            }

            _ => continue,
        }
    }

    Err(format!(
        "No action directive (respond or reverse_proxy) found in configuration for path: {}",
        current_path
    ))
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

/// Start the proxy server on the specified address
pub async fn start_proxy(
    addr: SocketAddr,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(&addr).await?;

    let https = HttpsConnector::new();
    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build::<_, Incoming>(https);

    info!("Tiny Proxy listening on http://{}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let client = client.clone();
        let config = config.clone();

        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                let client = client.clone();
                proxy(req, client, config.clone())
            });

            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                error!("Error serving connection: {:?}", err);
            }
        });
    }
}

/// Process a single request through the proxy
async fn proxy(
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
    let site_config = config.sites.get(host).ok_or_else(|| {
        error!("No configuration found for host: {}", host);
        error!(
            "Available hosts in config: {:?}",
            config.sites.keys().collect::<Vec<_>>()
        );
        anyhow::Error::msg(format!("No configuration found for host: {}", host))
    })?;

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
