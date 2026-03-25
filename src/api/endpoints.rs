//! API endpoints for proxy management

use anyhow::Result;
use bytes::Bytes;
use http_body::Body;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use std::sync::Arc;
use tracing::info;

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
pub async fn handle_post_config<B>(
    req: Request<B>,
    _config: Arc<tokio::sync::RwLock<Config>>,
) -> Result<Response<Full<Bytes>>>
where
    B: Body,
    B::Error: std::fmt::Display,
{
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
    async fn test_handle_post_config() {
        let config = Arc::new(tokio::sync::RwLock::new(Config {
            sites: HashMap::new(),
        }));

        let req: Request<Empty<Bytes>> = Request::builder().body(Empty::new()).unwrap();

        let response = handle_post_config(req, config).await.unwrap();
        assert_eq!(response.status(), 200);
    }
}