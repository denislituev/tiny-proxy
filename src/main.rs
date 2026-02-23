mod cli;
mod config;
mod proxy;

use clap::Parser;
use cli::Cli;
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let cli = Cli::parse();

    info!("Tiny Proxy Server v{}", env!("CARGO_PKG_VERSION"));
    info!("Loading config from: {}", cli.config);

    let config = config::Config::from_file(&cli.config)?;
    let addr: SocketAddr = cli.addr.parse()?;

    proxy::start_proxy(addr, config).await
}
