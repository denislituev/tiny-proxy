# Tiny Proxy Server

Simple HTTP proxy server with Caddy-like configuration format written in Rust.

## Features

- ✅ Simple Caddy-like configuration format
- ✅ Path-based routing with `handle_path`
- ✅ Direct responses with `respond`
- ✅ Multiple backend support
- ✅ Wildcard path matching
- ⚡ Fast and lightweight (Rust + Hyper)
- 🔧 CLI configuration with custom config file and address

## Installation

```bash
# Clone repository
git clone <repo-url>
cd tiny-proxy

# Build release binary
cargo build --release

# Binary will be at ./target/release/proxy
```

## Usage

### Basic Usage

```bash
# Run with default config (./file.caddy) and address (127.0.0.1:8080)
cargo run

# Or use the built binary
./target/release/proxy
```

### CLI Arguments

```bash
# Custom config file
./target/release/proxy --config ./examples/simple.caddy

# Custom address
./target/release/proxy --addr 0.0.0.0:8080

# Short form
./target/release/proxy -c ./examples/simple.caddy -a 0.0.0.0:8080

# Get help
./target/release/proxy --help

# Show version
./target/release/proxy --version
```

### Example Output

```
🚀 Tiny Proxy Server v0.1.0
📄 Loading config from: ./file.caddy
🚀 Tiny Proxy listening on http://127.0.0.1:8080
✅ Loaded 2 site(s)

Request: /api/v1/users from localhost:8080
Proxying to: http://localhost:9001/
```

## Configuration

### Configuration Format (Caddy-like)

```
<host>:<port> {
    <directive>
    <directive> {
        <nested_directive>
    }
}
```

### Supported Directives

#### reverse_proxy
Forward requests to backend server.

```caddy
localhost:8080 {
    reverse_proxy localhost:9001
}
```

#### handle_path
Route requests based on path pattern with wildcard support. The path prefix is removed when forwarding to backend.

```caddy
localhost:8080 {
    handle_path /api/v1/users/* {
        reverse_proxy localhost:9001
    }

    handle_path /api/v1/orders/* {
        reverse_proxy localhost:9002
    }
}
```

#### respond
Return direct HTTP response without proxying to backend.

```caddy
localhost:8080 {
    handle_path /health {
        respond 200 "Service is healthy"
    }
}
```

### Example Configuration

```caddy
# Main API gateway
localhost:8080 {
    # User service - removes /api/v1/users prefix
    handle_path /api/v1/users/* {
        reverse_proxy localhost:9001
    }

    # Order service - removes /api/v1/orders prefix
    handle_path /api/v1/orders/* {
        reverse_proxy localhost:9002
    }

    # Health check - direct response
    handle_path /health {
        respond 200 "OK"
    }
}

# Legacy API with path rewriting
localhost:8081 {
    uri_replace /old-api /v1
    header X-Gateway-Version "legacy-proxy"
    reverse_proxy localhost:9001
}
```

## Examples

See `examples/` directory for more configuration examples:

- `simple.caddy` - Basic proxy configuration
- `multi-site.caddy` - Multiple sites with different backends

## How It Works

1. Client sends request to proxy (e.g., `http://localhost:8080/api/v1/users/123`)
2. Proxy matches path against `handle_path` patterns
3. Removes path prefix (e.g., `/api/v1/users/123` → `/123`)
4. Forwards to backend (e.g., `http://localhost:9001/123`)
5. Returns response to client

## Performance

Load testing with k6 shows excellent performance:

| Metric | Result | Threshold |
|---------|---------|------------|
| **p(95) latency** | 1.42ms | < 500ms |
| **Error rate** | 0.00% | < 5% |
| **Throughput** | 150+ RPS | - |
| **Concurrent users** | 100+ | - |

See `tests/LOAD_TESTING.md` for detailed performance testing instructions.

## Development

### Build

```bash
cargo build
```

### Run Tests

```bash
cargo test
```

### Load Testing

```bash
# Install k6
brew install k6

# Run load test
k6 run tests/load-test.js
```

### Code Guidelines

All code comments MUST be in English. See `docs/CODE_GUIDELINES.md` for complete style guide.

## Current Status

### ✅ Implemented
- Basic proxy functionality (Task 1.1)
- Directive matching (Task 1.2)
- Wildcard pattern matching (Task 2.1)
- respond directive (Task 2.2)
- Configuration file parsing
- CLI argument support

### ⏳ In Progress
- header directive (Task 2.3)
- uri_replace directive (Task 2.4)
- Full directive integration (Task 2.5)
- Unit tests (Task 3.1)
- Mock backend for tests (Task 3.2)
- Integration tests (Task 3.3)

See `ex/tasks.md` for detailed task breakdown and `ex/roadmap.md` for development roadmap.

## Testing

### Manual Testing

```bash
# Start proxy
cargo run

# Start mock backends (in separate terminals)
~/go/bin/http-echo -listen=:9001 -text="Hello from User Service"
~/go/bin/http-echo -listen=:9002 -text="Hello from Order Service"

# Test proxy
curl http://localhost:8080/api/v1/users
curl http://localhost:8080/api/v1/orders
curl http://localhost:8080/health
```

### Load Testing

```bash
# Ensure proxy and backends are running
k6 run tests/load-test.js
```

## Contributing

1. Follow code guidelines (English comments only)
2. Add tests for new features
3. Update documentation
4. Run `cargo fmt` and `cargo clippy` before submitting
5. Submit pull request with clear description

## Important Note

> Configuration format is inspired by Caddy, but only a subset of directives is supported. This is not a Caddy-compatible implementation.

**Supported Directives:**
- `reverse_proxy <url>` - Forward requests to backend
- `handle_path <pattern> { ... }` - Route based on path pattern
- `respond <status> <body>` - Return direct response
- `header <name> <value>` - Add/modify headers (in progress)
- `uri_replace <find> <replace>` - Rewrite request URI (in progress)

## License

[Add your license here]

## Author

[Your name]

## Resources

- [Roadmap](ex/roadmap.md) - Development roadmap
- [Tasks](ex/tasks.md) - Detailed task breakdown
- [Code Guidelines](docs/CODE_GUIDELINES.md) - Style guide and best practices
- [Load Testing Guide](tests/LOAD_TESTING.md) - Performance testing with k6