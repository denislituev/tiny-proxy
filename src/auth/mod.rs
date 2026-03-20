//! Authentication and authorization module for tiny-proxy
//!
//! This module provides functionality for:
//! - Token validation via external services
//! - Header manipulation and substitution
//! - Authentication middleware integration
//!
//! # Example
//!
//! ```no_run
//! use tiny_proxy::auth;
//! use hyper::Request;
//!
//! # async fn example(req: Request<hyper::body::Incoming>) -> anyhow::Result<()> {
//! // Validate a token using an external validator
//! let is_valid = auth::validate_token(&req, "https://auth.example.com/validate").await?;
//!
//! // Process header substitutions
//! let value = auth::process_header_substitution("Bearer {header.Authorization}", &req)?;
//! # Ok(())
//! # }
//! ```

pub mod headers;
pub mod validator;

// Re-export commonly used functions for convenience
pub use headers::process_header_substitution;
pub use validator::validate_token;
