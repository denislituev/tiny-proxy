# Benchmarks

Comparative benchmarks of tiny-proxy, nginx, and Caddy as reverse proxies.

All three proxies were configured with equivalent routing rules, forwarding to the same backend (hashicorp/http-echo). The only variable is the proxy implementation.

## Environment

- **Host:** Docker Desktop on macOS (Apple Silicon, M-series)
- **Tool:** [hey](https://github.com/rakyll/hey)
- **Proxies:** tiny-proxy 0.3.0 (Rust, rustls), nginx (alpine), Caddy (alpine)
- **Backend:** hashicorp/http-echo (minimal response overhead)
- **Method:** 10 000 requests, 100 concurrent connections, best of 3 runs
- **Warmup:** 200 requests before each measurement
- **Containers:** each proxy and the backend ran in separate Docker containers on the same host

> **Note:** These are local benchmarks run through Docker networking on a single machine. Absolute numbers will differ on dedicated hardware. The relative comparison between proxies is what's useful here.

## Results

### 1. Plain Text (~11 bytes response)

| Proxy | RPS | Avg | p50 | p90 | p95 | p99 |
|-------|-----|-----|-----|-----|-----|-----|
| tiny-proxy | 16 275 | 5.8ms | 5.5ms | 8.6ms | 10.1ms | 22.2ms |
| nginx | 16 418 | 5.9ms | 5.1ms | 9.5ms | 11.2ms | 19.5ms |
| Caddy | 17 542 | 5.5ms | 4.8ms | 9.1ms | 10.5ms | 22.4ms |

All three proxies perform within ~7% of each other. The differences are within run-to-run variance.

### 2. JSON API (~200 bytes response)

| Proxy | RPS | Avg | p50 | p90 | p95 | p99 |
|-------|-----|-----|-----|-----|-----|-----|
| tiny-proxy | 17 184 | 5.7ms | 5.3ms | 7.9ms | 8.9ms | 16.2ms |
| nginx | 20 273 | 4.8ms | 4.2ms | 7.0ms | 8.3ms | 19.8ms |
| Caddy | 17 596 | 5.5ms | 4.6ms | 9.3ms | 10.9ms | 24.7ms |

nginx shows higher throughput on this scenario (~18% more RPS than tiny-proxy). Latency distributions are comparable at p50, with tiny-proxy showing tighter p99.

### 3. TLS Termination

Each request establishes a new TCP connection and TLS handshake (`-disable-keepalive`), making this the most demanding scenario.

| Proxy | RPS | Avg | p50 | p90 | p95 | p99 |
|-------|-----|-----|-----|-----|-----|-----|
| tiny-proxy | 2 672 | 36.9ms | 36.0ms | 49.1ms | 53.6ms | 61.5ms |
| nginx | 2 437 | 39.3ms | 33.6ms | 70.1ms | 88.0ms | 117.4ms |
| Caddy | 2 129 | 44.2ms | 33.5ms | 90.2ms | 115.5ms | 157.8ms |

tiny-proxy shows ~10% higher throughput than nginx and ~25% higher than Caddy. The p99 latency difference is more pronounced: tiny-proxy at 61ms vs nginx at 117ms and Caddy at 158ms. This is likely due to rustls (with aws-lc-rs backend) handling TLS handshakes efficiently.

## Configuration Details

All proxies used the same routing: two paths (`/text/`, `/json/`) forwarding to separate backend instances.

**tiny-proxy:**
```
localhost:8080 {
    handle_path /text/* { reverse_proxy backend:9000 }
    handle_path /json/* { reverse_proxy backend-json:9000 }
}
```

**nginx:**
```nginx
location /text/ { proxy_pass http://backend:9000/; }
location /json/ { proxy_pass http://backend-json:9000/; }
```

**Caddy:**
```
:8082 {
    reverse_proxy /text/* backend:9000
    reverse_proxy /json/* backend-json:9000
}
```

All proxies ran with out-of-the-box defaults — no worker tuning, buffer sizing, keepalive optimization, or OS-level tweaks. Both nginx and Caddy have extensive tuning options (e.g., `worker_processes`, `worker_connections`, `proxy_buffer_size`, `keepalive_timeout`) that can meaningfully improve their numbers. These benchmarks reflect a fair "zero-config" comparison, not maximum achievable performance for any of the proxies.

## Reproduce

```bash
cd benchmarks
docker compose up -d
./run.sh
```

See `benchmarks/` for compose file, proxy configs, and the runner script.

## Image Sizes

| Proxy | Docker image size |
|-------|-------------------|
| tiny-proxy | 22 MB (Alpine + 4.6 MB binary) |
| nginx | 43 MB (nginx:alpine) |
| Caddy | 40 MB (caddy:alpine) |
