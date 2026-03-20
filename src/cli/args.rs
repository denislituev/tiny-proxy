use clap::Parser;

/// Command line arguments for the proxy server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file (Caddy-like format)
    #[arg(short, long, default_value = "./file.caddy")]
    pub config: String,

    /// Address for proxy server to listen on
    #[arg(short = 'a', long, default_value = "127.0.0.1:8080")]
    pub addr: String,

    /// Enable management API server (requires 'api' feature)
    #[arg(long)]
    pub enable_api: bool,

    /// Address for API server to listen on
    #[arg(long, default_value = "127.0.0.1:8081")]
    pub api_addr: String,
}
