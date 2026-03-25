//! Middleware for API requests

use hyper::{Request, Response, StatusCode};
use http_body::Body;
use http_body_util::Full;
use bytes::Bytes;

/// API authentication middleware
pub async fn auth_middleware<B>(
    req: Request<B>,
    api_key: &str,
) -> Result<Request<B>, Response<Full<Bytes>>>
where
    B: Body,
{
    let provided_key = req
        .headers()
        .get("X-API-Key")
        .and_then(|h: &hyper::header::HeaderValue| h.to_str().ok());

    match provided_key {
        Some(key) if key == api_key => Ok(req),
        Some(_) => Err(unauthorized_response("Invalid API key")),
        None => Err(unauthorized_response("Missing API key")),
    }
}

fn unauthorized_response(message: &str) -> Response<Full<Bytes>> {
    let body = format!(r#"{{"error": "Unauthorized", "message": "{}"}}"#, message);

    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

/// Logging middleware
pub fn logging_middleware<B: Body>(req: Request<B>) -> Request<B> {
    let method = req.method();
    let path = req.uri().path();
    let client_ip = req
        .headers()
        .get("X-Real-IP")
        .or_else(|| req.headers().get("X-Forwarded-For"))
        .and_then(|h: &hyper::header::HeaderValue| h.to_str().ok())
        .unwrap_or("unknown");

    tracing::info!("API request from {}: {} {}", client_ip, method, path);

    req
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::Request;
    use http_body_util::Empty;

    #[tokio::test]
    async fn test_auth_middleware_valid_key() {
        let req: Request<Empty<Bytes>> = Request::builder()
            .header("X-API-Key", "secret-key-123")
            .body(Empty::new())
            .unwrap();

        let api_key = "secret-key-123";
        let result = auth_middleware(req, api_key).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_auth_middleware_invalid_key() {
        let req: Request<Empty<Bytes>> = Request::builder()
            .header("X-API-Key", "wrong-key")
            .body(Empty::new())
            .unwrap();

        let api_key = "secret-key-123";
        let result = auth_middleware(req, api_key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auth_middleware_missing_key() {
        let req: Request<Empty<Bytes>> = Request::builder()
            .body(Empty::new())
            .unwrap();

        let api_key = "secret-key-123";
        let result = auth_middleware(req, api_key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_logging_middleware() {
        let req: Request<Empty<Bytes>> = Request::builder()
            .header("X-Real-IP", "192.168.1.1")
            .body(Empty::new())
            .unwrap();

        let _logged_req = logging_middleware(req);
    }

    #[tokio::test]
    async fn test_unauthorized_response() {
        let response = unauthorized_response("Test error");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let content_type = response
            .headers()
            .get("Content-Type")
            .and_then(|h| h.to_str().ok());
        assert_eq!(content_type, Some("application/json"));
    }
}