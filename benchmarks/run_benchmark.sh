#!/bin/bash
# servo-fetch resource benchmark
# Methodology: Peak RSS of process tree (50ms sampling), median of 3 runs, 1 warmup discarded.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SERVO="$SCRIPT_DIR/../target/release/servo-fetch"
PW="$SCRIPT_DIR/playwright/fetch_parallel.js"
PP="$SCRIPT_DIR/puppeteer/fetch.js"

URL_1="https://example.com"
URLS_4=(
  "https://example.com"
  "https://httpbin.org/html"
  "https://www.iana.org/help/example-domains"
  "https://info.cern.ch/hypertext/WWW/TheProject.html"
)

get_tree_rss() {
  local pid="$1"
  local total=0
  local self_rss
  self_rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo 0)
  total=$((total + ${self_rss:-0}))
  local child
  for child in $(pgrep -P "$pid" 2>/dev/null); do
    local child_rss
    child_rss=$(get_tree_rss "$child")
    total=$((total + child_rss))
  done
  echo "$total"
}

run_once() {
  local max_rss=0
  local start
  start=$(python3 -c "import time; print(time.time())")

  "$@" > /dev/null 2>&1 &
  local pid=$!

  local rss
  while kill -0 "$pid" 2>/dev/null; do
    rss=$(get_tree_rss "$pid")
    [ "$rss" -gt "$max_rss" ] && max_rss=$rss
    sleep 0.05
  done
  wait "$pid" 2>/dev/null || true

  local end
  end=$(python3 -c "import time; print(time.time())")
  local elapsed
  elapsed=$(python3 -c "print(f'{${end} - ${start}:.2f}')")

  echo "$max_rss $elapsed"
}

measure() {
  local label="$1"; shift

  # Warmup
  run_once "$@" > /dev/null

  # 3 measured runs
  local rss_vals=()
  local time_vals=()
  local _run result rss_val time_val
  for _run in 1 2 3; do
    result=$(run_once "$@")
    rss_val=$(echo "$result" | awk '{print $1}')
    time_val=$(echo "$result" | awk '{print $2}')
    rss_vals+=("$rss_val")
    time_vals+=("$time_val")
  done

  # Sort and take median (index 2 of 3)
  local med_rss
  med_rss=$(printf '%s\n' "${rss_vals[@]}" | sort -n | sed -n '2p')
  local med_time
  med_time=$(printf '%s\n' "${time_vals[@]}" | sort -n | sed -n '2p')
  local rss_mb=$((med_rss / 1024))

  echo "| $label | ${rss_mb} MB | ${med_time}s |"
}

echo "## servo-fetch Resource Benchmark"
echo ""
echo "- **Date**: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "- **Machine**: $(uname -m), $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -p)"
echo "- **OS**: $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)"
echo "- **Method**: Peak RSS of process tree (50ms sampling), median of 3 runs, 1 warmup discarded"
echo ""
echo "### Versions"
echo ""
echo "- servo-fetch: $($SERVO --version 2>/dev/null || echo 'unknown')"
echo "- Node.js: $(node --version)"
echo "- Playwright: $(node -e "console.log(require('${SCRIPT_DIR}/playwright/node_modules/playwright/package.json').version)" 2>/dev/null || echo 'unknown')"
echo "- Puppeteer: $(node -e "console.log(require('${SCRIPT_DIR}/puppeteer/node_modules/puppeteer/package.json').version)" 2>/dev/null || echo 'unknown')"
echo ""
echo "### Single page (\`$URL_1\`)"
echo ""
echo "| Tool | Peak RSS | Time |"
echo "|------|----------|------|"
measure "curl (no JS)" curl -s "$URL_1"
measure "servo-fetch" "$SERVO" "$URL_1"
measure "Playwright" node "$PW" "$URL_1"
measure "Puppeteer" node "$PP" "$URL_1"
echo ""
echo "### 4 pages parallel"
echo ""
echo "| Tool | Peak RSS | Time |"
echo "|------|----------|------|"
measure "servo-fetch" "$SERVO" "${URLS_4[@]}"
measure "Playwright" node "$PW" "${URLS_4[@]}"
measure "Puppeteer" node "$PP" "${URLS_4[@]}"
