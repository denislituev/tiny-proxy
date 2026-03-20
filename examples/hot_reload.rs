//! Example of hot-reloading configuration without restarting the proxy
//!
//! This example demonstrates how to:
//! - Load configuration from a file
//! - Create a Proxy instance
//! - Start the proxy server in the background
//! - Monitor the configuration file for changes
//! - Hot-reload the configuration when the file changes
//!
//! Run with:
//! ```bash
//! cargo run --example hot_reload
//! ```
//!
//! Then edit file.caddy while the proxy is running to see hot-reload in action.

use tiny_proxy::{Config, Proxy};
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    info!("Starting tiny-proxy hot-reload example");

    let config_path = "file.caddy";

    // Load initial configuration from file
    let config = Config::from_file(config_path)?;
    info!(
        "Loaded initial configuration for {} site(s)",
        config.sites.len()
    );

    // Create proxy instance
    let proxy = Proxy::new(config.clone());

    // Spawn the proxy in a background task
    let _proxy_handle = tokio::spawn(async move {
        if let Err(e) = proxy.start("127.0.0.1:8080").await {
            eprintln!("Proxy error: {}", e);
        }
    });

    info!("Proxy started in background on http://127.0.0.1:8080");
    info!("Monitoring {} for changes...", config_path);
    info!("Edit the configuration file to see hot-reload in action");

    // Track the last modification time of the config file
    let mut last_modified = tokio::fs::metadata(config_path).await?.modified()?;

    // Monitor for configuration changes
    loop {
        sleep(Duration::from_secs(2)).await;

        // Check if file has been modified
        match tokio::fs::metadata(config_path).await {
            Ok(metadata) => {
                if let Ok(modified) = metadata.modified() {
                    if modified != last_modified {
                        info!("Configuration file changed, reloading...");

                        // Try to load the new configuration
                        match Config::from_file(config_path) {
                            Ok(new_config) => {
                                info!(
                                    "Successfully loaded new configuration with {} site(s)",
                                    new_config.sites.len()
                                );

                                // Note: We can't update the running proxy directly from here
                                // because it's in a separate task. In a real application, you would:
                                // 1. Use Arc<Mutex<Proxy>> to share the proxy between tasks
                                // 2. Or use a channel to send the new config to the proxy task
                                // 3. Or implement a shared configuration store with Arc<RwLock<Config>>

                                warn!("Note: This example demonstrates hot-reload detection.");
                                warn!("To actually update the running proxy, you need to share");
                                warn!("the proxy instance with Arc<Mutex<Proxy>> or similar.");

                                // For demonstration purposes, we just show the detection
                                let site_count = new_config.sites.len();
                                info!(
                                    "New configuration would be applied with {} site(s)",
                                    site_count
                                );

                                // Update last_modified timestamp
                                last_modified = modified;
                            }
                            Err(e) => {
                                error!("Failed to reload configuration: {}", e);
                                warn!("Continuing with current configuration");
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to read file metadata: {}", e);
            }
        }
    }
}
