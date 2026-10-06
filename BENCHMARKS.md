# Benchmarks

Comparative benchmarks of tiny-proxy, nginx, and Caddy as reverse proxies.

All three proxies were configured with equivalent routing rules, forwarding to the same backend (hashicorp/http-echo). The only variable is the proxy implementation.

- **Setup:** [`benchmarks/compose.yml`](benchmarks/compose.yml) · proxy configs: [`benchmarks/proxies/`](benchmarks/proxies/)
- **Runner:** [`benchmarks/run.sh`](benchmarks/run.sh) (3-way comparison) · [`benchmarks/run_ab.sh`](benchmarks/run_ab.sh) (version-vs-version)
- **Raw results:** [`benchmarks/results/`](benchmarks/results/)

## Methodology

- **Tool:** [hey](https://github.com/rakyll/hey) — 10 000 requests, 100 concurrent connections, best of 3 runs (per-scenario warmup of 200 requests)
- **Backend:** hashicorp/http-echo in its own container (minimal overhead)
- **TLS scenario:** `-disable-keepalive` — every request performs a full TCP + TLS handshake; `-host localhost` sets bare-hostname SNI (hey otherwise sends `host:port`, which rustls rejects per RFC 6066)
- All proxies run with out-of-the-box defaults — no worker tuning, buffer sizing, or OS-level tweaks. Both nginx and Caddy have extensive tuning options (`worker_processes`, `worker_connections`, `proxy_buffer_size`, `keepalive_timeout`) that can meaningfully improve their numbers. These benchmarks reflect a fair "zero-config" comparison, not maximum achievable performance for any proxy.
- tiny-proxy runs on the Tokio multi-thread runtime (one worker per CPU core); nginx uses `worker_processes auto`.

> **Note:** These are local benchmarks through Docker Desktop networking on a
> single machine. Absolute numbers will differ on dedicated hardware — the
> relative comparison is what's useful. Run-to-run variance on this setup is
> roughly ±10–15%; differences below that should be considered noise. For
> cleanest results, close background applications — other running containers
> (databases, message brokers, …) on the same Docker daemon measurably add
> noise.

## Environment (latest run, 2026-10-06)

- **Host:** Apple M1 Max (10 cores), 32 GB RAM, macOS 27.0
- **Docker:** Docker Desktop, engine 27.4.0
- **Images:**
  - tiny-proxy 0.6.0 — built from this repository (`Dockerfile`, `--all-features`, musl static binary)
  - nginx:alpine `sha256:8b1e78743a03…`
  - caddy:alpine `sha256:86deaf5e3d34…`
  - hashicorp/http-echo `sha256:fcb75f691c8b…`
- **tiny-proxy version in container:** verify anytime with `docker compose exec tiny-proxy tiny-proxy --version`

## Results — tiny-proxy 0.6.0 vs nginx vs Caddy (2026-10-06)

### 1. Plain Text (~11 bytes response)

| Proxy | RPS | Avg | p50 | p90 | p95 | p99 |
|-------|-----|-----|-----|-----|-----|-----|
| tiny-proxy | 17 047 | 5.8ms | 5.4ms | 8.2ms | 9.5ms | 15.3ms |
| nginx | 20 390 | 4.8ms | 4.2ms | 7.3ms | 9.2ms | 16.6ms |
| caddy | 19 406 | 5.0ms | 4.1ms | 8.3ms | 9.6ms | 17.4ms |

### 2. JSON API (~200 bytes response)

| Proxy | RPS | Avg | p50 | p90 | p95 | p99 |
|-------|-----|-----|-----|-----|-----|-----|
| tiny-proxy | 17 558 | 5.6ms | 5.2ms | 7.8ms | 8.9ms | 15.4ms |
| nginx | 20 360 | 4.8ms | 4.4ms | 6.9ms | 8.0ms | 17.4ms |
| caddy | 19 460 | 5.0ms | 4.2ms | 8.3ms | 10.1ms | 22.2ms |

### 3. TLS Termination

Every request establishes a new TCP connection and TLS handshake (`-disable-keepalive`), making this the most demanding scenario.

| Proxy | RPS | Avg | p50 | p90 | p95 | p99 |
|-------|-----|-----|-----|-----|-----|-----|
| tiny-proxy | 2 613 | 37.7ms | 37.0ms | 50.7ms | 55.8ms | **66.3ms** |
| nginx | 2 515 | 37.7ms | 31.0ms | 68.8ms | 83.0ms | 128.4ms |
| caddy | 2 091 | 45.8ms | 36.0ms | 89.1ms | 109.9ms | 149.7ms |

**Takeaways:**

- nginx leads plain HTTP throughput by ~15%; tiny-proxy and Caddy are close behind.
- tiny-proxy leads TLS termination in throughput and, more importantly, in tail latency: p99 of 66ms vs nginx 128ms and Caddy 150ms — rustls (aws-lc-rs backend) handles handshakes efficiently.
- tiny-proxy shows the tightest p99 across all scenarios.

Full raw output of this run: [`benchmarks/results/summary_20261006_112802.md`](benchmarks/results/summary_20261006_112802.md). Historical runs live in the same directory.

## Version A/B — did a release regress performance?

Absolute RPS drifts between Docker Desktop / macOS updates, so comparing runs from different months is misleading. Use `run_ab.sh` to compare two tiny-proxy versions **side by side, in the same environment, at the same moment**:

```bash
cd benchmarks
./run_ab.sh v0.5.0        # v0.5.0 (from git tag) vs current working tree
ROUNDS=9 ./run_ab.sh v0.5.0
```

How it works:

- builds one image from a `git archive` of the requested ref and one from the working tree;
- runs BOTH containers simultaneously (old on `8080/8443`, new on `8090/8446`) against the same backends — no container churn between measurements;
- performs interleaved rounds measuring each scenario on old, then immediately on new, so thermal drift and background load affect both equally;
- reports the **median** RPS across rounds (robust against outlier rounds).

### v0.5.0 → v0.6.0 (forward_auth release), 2026-10-06

| Scenario | v0.5.0 (median RPS) | v0.6.0 (median RPS) | Δ |
|----------|--------------------:|--------------------:|---|
| Plain Text | 16 384 | 14 300 | −13% |
| JSON API | 14 855 | 15 370 | +3% |
| TLS | 2 235 | 2 197 | −2% |

The text/json numbers flip sign between repeated A/B runs while using the identical code path (`handle_path` + `reverse_proxy`), which places the difference inside the ±10–15% environment noise of this setup. **Conclusion: v0.6.0 performs on par with v0.5.0** — the forward_auth middleware adds no measurable overhead when not configured. TLS is at parity as well.

## Reproduce

```bash
cd benchmarks
docker compose up -d          # or let run.sh do it
./run.sh                      # 3-way comparison (add --skip-tls to skip scenario 3)
./run_ab.sh v0.5.0            # version A/B against the current tree
```

Requirements: Docker, `hey` (`brew install hey`). Certificates are generated automatically on first run.

## Configuration Details

All proxies used the same routing: two paths (`/text/`, `/json/`) forwarding to separate backend instances.

**tiny-proxy** ([benchmarks/proxies/tiny-proxy.conf](benchmarks/proxies/tiny-proxy.conf)):

```
localhost:8080 {
    handle_path /text/* { reverse_proxy backend:9000 }
    handle_path /json/* { reverse_proxy backend-json:9000 }
}

localhost:8443 {
    tls /etc/ssl/tiny-proxy/cert.pem /etc/ssl/tiny-proxy/key.pem
    handle_path /text/* { reverse_proxy backend:9000 }
    handle_path /json/* { reverse_proxy backend-json:9000 }
}
```

**nginx** ([benchmarks/proxies/nginx.conf](benchmarks/proxies/nginx.conf)):

```nginx
worker_processes auto;

events { worker_connections 1024; }

http {
    server {
        listen 80;
        location /text/ { proxy_pass http://backend:9000/; proxy_set_header Host $host; proxy_http_version 1.1; }
        location /json/ { proxy_pass http://backend-json:9000/; proxy_set_header Host $host; proxy_http_version 1.1; }
    }
    server {
        listen 443 ssl;
        ssl_certificate /etc/ssl/bench/cert.pem;
        ssl_certificate_key /etc/ssl/bench/key.pem;
        # same locations
    }
}
```

**Caddy** ([benchmarks/proxies/Caddyfile](benchmarks/proxies/Caddyfile)):

```
:8082 {
    reverse_proxy /text/* backend:9000
    reverse_proxy /json/* backend-json:9000
}

:8445 {
    tls /etc/ssl/bench/cert.pem /etc/ssl/bench/key.pem
    reverse_proxy /text/* backend:9000
}
```

## Image Sizes

Measured locally on 2026-10-06 (`docker image ls`):

| Proxy | Docker image size |
|-------|-------------------|
| tiny-proxy 0.6.0 | 23 MB (Alpine + 5.6 MB static musl binary, `--all-features`) |
| nginx | 92 MB (nginx:alpine) |
| Caddy | 85 MB (caddy:alpine) |
