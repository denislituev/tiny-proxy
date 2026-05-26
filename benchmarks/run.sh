#!/usr/bin/env bash
# Benchmark: tiny-proxy vs nginx vs Caddy
#
# Usage:
#   cd benchmarks
#   ./run.sh            # run all tests
#   ./run.sh --skip-tls # skip TLS tests
#
# Prerequisites:
#   brew install hey

set -uo pipefail

cd "$(dirname "$0")"

# --- Config ---
REQUESTS=10000
CONNECTIONS=100
RUNS=3
RESULTS_DIR="results"

log()  { echo -e "\033[0;32m[BENCH]\033[0m $*" >&2; }

# --- Check hey ---
if ! command -v hey &>/dev/null; then
    echo "error: 'hey' not found. Install: brew install hey" >&2
    exit 1
fi

# --- Generate certs if needed ---
if [ ! -f certs/cert.pem ]; then
    log "Generating self-signed certificates..."
    mkdir -p certs
    openssl req -x509 -newkey rsa:2048 \
        -keyout certs/key.pem -out certs/cert.pem \
        -days 365 -nodes -subj "/CN=localhost" 2>/dev/null
fi

# --- Start services ---
log "Starting services..."
docker compose -f compose.yml up -d --build

log "Waiting for services to be ready..."
sleep 3

for port in 8080 8081 8082; do
    for attempt in $(seq 1 10); do
        if curl -sf "http://localhost:$port/text/" -o /dev/null 2>/dev/null; then
            log "HTTP port $port ✓"
            break
        fi
        if [ "$attempt" -eq 10 ]; then
            echo "error: HTTP port $port failed to respond" >&2
            docker compose -f compose.yml logs --tail 10
            exit 1
        fi
        sleep 1
    done
done

if [[ "${1:-}" != "--skip-tls" ]]; then
    for port in 8443 8444 8445; do
        for attempt in $(seq 1 10); do
            if curl -skf "https://localhost:$port/text/" -o /dev/null 2>/dev/null; then
                log "TLS port $port ✓"
                break
            fi
            if [ "$attempt" -eq 10 ]; then
                echo "error: TLS port $port failed to respond" >&2
                docker compose -f compose.yml logs --tail 10
                exit 1
            fi
            sleep 1
        done
    done
fi

# --- Prepare results ---
mkdir -p "$RESULTS_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
SUMMARY_FILE="$RESULTS_DIR/summary_${TIMESTAMP}.md"

# --- Parse hey output ---
parse_hey() {
    local output="$1"
    local rps avg p50 p90 p95 p99

    rps=$(echo "$output" | grep "Requests/sec" | awk '{printf "%.0f", $2}')
    avg=$(echo "$output" | grep "Average:" | awk '{print $2}')
    p50=$(echo "$output" | awk '/50%/ {print $3; exit}')
    p90=$(echo "$output" | awk '/90%/ {print $3; exit}')
    p95=$(echo "$output" | awk '/95%/ {print $3; exit}')
    p99=$(echo "$output" | awk '/99%/ {print $3; exit}')

    echo "${rps:-0}|${avg:-0}|${p50:-0}|${p90:-0}|${p95:-0}|${p99:-0}"
}

# --- Run a single benchmark ---
# Only the final result line goes to stdout (captured by caller).
# All progress messages go to stderr.
run_bench() {
    local label=$1
    local url=$2
    shift 2

    local best_rps=0
    local best_parsed=""

    for i in $(seq 1 $RUNS); do
        # Warmup on first run
        if [ "$i" -eq 1 ]; then
            hey -n 200 -c 10 "$@" "$url" >/dev/null 2>&1 || true
            sleep 1
        fi

        local output
        output=$(hey -n "$REQUESTS" -c "$CONNECTIONS" "$@" "$url" 2>&1) || true

        local parsed
        parsed=$(parse_hey "$output")
        local rps
        rps=$(echo "$parsed" | cut -d'|' -f1)

        if ! echo "$output" | grep -q '\[200\]'; then
            log "  $label run $i: FAILED (no HTTP 200 — check hey output below)" >&2
            echo "$output" >&2
            rps=0
        elif [ "${rps:-0}" -eq 0 ] 2>/dev/null; then
            log "  $label run $i: FAILED (0 RPS — check hey output below)" >&2
            echo "$output" >&2
        else
            log "  $label run $i: ${rps} RPS"
        fi

        if [ "${rps:-0}" -gt "${best_rps:-0}" ] 2>/dev/null; then
            best_rps=$rps
            best_parsed=$parsed
        fi

        sleep 1
    done

    if [ "${best_rps:-0}" -eq 0 ] 2>/dev/null; then
        echo "error: $label produced 0 RPS in all runs" >&2
        exit 1
    fi

    echo "${label}|${best_parsed}"
}

# --- Scenarios ---
log "Running benchmarks..."
echo "" >&2

declare -a ALL_RESULTS

log "Scenario 1: Plain text reverse proxy"
ALL_RESULTS+=("$(run_bench "tiny-proxy" "http://localhost:8080/text/")")
ALL_RESULTS+=("$(run_bench "nginx" "http://localhost:8081/text/")")
ALL_RESULTS+=("$(run_bench "caddy" "http://localhost:8082/text/")")
echo "" >&2

log "Scenario 2: JSON API proxy"
ALL_RESULTS+=("$(run_bench "tiny-proxy" "http://localhost:8080/json/")")
ALL_RESULTS+=("$(run_bench "nginx" "http://localhost:8081/json/")")
ALL_RESULTS+=("$(run_bench "caddy" "http://localhost:8082/json/")")
echo "" >&2

if [[ "${1:-}" != "--skip-tls" ]]; then
    # Non-standard TLS ports: hey puts "host:port" in SNI by default, which rustls
    # rejects (RFC 6066). -host sets SNI to the bare hostname.
    log "Scenario 3: TLS termination (-disable-keepalive -host localhost)"
    ALL_RESULTS+=("$(run_bench "tiny-proxy" "https://localhost:8443/text/" -disable-keepalive -host localhost)")
    ALL_RESULTS+=("$(run_bench "nginx" "https://localhost:8444/text/" -disable-keepalive -host localhost)")
    ALL_RESULTS+=("$(run_bench "caddy" "https://localhost:8445/text/" -disable-keepalive -host localhost)")
    echo "" >&2
fi

# --- Generate summary ---
log "Generating summary..."

print_table() {
    local title=$1
    local start=$2
    local count=$3

    echo "### $title"
    echo ""
    echo "| Proxy | RPS | Avg | p50 | p90 | p95 | p99 |"
    echo "|-------|-----|-----|-----|-----|-----|-----|"

    for r in "${ALL_RESULTS[@]:$start:$count}"; do
        IFS='|' read -r name rps avg p50 p90 p95 p99 <<< "$r"
        printf "| %s | %s | %s | %s | %s | %s | %s |\n" \
            "$name" "${rps:---}" "${avg:---}" "${p50:---}" "${p90:---}" "${p95:---}" "${p99:---}"
    done
    echo ""
}

{
    echo "# Benchmark Results"
    echo ""
    echo "**Date:** $(date)"
    echo "**Tool:** hey — $REQUESTS requests, $CONNECTIONS connections, best of $RUNS runs"
    echo "**Environment:** Docker Desktop on $(uname -sm)"
    echo ""

    print_table "1. Plain Text (~11 bytes response)" 0 3
    print_table "2. JSON API (~200 bytes response)" 3 3

    if [[ "${1:-}" != "--skip-tls" ]] && [ ${#ALL_RESULTS[@]} -gt 6 ]; then
        print_table "3. TLS Termination" 6 3
    fi

    echo "## Methodology"
    echo ""
    echo "- **Tool:** [hey](https://github.com/rakyll/hey)"
    echo "- **Warmup:** 200 requests before measurement"
    echo "- **Measurement:** $REQUESTS requests, $CONNECTIONS concurrent"
    echo "- **Best** of $RUNS runs reported"
    echo "- **Backend:** hashicorp/http-echo (minimal overhead)"
    if [[ "${1:-}" != "--skip-tls" ]]; then
        echo "- **TLS:** \`-disable-keepalive\` (new TCP+TLS handshake per request)"
        echo "- **SNI:** \`-host localhost\` (hey otherwise sends \`localhost:844x\` in SNI; rustls rejects that)"
    fi
    echo ""
    echo "## Reproduce"
    echo ""
    echo '```bash'
    echo "cd benchmarks"
    echo "docker compose up -d"
    echo "./run.sh"
    echo '```'
} > "$SUMMARY_FILE"

echo ""
cat "$SUMMARY_FILE"

log "Results saved to $SUMMARY_FILE"

log "Stopping services..."
docker compose -f compose.yml down

log "Done!"
