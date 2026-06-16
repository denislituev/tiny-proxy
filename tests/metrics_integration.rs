//! Integration tests for the Prometheus `/metrics` endpoint.
//!
//! All checks run in a single test because the Prometheus exporter installs a
//! process-global recorder whose HTTP listener is tied to the tokio runtime
//! that started it. Sharing one runtime keeps the listener alive across checks.

#![cfg(feature = "metrics")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use tiny_proxy::config::{Directive, SiteConfig};
use tiny_proxy::metrics;
use tiny_proxy::{Config, Proxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn make_config(host_port: &str) -> Config {
    let mut sites = HashMap::new();
    sites.insert(
        host_port.to_string(),
        SiteConfig {
            address: host_port.to_string(),
            directives: vec![Directive::Respond {
                status: 200,
                body: "ok".to_string(),
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

/// Send one HTTP/1.1 request, return the full raw response.
async fn http_get_raw(stream: &mut TcpStream, host: &str, path: &str) -> String {
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut buf)).await;
    String::from_utf8_lossy(&buf).to_string()
}

/// GET `/metrics` from the admin port.
async fn scrape_metrics(metrics_port: u16) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{metrics_port}"))
        .await
        .unwrap();
    http_get_raw(
        &mut stream,
        &format!("127.0.0.1:{metrics_port}"),
        "/metrics",
    )
    .await
}

/// Spawn a proxy on a random port and wait until it accepts connections.
async fn spawn_proxy() -> std::net::SocketAddr {
    let addr = get_random_port_addr().await;
    let host = format!("127.0.0.1:{}", addr.port());
    let shared = Arc::new(ArcSwap::from_pointee(make_config(&host)));
    let proxy = Proxy::from_shared(shared);
    tokio::spawn(async move {
        let _ = proxy.start(&addr.to_string()).await;
    });
    for _ in 0..40 {
        if TcpStream::connect(addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    addr
}

#[tokio::test(flavor = "multi_thread")]
async fn test_metrics_endpoint() {
    // --- bring up the metrics server on a random port ---
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let metrics_addr = listener.local_addr().unwrap();
    let metrics_port = metrics_addr.port();
    drop(listener);
    metrics::start_metrics_server(metrics_addr).expect("install prometheus recorder");

    // install() spawns the HTTP listener on a background thread; wait for it.
    for _ in 0..100 {
        if TcpStream::connect(metrics_addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // --- bring up the proxy under test ---
    let proxy_addr = spawn_proxy().await;
    let proxy_host = format!("127.0.0.1:{}", proxy_addr.port());

    // ---------------------------------------------------------------
    // 1. request counter increments + labels are present
    // ---------------------------------------------------------------
    let before = scrape_metrics(metrics_port).await;
    let before_count = extract_counter_value(&before, "http_requests_total");

    for _ in 0..3 {
        let mut s = TcpStream::connect(proxy_addr).await.unwrap();
        let _ = http_get_raw(&mut s, &proxy_host, "/test").await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    let after = scrape_metrics(metrics_port).await;
    let after_count = extract_counter_value(&after, "http_requests_total");
    assert!(
        after_count >= before_count + 3,
        "counter should increase by >=3: before={before_count} after={after_count}"
    );
    assert!(
        after.contains(r#"http_requests_total{method="GET",status="200""#),
        "expected labeled counter line in:\n{after}"
    );

    // ---------------------------------------------------------------
    // 2. latency histogram (bucket lines, not quantiles)
    // ---------------------------------------------------------------
    assert!(
        after.contains("http_request_duration_seconds_bucket"),
        "expected histogram buckets in:\n{after}"
    );
    assert!(
        !after.contains("http_request_duration_seconds{"),
        "metric should not be rendered as a summary (quantile form)"
    );
    assert!(
        after.contains(r#"le="+Inf""#),
        "expected +Inf bucket in:\n{after}"
    );

    // ---------------------------------------------------------------
    // 3. active-requests gauge returns to 0 once requests finish
    // ---------------------------------------------------------------
    let gauge_value = extract_gauge_value(&after, "http_active_requests");
    assert_eq!(
        gauge_value, 0.0,
        "active requests gauge should be 0 after request completes, got {gauge_value}"
    );

    // ---------------------------------------------------------------
    // 4. TLS handshake counter accepts ok/fail labels
    // ---------------------------------------------------------------
    metrics::tls_handshake("ok");
    metrics::tls_handshake("fail");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let body = scrape_metrics(metrics_port).await;
    assert!(
        body.contains(r#"tls_handshakes_total{status="ok"}"#),
        "expected tls_handshakes_total{{status=\"ok\"}} in:\n{body}"
    );
    assert!(
        body.contains(r#"tls_handshakes_total{status="fail"}"#),
        "expected tls_handshakes_total{{status=\"fail\"}} in:\n{body}"
    );

    // ---------------------------------------------------------------
    // 5. HELP / TYPE metadata for every metric (must come after each
    //    metric has at least one recorded sample)
    // ---------------------------------------------------------------
    for (name, type_str) in [
        ("http_requests_total", "counter"),
        ("http_request_duration_seconds", "histogram"),
        ("http_active_requests", "gauge"),
        ("tls_handshakes_total", "counter"),
    ] {
        assert!(
            body.contains(&format!("# HELP {name} ")),
            "missing HELP line for {name} in:\n{body}"
        );
        assert!(
            body.contains(&format!("# TYPE {name} {type_str}")),
            "missing TYPE line for {name} in:\n{body}"
        );
    }
}

/// Sum all sample values of a counter across all label sets.
fn extract_counter_value(metrics: &str, name: &str) -> u64 {
    metrics
        .lines()
        .filter(|l| l.starts_with(name) && l.contains('{'))
        .filter_map(|l| l.rsplit(' ').next())
        .filter_map(|v| v.parse::<u64>().ok())
        .sum()
}

/// Parse a label-less gauge (`name 42.0`).
fn extract_gauge_value(metrics: &str, name: &str) -> f64 {
    for line in metrics.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(name) {
            let rest = rest.trim_start();
            if !rest.starts_with('{') {
                if let Some(val) = rest.split_whitespace().next() {
                    if let Ok(v) = val.parse::<f64>() {
                        return v;
                    }
                }
            }
        }
    }
    f64::NAN
}
