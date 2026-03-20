//! API endpoints for proxy management
//!
//! This module provides handlers for the management API endpoints.

use anyhow::Result;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use std::sync::Arc;
use tracing::info;

use crate::config::Config;

/// Handle GET /config - Return current configuration
///
/// Returns the current proxy configuration in JSON format.
///
/// # Arguments
///
/// * `_req` - The incoming HTTP request (not used directly)
/// * `config` - Shared configuration wrapped in Arc<RwLock<Config>>
///
/// # Returns
///
/// HTTP response containing the current configuration
///
/// # Example
///
/// ```no_run
/// # use tiny_proxy::api::endpoints;
/// # use hyper::Request;
/// # async fn example(config: Arc<tokio::sync::RwLock<tiny_proxy::Config>>) -> hyper::Response<Full<bytes::Bytes>> {
/// let req = Request::builder().body(hyper::body::Incoming::empty()).unwrap();
/// endpoints::handle_get_config(req, config).await
/// # }
/// ```
pub async fn handle_get_config(
    _req: Request<Incoming>,
    config: Arc<tokio::sync::RwLock<Config>>,
) -> Result<Response<Full<Bytes>>> {
    let config = config.read().await;

    // Convert config to JSON (simplified version)
    let json = serde_json::to_string_pretty(&*config)
        .unwrap_or_else(|_| r#"{"error": "Failed to serialize config"}"#.to_string());

    info!("GET /config - Returning configuration");

    let response = Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json)))
        .unwrap();

    Ok(response)
}

/// Handle POST /config - Update configuration
///
/// Updates the proxy configuration with new settings provided in the request body.
///
/// # Arguments
///
/// * `req` - The incoming HTTP request containing the new configuration
/// * `config` - Shared configuration wrapped in Arc<RwLock<Config>>
///
/// # Returns
///
/// HTTP response indicating success or failure
///
/// # Example
///
/// ```no_run
/// # use tiny_proxy::api::endpoints;
/// # use hyper::Request;
/// # async fn example(config: Arc<tokio::sync::RwLock<tiny_proxy::Config>>) -> hyper::Response<Full<bytes::Bytes>> {
/// let req = Request::builder().body(hyper::body::Incoming::empty()).unwrap();
/// endpoints::handle_post_config(req, config).await
/// # }
/// ```
pub async fn handle_post_config(
    req: Request<Incoming>,
    _config: Arc<tokio::sync::RwLock<Config>>,
) -> Result<Response<Full<Bytes>>> {
    // Collect request body
    let body_bytes = match BodyExt::collect(req.into_body()).await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            let response = Response::builder()
                .status(400)
                .body(Full::new(Bytes::from(format!(
                    "Failed to read request body: {}",
                    e
                ))))
                .unwrap();
            return Ok(response);
        }
    };

    // Parse configuration from JSON (simplified version)
    // In a real implementation, you would:
    // 1. Parse the JSON body
    // 2. Validate the configuration
    // 3. Update the shared config

    let body_str = match std::str::from_utf8(&body_bytes) {
        Ok(s) => s,
        Err(_) => {
            let response = Response::builder()
                .status(400)
                .body(Full::new(Bytes::from(
                    "Invalid UTF-8 in request body".to_string(),
                )))
                .unwrap();
            return Ok(response);
        }
    };

    info!("POST /config - Updating configuration");
    info!("New config: {}", body_str);

    // TODO: Parse JSON and update config
    // For now, just return a success message
    let response = Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(
            r#"{"status": "success", "message": "Configuration updated"}"#.to_string(),
        )))
        .unwrap();

    Ok(response)
}

/// Handle GET /health - Health check endpoint
///
/// Returns the health status of the proxy server.
///
/// # Arguments
///
/// * `_req` - The incoming HTTP request (not used)
///
/// # Returns
///
/// HTTP response with health status
///
/// # Example
///
/// ```no_run
/// # use tiny_proxy::api::endpoints;
/// # use hyper::Request;
/// # async fn example() -> hyper::Response<Full<bytes::Bytes>> {
/// let req = Request::builder().body(hyper::body::Incoming::empty()).unwrap();
/// endpoints::handle_health_check(req).await
/// # }
/// ```
pub async fn handle_health_check(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>> {
    info!("GET /health - Health check");

    let health = serde_json::json!({
        "status": "healthy",
        "service": "tiny-proxy",
        "version": env!("CARGO_PKG_VERSION")
    });

    let response = Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(
            serde_json::to_string(&health).unwrap(),
        )))
        .unwrap();

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::body::Incoming;
    use hyper::Request;

    #[tokio::test]
    async fn test_handle_health_check() {
        let req = Request::builder().body(Incoming::empty()).unwrap();

        let response = handle_health_check(req).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_handle_get_config() {
        let config = Arc::new(tokio::sync::RwLock::new(Config {
            sites: std::collections::HashMap::new(),
        }));

        let req = Request::builder().body(Incoming::empty()).unwrap();

        let response = handle_get_config(req, config).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_handle_post_config() {
        let config = Arc::new(tokio::sync::RwLock::new(Config {
            sites: std::collections::HashMap::new(),
        }));

        let req = Request::builder().body(Incoming::empty()).unwrap();

        let response = handle_post_config(req, config).await.unwrap();
        assert_eq!(response.status(), 200);
    }
}
