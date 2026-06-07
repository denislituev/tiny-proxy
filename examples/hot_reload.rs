//! Example of hot-reloading configuration without restarting the proxy
//!
//! Run with:
//! ```bash
//! cargo run --example hot_reload
//! ```
//!
//! Then edit `file.conf` while the proxy is running.

use arc_swap::ArcSwap;
use std::sync::Arc;
use tiny_proxy::{Config, Proxy};
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let config_path = "file.conf";
    let config = Config::from_file(config_path)?;
    info!(
        "Loaded initial configuration for {} site(s)",
        config.sites.len()
    );

    let shared = Arc::new(ArcSwap::from_pointee(config));
    let proxy = Proxy::from_shared(shared.clone());

    let _proxy_handle = tokio::spawn(async move {
        if let Err(e) = proxy.start("127.0.0.1:8080").await {
            eprintln!("Proxy error: {}", e);
        }
    });

    info!("Proxy started on http://127.0.0.1:8080");
    info!("Monitoring {} for changes...", config_path);

    let mut last_modified = tokio::fs::metadata(config_path).await?.modified()?;

    loop {
        sleep(Duration::from_secs(2)).await;

        let metadata = tokio::fs::metadata(config_path).await?;
        let modified = metadata.modified()?;
        if modified == last_modified {
            continue;
        }

        info!("Configuration file changed, reloading...");
        match Config::from_file(config_path) {
            Ok(new_config) => {
                let sites_count = new_config.sites.len();
                shared.store(Arc::new(new_config));
                info!("Configuration updated ({} sites)", sites_count);
                last_modified = modified;
            }
            Err(e) => {
                error!("Failed to reload configuration: {}", e);
            }
        }
    }
}
