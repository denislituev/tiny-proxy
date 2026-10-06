#!/usr/bin/env bash
# A/B benchmark: compare a git ref (e.g. v0.5.0) against the current working
# tree, built and measured back-to-back on the same day in the same
# environment. nginx / Caddy are not involved — only tiny-proxy is measured.
#
# Usage:
#   cd benchmarks
#   ./run_ab.sh v0.5.0          # compare v0.5.0 vs current tree
#
# Methodology (differs from run.sh on purpose):
#   - BOTH versions run side by side in separate containers (old on
#     8080/8443, new on 8090/8446) hitting the same backends, so there is no
#     container churn between measurements;
#   - ROUNDS interleaved measurement rounds; within a round each scenario is
#     measured on old, then immediately on new, so thermal drift / background
#     load hits both versions equally;
#   - median RPS across rounds is reported (not "best of N"), which is robust
#     against outlier rounds;
#   - same load profile as run.sh: 10 000 requests / 100 connections.
#
# Why: absolute RPS drifts between Docker Desktop / macOS updates, so comparing
# runs from different days is misleading. This script removes the environment
# variable from the comparison entirely.

set -uo pipefail

cd "$(dirname "$0")"

OLD_REF=${1:?usage: run_ab.sh <git-ref, e.g. v0.5.0>}
OLD_LABEL=${OLD_REF//\//_}
OLD_TAG="tiny-proxy-ab:${OLD_LABEL}"
NEW_TAG="tiny-proxy-ab:current"

REQUESTS=${REQUESTS:-10000}
CONNECTIONS=${CONNECTIONS:-100}
ROUNDS=${ROUNDS:-5}

log() { echo -e "\033[0;32m[A/B]\033[0m $*" >&2; }

# --- Build both images ---
log "Building $OLD_TAG from git ref $OLD_REF ..."
OLD_CTX="${TMPDIR:-/tmp}/tp-ab-${OLD_LABEL}"
rm -rf "$OLD_CTX" && mkdir -p "$OLD_CTX"
# NOTE: git archive from a subdirectory only exports that subtree — run from repo root.
git -C .. archive "$OLD_REF" | tar -x -C "$OLD_CTX" || { echo "error: git archive $OLD_REF failed" >&2; exit 1; }
docker build -q -t "$OLD_TAG" "$OLD_CTX" >/dev/null || { echo "error: docker build $OLD_TAG failed" >&2; exit 1; }

log "Building $NEW_TAG from working tree ..."
docker build -q -t "$NEW_TAG" .. >/dev/null || { echo "error: docker build $NEW_TAG failed" >&2; exit 1; }

# --- Start everything (both versions + backends) ---
log "Starting services (old on 8080/8443, new on 8090/8446) ..."
TINY_PROXY_IMAGE_OLD=$OLD_TAG TINY_PROXY_IMAGE_NEW=$NEW_TAG \
    docker compose -f compose.yml -f ab-override.yml up -d >/dev/null

for port in 8080 8090; do
    ok=0
    for _ in $(seq 1 15); do
        if curl -sf "http://localhost:$port/text/" -o /dev/null 2>/dev/null; then ok=1; break; fi
        sleep 1
    done
    [ "$ok" = 1 ] || { echo "error: proxy on port $port did not become ready" >&2; \
        docker compose -f compose.yml -f ab-override.yml logs --tail 20; exit 1; }
done

cleanup() {
    # Interpolation vars are required even for `down` — pass dummies.
    TINY_PROXY_IMAGE_OLD=x TINY_PROXY_IMAGE_NEW=x \
        docker compose -f compose.yml -f ab-override.yml down >/dev/null 2>&1
}
trap cleanup EXIT

# --- Measurement ---
port_for() {
    case $1 in
        old-text) echo "http://localhost:8080/text/" ;;
        old-json) echo "http://localhost:8080/json/" ;;
        old-tls)  echo "https://localhost:8443/text/" ;;
        new-text) echo "http://localhost:8090/text/" ;;
        new-json) echo "http://localhost:8090/json/" ;;
        new-tls)  echo "https://localhost:8446/text/" ;;
    esac
}

# One hey run. Prints RPS, or nothing on repeated failure.
measure() {
    local key=$1 url=$2; shift 2
    local out rps attempt
    for attempt in 1 2 3; do
        out=$(hey -n "$REQUESTS" -c "$CONNECTIONS" "$@" "$url" 2>&1) || true
        if echo "$out" | grep -q '\[200\]'; then
            rps=$(echo "$out" | grep "Requests/sec" | awk '{printf "%.0f", $2}')
            if [ "${rps:-0}" -gt 0 ] 2>/dev/null; then
                echo "$rps"
                return 0
            fi
        fi
        sleep 2
    done
}

median() { sort -n | awk '{a[NR]=$1} END {print (NR%2) ? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2}'; }

# --- Warmup (both ports, once) ---
for port in 8080 8090; do
    hey -n 200 -c 10 "http://localhost:$port/text/" >/dev/null 2>&1 || true
done
sleep 1

# --- Interleaved rounds ---
# Bash 3.2 (macOS default) has no associative arrays — plain vars per combo.
SAMPLES_old_text=""; SAMPLES_old_json=""; SAMPLES_old_tls=""
SAMPLES_new_text=""; SAMPLES_new_json=""; SAMPLES_new_tls=""

log "Running $ROUNDS interleaved rounds (old and new measured back-to-back) ..."
for round in $(seq 1 $ROUNDS); do
    for scenario in text json tls; do
        for version in old new; do
            key="${version}-${scenario}"
            rps=$(measure "$key" "$(port_for "$key")" $([ "$scenario" = tls ] && echo -disable-keepalive -host localhost))
            if [ -z "$rps" ]; then
                log "round $round $version/$scenario: SKIPPED (measurement failed)"
                continue
            fi
            eval "SAMPLES_${version}_${scenario}=\"\${SAMPLES_${version}_${scenario}} \$rps\""
            log "round $round $version/$scenario: $rps RPS"
        done
    done
    sleep 1
done

# --- Report (stdout): label|scenario|median_rps|rounds ---
for version in old new; do
    case $version in
        old) label=$OLD_REF ;;
        new) label=current ;;
    esac
    for scenario in text json tls; do
        eval "samples=\"\${SAMPLES_${version}_${scenario}}\""
        med=$(echo "$samples" | tr ' ' '\n' | grep -v '^$' | median)
        [ -n "$med" ] || med=0
        echo "${label}|${scenario}|$(printf '%.0f' "$med")|$(echo "$samples" | tr ' ' '\n' | grep -v '^$' | tr '\n' ' ')"
    done
done

log "A/B done."
