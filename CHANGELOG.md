# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.3.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.3.0
[0.2.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.2.0
[0.1.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.1.0