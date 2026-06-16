use clap::Parser;

/// Command line arguments for the proxy server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file (Caddy-like format)
    #[arg(short, long, default_value = "./file.conf")]
    pub config: String,

    /// Address for proxy server to listen on.
    /// When omitted, auto-detects listeners from config (one per site address).
    /// Use this to override when you want a single listener on a specific address.
    #[arg(short = 'a', long)]
    pub addr: Option<String>,

    /// Max concurrent connections (default: CPU cores * 256, use 0 for default)
    #[arg(long, default_value_t = 0)]
    pub max_concurrency: usize,

    /// Enable management API server (requires 'api' feature)
    #[arg(long)]
    pub enable_api: bool,

    /// Address for API server to listen on
    #[arg(long, default_value = "127.0.0.1:8081")]
    pub api_addr: String,

    /// Address for Prometheus metrics server (requires 'metrics' feature).
    /// Can also be set via the `TINY_PROXY_METRICS_ADDR` environment variable.
    #[arg(long, env = "TINY_PROXY_METRICS_ADDR")]
    pub metrics_addr: Option<String>,
}
