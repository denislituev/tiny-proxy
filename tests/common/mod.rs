//! Common test utilities for tiny-proxy integration tests
//!
//! This module provides shared utilities for testing the proxy server,
//! including mock backends, test proxy instances, and helper functions.

pub use mock_server::MockServer;

mod mock_server;

use std::time::Duration;
use tokio::time::sleep;

/// Create a test backend server that responds with the given status and body
pub fn setup_test_backend(port: u16) -> MockServer {
    MockServer::new(port)
}

/// Make a simple GET request to the given URL
///
/// # Example
///
/// ```no_run
/// use tiny_proxy::tests::common::make_test_request;
///
/// #[tokio::test]
/// async fn test_example() {
///     let response = make_test_request("http://localhost:8080").await.unwrap();
///     assert_eq!(response.status(), 200);
/// }
/// ```
pub async fn make_test_request(url: &str) -> Result<reqwest::Response, reqwest::Error> {
    reqwest::get(url).await
}

/// Wait for a server to become available at the given URL
///
/// This is useful when starting a server asynchronously and need to wait
/// for it to be ready before making test requests.
///
/// # Arguments
///
/// * `url` - The URL to check
/// * `max_attempts` - Maximum number of connection attempts
/// * `interval_ms` - Interval between attempts in milliseconds
///
/// # Example
///
/// ```no_run
/// use tiny_proxy::tests::common::wait_for_server;
///
/// #[tokio::main]
/// async fn main() {
///     // Start your server...
///
///     // Wait for it to be ready
///     wait_for_server("http://localhost:8080", 10, 100).await.unwrap();
///
///     // Now you can make requests
/// }
/// ```
pub async fn wait_for_server(
    url: &str,
    max_attempts: usize,
    interval_ms: u64,
) -> Result<(), reqwest::Error> {
    for _ in 0..max_attempts {
        match reqwest::get(url).await {
            Ok(response) => {
                // Server is responding
                drop(response);
                return Ok(());
            }
            Err(_) => {
                // Server not ready yet, wait and retry
                sleep(Duration::from_millis(interval_ms)).await;
            }
        }
    }

    Err(reqwest::Error::from(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!(
            "Server at {} did not respond after {} attempts",
            url, max_attempts
        ),
    )))
}

/// Find an available port on localhost
///
/// This is useful for tests that need to start servers on random ports
/// to avoid port conflicts.
///
/// # Example
///
/// ```no_run
/// use tiny_proxy::tests::common::find_available_port;
///
/// #[tokio::main]
/// async fn main() {
///     let port = find_available_port(8000, 9000).unwrap();
///     println!("Using port: {}", port);
/// }
/// ```
pub fn find_available_port(range_start: u16, range_end: u16) -> Option<u16> {
    for port in range_start..=range_end {
        if let Ok(listener) = std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
            drop(listener);
            return Some(port);
        }
    }
    None
}

/// Create a temporary config file with the given content
///
/// This is useful for tests that need to test different configurations.
/// The file is automatically deleted when the returned `TempConfigFile` is dropped.
///
/// # Example
///
/// ```no_run
/// use tiny_proxy::tests::common::TempConfigFile;
///
/// #[tokio::test]
/// async fn test_custom_config() {
///     let config = TempConfigFile::new("localhost:8080 { reverse_proxy backend:3000 }");
///
///     // Use config.path() to load the configuration
///     let loaded_config = tiny_proxy::Config::from_file(config.path()).unwrap();
///
///     // Test with the loaded configuration...
/// }
/// ```
pub struct TempConfigFile {
    path: String,
}

impl TempConfigFile {
    /// Create a new temporary config file with the given content
    pub fn new(content: &str) -> Self {
        use std::io::Write;
        let mut temp_file = std::env::temp_dir();
        temp_file.push(format!("tiny-proxy-test-{}.caddy", uuid::Uuid::new_v4()));

        let path = temp_file.to_string_lossy().to_string();

        let mut file = std::fs::File::create(&path).expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write temp file");

        Self { path }
    }

    /// Get the path to the temporary config file
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for TempConfigFile {
    fn drop(&mut self) {
        // Best effort cleanup - ignore errors
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_available_port() {
        let port = find_available_port(12000, 13000).expect("Should find available port");
        assert!(port >= 12000 && port <= 13000);
    }

    #[tokio::test]
    async fn test_temp_config_file() {
        let config = TempConfigFile::new("test config content");

        assert!(std::path::Path::new(config.path()).exists());
        let content = std::fs::read_to_string(config.path()).unwrap();
        assert_eq!(content, "test config content");
    }

    #[tokio::test]
    async fn test_wait_for_server_timeout() {
        // Test with a server that doesn't exist
        let result = wait_for_server("http://localhost:99999", 2, 10).await;
        assert!(result.is_err());
    }
}
