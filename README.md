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
- **TLS Termination**: HTTPS on the frontend with SNI-based multi-domain support
- **Method-based Routing**: Different behavior for different HTTP methods
- **Direct Responses**: Respond with custom status codes and bodies
- **Authentication Module**: Token validation and header substitution
- **Management API**: REST API for runtime configuration management (optional feature)
- **Prometheus Metrics**: Request counters, latency histograms, and TLS handshake metrics (optional feature)

## Installation

### As CLI

```bash
# Install via cargo
cargo install --path .

# Or build and run directly
cargo build --release
./target/release/tiny-proxy --config config.conf
```

### As Library

Add to your `Cargo.toml`:

```toml
[dependencies]
tiny-proxy = "0.4"
```

## Docker

### Quick Start

```bash
# Pull from GitHub Container Registry
docker pull ghcr.io/denislituev/tiny-proxy:latest

# Run with a local config
docker run -d \
  -p 8080:8080 \
  -v $(pwd)/config.conf:/etc/tiny-proxy/config.conf:ro \
  ghcr.io/denislituev/tiny-proxy:latest

# With TLS
docker run -d \
  -p 8443:8443 \
  -v $(pwd)/config.conf:/etc/tiny-proxy/config.conf:ro \
  -v $(pwd)/certs:/etc/ssl/tiny-proxy:ro \
  ghcr.io/denislituev/tiny-proxy:latest
```

### Docker Compose

```yaml
services:
  proxy:
    image: ghcr.io/denislituev/tiny-proxy:latest
    ports:
      - "8080:8080"
    volumes:
      - ./config.conf:/etc/tiny-proxy/config.conf:ro
```

See [`docker-compose.yml`](docker-compose.yml) for a full example with TLS + echo backends.

### Build from Source

```bash
git clone https://github.com/denislituev/tiny-proxy.git
cd tiny-proxy
docker build -t tiny-proxy .
```

The image is based on Alpine Linux with CA certificates (~7 MB), so HTTPS backends work out of the box.

## Usage

### CLI Mode

Run as standalone server:

```bash
# Auto-detect listeners from config (recommended)
tiny-proxy --config config.conf

# Or specify a single listen address
tiny-proxy --config config.conf --addr 127.0.0.1:8080
```

#### CLI Arguments

- `--config, -c`: Path to configuration file (default: `./file.conf`)
- `--addr, -a`: Optional. Bind a **single** listener on this address (plain `start()`).
  When omitted, **auto-detect mode** (`start_all()`): one listener per site address in config.
  TLS sites → HTTPS with SNI; non-TLS → HTTP. In auto-detect mode only, each TLS port also
  gets an HTTP→HTTPS redirect listener (`redirect_port = tls_port - 443 + 80`, e.g. 443→80,
  8443→8080). With `--addr`, only the specified listener runs — **no** automatic redirect server.

### Library Mode

#### Basic Example

```rust
use tiny_proxy::{Config, Proxy};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration from file
    let config = Config::from_file("config.conf")?;
    
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
    let config = Config::from_file("config.conf")?;
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

Update configuration at runtime without restart. The proxy uses `Arc<ArcSwap<Config>>` internally,
so routing and directive changes take effect on the next request (including keep-alive connections).

> **TLS certificates**: cert/key files and `TlsAcceptor` are loaded when a listener starts.
> Hot-reload updates site routing and directives, but **not** TLS certificates — to pick up
> new certs or keys, restart the proxy (or the TLS listener).

Example:

```rust
use arc_swap::ArcSwap;
use tiny_proxy::{Config, Proxy};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_file("config.conf")?;
    let proxy = Proxy::new(config);

    // Get shared config handle for hot-reload
    let config_handle = proxy.shared_config();

    // Spawn proxy in background
    let handle = tokio::spawn(async move {
        if let Err(e) = proxy.start("127.0.0.1:8080").await {
            eprintln!("Proxy error: {}", e);
        }
    });

    // Update config at runtime — takes effect on the next request
    let new_config = Config::from_file("new-config.conf")?;
    config_handle.store(Arc::new(new_config));

    handle.await?;
    Ok(())
}
```

Or use the built-in `update_config` method:

```rust
let new_config = Config::from_file("updated-config.conf")?;
proxy.update_config(new_config);
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

Forward requests to a backend server. Supports optional block syntax for timeout configuration.

```caddy
# Simple
localhost:8080 {
    reverse_proxy http://backend:3000
}

# With timeouts (for LLM/SSE backends)
localhost:8080 {
    reverse_proxy http://llm-backend:8000 {
        connect_timeout 10s
        read_timeout 600s
    }
}
```

Timeout values support duration suffixes: `30s`, `5m`, `2h`, `1d`, or plain numbers (seconds).

#### `tls`

Enable HTTPS on the frontend with TLS termination. Specify paths to the certificate chain and private key (PEM format).

```caddy
# Single domain with TLS
example.com:443 {
    tls /etc/ssl/cert.pem /etc/ssl/key.pem
    reverse_proxy backend:8080
}

# Multiple domains on port 443 (SNI-based routing)
example.com:443 {
    tls /etc/ssl/example.com/cert.pem /etc/ssl/example.com/key.pem
    reverse_proxy backend:8080
}

api.example.com:443 {
    tls /etc/ssl/api.example.com/cert.pem /etc/ssl/api.example.com/key.pem
    reverse_proxy api-backend:3000
}
```

**Auto-detect mode** (no `--addr`, uses `start_all()`):
- One HTTPS listener per TLS site address, with SNI-based certificate selection
- HTTP→HTTPS redirect per TLS port: `redirect_port = tls_port - 443 + 80` (443→80, 8443→8080)
- Correct `X-Forwarded-Proto: https` sent to backends

**Single-address mode** (`--addr`): only the given listener is started — no automatic redirect server.
Use this when you bind one port manually; use auto-detect for full multi-site + redirect setup.

> If the redirect port is already in use, HTTPS continues to work; redirect is skipped with a warning.

> **Host header**: on default ports (443/80), browsers omit the port (`Host: example.com`).
> On non-default TLS ports (e.g. 8443), browsers include it (`Host: example.com:8443`) — config
> keys must match. See `find_site` docs in `handler.rs` for details.

> **Known limitation**: TLS certs are loaded at listener startup; hot-reload does not reload them (see Hot-Reload above).

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

Add, modify, or remove request headers.

```caddy
localhost:8080 {
    # Add header with placeholder
    header X-Request-ID {uuid}

    # Add static header
    header X-Custom-Header custom-value

    # Remove header (prefix with -)
    header -Accept-Encoding

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

#### `strip_prefix`

Remove a prefix from the request URI path.

```caddy
localhost:8080 {
    strip_prefix /api
    reverse_proxy http://backend:3000
}
```

Request `/api/users/123` → backend receives `/users/123`.

#### `redirect`

Return a redirect response with `Location` header.

```caddy
localhost:8080 {
    # Permanent redirect (default 301)
    redirect https://new-domain.com

    # Temporary redirect
    redirect 302 /maintenance
}
```

Supported status codes: `301` (permanent), `302` (temporary), `307`, `308`.

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
- `tls` - HTTPS on the frontend (TLS termination, SNI-based multi-domain)
- `api` - Management API for runtime configuration
- `logging` - Structured access logs

> **Note**: HTTPS **backend** connections (`hyper-rustls`) are always available —
> the `tls` feature only controls frontend TLS termination.

### Optional Features

```toml
# Minimal - core HTTP proxy with HTTPS backend support (for embedding)
[dependencies]
tiny-proxy = { version = "0.4", default-features = false }

# With frontend TLS termination
[dependencies]
tiny-proxy = { version = "0.4", default-features = false, features = ["tls"] }

# With management API
[dependencies]
tiny-proxy = { version = "0.4", default-features = false, features = ["tls", "api"] }

# With Prometheus metrics
[dependencies]
tiny-proxy = { version = "0.4", default-features = false, features = ["metrics"] }

# Full standalone (same as default)
[dependencies]
tiny-proxy = "0.4"
```

#### `cli` (default)

Enable CLI dependencies and `tiny-proxy` binary.

#### `tls` (default)

Enable **frontend TLS termination** (HTTPS listeners) with SNI-based multi-domain support:

- Frontend: `rustls` + `tokio-rustls` for HTTPS listeners with SNI-based routing
- `rustls-pemfile` for loading PEM certificate chains and private keys

> HTTPS **backend** connections (`hyper-rustls`) are always available, regardless
> of this feature.

#### `api` (default)

Management API for runtime configuration:

```rust
use arc_swap::ArcSwap;
use tiny_proxy::api;
use std::sync::Arc;

let config = Arc::new(ArcSwap::from_pointee(Config::from_file("config.conf")?));
api::start_api_server("127.0.0.1:8081", config).await?;
```

#### `metrics` (optional)

Prometheus metrics exposed via a separate admin HTTP server on `/metrics`:

- `http_requests_total{method,status,site}` — counter
- `http_request_duration_seconds{method,status}` — histogram
- `http_active_requests` — gauge (in-flight requests)
- `tls_handshakes_total{status}` — counter (`ok` / `fail`)

```bash
# CLI flag or TINY_PROXY_METRICS_ADDR env var
cargo run --features metrics -- --config config.conf --metrics-addr 127.0.0.1:9090
curl http://127.0.0.1:9090/metrics
```

Or from library code:

```rust
use tiny_proxy::metrics;

metrics::start_metrics_server("127.0.0.1:9090".parse()?)?;
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
- `Proxy::from_shared(config)` - Create proxy from shared `Arc<ArcSwap<Config>>`
- `Proxy::start(addr)` - Start proxy server
- `Proxy::shared_config()` - Get `Arc<ArcSwap<Config>>` for external config updates
- `Proxy::config_snapshot()` - Read current configuration as `Arc<Config>`
- `Proxy::update_config(config)` - Update configuration at runtime

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
- ⏳ Buffering control
- ✅ TLS/SSL termination (SNI, multi-domain, HTTP→HTTPS redirect)
- ⏳ WebSocket support
- ⏳ Rate limiting
- ✅ Structured access log with X-Request-ID (method, path, host, status, duration, bytes_sent)
- ✅ Prometheus metrics (request counters, latency histograms, TLS handshake counters)

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

