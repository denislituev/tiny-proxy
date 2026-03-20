use clap::Parser;
use tiny_proxy::cli::Cli;
use tiny_proxy::{Config, Proxy};
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

    let config = Config::from_file(&cli.config)?;
    let proxy = Proxy::new(config);
    Ok(proxy.start(&cli.addr).await?)
}
