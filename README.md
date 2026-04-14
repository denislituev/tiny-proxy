# Tiny Proxy

[![CI](https://github.com/denislituev/tiny-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/denislituev/tiny-proxy/actions)
[![Version](https://img.shields.io/crates/v/tiny-proxy)](https://crates.io/crates/tiny-proxy)
[![Downloads](https://img.shields.io/crates/d/tiny-proxy)](https://crates.io/crates/tiny-proxy)

[![License](https://img.shields.io/crates/l/tiny-proxy)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://www.rust-lang.org/)

Lightweight, embeddable HTTP reverse proxy written in Rust with Caddy-like configuration syntax.

## Features

- **Embeddable Library**: Use as a library in your Rust applications or run as standalone CLI
- **Caddy-like Configuration**: Simple, human-readable configuration format
- **Path-based Routing**: Pattern matching with wildcard support
- **Header Manipulation**: Add, modify, or remove headers
- **URI Rewriting**: Replace parts of request URIs
- **HTTP/HTTPS Backend Support**: Full support for both HTTP and HTTPS backends
- **Method-based Routing**: Different behavior for different HTTP methods
- **Direct Responses**: Respond with custom status codes and bodies
- **Authentication Module**: Token validation and header substitution
- **Management API**: REST API for runtime configuration management (optional feature)

## Installation

### As CLI

```bash
# Install via cargo
cargo install --path .

# Or build and run directly
cargo build --release
./target/release/tiny-proxy --config config.caddy
```

### As Library

Add to your `Cargo.toml`:

```toml
[dependencies]
tiny-proxy = "0.2"
```

## Usage

### CLI Mode

Run as standalone server:

```bash
tiny-proxy --config config.caddy --addr 127.0.0.1:8080
```

#### CLI Arguments

- `--config, -c`: Path to configuration file (default: `./file.caddy`)
- `--addr, -a`: Address to listen on (default: `127.0.0.1:8080`)

### Library Mode

#### Basic Example

```rust
use tiny_proxy::{Config, Proxy};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration from file
    let config = Config::from_file("config.caddy")?;
    
    // Create and start proxy
    let proxy = Proxy::new(config);
    proxy.start("127.0.0.1:8080").await?;
    
    Ok(())
}
```

#### Background Execution

Run proxy in background while doing other work:

```rust
use tiny_proxy::{Config, Proxy};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_file("config.caddy")?;
    let proxy = Arc::new(Proxy::new(config));
    
    // Spawn proxy in background
    let handle = tokio::spawn(async move {
        if let Err(e) = proxy.start("127.0.0.1:8080").await {
            eprintln!("Proxy error: {}", e);
        }
    });
    
    // Do other work here...
    handle.await?;
    
    Ok(())
}
```

#### Hot-Reload Configuration

Update configuration at runtime without restart. The proxy uses `Arc<RwLock<Config>>` internally,
so any config change takes effect immediately for new connections:

```rust
use tiny_proxy::{Config, Proxy};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_file("config.caddy")?;
    let proxy = Proxy::new(config);

    // Get shared config handle for hot-reload
    let config_handle = proxy.shared_config();

    // Spawn proxy in background
    let handle = tokio::spawn(async move {
        if let Err(e) = proxy.start("127.0.0.1:8080").await {
            eprintln!("Proxy error: {}", e);
        }
    });

    // Update config at runtime — takes effect immediately
    let new_config = Config::from_file("new-config.caddy")?;
    {
        let mut guard = config_handle.write().await;
        *guard = new_config;
    }

    handle.await?;
    Ok(())
}
```

Or use the built-in `update_config` method:

```rust
let new_config = Config::from_file("updated-config.caddy")?;
proxy.update_config(new_config).await;
```

## Configuration

tiny-proxy uses a Caddy-like configuration format.

### Basic Syntax

```caddy
site_address {
    directive1 arg1 arg2
    directive2 {
        nested_directive
    }
}
```

### Supported Directives

#### `reverse_proxy`

Forward requests to a backend server.

```caddy
localhost:8080 {
    reverse_proxy http://backend:3000
}
```

#### `handle_path`

Match paths with pattern (supports wildcard `*`).

```caddy
localhost:8080 {
    handle_path /api/* {
        reverse_proxy api-service:8000
    }
}
```

#### `uri_replace`

Replace part of the request URI.

```caddy
localhost:8080 {
    uri_replace /old-path /new-path
    reverse_proxy backend:3000
}
```

#### `header`

Add or modify request headers.

```caddy
localhost:8080 {
    header X-Request-ID {uuid}
    header X-Custom-Header custom-value
    reverse_proxy backend:3000
}
```

#### `method`

Apply directives based on HTTP method.

```caddy
localhost:8080 {
    method GET HEAD {
        respond 200 "OK"
    }
    reverse_proxy backend:3000
}
```

#### `respond`

Return a direct response with custom status and body.

```caddy
localhost:8080 {
    respond 200 "Service is healthy"
}
```

### Configuration Examples

#### Simple Reverse Proxy

```caddy
localhost:8080 {
    reverse_proxy http://backend:3000
}
```

#### Multi-site Configuration

```caddy
api.example.com {
    reverse_proxy http://api-service:8000
}

static.example.com {
    reverse_proxy http://static-service:8001
}
```

#### API with Versioning

```caddy
localhost:8080 {
    handle_path /api/v1/* {
        handle_path /users/* {
            reverse_proxy http://user-service:8001
        }
        reverse_proxy http://api-service:8000
    }
    reverse_proxy http://default-backend:3000
}
```

#### Headers and URI Rewriting

```caddy
localhost:8080 {
    header X-Forwarded-For {header.X-Forwarded-For}
    header X-Request-ID {uuid}
    uri_replace /api /backend
    reverse_proxy http://backend:3000
}
```

#### Health Check Endpoint

```caddy
localhost:8080 {
    method GET HEAD {
        respond 200 "OK"
    }
    reverse_proxy http://backend:3000
}
```

### Placeholders

Use placeholders in header values:

- `{header.Name}` - Value of request header with that name
- `{env.VAR}` - Value of environment variable
- `{uuid}` - Random UUID

## Features

### Default Features

- `cli` - Command-line interface support
- `tls` - HTTPS backend support
- `api` - Management API for runtime configuration

### Optional Features

```toml
# Minimal - core proxy only (for embedding in other applications)
[dependencies]
tiny-proxy = { version = "0.2", default-features = false }

# With HTTPS backend support
[dependencies]
tiny-proxy = { version = "0.2", default-features = false, features = ["tls"] }

# With management API
[dependencies]
tiny-proxy = { version = "0.2", default-features = false, features = ["tls", "api"] }

# Full standalone (same as default)
[dependencies]
tiny-proxy = "0.2"
```

#### `cli` (default)

Enable CLI dependencies and `tiny-proxy` binary.

#### `tls` (default)

Enable HTTPS backend support using `hyper-rustls` (pure Rust TLS).

#### `api` (default)

Management API for runtime configuration:

```rust
use tiny_proxy::api;
use std::sync::Arc;
use tokio::sync::RwLock;

let config = Arc::new(RwLock::new(Config::from_file("config.caddy")?));
api::start_api_server("127.0.0.1:8081", config).await?;
```

## API Documentation

See the [module documentation](https://docs.rs/tiny-proxy) for detailed API reference.

### Main Types

- `Config` - Configuration container
- `Proxy` - Proxy instance
- `Directive` - Configuration directives
- `SiteConfig` - Per-site configuration

### Main Functions

- `Config::from_file(path)` - Load configuration from file
- `Config::from_str(content)` - Parse configuration from string
- `Proxy::new(config)` - Create proxy instance
- `Proxy::from_shared(config)` - Create proxy from shared `Arc<RwLock<Config>>`
- `Proxy::start(addr)` - Start proxy server
- `Proxy::shared_config()` - Get `Arc<RwLock<Config>>` for external config updates
- `Proxy::config_snapshot()` - Read current configuration as owned value
- `Proxy::update_config(config)` - Update configuration at runtime (async)

## Testing

Run all tests:

```bash
cargo test
```

Run specific tests:

```bash
# Specific test
cargo test test_pattern_matching
```

Run tests with logging:

```bash
RUST_LOG=debug cargo test
```

## Benchmarking

Run benchmarks:

```bash
cargo bench
```

Run specific benchmark:

```bash
cargo bench -- benchmark_name
```

## Development

### Project Structure

```
tiny-proxy/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library entry point
│   ├── cli/                 # CLI module
│   ├── config/              # Configuration parsing
│   ├── proxy/               # Proxy logic
│   ├── auth/                # Authentication (optional)
│   └── api/                 # Management API (optional)
├── examples/                # Usage examples
├── benches/                 # Benchmarks
```

### Build with Features

```bash
# Default (CLI + TLS + API)
cargo build

# Library only (no CLI dependencies)
cargo build --no-default-features

# Library with HTTPS support
cargo build --no-default-features --features tls

# Library with API for config management
cargo build --no-default-features --features tls,api

# CLI without API
cargo build --no-default-features --features cli,tls
```

### Run Examples

```bash
# Basic example
cargo run --example basic

# Background execution
cargo run --example background
```

## Roadmap



### Current Status

- ✅ Library mode
- ✅ CLI mode
- ✅ Configuration parsing
- ✅ Reverse proxy
- ✅ Path-based routing
- ✅ Header manipulation
- ✅ URI rewriting
- ✅ Method-based routing
- ✅ Direct responses
- ✅ Authentication module (basic)
- ✅ Management API with hot-reload

### Planned Features

- ⏳ Static file serving
- ⏳ Try files (SPA support)
- ⏳ Timeout configurations
- ⏳ Buffering control
- ⏳ TLS/SSL support
- ⏳ WebSocket support
- ⏳ Rate limiting
- ⏳ Request/response logging
- ⏳ Metrics and monitoring

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Run `cargo test` and `cargo clippy`
6. Submit a pull request

## License

See [LICENSE](LICENSE) file.

