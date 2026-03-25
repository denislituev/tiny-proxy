//! Management API module for tiny-proxy
//!
//! This module provides a REST API for managing the proxy configuration,
//! including viewing and updating the configuration at runtime.
//!
//! # Example
//!
//! ```no_run
//! use tiny_proxy::api;
//! use tiny_proxy::Config;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! let config = Arc::new(RwLock::new(Config::from_file("config.caddy")?));
//!
//! // Start the management API server
//! api::start_api_server("127.0.0.1:8081", config).await?;
//! # Ok(())
//! # }
//! ```

pub mod endpoints;
pub mod middleware;
pub mod server;

// Re-export commonly used functions for convenience
pub use server::start_api_server;
