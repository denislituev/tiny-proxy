//! Example of running tiny-proxy in the background
//!
//! This example demonstrates how to:
//! - Load configuration from a file
//! - Create a Proxy instance
//! - Start the proxy server in the background using tokio::spawn
//! - Continue doing other work while the proxy runs
//! - Gracefully stop the proxy
//!
//! Run with:
//! ```bash
//! cargo run --example background
//! ```

use tiny_proxy::{Config, Proxy};
use tokio::time::{sleep, Duration};
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    info!("Starting tiny-proxy background example");

    // Load configuration from file
    let config = Config::from_file("file.caddy")?;

    info!("Loaded configuration for {} site(s)", config.sites.len());

    // Create proxy instance wrapped in Arc for thread-safe sharing
    let proxy = std::sync::Arc::new(Proxy::new(config));

    // Spawn the proxy in a background task
    let proxy_handle = tokio::spawn(async move {
        if let Err(e) = proxy.start("127.0.0.1:8080").await {
            eprintln!("Proxy error: {}", e);
        }
    });

    info!("Proxy started in background on http://127.0.0.1:8080");
    info!("Doing other work while proxy is running...");

    // Simulate doing other work
    for i in 1..=5 {
        info!("Main task: working... ({}/5)", i);
        sleep(Duration::from_secs(2)).await;
    }

    info!("Main task completed");
    info!("Stopping proxy...");

    // Abort the proxy task to stop the server
    proxy_handle.abort();

    // Wait for the task to finish cleanup
    match proxy_handle.await {
        Ok(_) => info!("Proxy stopped gracefully"),
        Err(_) => info!("Proxy was aborted"),
    }

    Ok(())
}
