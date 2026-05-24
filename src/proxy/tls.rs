//! TLS termination for incoming HTTPS connections.
//!
//! This module provides:
//! - Certificate/key loading from PEM files
//! - SNI-based certificate resolution (multiple domains on one port)
//! - HTTPS listener that wraps `tokio::net::TcpStream` with `tokio-rustls`
//! - HTTP → HTTPS redirect server (port derived from TLS port: `tls_port - 443 + 80`)

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::BodyExt;
use rustls::server::ResolvesServerCert;
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};

use crate::config::TlsConfig;

// ---------------------------------------------------------------------------
// Certificate loading helpers
// ---------------------------------------------------------------------------

/// Load a certificate chain from a PEM file.
///
/// Returns all certificates found in the file (typically one leaf + intermediates).
fn load_certs(
    path: &str,
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, std::io::Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("No certificates found in {}", path),
        ));
    }
    Ok(certs)
}

/// Load a private key from a PEM file.
///
/// Supports RSA and EC keys. Returns the first key found.
fn load_key(path: &str) -> Result<rustls::pki_types::PrivateKeyDer<'static>, std::io::Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    // private_key() tries RSA, PKCS8, EC, in order
    rustls_pemfile::private_key(&mut reader)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("No private key found in {}", path),
        )
    })
}

// ---------------------------------------------------------------------------
// SNI-based certificate resolver
// ---------------------------------------------------------------------------

/// Maps SNI hostnames to pre-loaded `CertifiedKey` pairs.
///
/// When a TLS ClientHello arrives with an SNI extension, the resolver
/// looks up the hostname (case-insensitive) and returns the matching
/// certificate chain + signing key.
///
/// A single default certificate can be set for clients that don't send SNI.
struct SniCertResolver {
    entries: HashMap<String, Arc<CertifiedKey>>,
    default: Option<Arc<CertifiedKey>>,
}

impl std::fmt::Debug for SniCertResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SniCertResolver")
            .field("entries", &self.entries.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl SniCertResolver {
    fn new(
        entries: HashMap<String, Arc<CertifiedKey>>,
        default: Option<Arc<CertifiedKey>>,
    ) -> Self {
        Self { entries, default }
    }
}

impl ResolvesServerCert for SniCertResolver {
    fn resolve(&self, client_hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        if let Some(sni) = client_hello.server_name() {
            if let Some(cert) = self.entries.get(&sni.to_ascii_lowercase()) {
                return Some(Arc::clone(cert));
            }
        }
        self.default.clone()
    }
}

// ---------------------------------------------------------------------------
// Public builder: create a TlsAcceptor from site configs
// ---------------------------------------------------------------------------

/// Build a `TlsAcceptor` from a collection of `{ hostname -> TlsConfig }` entries.
///
/// All entries share the same listener / port (the caller is responsible for
/// grouping sites by port before calling this).
///
/// If `default_hostname` is provided, that domain's certificate is used as the
/// fallback for clients that don't send SNI.
///
/// # Errors
///
/// Returns an error if any certificate or key file cannot be loaded or parsed.
pub fn build_tls_acceptor(
    sites: &[(String, TlsConfig)], // (hostname, tls_config)
    default_hostname: Option<&str>,
) -> Result<TlsAcceptor, std::io::Error> {
    if sites.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cannot build TLS acceptor: no TLS sites provided",
        ));
    }

    let mut entries: HashMap<String, Arc<CertifiedKey>> = HashMap::new();
    let mut default: Option<Arc<CertifiedKey>> = None;

    for (hostname, tls_cfg) in sites {
        let certs = load_certs(&tls_cfg.cert_path)?;
        let key = load_key(&tls_cfg.key_path)?;

        let signing_key =
            rustls::crypto::aws_lc_rs::sign::any_supported_type(&key).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Unsupported key type in {} for host {}: {}",
                        tls_cfg.key_path, hostname, e
                    ),
                )
            })?;

        let certified_key = Arc::new(CertifiedKey::new(certs, signing_key));

        if default_hostname.is_some()
            && hostname.eq_ignore_ascii_case(default_hostname.unwrap_or(""))
        {
            default = Some(Arc::clone(&certified_key));
        }

        entries.insert(hostname.to_ascii_lowercase(), certified_key);
    }

    // If no explicit default, use the lexicographically first SNI hostname
    if default.is_none() {
        let mut hostnames: Vec<_> = entries.keys().cloned().collect();
        hostnames.sort();
        if let Some(first) = hostnames.first() {
            default = entries.get(first).cloned();
        }
    }

    let resolver = Arc::new(SniCertResolver::new(entries, default));

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);

    Ok(TlsAcceptor::from(Arc::new(config)))
}

// ---------------------------------------------------------------------------
// HTTPS listener
// ---------------------------------------------------------------------------

/// Bind a TLS listener and serve incoming connections.
///
/// Each accepted connection goes through:
/// 1. TLS handshake via `TlsAcceptor`
/// 2. HTTP/1.1 parsing via hyper
/// 3. Dispatched to the same `service_fn` that the plain-HTTP path uses
///
/// The `service_fn` closure is constructed per-connection and receives
/// `(req, remote_addr)` — exactly the same data as the non-TLS path.
///
/// This function runs forever (until the listener is closed or an unrecoverable error).
pub async fn listen_tls<F, Fut>(
    addr: SocketAddr,
    acceptor: TlsAcceptor,
    semaphore: Arc<tokio::sync::Semaphore>,
    make_service: F,
) -> anyhow::Result<()>
where
    F: Fn(hyper::Request<hyper::body::Incoming>, std::net::SocketAddr) -> Fut
        + Clone
        + Send
        + 'static,
    Fut: std::future::Future<
            Output = Result<
                hyper::Response<
                    http_body_util::combinators::BoxBody<
                        bytes::Bytes,
                        Box<dyn std::error::Error + Send + Sync>,
                    >,
                >,
                anyhow::Error,
            >,
        > + Send,
{
    let listener = TcpListener::bind(addr).await?;
    info!("TLS listener bound on https://{}", addr);

    loop {
        let (tcp_stream, remote_addr) = listener.accept().await?;
        let io = hyper_util::rt::TokioIo::new(tcp_stream);
        let acceptor = acceptor.clone();
        let semaphore = semaphore.clone();
        let make_service = make_service.clone();

        let permit = match semaphore.try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                warn!(
                    "TLS concurrency limit exceeded, rejecting connection from {}",
                    remote_addr
                );
                continue;
            }
        };

        tokio::task::spawn(async move {
            let _permit = permit; // held until task completes

            let tls_stream = match acceptor.accept(io.into_inner()).await {
                Ok(s) => s,
                Err(e) => {
                    // Handshake failures are common (wrong SNI, expired cert, etc.)
                    // Don't log at error level to avoid noise
                    info!("TLS handshake failed from {}: {}", remote_addr, e);
                    return;
                }
            };

            let io = hyper_util::rt::TokioIo::new(tls_stream);
            let make_service = make_service.clone();

            let service = hyper::service::service_fn(move |req| {
                let make_service = make_service.clone();
                make_service(req, remote_addr)
            });

            let mut builder = hyper::server::conn::http1::Builder::new();
            builder.keep_alive(true).pipeline_flush(false);

            if let Err(e) = builder.serve_connection(io, service).await {
                error!("TLS connection error from {}: {:?}", remote_addr, e);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// HTTP → HTTPS redirect server
// ---------------------------------------------------------------------------

/// Strip the port from a `Host` header value, handling IPv6 (`[::1]:8080` → `::1`).
fn hostname_from_host_header(host: &str) -> &str {
    if host.starts_with('[') {
        if let Some(end) = host.find(']') {
            return &host[1..end];
        }
    }
    host.rsplit(':').next_back().unwrap_or(host)
}

/// Start a plain-HTTP server that redirects all requests to HTTPS.
///
/// Listens on `addr` (the computed redirect port) and responds with `301 Moved Permanently`.
/// The `Location` header targets `https://{host}{uri}` when `tls_port` is 443, or
/// `https://{host}:{tls_port}{uri}` for non-standard TLS ports (e.g. 8443).
///
/// For each request, the `Host` header is used to construct the redirect target:
/// ```text
/// HTTP/1.1 301 Moved Permanently
/// Location: https://{host}{uri}          (tls_port == 443)
/// Location: https://{host}:8443{uri}     (tls_port == 8443)
/// ```
///
/// If the `Host` header is missing, `"localhost"` is used as the hostname.
pub async fn listen_http_redirect(addr: SocketAddr, tls_port: u16) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!(
        "HTTP→HTTPS redirect server listening on http://{} → :{}",
        addr, tls_port
    );

    loop {
        let (stream, remote_addr) = listener.accept().await?;
        let io = hyper_util::rt::TokioIo::new(stream);

        tokio::task::spawn(async move {
            let service =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let host = req
                        .headers()
                        .get(hyper::header::HOST)
                        .and_then(|h| h.to_str().ok())
                        .unwrap_or("localhost");

                    // Strip port from host if present — we'll add the TLS port
                    let hostname = hostname_from_host_header(host);

                    let uri = req
                        .uri()
                        .path_and_query()
                        .map(|pq| pq.as_str())
                        .unwrap_or("/");

                    let location = if tls_port == 443 {
                        format!("https://{}{}", hostname, uri)
                    } else {
                        format!("https://{}:{}{}", hostname, tls_port, uri)
                    };

                    let body = format!(
                        "<!DOCTYPE html><html><head><title>301 Moved</title></head>\
                         <body><h1>Moved Permanently</h1><p>The document has moved \
                         <a href=\"{}\">here</a>.</p></body></html>",
                        location
                    );
                    let body_len = body.len();

                    let response = hyper::Response::builder()
                        .status(301)
                        .header("Location", &location)
                        .header("Content-Type", "text/html; charset=utf-8")
                        .header("Content-Length", body_len)
                        .body(
                            http_body_util::Full::new(bytes::Bytes::from(body))
                                .map_err(|e| {
                                    Box::new(e) as Box<dyn std::error::Error + Send + Sync>
                                })
                                .boxed(),
                        )
                        .expect("static redirect response build");

                    std::future::ready(Ok::<_, hyper::Error>(response))
                });

            let mut builder = hyper::server::conn::http1::Builder::new();
            builder.keep_alive(false).pipeline_flush(false);

            if let Err(e) = builder.serve_connection(io, service).await {
                debug!("Redirect connection error from {}: {:?}", remote_addr, e);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_tls_acceptor_empty_sites() {
        let result = build_tls_acceptor(&[], None);
        assert!(result.is_err());
    }

    #[test]
    fn test_hostname_from_host_header_ipv6() {
        assert_eq!(hostname_from_host_header("[::1]:8080"), "::1");
        assert_eq!(hostname_from_host_header("example.com:8080"), "example.com");
        assert_eq!(hostname_from_host_header("example.com"), "example.com");
    }

    #[test]
    fn test_load_certs_nonexistent_file() {
        let result = load_certs("/nonexistent/path/cert.pem");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_key_nonexistent_file() {
        let result = load_key("/nonexistent/path/key.pem");
        assert!(result.is_err());
    }
}
