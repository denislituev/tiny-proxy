use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{RwLock, Semaphore};
use tracing::{error, info, warn};

#[cfg(feature = "tls")]
use crate::proxy::tls::{build_tls_acceptor, listen_http_redirect, listen_tls};

use crate::config::{extract_hostname, resolve_listen_addr, tls_redirect_port, Config};
use crate::proxy::handler::proxy;

/// HTTP Proxy server that can be embedded into other applications
///
/// This struct encapsulates the proxy state and allows programmatic control
/// over the proxy lifecycle. Configuration is stored in an `Arc<RwLock<Config>>`
/// so it can be hot-reloaded at runtime (e.g. via the API server).
///
/// # Example
///
/// ```no_run
/// use tiny_proxy::{Config, Proxy};
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let config = Config::from_file("file.caddy")?;
///     let proxy = Proxy::new(config);
///     proxy.start("127.0.0.1:8080").await?;
///     Ok(())
/// }
/// ```
///
/// # Hot-reload Example
///
/// ```no_run
/// use tiny_proxy::{Config, Proxy};
/// use std::sync::Arc;
/// use tokio::sync::RwLock;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let config = Config::from_file("config.caddy")?;
///     let proxy = Proxy::new(config);
///
///     // Get a handle to the shared config for hot-reload
///     let config_handle = proxy.shared_config();
///
///     // Spawn proxy in background
///     let handle = tokio::spawn(async move {
///         if let Err(e) = proxy.start("127.0.0.1:8080").await {
///             eprintln!("Proxy error: {}", e);
///         }
///     });
///
///     // Later, update config at runtime
///     let new_config = Config::from_file("updated-config.caddy")?;
///     {
///         let mut guard = config_handle.write().await;
///         *guard = new_config;
///     }
///
///     handle.await?;
///     Ok(())
/// }
/// ```
pub struct Proxy {
    config: Arc<RwLock<Config>>,
    client: Client<HttpsConnector<HttpConnector>, Incoming>,
    max_concurrency: usize,
    semaphore: Arc<Semaphore>,
}

impl Proxy {
    /// Create a new proxy instance with the given configuration
    ///
    /// The configuration is internally wrapped in `Arc<RwLock<Config>>`
    /// so it can be shared with an API server for hot-reload.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration loaded from file or constructed programmatically
    ///
    /// # Returns
    ///
    /// A new `Proxy` instance ready to be started
    pub fn new(config: Config) -> Self {
        let mut http = HttpConnector::new();
        http.set_keepalive(Some(Duration::from_secs(60)));
        http.set_nodelay(true);
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("Failed to load native TLS root certificates")
            .https_or_http()
            .enable_http1()
            .wrap_connector(http);

        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(100)
            .pool_idle_timeout(Duration::from_secs(90))
            .build::<_, Incoming>(https);

        let max_concurrency = std::env::var("TINY_PROXY_MAX_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| num_cpus::get() * 256);

        let semaphore = Arc::new(Semaphore::new(max_concurrency));

        info!(
            "Proxy initialized with max_concurrency={} (default: {})",
            max_concurrency,
            num_cpus::get() * 256
        );

        Self {
            config: Arc::new(RwLock::new(config)),
            client,
            max_concurrency,
            semaphore,
        }
    }

    /// Create a new proxy instance from an already shared configuration
    ///
    /// Use this when you already have an `Arc<RwLock<Config>>` that is
    /// shared with an API server or other component.
    ///
    /// # Arguments
    ///
    /// * `config` - Shared configuration wrapped in `Arc<RwLock<Config>>`
    pub fn from_shared(config: Arc<RwLock<Config>>) -> Self {
        let mut http = HttpConnector::new();
        http.set_keepalive(Some(Duration::from_secs(60)));
        http.set_nodelay(true);
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("Failed to load native TLS root certificates")
            .https_or_http()
            .enable_http1()
            .wrap_connector(http);

        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(100)
            .pool_idle_timeout(Duration::from_secs(90))
            .build::<_, Incoming>(https);

        let max_concurrency = std::env::var("TINY_PROXY_MAX_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| num_cpus::get() * 256);

        let semaphore = Arc::new(Semaphore::new(max_concurrency));

        info!(
            "Proxy initialized with max_concurrency={} (default: {})",
            max_concurrency,
            num_cpus::get() * 256
        );

        Self {
            config,
            client,
            max_concurrency,
            semaphore,
        }
    }

    /// Start the proxy server on the specified address
    ///
    /// This method blocks indefinitely, handling incoming connections.
    /// To run the proxy in the background, spawn it in a tokio task.
    ///
    /// Starts a **single** listener on `addr`. If matching sites use TLS, an HTTPS
    /// listener is started; otherwise plain HTTP. Does **not** start HTTP→HTTPS
    /// redirect servers — use [`Self::start_all`] for auto-detect multi-listener mode.
    ///
    /// # Arguments
    ///
    /// * `addr` - Address to listen on (e.g., "127.0.0.1:8080" or "0.0.0.0:8443")
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tiny_proxy::{Config, Proxy};
    /// # #[tokio::main]
    /// # async fn main() -> anyhow::Result<()> {
    /// # let config = Config::from_file("config.caddy")?;
    /// # let proxy = Proxy::new(config);
    /// proxy.start("127.0.0.1:8080").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let addr: SocketAddr = addr.parse()?;
        self.start_with_addr(addr).await
    }

    /// Start the proxy server with a parsed SocketAddr
    ///
    /// Same as [`Self::start`]: one listener on `addr`, HTTPS or HTTP depending on
    /// site TLS config. No automatic HTTP→HTTPS redirect — see [`Self::start_all`].
    ///
    /// # Arguments
    ///
    /// * `addr` - Parsed SocketAddr to listen on
    pub async fn start_with_addr(&self, addr: SocketAddr) -> anyhow::Result<()> {
        // Check if any site on this address has TLS configured
        let config_snapshot = self.config.read().await.clone();
        let tls_sites: Vec<(String, crate::config::TlsConfig)> = config_snapshot
            .sites
            .values()
            .filter(|site| {
                // Check if the site's address matches the listening addr
                // Site address can be "host:port" or just ":port"
                site_addr_matches(&site.address, &addr) && site.tls.is_some()
            })
            .filter_map(|site| {
                // Extract hostname for SNI, TLS config
                let hostname = extract_hostname(&site.address);
                site.tls.clone().map(|tls| (hostname.to_string(), tls))
            })
            .collect();

        if !tls_sites.is_empty() {
            #[cfg(feature = "tls")]
            {
                self.start_tls(addr, tls_sites).await
            }
            #[cfg(not(feature = "tls"))]
            {
                anyhow::bail!(
                    "TLS configuration found for {} but 'tls' feature is disabled. \
                     Refusing to start as plain HTTP (security risk). \
                     Rebuild with --features tls or remove 'tls' from config.",
                    addr
                );
            }
        } else {
            self.start_http(addr).await
        }
    }

    /// Start all listeners defined in the configuration (auto-detect mode).
    ///
    /// Scans the config for all unique listen addresses and starts a listener
    /// for each. TLS sites get HTTPS listeners with SNI; non-TLS sites get HTTP.
    ///
    /// For each distinct TLS port, also starts an HTTP→HTTPS redirect listener:
    /// `redirect_port = tls_port - 443 + 80` (e.g. 443→80, 8443→8080).
    /// Redirect bind is best-effort: if the redirect port is in use, HTTPS still works.
    ///
    /// Unlike [`Self::start`] / [`Self::start_with_addr`], this method spawns multiple
    /// listeners and redirect servers. Use this when the config defines several site
    /// addresses (CLI without `--addr`).
    ///
    /// This method blocks until all listener tasks finish (typically forever).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tiny_proxy::{Config, Proxy};
    /// # #[tokio::main]
    /// # async fn main() -> anyhow::Result<()> {
    /// # let config = Config::from_file("config.caddy")?;
    /// # let proxy = std::sync::Arc::new(Proxy::new(config));
    /// proxy.start_all().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn start_all(&self) -> anyhow::Result<()> {
        let config_snapshot = self.config.read().await.clone();

        // Group sites by resolved listen socket (multiple hostnames may share one port)
        let mut socket_groups: HashMap<SocketAddr, Vec<&crate::config::SiteConfig>> =
            HashMap::new();
        for site in config_snapshot.sites.values() {
            let listen_addr = resolve_listen_addr(&site.address)?;
            socket_groups
                .entry(listen_addr)
                .or_default()
                .push(site);
        }

        let mut http_handles = Vec::new();
        let mut tls_redirects: HashSet<(SocketAddr, u16)> = HashSet::new(); // (redirect bind addr, tls_port)

        for (listen_addr, sites) in socket_groups {
            let tls_sites: Vec<_> = sites.iter().copied().filter(|s| s.tls.is_some()).collect();
            let has_tls = !tls_sites.is_empty();
            let has_plain = tls_sites.len() != sites.len();

            if has_tls && has_plain {
                anyhow::bail!(
                    "Mixed TLS and non-TLS sites on the same listen address {} is not supported",
                    listen_addr
                );
            }

            if has_tls {
                #[cfg(feature = "tls")]
                {
                    let tls_entries: Vec<(String, crate::config::TlsConfig)> = tls_sites
                        .iter()
                        .filter_map(|s| {
                            let hostname = extract_hostname(&s.address);
                            s.tls.clone().map(|tls| (hostname.to_string(), tls))
                        })
                        .collect();

                    let tls_port = listen_addr.port();

                    let client = self.client.clone();
                    let config = self.config.clone();
                    let semaphore = self.semaphore.clone();

                    let acceptor = build_tls_acceptor(&tls_entries, None)?;
                    info!(
                        "Starting HTTPS listener on {} ({} domain(s))",
                        listen_addr,
                        tls_entries.len()
                    );

                    let handle = tokio::spawn(async move {
                        if let Err(e) =
                            listen_tls(listen_addr, acceptor, semaphore, move |req, remote_addr| {
                                let client = client.clone();
                                let config = config.clone();
                                async move {
                                    let config_guard = config.read().await;
                                    let config_snapshot = Arc::new(config_guard.clone());
                                    drop(config_guard);
                                    proxy(req, client, config_snapshot, remote_addr, true).await
                                }
                            })
                            .await
                        {
                            error!("TLS listener error: {}", e);
                        }
                    });
                    http_handles.push(handle);

                    tls_redirects.insert((
                        SocketAddr::new(listen_addr.ip(), tls_redirect_port(tls_port)),
                        tls_port,
                    ));
                }

                #[cfg(not(feature = "tls"))]
                {
                    anyhow::bail!(
                        "TLS configuration found for {} but 'tls' feature is disabled. \
                         Refusing to start as plain HTTP (security risk). \
                         Rebuild with --features tls or remove 'tls' from config.",
                        listen_addr
                    );
                }
            } else {
                let client = self.client.clone();
                let config = self.config.clone();
                let semaphore = self.semaphore.clone();
                let max_concurrency = self.max_concurrency;

                let handle = tokio::spawn(async move {
                    if let Err(e) =
                        Self::run_http_loop(listen_addr, client, config, semaphore, max_concurrency)
                            .await
                    {
                        error!("HTTP listener error: {}", e);
                    }
                });
                http_handles.push(handle);
            }
        }

        #[cfg(feature = "tls")]
        for (redirect_addr, tls_port) in tls_redirects {
            info!(
                "Starting HTTP→HTTPS redirect on http://{} → :{}",
                redirect_addr, tls_port
            );
            let handle = tokio::spawn(async move {
                match listen_http_redirect(redirect_addr, tls_port).await {
                    Ok(()) => {}
                    Err(e) => {
                        warn!(
                            "HTTP redirect on port {} failed (HTTPS on :{} still active): {}",
                            redirect_addr.port(), tls_port, e
                        );
                    }
                }
            });
            http_handles.push(handle);
        }

        if http_handles.is_empty() {
            warn!("No listeners configured — proxy has no sites");
            return Ok(());
        }

        info!(
            "Started {} listener(s), max concurrency: {} ({})",
            http_handles.len(),
            self.max_concurrency,
            if self.max_concurrency == num_cpus::get() * 256 {
                "default"
            } else {
                "custom"
            }
        );

        // Wait for any listener to finish (they run forever, so this blocks indefinitely)
        // If one fails, the others keep running.
        for handle in http_handles {
            if let Err(e) = handle.await {
                error!("Listener task panicked: {}", e);
            }
        }

        Ok(())
    }

    /// Start a plain HTTP listener on the given address.
    async fn start_http(&self, addr: SocketAddr) -> anyhow::Result<()> {
        Self::run_http_loop(
            addr,
            self.client.clone(),
            self.config.clone(),
            self.semaphore.clone(),
            self.max_concurrency,
        )
        .await
    }

    /// Core HTTP accept loop — shared between `start_http` and `start_all`.
    async fn run_http_loop(
        addr: SocketAddr,
        client: Client<HttpsConnector<HttpConnector>, Incoming>,
        config: Arc<RwLock<Config>>,
        semaphore: Arc<Semaphore>,
        max_concurrency: usize,
    ) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&addr).await?;
        info!("Tiny Proxy listening on http://{}", addr);

        loop {
            let (stream, remote_addr) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let client = client.clone();
            let config = config.clone();
            let semaphore = semaphore.clone();

            match semaphore.try_acquire_owned() {
                Ok(permit) => {
                    tokio::task::spawn(async move {
                        let _permit = permit;
                        let service = service_fn(move |req| {
                            let client = client.clone();
                            let config = config.clone();

                            let config_clone = config.clone();
                            async move {
                                let config_guard = config_clone.read().await;
                                let config_snapshot = Arc::new(config_guard.clone());
                                drop(config_guard);
                                proxy(req, client, config_snapshot, remote_addr, false).await
                            }
                        });

                        let mut builder = hyper::server::conn::http1::Builder::new();
                        builder.keep_alive(true).pipeline_flush(false);

                        builder.serve_connection(io, service).await
                    });
                }
                Err(_) => {
                    warn!(
                        "Concurrency limit exceeded ({}), rejecting connection",
                        max_concurrency
                    );
                }
            }
        }
    }

    /// Start a TLS listener on the given address with the specified TLS sites.
    #[cfg(feature = "tls")]
    async fn start_tls(
        &self,
        addr: SocketAddr,
        tls_sites: Vec<(String, crate::config::TlsConfig)>,
    ) -> anyhow::Result<()> {
        let acceptor = build_tls_acceptor(&tls_sites, None)?;
        info!(
            "Starting HTTPS listener on https://{} ({} domain(s))",
            addr,
            tls_sites.len()
        );

        let client = self.client.clone();
        let config = self.config.clone();
        let semaphore = self.semaphore.clone();

        listen_tls(addr, acceptor, semaphore, move |req, remote_addr| {
            let client = client.clone();
            let config = config.clone();
            async move {
                let config_guard = config.read().await;
                let config_snapshot = Arc::new(config_guard.clone());
                drop(config_guard);
                proxy(req, client, config_snapshot, remote_addr, true).await
            }
        })
        .await
    }

    /// Get a reference to the shared configuration handle
    ///
    /// This returns a clone of the `Arc<RwLock<Config>>`, allowing
    /// external code (e.g. an API server) to read and update the
    /// configuration at runtime.
    ///
    /// # Returns
    ///
    /// A cloned `Arc<RwLock<Config>>`
    pub fn shared_config(&self) -> Arc<RwLock<Config>> {
        self.config.clone()
    }

    /// Get a snapshot of the current configuration
    ///
    /// Reads the current configuration and returns an owned clone.
    /// This is useful for inspecting config without holding a lock.
    ///
    /// # Returns
    ///
    /// A cloned `Config`
    pub async fn config_snapshot(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Get current concurrency limit
    ///
    /// # Returns
    ///
    /// Current maximum number of concurrent connections
    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    /// Update concurrency limit at runtime
    ///
    /// # Arguments
    ///
    /// * `max` - New maximum number of concurrent connections
    ///
    /// # Note
    ///
    /// This updates the semaphore immediately. New connections will use
    /// the new limit, but existing connections are not affected.
    pub fn set_max_concurrency(&mut self, max: usize) {
        self.max_concurrency = max;
        self.semaphore = Arc::new(Semaphore::new(max));
        info!("Max concurrency updated to {}", max);
    }

    /// Update the configuration at runtime (hot-reload)
    ///
    /// Atomically replaces routing configuration. New connections use the updated
    /// config immediately; in-flight connections keep their original snapshot.
    ///
    /// **TLS certificates** are loaded when a listener starts. This method updates
    /// site routing and directives only — not cert/key files or `TlsAcceptor`.
    /// Restart the proxy (or TLS listener) to pick up new certificates.
    ///
    /// # Arguments
    ///
    /// * `config` - New configuration to use
    pub async fn update_config(&self, config: Config) {
        let mut guard = self.config.write().await;
        info!("Configuration updated ({} sites)", config.sites.len());
        *guard = config;
    }
}

/// Check if a site address string matches a SocketAddr.
///
/// The site address may be `"host:port"`, `"host"` (no port),
/// or `":port"` (any host). This function compares ports and,
/// if the site specifies a hostname (not empty, not `0.0.0.0`),
/// also compares hostnames.
fn site_addr_matches(site_address: &str, listen_addr: &SocketAddr) -> bool {
    let mut parts = site_address.rsplitn(2, ':');
    let port_str = parts.next().unwrap_or("");
    let host_str = parts.next().unwrap_or("");

    let site_port: u16 = match port_str.parse() {
        Ok(p) => p,
        Err(_) => return false,
    };

    if site_port != listen_addr.port() {
        return false;
    }

    // If site has a specific hostname, check it
    if host_str.is_empty() || host_str == "0.0.0.0" || host_str == "::" {
        return true; // wildcard host
    }

    // Match against listen addr IP
    // site may have "localhost" → resolve to 127.0.0.1
    let site_ip = if host_str == "localhost" {
        std::net::IpAddr::from(std::net::Ipv4Addr::new(127, 0, 0, 1))
    } else if let Ok(ip) = host_str.parse::<std::net::IpAddr>() {
        ip
    } else {
        // hostname-based (e.g. "example.com:443") — match by port only
        return true;
    };

    site_ip == listen_addr.ip()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_proxy_creation() {
        let config = Config {
            sites: HashMap::new(),
        };
        let proxy = Proxy::new(config);
        // Can't check sites len synchronously anymore, use snapshot
        let rt = tokio::runtime::Runtime::new().unwrap();
        let snapshot = rt.block_on(proxy.config_snapshot());
        assert_eq!(snapshot.sites.len(), 0);
    }

    #[tokio::test]
    async fn test_config_access() {
        let mut config = Config {
            sites: HashMap::new(),
        };
        config.sites.insert(
            "localhost:8080".to_string(),
            crate::config::SiteConfig {
                address: "localhost:8080".to_string(),
                directives: vec![],
                tls: None,
            },
        );

        let proxy = Proxy::new(config);
        let snapshot = proxy.config_snapshot().await;
        assert_eq!(snapshot.sites.len(), 1);
        assert!(snapshot.sites.contains_key("localhost:8080"));
    }

    #[tokio::test]
    async fn test_config_update() {
        let config1 = Config {
            sites: HashMap::new(),
        };
        let proxy = Proxy::new(config1);
        let snapshot = proxy.config_snapshot().await;
        assert_eq!(snapshot.sites.len(), 0);

        let mut config2 = Config {
            sites: HashMap::new(),
        };
        config2.sites.insert(
            "test.local".to_string(),
            crate::config::SiteConfig {
                address: "test.local".to_string(),
                directives: vec![],
                tls: None,
            },
        );

        proxy.update_config(config2).await;
        let snapshot = proxy.config_snapshot().await;
        assert_eq!(snapshot.sites.len(), 1);
        assert!(snapshot.sites.contains_key("test.local"));
    }

    #[tokio::test]
    async fn test_shared_config_handle() {
        let config = Config {
            sites: HashMap::new(),
        };
        let proxy = Proxy::new(config);

        let handle = proxy.shared_config();

        // Update via the shared handle
        {
            let mut guard = handle.write().await;
            guard.sites.insert(
                "shared.local".to_string(),
                crate::config::SiteConfig {
                    address: "shared.local".to_string(),
                    directives: vec![],
                    tls: None,
                },
            );
        }

        // Verify the proxy sees the update
        let snapshot = proxy.config_snapshot().await;
        assert_eq!(snapshot.sites.len(), 1);
        assert!(snapshot.sites.contains_key("shared.local"));
    }

    #[test]
    fn test_from_shared() {
        let config = Config {
            sites: HashMap::new(),
        };
        let shared = Arc::new(RwLock::new(config));
        let proxy = Proxy::from_shared(shared.clone());

        // Verify both point to the same config
        let rt = tokio::runtime::Runtime::new().unwrap();
        {
            let mut guard = rt.block_on(shared.write());
            guard.sites.insert(
                "from-shared.local".to_string(),
                crate::config::SiteConfig {
                    address: "from-shared.local".to_string(),
                    directives: vec![],
                    tls: None,
                },
            );
        }
        let snapshot = rt.block_on(proxy.config_snapshot());
        assert_eq!(snapshot.sites.len(), 1);
        assert!(snapshot.sites.contains_key("from-shared.local"));
    }

    // --- site_addr_matches tests ---

    #[test]
    fn test_site_addr_matches_localhost() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        assert!(site_addr_matches("localhost:8080", &addr));
    }

    #[test]
    fn test_site_addr_matches_ip() {
        let addr: SocketAddr = "0.0.0.0:443".parse().unwrap();
        assert!(site_addr_matches("0.0.0.0:443", &addr));
    }

    #[test]
    fn test_site_addr_matches_hostname_by_port() {
        let addr: SocketAddr = "0.0.0.0:443".parse().unwrap();
        // Domain-based address matches by port only
        assert!(site_addr_matches("example.com:443", &addr));
    }

    #[test]
    fn test_site_addr_matches_port_mismatch() {
        let addr: SocketAddr = "0.0.0.0:443".parse().unwrap();
        assert!(!site_addr_matches("example.com:8443", &addr));
    }

    #[test]
    fn test_site_addr_matches_wildcard_host() {
        let addr: SocketAddr = "0.0.0.0:9090".parse().unwrap();
        assert!(site_addr_matches(":9090", &addr));
    }
}
