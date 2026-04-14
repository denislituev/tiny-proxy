//! API endpoints for proxy management

use anyhow::Result;
use bytes::Bytes;
use http_body::Body;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use std::sync::Arc;
use tracing::{error, info};

use crate::config::Config;

/// Handle GET /config
pub async fn handle_get_config<B>(
    _req: Request<B>,
    config: Arc<tokio::sync::RwLock<Config>>,
) -> Result<Response<Full<Bytes>>>
where
    B: Body,
{
    let config = config.read().await;

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

/// Handle POST /config
///
/// Accepts a JSON body representing the new configuration and atomically
/// replaces the current configuration. The new config takes effect
/// immediately for all new incoming proxy connections.
///
/// # Request Body
///
/// JSON representation of the full `Config` struct, e.g.:
/// ```json
/// {
///   "sites": {
///     "localhost:8080": {
///       "address": "localhost:8080",
///       "directives": [
///         { "ReverseProxy": { "to": "localhost:9001" } }
///       ]
///     }
///   }
/// }
/// ```
pub async fn handle_post_config<B>(
    req: Request<B>,
    config: Arc<tokio::sync::RwLock<Config>>,
) -> Result<Response<Full<Bytes>>>
where
    B: Body,
    B::Error: std::fmt::Display,
{
    let body_bytes = match BodyExt::collect(req.into_body()).await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            let error_json = serde_json::json!({
                "status": "error",
                "message": format!("Failed to read request body: {}", e)
            });
            let response = Response::builder()
                .status(400)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(
                    serde_json::to_string(&error_json).unwrap(),
                )))
                .unwrap();
            return Ok(response);
        }
    };

    let body_str = match std::str::from_utf8(&body_bytes) {
        Ok(s) => s,
        Err(_) => {
            let error_json = serde_json::json!({
                "status": "error",
                "message": "Invalid UTF-8 in request body"
            });
            let response = Response::builder()
                .status(400)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(
                    serde_json::to_string(&error_json).unwrap(),
                )))
                .unwrap();
            return Ok(response);
        }
    };

    // Parse JSON body into Config
    let new_config: Config = match serde_json::from_str(body_str) {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to parse config JSON: {}", e);
            let error_json = serde_json::json!({
                "status": "error",
                "message": format!("Invalid configuration JSON: {}", e)
            });
            let response = Response::builder()
                .status(400)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(
                    serde_json::to_string(&error_json).unwrap(),
                )))
                .unwrap();
            return Ok(response);
        }
    };

    // Atomically replace the configuration
    {
        let mut guard = config.write().await;
        let sites_count = new_config.sites.len();
        *guard = new_config;
        info!(
            "POST /config - Configuration updated successfully ({} sites)",
            sites_count
        );
    }

    let response = Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(
            r#"{"status": "success", "message": "Configuration updated"}"#.to_string(),
        )))
        .unwrap();

    Ok(response)
}

/// Handle GET /health
pub async fn handle_health_check<B>(_req: Request<B>) -> Result<Response<Full<Bytes>>>
where
    B: Body,
{
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
    use http_body_util::Empty;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_handle_health_check() {
        let req: Request<Empty<Bytes>> = Request::builder().body(Empty::new()).unwrap();

        let response = handle_health_check(req).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_handle_get_config() {
        let config = Arc::new(tokio::sync::RwLock::new(Config {
            sites: HashMap::new(),
        }));

        let req: Request<Empty<Bytes>> = Request::builder().body(Empty::new()).unwrap();

        let response = handle_get_config(req, config).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_handle_post_config_valid_json() {
        let config = Arc::new(tokio::sync::RwLock::new(Config {
            sites: HashMap::new(),
        }));

        let new_config_json = r#"{
            "sites": {
                "localhost:8080": {
                    "address": "localhost:8080",
                    "directives": [
                        {"ReverseProxy": {"to": "localhost:9001"}}
                    ]
                }
            }
        }"#;

        let req = Request::builder()
            .method("POST")
            .uri("/config")
            .body(Full::new(Bytes::from(new_config_json.to_string())))
            .unwrap();

        let response = handle_post_config(req, config.clone()).await.unwrap();
        assert_eq!(response.status(), 200);

        // Verify config was actually updated
        let guard = config.read().await;
        assert_eq!(guard.sites.len(), 1);
        assert!(guard.sites.contains_key("localhost:8080"));
    }

    #[tokio::test]
    async fn test_handle_post_config_invalid_json() {
        let config = Arc::new(tokio::sync::RwLock::new(Config {
            sites: HashMap::new(),
        }));

        let req = Request::builder()
            .method("POST")
            .uri("/config")
            .body(Full::new(Bytes::from("not valid json")))
            .unwrap();

        let response = handle_post_config(req, config.clone()).await.unwrap();
        assert_eq!(response.status(), 400);

        // Verify config was NOT updated
        let guard = config.read().await;
        assert_eq!(guard.sites.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_post_config_empty_body() {
        let config = Arc::new(tokio::sync::RwLock::new(Config {
            sites: HashMap::new(),
        }));

        let req: Request<Empty<Bytes>> = Request::builder()
            .method("POST")
            .uri("/config")
            .body(Empty::new())
            .unwrap();

        let response = handle_post_config(req, config).await.unwrap();
        assert_eq!(response.status(), 400);
    }
}
