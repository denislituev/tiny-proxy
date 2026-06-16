//! Integration tests for `header_up` on upstream requests.

use std::collections::HashMap;
use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tiny_proxy::config::{Directive, HeaderDirective, SiteConfig};
use tiny_proxy::{Config, Proxy};
use tokio::net::TcpListener;

async fn get_random_port_addr() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

/// Backend that echoes the `Host` and `X-Original-Uri` request headers.
async fn echo_upstream_headers(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let uri = req
        .headers()
        .get("x-original-uri")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let has_accept_encoding = req.headers().contains_key("accept-encoding");

    let body = format!("host={host}|uri={uri}|ae={has_accept_encoding}");
    Ok(Response::new(Full::new(Bytes::from(body))))
}

async fn start_echo_backend() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let service = service_fn(echo_upstream_headers);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    addr
}

fn proxy_config(proxy_addr: std::net::SocketAddr, backend_addr: std::net::SocketAddr) -> Config {
    let host = format!("127.0.0.1:{}", proxy_addr.port());
    let mut sites = HashMap::new();
    sites.insert(
        host.clone(),
        SiteConfig {
            address: host,
            directives: vec![Directive::ReverseProxy {
                to: format!("http://{}", backend_addr),
                connect_timeout: None,
                read_timeout: None,
                header_up: vec![
                    HeaderDirective {
                        name: "Host".to_string(),
                        value: Some("api.example.com".to_string()),
                    },
                    HeaderDirective {
                        name: "X-Original-Uri".to_string(),
                        value: Some("{request.uri}".to_string()),
                    },
                    HeaderDirective {
                        name: "Accept-Encoding".to_string(),
                        value: None,
                    },
                ],
            }],
            tls: None,
        },
    );
    Config { sites }
}

#[tokio::test]
async fn test_header_up_reaches_backend() {
    let backend_addr = start_echo_backend().await;
    let proxy_addr = get_random_port_addr().await;
    let proxy_host = format!("127.0.0.1:{}", proxy_addr.port());

    let proxy = Proxy::new(proxy_config(proxy_addr, backend_addr));
    tokio::spawn(async move {
        proxy.start_with_addr(proxy_addr).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build::<_, Full<Bytes>>(hyper_util::client::legacy::connect::HttpConnector::new());

    let uri = format!("http://{proxy_host}/items?limit=3")
        .parse()
        .unwrap();
    let response = client.get(uri).await.expect("proxy request should succeed");
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(
        std::str::from_utf8(&body).unwrap(),
        "host=api.example.com|uri=/items?limit=3|ae=false"
    );
}
