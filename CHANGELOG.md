# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.2.0
[0.1.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.1.0