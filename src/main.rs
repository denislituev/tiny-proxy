mod cli;
mod config;
mod proxy;

use clap::Parser;
use cli::Cli;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    println!("Tiny Proxy Server v{}", env!("CARGO_PKG_VERSION"));
    println!("Loading config from: {}", cli.config);

    let config = config::Config::from_file(&cli.config)?;
    let addr: SocketAddr = cli.addr.parse()?;

    proxy::start_proxy(addr, config).await
}
