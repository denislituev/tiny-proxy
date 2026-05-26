//! TLS termination example
//!
//! This example demonstrates how to:
//! - Configure tiny-proxy with TLS (HTTPS) termination
//! - Use `start_all()` to auto-detect HTTP/TLS listeners
//! - Generate self-signed certificates for local testing
//!
//! # Setup
//!
//! Generate certificates first:
//! ```bash
//! mkdir -p examples/certs
//! openssl req -x509 -newkey rsa:2048 \
//!   -keyout examples/certs/server.key \
//!   -out examples/certs/server.crt \
//!   -days 365 -nodes -subj "/CN=localhost"
//! ```
//!
//! # Run
//!
//! ```bash
//! cargo run --features tls --example tls
//! ```
//!
//! # Test
//!
//! ```bash
//! curl -k https://localhost:8443/health
//! curl -k https://localhost:8443/users/123
//! ```

use tiny_proxy::{Config, Proxy};
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    info!("Starting tiny-proxy TLS example");

    let config = Config::from_file("examples/tls.conf")?;
    info!("Loaded configuration for {} site(s)", config.sites.len());

    let proxy = Proxy::new(config);

    // start_all() auto-detects TLS sites and starts:
    //  - HTTPS listeners for sites with `tls` directive
    //  - HTTP→HTTPS redirect on port 80 (for TLS on 443)
    info!("Starting listeners (auto-detect from config)");
    proxy.start_all().await?;

    Ok(())
}
