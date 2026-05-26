# Multi-stage build for tiny-proxy
#
# Build stage  — compiles a static binary with musl on Alpine
# Runtime     — minimal Alpine image with CA certificates (~7 MB)

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
FROM rust:1.85-alpine AS builder

RUN apk add --no-cache musl-dev cmake make gcc nasm perl

WORKDIR /build

# Cargo.toml references benches/ and examples/ — provide stubs so --locked resolves
COPY Cargo.toml Cargo.lock ./
COPY benches/ benches/
COPY examples/ examples/
RUN mkdir src && echo "" > src/lib.rs && echo "fn main() {}" > src/main.rs

RUN cargo build --release --all-features --locked 2>/dev/null || true
RUN rm -rf src

COPY src/ src/
RUN touch src/main.rs src/lib.rs && cargo build --release --all-features --locked

RUN strip /build/target/release/tiny-proxy

# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------
FROM alpine:3.21

RUN apk add --no-cache ca-certificates

COPY --from=builder /build/target/release/tiny-proxy /usr/local/bin/tiny-proxy

# Config and certs mount points
VOLUME /etc/tiny-proxy
VOLUME /etc/ssl/tiny-proxy

ENTRYPOINT ["tiny-proxy"]
CMD ["-c", "/etc/tiny-proxy/config.conf"]
