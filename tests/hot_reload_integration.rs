//! Integration test: hot-reload applies on keep-alive connections (per-request config load).

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use tiny_proxy::config::{Directive, SiteConfig};
use tiny_proxy::{Config, Proxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn make_config(host_port: &str, body: &str) -> Config {
    let mut sites = HashMap::new();
    sites.insert(
        host_port.to_string(),
        SiteConfig {
            address: host_port.to_string(),
            directives: vec![Directive::Respond {
                status: 200,
                body: body.to_string(),
            }],
            tls: None,
        },
    );
    Config { sites }
}

async fn get_random_port_addr() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

/// Send one HTTP/1.1 request on an existing stream and return the response body.
async fn http_get_on_stream(stream: &mut TcpStream, host: &str, path: &str) -> String {
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: keep-alive\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);

    // Body follows the header block (after \r\n\r\n)
    response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string()
}

#[tokio::test]
async fn test_hot_reload_on_keep_alive_connection() {
    let addr = get_random_port_addr().await;
    let host = format!("127.0.0.1:{}", addr.port());

    let shared = Arc::new(ArcSwap::from_pointee(make_config(&host, "version-1")));
    let proxy = Proxy::from_shared(shared.clone());

    let listen_addr = addr;
    tokio::spawn(async move {
        proxy.start_with_addr(listen_addr).await.unwrap();
    });

    // Let the listener come up
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    let body1 = http_get_on_stream(&mut stream, &host, "/").await;
    assert_eq!(body1, "version-1");

    shared.store(Arc::new(make_config(&host, "version-2")));

    let body2 = http_get_on_stream(&mut stream, &host, "/").await;
    assert_eq!(
        body2, "version-2",
        "second request on same keep-alive connection must use reloaded config"
    );
}
