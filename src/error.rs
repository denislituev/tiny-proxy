//! Error types for tiny-proxy
//!
//! This module defines custom error types using `thiserror` for better
//! error handling and reporting throughout the application.

use std::net::AddrParseError;
use thiserror::Error;

/// Main error type for the proxy application
#[derive(Error, Debug)]
pub enum ProxyError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Backend proxy errors
    #[error("Backend error: {0}")]
    Backend(String),

    /// Parsing errors (config, directives, etc.)
    #[error("Parse error: {0}")]
    Parse(String),

    /// I/O errors automatically converted from std::io::Error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP-related errors
    #[error("HTTP error: {0}")]
    Http(String),

    /// Network connection errors
    #[error("Connection error: {0}")]
    Connection(String),

    /// Directive processing errors
    #[error("Directive error: {0}")]
    Directive(String),

    /// URL parsing errors
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Address parsing errors
    #[error("Invalid address: {0}")]
    Address(String),
}

/// Type alias for Result with ProxyError
///
/// This makes error handling more ergonomic throughout the codebase.
pub type Result<T> = std::result::Result<T, ProxyError>;

impl From<AddrParseError> for ProxyError {
    fn from(err: AddrParseError) -> Self {
        ProxyError::Address(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ProxyError::Config("Test error".to_string());
        assert_eq!(err.to_string(), "Configuration error: Test error");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let proxy_err: ProxyError = io_err.into();
        assert!(matches!(proxy_err, ProxyError::Io(_)));
        assert!(proxy_err.to_string().contains("file not found"));
    }

    #[test]
    fn test_result_type_alias() {
        let result: Result<String> = Ok("test".to_string());
        assert!(result.is_ok());

        let result: Result<String> = Err(ProxyError::Config("error".to_string()));
        assert!(result.is_err());
    }
}
