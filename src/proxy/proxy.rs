use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::config::Config;
use crate::proxy::handler::proxy;

/// HTTP Proxy server that can be embedded into other applications
///
/// This struct encapsulates the proxy state and allows programmatic control
/// over the proxy lifecycle.
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
pub struct Proxy {
    config: Config,
    client: Client<HttpsConnector<HttpConnector>, Incoming>,
    max_concurrency: usize,
    semaphore: Arc<Semaphore>,
}

impl Proxy {
    /// Create a new proxy instance with the given configuration
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
            .unwrap()
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
    /// # Arguments
    ///
    /// * `addr` - Address to listen on (e.g., "127.0.0.1:8080" or "0.0.0.0:8080")
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
    ///
    /// To run in background:
    /// ```no_run
    /// # use tiny_proxy::{Config, Proxy};
    /// # #[tokio::main]
    /// # async fn main() -> anyhow::Result<()> {
    /// # let config = Config::from_file("config.caddy")?;
    /// # let proxy = std::sync::Arc::new(Proxy::new(config));
    /// let handle = tokio::spawn(async move {
    ///     if let Err(e) = proxy.start("127.0.0.1:8080").await {
    ///         eprintln!("Proxy error: {}", e);
    ///     }
    /// });
    /// # handle.await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let addr: SocketAddr = addr.parse()?;
        self.start_with_addr(addr).await
    }

    /// Start the proxy server with a parsed SocketAddr
    ///
    /// This is a convenience method if you already have a parsed SocketAddr.
    ///
    /// # Arguments
    ///
    /// * `addr` - Parsed SocketAddr to listen on
    pub async fn start_with_addr(&self, addr: SocketAddr) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&addr).await?;

        info!("Tiny Proxy listening on http://{}", addr);
        info!(
            "Max concurrency: {} ({})",
            self.max_concurrency,
            if self.max_concurrency == num_cpus::get() * 256 {
                "default"
            } else {
                "custom"
            }
        );

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let client = self.client.clone();
            let config = Arc::new(self.config.clone());
            let semaphore = self.semaphore.clone();

            match semaphore.try_acquire_owned() {
                Ok(permit) => {
                    tokio::task::spawn(async move {
                        let _permit = permit;
                        let service = service_fn(move |req| {
                            let client = client.clone();
                            let config = config.clone();
                            proxy(req, client, config)
                        });

                        let mut builder = hyper::server::conn::http1::Builder::new();
                        builder.keep_alive(true).pipeline_flush(false);

                        builder.serve_connection(io, service).await
                    });
                }
                Err(_) => {
                    warn!(
                        "Concurrency limit exceeded ({}), rejecting connection",
                        self.max_concurrency
                    );
                }
            }
        }
    }

    /// Get a reference to current configuration
    ///
    /// This allows inspection of current proxy configuration
    /// without modifying it.
    ///
    /// # Returns
    ///
    /// Reference to current `Config`
    pub fn config(&self) -> &Config {
        &self.config
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

    /// Update the configuration
    ///
    /// This allows hot-reload of configuration without restarting the proxy.
    /// New connections will use the updated configuration immediately.
    ///
    /// # Arguments
    ///
    /// * `config` - New configuration to use
    ///
    /// # Note
    ///
    /// This operation is atomic for new connections. Existing connections
    /// will continue to use their original configuration.
    pub fn update_config(&mut self, config: Config) {
        self.config = config;
        info!("Configuration updated");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_creation() {
        let config = Config {
            sites: std::collections::HashMap::new(),
        };
        let proxy = Proxy::new(config);
        assert_eq!(proxy.config().sites.len(), 0);
    }

    #[test]
    fn test_config_access() {
        let mut config = Config {
            sites: std::collections::HashMap::new(),
        };
        config.sites.insert(
            "localhost:8080".to_string(),
            crate::config::SiteConfig {
                address: "localhost:8080".to_string(),
                directives: vec![],
            },
        );

        let proxy = Proxy::new(config);
        assert_eq!(proxy.config().sites.len(), 1);
        assert!(proxy.config().sites.contains_key("localhost:8080"));
    }

    #[test]
    fn test_config_update() {
        let config1 = Config {
            sites: std::collections::HashMap::new(),
        };
        let mut proxy = Proxy::new(config1);
        assert_eq!(proxy.config().sites.len(), 0);

        let mut config2 = Config {
            sites: std::collections::HashMap::new(),
        };
        config2.sites.insert(
            "test.local".to_string(),
            crate::config::SiteConfig {
                address: "test.local".to_string(),
                directives: vec![],
            },
        );

        proxy.update_config(config2);
        assert_eq!(proxy.config().sites.len(), 1);
        assert!(proxy.config().sites.contains_key("test.local"));
    }
}
