//! API server for proxy management
//!
//! This module provides a REST API for managing the proxy configuration,
//! including viewing and updating configuration settings.

use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::api::endpoints;

use crate::config::Config;
use crate::error::Result;

/// Start the API server for proxy management
///
/// This server provides REST endpoints for:
/// - GET /config - Get current configuration
/// - POST /config - Update configuration
/// - GET /health - Health check endpoint
///
/// # Arguments
///
/// * `addr` - Address to listen on (e.g., "127.0.0.1:8081")
/// * `config` - Shared configuration wrapped in Arc<RwLock<Config>>
///
/// # Example
///
/// ```no_run
/// # use tiny_proxy::{Config, api};
/// # use std::sync::Arc;
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let config = Arc::new(tokio::sync::RwLock::new(Config::from_file("config.caddy")?));
/// api::server::start_api_server("127.0.0.1:8081", config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn start_api_server(addr: &str, config: Arc<tokio::sync::RwLock<Config>>) -> Result<()> {
    let addr: SocketAddr = addr.parse()?;
    start_api_server_with_addr(addr, config).await
}

/// Start the API server with a parsed SocketAddr
///
/// This is a convenience method if you already have a parsed SocketAddr.
///
/// # Arguments
///
/// * `addr` - Parsed SocketAddr to listen on
/// * `config` - Shared configuration wrapped in Arc<RwLock<Config>>
pub async fn start_api_server_with_addr(
    addr: SocketAddr,
    config: Arc<tokio::sync::RwLock<Config>>,
) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;

    info!("API server listening on http://{}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let config = config.clone();

        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                let config = config.clone();
                handle_api_request(req, config)
            });

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                error!("Error serving API connection: {:?}", err);
            }
        });
    }
}

/// Handle incoming API requests
///
/// Routes requests to appropriate endpoints based on method and path.
async fn handle_api_request(
    req: Request<Incoming>,
    config: Arc<tokio::sync::RwLock<Config>>,
) -> anyhow::Result<Response<Full<bytes::Bytes>>> {
    // TODO: Add authentication middleware if needed
    // let req = middleware::auth_middleware(req, api_key).await?;

    let path = req.uri().path();
    let method = req.method();

    info!("API request: {} {}", method, path);

    match (method.as_str(), path) {
        ("GET", "/config") => endpoints::handle_get_config(req, config).await,
        ("POST", "/config") => endpoints::handle_post_config(req, config).await,
        ("GET", "/health") => endpoints::handle_health_check(req).await,
        _ => {
            // 404 Not Found
            let response = Response::builder()
                .status(404)
                .body(Full::new(bytes::Bytes::from("Not Found".to_string())))
                .unwrap();
            Ok(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_server_address_parsing() {
        let addr: SocketAddr = "127.0.0.1:8081".parse().unwrap();
        assert_eq!(addr.port(), 8081);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }
}
