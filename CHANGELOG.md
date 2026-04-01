# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/denislituev/tiny-proxy/releases/tag/v0.1.0