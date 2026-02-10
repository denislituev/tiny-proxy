use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file (Caddy-like format)
    #[arg(short, long, default_value = "./file.caddy")]
    pub config: String,

    /// Address to listen on
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    pub addr: String,
}
