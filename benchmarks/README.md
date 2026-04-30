# Benchmarks

Resource usage comparison of tools that execute JavaScript and render web pages.

## Results

### Single page (`https://example.com`)

| Tool | Peak RSS | Time |
| ---- | -------- | ---- |
| curl (no JS) | 3 MB | 0.17s |
| **servo-fetch** | **64 MB** | **0.47s** |
| Playwright | 224 MB | 0.82s |
| Puppeteer | 590 MB | 1.60s |

### 4 pages parallel

| Tool | Peak RSS | Time |
| ---- | -------- | ---- |
| **servo-fetch** | **114 MB** | **1.50s** |
| Playwright | 502 MB | 3.26s |
| Puppeteer | 1065 MB | 4.27s |

### Key takeaways

- **vs Playwright**: 3.5× less memory, 1.7–2.2× faster
- **vs Puppeteer**: 9× less memory, 2.8–3.4× faster
- **Per additional page**: servo-fetch ~17 MB, Playwright ~93 MB, Puppeteer ~158 MB

## Methodology

### Task

All tools perform the same operation: navigate to a URL, wait for the `load` event (JavaScript executed), and extract `document.body.innerText`. For the parallel test, all URLs are fetched concurrently.

### Memory measurement

Peak RSS (Resident Set Size) of the entire process tree — the target process plus all its descendants. Chromium spawns separate renderer processes per page; all are included.

We sample every 50ms using a recursive `pgrep -P` walk. See [`run_benchmark.sh`](run_benchmark.sh) for the implementation.

On Linux, `smem` (PSS) would be more precise for shared memory. macOS does not expose PSS, so we use RSS. This slightly overcounts shared libraries in Chromium's multi-process architecture, but reflects actual system memory pressure (what the OOM killer sees).

### Procedure

1. **Warmup**: 1 run discarded (DNS cache, disk cache, JIT warmup)
2. **Measurement**: 3 runs, median reported
3. **Sampling**: process tree RSS polled every 50ms; peak value recorded
4. **Timing**: wall-clock via `time.time()` delta

### Test machine

- **Hardware**: Apple M3 Pro, 18 GB RAM
- **OS**: macOS (arm64)

### Versions

Run `./run_benchmark.sh` to see exact versions. The script auto-detects and prints all tool versions at the top of its output.

## Reproduction

```bash
# 1. Build servo-fetch
cargo build --release

# 2. Install comparison tools
cd benchmarks/playwright && npm install && npx playwright install chromium && cd ..
cd puppeteer && npm install && cd ..

# 3. Run
./run_benchmark.sh
```
