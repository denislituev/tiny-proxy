# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Prometheus metrics** (optional `metrics` feature): request counters, latency
  histograms, active-request gauge, and TLS handshake counters, exposed on
  `/metrics` via a dedicated admin HTTP server.
  - `http_requests_total{method,status,site}` — counter
  - `http_request_duration_seconds{method,status}` — histogram (11 buckets, 5ms–10s)
  - `http_active_requests` — gauge (in-flight requests)
  - `tls_handshakes_total{status}` — counter (`ok` / `fail`)
  - CLI flag `--metrics-addr` and env var `TINY_PROXY_METRICS_ADDR`
- **Lock-free hot-reload**: config storage switched to `Arc<ArcSwap<Config>>`
  — reads are wait-free, snapshots return `Arc<Config>` instead of cloning.
  `Proxy::config_snapshot()` and `Proxy::update_config()` are now synchronous.
- Integration tests: config hot-reload on keep-alive connections, Prometheus
  `/metrics` endpoint (counters, histogram buckets, gauge, metadata).
- Dependencies: `arc-swap`, `metrics`, `metrics-exporter-prometheus` (optional)

### Changed

- `hyper-rustls` is now a **core dependency** (was optional under `tls`).
  HTTPS **backend** connections are always available; the `tls` feature now
  controls only frontend TLS termination (rustls / tokio-rustls / rustls-pemfile).
- `cargo-deny` configuration moved from `deny.toml` to `.cargo/deny.toml`;
  CI now invokes `cargo deny check --config .cargo/deny.toml`.

### Fixed

- Hot-reload now applies on every HTTP request, including subsequent requests on
  keep-alive connections (previously only the first request on a connection saw
  config updates).
- Hot-reload on TLS listeners started via `start_with_addr` / `start_tls` now
  picks up routing changes without a restart.
- `cargo build --no-default-features` no longer fails: `hyper-util` enables the
  `client-legacy` feature, and `hyper-rustls` is no longer optional in the core
  proxy module.
- Missing `#[cfg(feature = "tls")]` guards in `proxy.rs` for the redirect-port
  helper and `HashSet` import.
- `rustdoc` warning: `Full<Bytes>` in a doc comment was parsed as an unclosed
  HTML tag; the type names are now wrapped in backticks.

## [0.4.0] - 2026-05-25

### Added

- **TLS termination** — HTTPS on the frontend with SNI-based multi-domain support
  - `tls` directive in config: `tls /path/to/cert.pem /path/to/key.pem`
  - SNI-based certificate resolution via `rustls` + `tokio-rustls`
  - HTTP to HTTPS redirect: `redirect_port = tls_port - 443 + 80`
  - `X-Forwarded-Proto: https` header set for TLS connections
  - Auto-detect mode (`start_all()`) starts HTTPS listeners + redirect servers
- **Structured access log** — every request logged with method, path, host, status, duration, bytes sent
  - Auto-generated `X-Request-ID` (UUID v4) for each request
  - Feature flag `logging` (enabled by default)
- **Docker support**
  - Multi-stage `Dockerfile`: Rust Alpine build to Alpine runtime (~22 MB)
  - `docker-compose.yml` with TLS + echo backends
  - GitHub Actions workflow for publishing to GHCR (multi-arch amd64/arm64)
  - Docker build step in CI to catch Dockerfile regressions
- **Security audit** in CI: `cargo deny check` + `cargo audit --deny warnings`
- **Benchmarks** in `BENCHMARKS.md` comparing tiny-proxy vs nginx vs Caddy
- `--addr` CLI argument is now optional — auto-detects listeners from config
- `start_all()` method — auto-detect HTTP/TLS listeners per site address
- `find_site()` with port normalization — browsers omit port 443/80 in Host header

### Changed

- `--addr` / `-a` changed from `String` to `Option<String>`
- `cargo deny` and `cargo audit` added to CI pipeline
- Release workflow updated for GHCR instead of Docker Hub

### Fixed

- Duplicate site addresses now rejected at parse time instead of silent HashMap overwrite
- Mixed TLS and non-TLS sites on the same address rejected at parse time

## [0.3.0] - 2026-04-27

### Added

- `header -Name` directive — remove request headers before forwarding to backend (#13)
- `strip_prefix` directive — remove a prefix from the request URI path (#14)
- `redirect` directive — return 301/302/307/308 redirect responses with `Location` header (#16)
- Configurable timeouts for `reverse_proxy` — block syntax with `connect_timeout` and `read_timeout` (#17)
- `parse_duration` helper — supports `30s`, `5m`, `2h`, `1d` and plain numbers
- Block syntax for `reverse_proxy` — allows timeout configuration inside `reverse_proxy URL { ... }`
- 15 new unit tests: header removal (2), strip_prefix (4), redirect (2), parse_duration (5), reverse_proxy parsing (4)
- Integration test verifying all Phase 1 directives end-to-end

### Changed

- `Directive::Header` value field changed from `String` to `Option<String>` (breaking API change)
- `Directive::ReverseProxy` now includes `connect_timeout: Option<u64>` and `read_timeout: Option<u64>` fields
- `ActionResult::ReverseProxy` now includes timeout fields
- `handle_reverse_proxy()` signature updated to accept timeout parameters
- Replaced `.unwrap()` with `?` in `proxy()` handler for `Respond` and `ReverseProxy` response building
- Hardcoded 30-second backend timeout replaced with configurable `read_timeout` (default: 30s)
- Benchmarks updated for new `Directive` variant fields

### Configuration Examples

```caddy
# Header removal
localhost:8080 {
    header -Authorization
    reverse_proxy http://backend:3000
}

# Strip prefix
localhost:8080 {
    strip_prefix /api
    reverse_proxy http://backend:3000
}

# Redirect
localhost:8080 {
    redirect 301 https://new-domain.com
}

# Configurable timeouts (for LLM/SSE backends)
localhost:8080 {
    reverse_proxy http://llm-backend:8000 {
        connect_timeout 10s
        read_timeout 600s
    }
}
```

## [0.2.0] - 2026-04-14

### Added

- Hot-reload configuration: `Proxy` now uses `Arc<RwLock<Config>>` internally, enabling live config updates without restart
- `Proxy::from_shared(config)` — create a proxy from an existing shared config handle
- `Proxy::shared_config()` — get a clone of the `Arc<RwLock<Config>>` for external use (e.g. API server)
- `Proxy::config_snapshot()` — async method to read current config as an owned value
- `Deserialize` support for `Config`, `SiteConfig`, and `Directive` models (via `api` feature)
- Proper JSON error responses for `POST /config` with `"status": "error"` and descriptive messages

### Changed

- `POST /config` now **actually updates** the configuration (was only logging before)
- `Proxy::update_config()` is now `async fn update_config(&self, config)` (was `fn update_config(&mut self, config)`)
- `Proxy::config()` replaced with `async fn config_snapshot(&self)` (returns owned `Config` instead of reference)
- API and proxy server share the same `Arc<RwLock<Config>>` instance — updates are immediately visible to new connections
- Header directives now properly process placeholders (`{uuid}`, `{header.Name}`, `{env.VAR}`)

### Fixed

- `POST /config` endpoint was returning success without applying changes (#11)
- Header placeholders (`{uuid}`, `{header.Name}`, `{env.VAR}`) were not being processed in `header` directive (#9)
- Request forwarding issues in proxy handler (#10)

## [0.1.0] - 2026-04-01

### Added

- Core reverse proxy functionality with streaming response support (SSE-friendly)
- Embeddable library mode for use in Rust applications
- CLI mode for standalone execution
- Caddy-like configuration syntax parsing
- Path-based routing with wildcard support (`/api/*`)
- Header manipulation directives (`header`)
- URI rewriting (`uri_replace`)
- HTTP and HTTPS backend support via `hyper-tls`
- Method-based routing (`method GET POST { ... }`)
- Direct responses (`respond 200 "OK"`)
- Authentication module:
  - Header value substitution (`{header.Name}`, `{env.VAR}`, `{uuid}`)
  - Remote IP extraction from `X-Forwarded-For` / `X-Real-IP`
  - External token validation helper
- Management REST API:
  - `GET /config` - view current configuration
  - `POST /config` - update configuration
  - `GET /health` - health check endpoint
- Connection pooling with configurable limits
- Automatic concurrency limiting (default: CPU cores × 256)
- Graceful shutdown via SIGTERM/SIGINT
- Feature flags for modular builds:
  - `cli` - CLI dependencies and binary
  - `tls` - HTTPS backend support
  - `api` - Management API
- GitHub Actions CI/CD:
  - Automated testing and linting
  - Multi-platform release builds (Linux, macOS, Windows)
  - Automatic crates.io publishing
  - GitHub Release creation
- Example programs for library usage

[0.4.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.4.0
[0.3.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.3.0
[0.2.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.2.0
[0.1.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.1.0
