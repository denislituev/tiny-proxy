# Tiny Proxy Server

Simple HTTP proxy server with Caddy-like configuration format written in Rust.

## Features

- Simple Caddy-like configuration format
- Path-based routing with `handle_path`
- Direct responses with `respond`
- Multiple backend support
- Wildcard path matching
- Header modification with `header` directive
- URI rewriting with `uri_replace` directive
- Fast and lightweight (Rust + Hyper)
- CLI configuration with custom config file and address
- REST API for dynamic configuration management (coming soon)

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
Tiny Proxy Server v0.1.0
Loading config from: ./file.caddy
Tiny Proxy listening on http://127.0.0.1:8080
Loaded 2 site(s)

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

### Implemented
- Basic proxy functionality (Task 1.1)
- Directive matching (Task 1.2)
- Wildcard pattern matching (Task 2.1)
- respond directive (Task 2.2)
- header directive (Task 2.3)
- uri_replace directive (Task 2.4)
- Full directive integration (Task 2.5)
- Configuration file parsing
- CLI argument support

### In Progress
- Unit tests (Task 3.1)
- Mock backend for tests (Task 3.2)
- Integration tests (Task 3.3)

### Planned Features
- **Configuration Management API** (Phase 5)
  - REST API for dynamic configuration updates without server restart
  - Hot reload configuration changes
  - Configuration versioning and rollback
  - API authentication and security
  - Configuration validation
  - Sites management endpoints

See `ex/tasks.md` for detailed task breakdown and `ex/roadmap.md` for development roadmap.

## Configuration Management API (Coming Soon)

The proxy server will include a REST API for dynamic configuration management, allowing you to update the configuration without restarting the server. This feature is inspired by Caddy's admin API and will provide:

### Planned API Endpoints

**Configuration Management:**
- `GET /config` - Get current configuration (Caddyfile format)
- `GET /config/json` - Get current configuration (JSON format)
- `POST /config` - Update existing configuration
- `PUT /config` - Replace entire configuration
- `DELETE /config` - Reset to default configuration
- `GET /config/validate` - Validate configuration without applying

**Sites Management:**
- `GET /config/sites` - List all sites
- `GET /config/sites/{host}` - Get specific site configuration
- `POST /config/sites` - Add new site
- `PUT /config/sites/{host}` - Update site configuration
- `DELETE /config/sites/{host}` - Remove site

**Version Control:**
- `GET /config/history` - Get configuration history
- `POST /config/rollback/{version}` - Rollback to specific version

**Management:**
- `POST /config/reload` - Reload configuration
- `GET /config/version` - Get current configuration version
- `GET /config/status` - Get configuration status

### Key Features

- **Hot Reload**: Update configuration without server restart
- **Atomic Updates**: Configuration changes are applied atomically
- **Rollback**: Easy rollback to previous working configuration
- **Validation**: Configuration validation before applying changes
- **Authentication**: API key-based authentication for security
- **Graceful Transition**: Existing connections complete with old configuration

### Usage Example

```bash
# Get current configuration
curl http://localhost:8082/config

# Add new site
curl -X POST http://localhost:8082/config/sites \
  -H "Authorization: Bearer your-api-key" \
  -d 'localhost:9090 {
       reverse_proxy backend:3000
     }'

# Validate configuration
curl -X POST http://localhost:8082/config/validate \
  -H "Content-Type: text/plain" \
  -d @new-config.caddy

# Rollback to previous version
curl -X POST http://localhost:8082/config/rollback/1 \
  -H "Authorization: Bearer your-api-key"
```

This feature will make the proxy server production-ready for dynamic environments where configuration changes need to be applied without downtime.

## Simplified Authentication via Headers (Coming Soon)

The proxy server will support simplified authentication through headers instead of implementing full OIDC integration like Caddy's `authproxy` directive. This approach provides flexibility while keeping the implementation lightweight and manageable.

### Key Features

- **Header Substitution**: Pass headers from client to upstream
  - Syntax: `header Authorization "{header.Authorization}"`
  - Forward existing headers: `header X-Forwarded-For "{header.X-Forwarded-For}"`

- **Request ID Generation**: Automatic UUID generation for tracing
  - Syntax: `header X-Request-Id "{uuid}"`
  - Support for existing IDs from clients

- **Environment Variables**: Secure token storage
  - Syntax: `header Authorization "Bearer {env.API_TOKEN}"`
  - Default values: `header Authorization "Bearer {env.API_TOKEN:default-token}"`

- **Static Token Authentication**: Service-to-service communication
  - Direct token specification in config
  - Different tokens for different routes

- **Token Validation via External Service** (optional)
  - Directive: `validate_token <url>`
  - Delegates token validation to external service
  - Handles 401/403 responses appropriately

### Configuration Examples

```caddy
# Bearer Token Authorization
localhost:8099 {
    handle_path /api/* {
        # Forward client's Authorization header
        header Authorization "{header.Authorization}"
        # Add request ID
        header X-Request-Id "{uuid}"
        reverse_proxy dev-constructor.dev:443
    }
}

# Static Token for Service-to-Service
localhost:8099 {
    handle_path /api/platform/* {
        header Authorization "Bearer {env.PLATFORM_TOKEN}"
        header X-Service-Id "platform-api"
        reverse_proxy dev-constructor.dev:443
    }

    handle_path /api/webui/* {
        header Authorization "Bearer {env.WEBUI_TOKEN}"
        header X-Service-Id "web-ui"
        reverse_proxy dev-constructor.dev:443
    }
}

# Token Validation via External Service
localhost:8099 {
    handle_path /api/* {
        # Validate token first
        validate_token http://token-validator:8080/validate
        
        # Add user context
        header Authorization "{header.Authorization}"
        header X-User-Id "{env.DEFAULT_USER_ID}"
        
        reverse_proxy dev-constructor.dev:443
    }
}
```

### Comparison with Caddy's authproxy

| Feature | Caddy authproxy | Tiny-proxy (Simplified) |
|---------|-----------------|------------------------|
| OIDC Integration | Full | No (simplified) |
| Login Redirect | Automatic | No |
| JWT Validation | Built-in | External service |
| Token in Headers | Automatic | Via header directive |
| Service-to-Service | Secrets | Static tokens |
| Request ID | Automatic | Generation |
| Session Management | Full | No |
| Implementation Complexity | High | Low |

### Why Simplified Authentication?

The simplified approach provides:

1. **Simpler Implementation**: No need for full OIDC/OpenID Connect libraries
2. **Flexibility**: Works with any authentication system that uses headers
3. **Security**: Tokens stored securely via environment variables
4. **Performance**: No overhead from session management or JWT validation
5. **Integration**: Easy integration with existing auth providers

This makes the proxy suitable for:
- Service-to-service communication with static tokens
- Gateway proxies that forward authentication headers
- Development environments with simplified auth
- Organizations using external token validation services

For production OIDC integration, consider using Caddy's authproxy plugin or implementing a dedicated auth gateway before the proxy.


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
- `header <name> <value>` - Add/modify headers
- `uri_replace <find> <replace>` - Rewrite request URI
- `validate_token <url>` - Validate tokens via external service (coming soon)
- Header substitution: `{header.Name}`, `{uuid}`, `{env.VAR}` (coming soon)

## License

[Add your license here]

## Author

[Your name]

## Resources

- [Roadmap](ex/roadmap.md) - Development roadmap
- [Tasks](ex/tasks.md) - Detailed task breakdown
- [Code Guidelines](docs/CODE_GUIDELINES.md) - Style guide and best practices
- [Load Testing Guide](tests/LOAD_TESTING.md) - Performance testing with k6