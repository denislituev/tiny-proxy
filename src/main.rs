mod config;

use crate::config::SiteConfig;
use bytes::Bytes;
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

static TARGET: &str = "http://127.0.0.1:5001";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::from_file("./file.caddy")?;
    println!("Config: {:?}", config);

    let addr: SocketAddr = "127.0.0.1:8080".parse()?;
    let listener = TcpListener::bind(addr).await?;

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

async fn proxy(
    mut req: Request<Incoming>,
    client: Client<HttpsConnector<HttpConnector>, Incoming>,
    config: config::Config,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Build a new URI
    // let path = req.uri().path_and_query().map(|x| x.as_str()).unwrap_or("/");
    let path = req.uri().host().unwrap();
    println!("{:?} {}", req, path);
    let site_config = config.sites.get(path).unwrap();
    // println!("{:?}", site_config);

    let target_base = site_config.address.trim_end_matches('/');
    // let target_base = TARGET.trim_end_matches('/');
    let full_url = format!("{}{}", target_base, path);

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
                eprintln!(
                    "   Reason: Connection refused - backend {} unavailable",
                    TARGET
                );
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
