mod cli;
mod config;

use bytes::Bytes;
use clap::Parser;
use cli::Cli;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Tiny Proxy Server - Simple HTTP proxy with Caddy-like configuration

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    println!("🚀 Tiny Proxy Server v{}", env!("CARGO_PKG_VERSION"));
    println!("📄 Loading config from: {}", cli.config);

    let config = config::Config::from_file(&cli.config)?;

    let addr: SocketAddr = cli.addr.parse()?;
    let listener = TcpListener::bind(&addr).await?;

    let https = HttpsConnector::new();
    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build::<_, Incoming>(https);

    println!("🚀 Tiny Proxy listening on http://{}", addr);

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
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

// Match path against pattern (supports wildcard *)
// Returns Some(remaining_path) if match, None otherwise
fn match_pattern(pattern: &str, path: &str) -> Option<String> {
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

// Find matching directive for given path
fn find_matching_directive(
    path: &str,
    site_config: &config::SiteConfig,
) -> Option<config::Directive> {
    for directive in &site_config.directives {
        match directive {
            config::Directive::HandlePath {
                pattern,
                directives,
            } => {
                if let Some(_) = match_pattern(pattern, path) {
                    // Found matching handle_path, return first nested directive
                    return directives.first().cloned();
                }
            }
            config::Directive::ReverseProxy { .. } => {
                // Direct reverse_proxy without handle_path
                return Some(directive.clone());
            }
            config::Directive::Respond { .. } => {
                // Direct respond
                return Some(directive.clone());
            }
            _ => {
                // Ignore other directives for now (header, uri_replace)
                continue;
            }
        }
    }
    None
}

async fn proxy(
    mut req: Request<Incoming>,
    client: Client<HttpsConnector<HttpConnector>, Incoming>,
    config: config::Config,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Get path from URI
    let path = req.uri().path();

    // Get host from Host header (includes port, e.g., "localhost:8080")
    let host = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");

    println!("Request: {} from {}", path, host);

    // Find site configuration by host (with port!)
    let site_config = config.sites.get(host).unwrap_or_else(|| {
        eprintln!("❌ No configuration found for host: {}", host);
        eprintln!(
            "Available hosts in config: {:?}",
            config.sites.keys().collect::<Vec<_>>()
        );
        panic!("No configuration found for host: {}", host);
    });

    // Find matching directive for this path
    let directive = find_matching_directive(path, site_config).unwrap_or_else(|| {
        eprintln!("❌ No matching directive for path: {}", path);
        eprintln!("Available directives: {:?}", site_config.directives);
        panic!("No matching directive for path: {}", path);
    });

    // Get backend URL and compute path to send
    let (backend_url, path_to_send) = match directive {
        config::Directive::ReverseProxy { to } => (to.clone(), path.to_string()),
        config::Directive::HandlePath {
            pattern,
            directives,
            ..
        } => {
            // Get remaining path after removing prefix
            let path_to_send =
                match_pattern(pattern.as_str(), path).unwrap_or_else(|| "/".to_string());

            // Get backend from nested reverse_proxy
            let backend = directives
                .iter()
                .find_map(|d| {
                    if let config::Directive::ReverseProxy { to } = d {
                        Some(to.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("No reverse_proxy in handle_path"));

            (backend, path_to_send)
        }
        config::Directive::Respond { status, body } => {
            // Direct response - return immediately
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            return Ok(Response::builder()
                .status(status_code)
                .body(Full::new(Bytes::from(body.clone())))
                .unwrap());
        }
        _ => panic!("Unsupported directive type"),
    };

    // Add protocol if missing
    let backend_with_proto =
        if backend_url.starts_with("http://") || backend_url.starts_with("https://") {
            backend_url
        } else {
            format!("http://{}", backend_url)
        };

    let full_url = format!("{}{}", backend_with_proto, path_to_send);

    println!("Proxying to: {}", full_url);

    let new_uri = match full_url.parse::<Uri>() {
        Ok(uri) => uri,
        Err(e) => {
            eprintln!("Invalid URI: {:?}", e);
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
                    eprintln!("❌ Error reading response body: {:?}", e);
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
            eprintln!("❌ Backend connection failed: {}", error_msg);

            // Check error type for more detailed logging
            if e.is_connect() {
                eprintln!("   Reason: Connection refused - backend unavailable");
            } else {
                eprintln!("   Reason: Other connection error");
            }

            Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "Backend service unavailable",
            ))
        }
    }
}

/// Creates HTTP response with error
fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
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
