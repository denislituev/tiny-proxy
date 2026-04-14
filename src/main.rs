use clap::Parser;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use tiny_proxy::cli::Cli;
use tiny_proxy::config::Config;

#[cfg(feature = "api")]
use std::sync::Arc;

#[cfg(feature = "api")]
use tokio::sync::{broadcast, RwLock};

#[cfg(feature = "api")]
use tiny_proxy::start_api_server;
use tiny_proxy::Proxy;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    // Parse CLI arguments
    let cli = Cli::parse();

    info!("Tiny Proxy Server v{}", env!("CARGO_PKG_VERSION"));
    info!("Loading config from: {}", cli.config);

    // Load configuration
    let config = Config::from_file(&cli.config)?;

    #[cfg(feature = "api")]
    if cli.enable_api {
        run_with_api(cli, config).await?;
    } else {
        run_proxy_only(cli, config).await?;
    }

    #[cfg(not(feature = "api"))]
    run_proxy_only(cli, config).await?;

    Ok(())
}

/// Run proxy server only (no API server)
async fn run_proxy_only(cli: Cli, config: Config) -> Result<(), anyhow::Error> {
    let mut proxy = Proxy::new(config);

    // Set custom concurrency limit if specified
    if cli.max_concurrency > 0 {
        proxy.set_max_concurrency(cli.max_concurrency);
    }

    info!("Starting proxy server on {}", cli.addr);

    // Setup graceful shutdown
    let shutdown_signal = setup_shutdown_signal();

    // Use tokio::select to wait for either proxy completion or shutdown signal
    tokio::select! {
        result = proxy.start(&cli.addr) => {
            if let Err(e) = result {
                error!("Proxy server error: {}", e);
                Err(e)
            } else {
                Ok(())
            }
        },
        _ = shutdown_signal => {
            info!("Shutdown signal received");
            info!("Proxy server shutting down...");
            Ok(())
        }
    }
}

/// Run both proxy and API servers in parallel (requires 'api' feature)
#[cfg(feature = "api")]
async fn run_with_api(cli: Cli, config: Config) -> Result<(), anyhow::Error> {
    // Create shared configuration
    let shared_config = Arc::new(RwLock::new(config.clone()));

    // Create shutdown channel
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    info!("Starting proxy server on {}", cli.addr);
    info!("Starting API server on {}", cli.api_addr);

    // Spawn API server task
    let api_handle = tokio::spawn(run_api_server(
        cli.api_addr.clone(),
        shared_config.clone(),
        shutdown_tx.subscribe(),
    ));

    // Spawn proxy server task with shared config
    let proxy_handle = tokio::spawn(run_proxy_server(
        cli.addr.clone(),
        cli.max_concurrency,
        shared_config,
        shutdown_tx.subscribe(),
    ));

    // Wrap in Option to allow multiple uses
    let mut api_handle = Some(api_handle);
    let mut proxy_handle = Some(proxy_handle);

    // Wait for shutdown signal
    tokio::select! {
        // API server completed
        api_result = async { api_handle.as_mut().unwrap().await } => {
            match api_result {
                Ok(Ok(())) => info!("API server shut down gracefully"),
                Ok(Err(e)) => error!("API server error: {}", e),
                Err(e) => error!("API server task panicked: {}", e),
            }
            // Notify proxy to shutdown
            let _ = shutdown_tx.send(());
        },
        // Proxy server completed
        proxy_result = async { proxy_handle.as_mut().unwrap().await } => {
            match proxy_result {
                Ok(Ok(())) => info!("Proxy server shut down gracefully"),
                Ok(Err(e)) => error!("Proxy server error: {}", e),
                Err(e) => error!("Proxy server task panicked: {}", e),
            }
            // Notify API to shutdown
            let _ = shutdown_tx.send(());
        },
        // Shutdown signal received (Ctrl+C, SIGTERM)
        _ = setup_shutdown_signal() => {
            info!("Shutdown signal received");
            let _ = shutdown_tx.send(());
        }
    }

    // Wait for both servers to finish (with timeout)
    info!("Waiting for servers to shut down...");

    let timeout = tokio::time::Duration::from_secs(30);

    // Wait for API server
    match tokio::time::timeout(timeout, api_handle.take().unwrap()).await {
        Ok(Ok(Ok(()))) => info!("API server shut down"),
        Ok(Ok(Err(e))) => warn!("API server shutdown error: {}", e),
        Ok(Err(e)) => warn!("API server task error: {}", e),
        Err(_) => {
            warn!("API server shutdown timeout");
            if let Some(handle) = api_handle {
                handle.abort();
            }
        }
    }

    // Wait for proxy server
    match tokio::time::timeout(timeout, proxy_handle.take().unwrap()).await {
        Ok(Ok(Ok(()))) => info!("Proxy server shut down"),
        Ok(Ok(Err(e))) => warn!("Proxy server shutdown error: {}", e),
        Ok(Err(e)) => warn!("Proxy server task error: {}", e),
        Err(_) => {
            warn!("Proxy server shutdown timeout");
            if let Some(handle) = proxy_handle {
                handle.abort();
            }
        }
    }

    info!("All servers shut down");
    Ok(())
}

/// Run proxy server with shared config and shutdown support
#[cfg(feature = "api")]
async fn run_proxy_server(
    addr: String,
    max_concurrency: usize,
    shared_config: Arc<RwLock<Config>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), anyhow::Error> {
    // Create proxy from the shared config handle — any updates to
    // shared_config (e.g. via POST /config) are immediately visible
    // to new proxy connections.
    let mut proxy = Proxy::from_shared(shared_config);

    // Set custom concurrency limit if specified
    if max_concurrency > 0 {
        proxy.set_max_concurrency(max_concurrency);
    }

    // Run proxy server
    tokio::select! {
        result = proxy.start(&addr) => {
            result
        },
        _ = shutdown_rx.recv() => {
            info!("Proxy server received shutdown signal");
            Ok(())
        }
    }
}

/// Run API server with shared config and shutdown support
#[cfg(feature = "api")]
async fn run_api_server(
    addr: String,
    shared_config: Arc<RwLock<Config>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), anyhow::Error> {
    tokio::select! {
        result = start_api_server(&addr, shared_config) => {
            result.map_err(|e| e.into())
        },
        _ = shutdown_rx.recv() => {
            info!("API server received shutdown signal");
            Ok(())
        }
    }
}

/// Setup shutdown signal handlers for SIGTERM and SIGINT
async fn setup_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        // Handle SIGTERM
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to setup SIGTERM handler: {}", e);
                // If we can't setup SIGTERM, use a future that never completes
                return std::future::pending().await;
            }
        };

        // Handle SIGINT (Ctrl+C)
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to setup SIGINT handler: {}", e);
                return std::future::pending().await;
            }
        };

        // Wait for either signal
        tokio::select! {
            _ = sigterm.recv() => info!("SIGTERM received"),
            _ = sigint.recv() => info!("SIGINT (Ctrl+C) received"),
        }
    }

    #[cfg(windows)]
    {
        use tokio::signal::ctrl_c;

        match ctrl_c().await {
            Ok(()) => info!("Ctrl+C received"),
            Err(e) => {
                warn!("Failed to setup Ctrl+C handler: {}", e);
                std::future::pending().await
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Fallback for other platforms - use tokio::signal if available
        #[cfg(feature = "tokio/signal")]
        {
            use tokio::signal::ctrl_c;
            match ctrl_c().await {
                Ok(()) => info!("Ctrl+C received"),
                Err(e) => {
                    warn!("Failed to setup Ctrl+C handler: {}", e);
                    std::future::pending().await
                }
            }
        }

        #[cfg(not(feature = "tokio/signal"))]
        {
            // If no signal support, just hang forever
            warn!("No signal support on this platform, manual shutdown required");
            std::future::pending().await
        }
    }
}
