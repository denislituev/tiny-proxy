//! Tiny Proxy Server - Embeddable HTTP Reverse Proxy
//!
//! This library provides a lightweight, configurable HTTP reverse proxy
//! that can be embedded into Rust applications or run as a standalone CLI tool.
//!
//! ## Features
//!
//! - Configuration via Caddy-like syntax
//! - Path-based routing with pattern matching
//! - Header manipulation
//! - URI rewriting
//! - HTTP/HTTPS backend support
//!
//! ## Example (Library Mode)
//!
//! ```no_run
//! use tiny_proxy::{Config, Proxy};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Load configuration from file
//!     let config = Config::from_file("config.caddy")?;
//!
//!     // Create and start proxy
//!     let proxy = Proxy::new(config);
//!     proxy.start("127.0.0.1:8080").await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Example (Background Execution)
//!
//! To run the proxy in the background while doing other work:
//!
//! ```no_run
//! use tiny_proxy::{Config, Proxy};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = Config::from_file("config.caddy")?;
//!     let proxy = Proxy::new(config);
//!
//!     // Spawn proxy in background
//!     let handle = tokio::spawn(async move {
//!         if let Err(e) = proxy.start("127.0.0.1:8080").await {
//!             eprintln!("Proxy error: {}", e);
//!         }
//!     });
//!
//!     // Do other work here...
//!
//!     handle.await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Example (CLI Mode)
//!
//! When built as a binary, the proxy can be run from command line:
//!
//! ```bash
//! tiny-proxy --config config.caddy --addr 127.0.0.1:8080
//! ```
//!
//! ## Configuration Format
//!
//! The proxy uses a Caddy-like configuration format:
//!
//! ```text
//! localhost:8080 {
//!     reverse_proxy backend:3000
//!     header X-Forwarded-For {remote_ip}
//! }
//! ```
//!
//! For more configuration options, see the [config] module documentation.

#[cfg(feature = "cli")]
pub mod cli;
pub mod config;
pub mod error;
pub mod proxy;

#[cfg(feature = "api")]
pub mod api;

// Re-export commonly used types for convenience
pub use config::Config;
pub use error::{ProxyError, Result};
pub use proxy::{ActionResult, Proxy};

#[cfg(feature = "api")]
pub use api::start_api_server;
