//! Basic example of using tiny-proxy as a library
//!
//! This example demonstrates how to:
//! - Load configuration from a file
//! - Create a Proxy instance
//! - Start the proxy server
//!
//! Run with:
//! ```bash
//! cargo run --example basic
//! ```

use tiny_proxy::{Config, Proxy};
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    info!("Starting tiny-proxy library example");

    // Load configuration from file
    // Make sure file.caddy exists in the project root or provide a different path
    let config = Config::from_file("file.caddy")?;

    info!("Loaded configuration for {} site(s)", config.sites.len());

    // Create proxy instance
    let proxy = Proxy::new(config);

    // Start the proxy server
    // This will block indefinitely, handling incoming connections
    info!("Starting proxy on http://127.0.0.1:8080");
    proxy.start("127.0.0.1:8080").await?;

    Ok(())
}
