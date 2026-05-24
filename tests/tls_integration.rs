//! Integration tests for TLS termination.
//!
//! Generates self-signed certificates at runtime using `rcgen`,
//! then exercises the full HTTPS pipeline: handshake, SNI, redirect, X-Forwarded-Proto.

use http_body_util::BodyExt;
use tiny_proxy::config::tls_redirect_port;

/// Generate a self-signed certificate + key pair (PEM) for the given hostname.
fn generate_self_signed_cert(hostname: &str) -> (String, String) {
    let mut params = rcgen::CertificateParams::new(vec![hostname.to_string()]).unwrap();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, hostname);

    let key_pair = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key_pair).unwrap();

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    (cert_pem, key_pem)
}

/// Write PEM data to a temp file, return the path.
fn write_temp_pem(prefix: &str, data: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::with_prefix(prefix).unwrap();
    use std::io::Write;
    f.write_all(data.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

/// Build a Config with a single HTTPS site for the given hostname + port.
fn make_tls_config(hostname_port: &str, cert_path: &str, key_path: &str) -> tiny_proxy::Config {
    let mut sites = std::collections::HashMap::new();
    sites.insert(
        hostname_port.to_string(),
        tiny_proxy::config::SiteConfig {
            address: hostname_port.to_string(),
            directives: vec![tiny_proxy::config::Directive::Respond {
                status: 200,
                body: "TLS OK".to_string(),
            }],
            tls: Some(tiny_proxy::config::TlsConfig {
                cert_path: cert_path.to_string(),
                key_path: key_path.to_string(),
            }),
        },
    );
    tiny_proxy::Config { sites }
}

/// Build a TLS root certificate store that trusts our self-signed cert.
fn build_tls_root_store(cert_pem: &str) -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    let cert_der = rustls_pemfile::certs(&mut std::io::BufReader::new(cert_pem.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    roots.add(cert_der[0].clone()).unwrap();
    roots
}

/// Build a hyper HTTP client that trusts our self-signed cert.
fn build_tls_https_client(
    roots: rustls::RootCertStore,
) -> hyper_util::client::legacy::Client<
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    http_body_util::Empty<bytes::Bytes>,
> {
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let mut http = hyper_util::client::legacy::connect::HttpConnector::new();
    http.enforce_http(false); // Allow HTTPS URLs to pass through

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .wrap_connector(http);

    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build::<_, http_body_util::Empty<bytes::Bytes>>(https)
}

/// Collect response body bytes.
async fn body_bytes(body: hyper::body::Incoming) -> Vec<u8> {
    body.collect().await.unwrap().to_bytes().to_vec()
}

/// Bind to a random port, return the address. Drops the listener
/// so the port is available for the proxy to bind.
async fn get_random_port_addr() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

#[tokio::test]
async fn test_tls_handshake_and_response() {
    // 1. Generate self-signed cert
    let (cert_pem, key_pem) = generate_self_signed_cert("localhost");
    let cert_file = write_temp_pem("cert", &cert_pem);
    let key_file = write_temp_pem("key", &key_pem);

    // 2. Build client trust store
    let roots = build_tls_root_store(&cert_pem);

    // 3. Get a random port
    let addr = get_random_port_addr().await;

    // 4. Build proxy config using the actual port
    let config = make_tls_config(
        &format!("localhost:{}", addr.port()),
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    );

    // 5. Start proxy
    let proxy = tiny_proxy::Proxy::new(config);
    let proxy_handle = tokio::spawn(async move { proxy.start(&addr.to_string()).await });

    // Give the proxy time to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 6. Make HTTPS request
    let client = build_tls_https_client(roots);
    let uri = format!("https://localhost:{}/", addr.port())
        .parse::<hyper::Uri>()
        .unwrap();

    let resp = client.get(uri).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = body_bytes(resp.into_body()).await;
    assert_eq!(&body[..], b"TLS OK");

    proxy_handle.abort();
}

#[tokio::test]
async fn test_tls_x_forwarded_proto_https() {
    let (cert_pem, key_pem) = generate_self_signed_cert("localhost");
    let cert_file = write_temp_pem("cert", &cert_pem);
    let key_file = write_temp_pem("key", &key_pem);

    let roots = build_tls_root_store(&cert_pem);

    // Start backend on random port — echoes X-Forwarded-Proto
    let backend_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_addr = backend_listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = backend_listener.accept().await.unwrap();
            let io = hyper_util::rt::TokioIo::new(stream);
            let service = hyper::service::service_fn(
                |req: hyper::Request<hyper::body::Incoming>| async move {
                    let proto = req
                        .headers()
                        .get("X-Forwarded-Proto")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("missing")
                        .to_string();
                    let body = http_body_util::Full::new(bytes::Bytes::from(proto))
                        .map_err(|e| -> std::convert::Infallible { e })
                        .boxed();
                    Ok::<_, std::convert::Infallible>(hyper::Response::new(body))
                },
            );
            tokio::spawn(async move {
                hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                    .ok();
            });
        }
    });

    // Get a random port for the proxy
    let proxy_addr = get_random_port_addr().await;

    // Proxy config pointing to backend
    let mut sites = std::collections::HashMap::new();
    sites.insert(
        format!("localhost:{}", proxy_addr.port()),
        tiny_proxy::config::SiteConfig {
            address: format!("localhost:{}", proxy_addr.port()),
            directives: vec![tiny_proxy::config::Directive::ReverseProxy {
                to: format!("http://127.0.0.1:{}", backend_addr.port()),
                connect_timeout: None,
                read_timeout: None,
            }],
            tls: Some(tiny_proxy::config::TlsConfig {
                cert_path: cert_file.path().to_str().unwrap().to_string(),
                key_path: key_file.path().to_str().unwrap().to_string(),
            }),
        },
    );

    let config = tiny_proxy::Config { sites };
    let proxy = tiny_proxy::Proxy::new(config);

    let proxy_handle = tokio::spawn(async move { proxy.start(&proxy_addr.to_string()).await });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Make HTTPS request through proxy
    let client = build_tls_https_client(roots);
    let uri = format!("https://localhost:{}/", proxy_addr.port())
        .parse::<hyper::Uri>()
        .unwrap();

    let resp = client.get(uri).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = body_bytes(resp.into_body()).await;
    assert_eq!(&body[..], b"https", "X-Forwarded-Proto should be 'https'");

    proxy_handle.abort();
}

#[tokio::test]
async fn test_find_site_tls_port_normalization() {
    // This tests the Host header normalization: browser sends "example.com"
    // on HTTPS, but config key is "example.com:443"
    let config = tiny_proxy::Config {
        sites: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "example.com:443".to_string(),
                tiny_proxy::config::SiteConfig {
                    address: "example.com:443".to_string(),
                    directives: vec![],
                    tls: Some(tiny_proxy::config::TlsConfig {
                        cert_path: "/fake/cert.pem".to_string(),
                        key_path: "/fake/key.pem".to_string(),
                    }),
                },
            );
            m
        },
    };

    // Exact match
    let exact = tiny_proxy::proxy::handler::find_site(&config, "example.com:443", true);
    assert!(exact.is_some());

    // Browser-style Host without port (TLS → try :443)
    let no_port = tiny_proxy::proxy::handler::find_site(&config, "example.com", true);
    assert!(
        no_port.is_some(),
        "Should find example.com:443 when Host='example.com' and is_tls=true"
    );

    // Non-TLS → try :80, should NOT find :443
    let wrong_proto = tiny_proxy::proxy::handler::find_site(&config, "example.com", false);
    assert!(
        wrong_proto.is_none(),
        "Should NOT find :443 site when is_tls=false (would try :80)"
    );
}

/// TLS GET with an explicit SNI hostname (connects to 127.0.0.1).
async fn tls_http_get(
    sni_host: &str,
    port: u16,
    roots: &rustls::RootCertStore,
    path: &str,
) -> (u16, Vec<u8>) {
    use rustls::pki_types::ServerName;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::TlsConnector;

    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("TCP connect");
    let config = std::sync::Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots.clone())
            .with_no_client_auth(),
    );
    let connector = TlsConnector::from(config);
    let server_name = ServerName::try_from(sni_host.to_string()).expect("valid SNI");
    let mut tls = connector
        .connect(server_name, stream)
        .await
        .expect("TLS handshake");

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, sni_host
    );
    tls.write_all(request.as_bytes())
        .await
        .expect("write request");

    let mut buf = vec![0u8; 8192];
    let n = tls.read(&mut buf).await.expect("read response");
    buf.truncate(n);

    let response = String::from_utf8_lossy(&buf);
    let status_line = response.lines().next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let body_start = response.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    (status, buf[body_start..].to_vec())
}

/// Plain HTTP GET (for redirect tests).
async fn plain_http_get(host: &str, port: u16, path: &str) -> (u16, Option<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("TCP connect");
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");

    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.expect("read response");
    buf.truncate(n);

    let response = String::from_utf8_lossy(&buf);
    let status_line = response.lines().next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let location = response.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("location") {
            Some(value.trim().to_string())
        } else {
            None
        }
    });

    (status, location)
}

fn make_multi_sni_config(
    port: u16,
    sites: &[(&str, &str, &str, &str)], // (hostname, cert_path, key_path, body)
) -> tiny_proxy::Config {
    let mut map = std::collections::HashMap::new();
    for (hostname, cert_path, key_path, body) in sites {
        let address = format!("{}:{}", hostname, port);
        map.insert(
            address.clone(),
            tiny_proxy::config::SiteConfig {
                address,
                directives: vec![tiny_proxy::config::Directive::Respond {
                    status: 200,
                    body: (*body).to_string(),
                }],
                tls: Some(tiny_proxy::config::TlsConfig {
                    cert_path: (*cert_path).to_string(),
                    key_path: (*key_path).to_string(),
                }),
            },
        );
    }
    tiny_proxy::Config { sites: map }
}

#[tokio::test]
async fn test_start_all_multi_sni() {
    let (cert_a, key_a) = generate_self_signed_cert("alpha.local");
    let (cert_b, key_b) = generate_self_signed_cert("beta.local");
    let cert_file_a = write_temp_pem("cert-a", &cert_a);
    let key_file_a = write_temp_pem("key-a", &key_a);
    let cert_file_b = write_temp_pem("cert-b", &cert_b);
    let key_file_b = write_temp_pem("key-b", &key_b);

    let tls_port = get_random_port_addr().await.port();
    let config = make_multi_sni_config(
        tls_port,
        &[
            (
                "alpha.local",
                cert_file_a.path().to_str().unwrap(),
                key_file_a.path().to_str().unwrap(),
                "ALPHA",
            ),
            (
                "beta.local",
                cert_file_b.path().to_str().unwrap(),
                key_file_b.path().to_str().unwrap(),
                "BETA",
            ),
        ],
    );

    let mut roots = rustls::RootCertStore::empty();
    for pem in [&cert_a, &cert_b] {
        let cert_der = rustls_pemfile::certs(&mut std::io::BufReader::new(pem.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        roots.add(cert_der[0].clone()).unwrap();
    }

    let proxy = tiny_proxy::Proxy::new(config);
    let proxy_handle = tokio::spawn(async move { proxy.start_all().await });

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let (status_a, body_a) = tls_http_get("alpha.local", tls_port, &roots, "/").await;
    assert_eq!(status_a, 200);
    assert_eq!(&body_a[..], b"ALPHA");

    let (status_b, body_b) = tls_http_get("beta.local", tls_port, &roots, "/").await;
    assert_eq!(status_b, 200);
    assert_eq!(&body_b[..], b"BETA");

    proxy_handle.abort();
}

#[tokio::test]
async fn test_start_all_http_to_https_redirect() {
    let (cert_pem, key_pem) = generate_self_signed_cert("redirect.local");
    let cert_file = write_temp_pem("cert", &cert_pem);
    let key_file = write_temp_pem("key", &key_pem);

    let tls_port = get_random_port_addr().await.port();
    let redirect_port = tls_redirect_port(tls_port);

    let config = make_tls_config(
        &format!("redirect.local:{}", tls_port),
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    );

    let proxy = tiny_proxy::Proxy::new(config);
    let proxy_handle = tokio::spawn(async move { proxy.start_all().await });

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let (status, location) = plain_http_get("redirect.local", redirect_port, "/hello?x=1").await;
    assert_eq!(status, 301);
    let expected = format!("https://redirect.local:{}/hello?x=1", tls_port);
    assert_eq!(location.as_deref(), Some(expected.as_str()));

    proxy_handle.abort();
}
