# Benchmarks

[![Python 3.11+](https://img.shields.io/badge/python-3.11%2B-blue?logo=python&logoColor=white)](https://www.python.org/)
[![uv](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/uv/main/assets/badge/v0.json)](https://github.com/astral-sh/uv)
[![Ruff](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json)](https://github.com/astral-sh/ruff)

Benchmarks for servo-fetch. Run with `./benchmarks/bench`. Every result
file records the machine, tool versions, and fixture SHA.

## Three axes

We measure three axes and report them separately.

**Axis 1 — Engine fairness** (direct binary, all invoked the same way):

```text
servo-fetch URL                        # Rust + Servo (Stylo + SpiderMonkey)
chrome-headless-shell --dump-dom URL   # Chromium (Blink + V8)
lightpanda fetch URL                   # Zig + custom engine
curl -sL URL                           # no-JS lower bound
```

Peer install and licenses: [Peer engines](#peer-engines-axis-1).

**Axis 2 — Production reality** (the deployment cost readers actually pay):

```text
node + playwright → chromium           # representative AI-agent stack
servo-fetch URL                        # zero-wrapper
```

**Axis 3 — Extraction quality** — word-F1 and `with[]` / `without[]`
snippet coverage on seven page-type fixtures, plus the
[scrapinghub/article-extraction-benchmark](https://github.com/scrapinghub/article-extraction-benchmark) (MIT, 2020).

## Methodology

- **Same invocation layer per table.** Every runner is called the same
  way — all direct binary, or all through an identical wrapper. Never mixed.
- **Engines to engines, extractors to extractors.** Categories are
  measured separately.
- **Three metrics.** Wall-clock (hyperfine, warmup + min-runs + outlier
  detection), peak memory including children, and output validity.
- **Local fixtures.** An in-memory HTTP server on `127.0.0.1` eliminates
  DNS, TLS, and tenant variance.
- **Pin and publish.** Tool version, fixture SHA, and machine spec go
  in every result file.

## Sample numbers

Apple M3 Pro (11P / 11L, 18 GiB), macOS 26.3, servo-fetch 0.7.1,
Chrome for Testing 148.0.7778.96, Playwright 1.59.1, Lightpanda
nightly, on 2026-05-06. Cloud-CI is not quoted — shared-tenant noise
dominates the signal ([Bheisler 2019](https://bheisler.github.io/post/benchmarking-in-the-cloud/)).
Read [Caveats](#caveats) before quoting any of this.
Full raw data (Markdown + JSON): [`results/published-*.{md,json}`](results/).

### Axis 1 — Engine fairness (direct-binary peers)

Direct-binary invocation (no Node.js wrapper). `chrome-headless-shell`
is pinned (148.0.7778.96); Lightpanda runs nightly builds (see
[Caveats](#caveats)). `curl` is the no-JS floor and omitted below.

| Benchmark           | servo-fetch | chrome-headless-shell | lightpanda |
| ------------------- | ----------: | --------------------: | ---------: |
| Time — static-small |     ~239 ms |               ~265 ms |    ~289 ms |
| Time — spa-light    |     ~235 ms |               ~254 ms |    ~255 ms |
| Time — spa-heavy    |     ~272 ms |               ~284 ms |    ~289 ms |

Full matrix with stddev and `curl` floor: [`results/published-time.md`](results/published-time.md).

### Axis 2 — Production reality (servo-fetch vs Playwright)

| Benchmark                     | servo-fetch | playwright:optimized |
| ----------------------------- | ----------: | -------------------: |
| Time — static-small           |     ~231 ms |              ~645 ms |
| Time — spa-light              |     ~235 ms |              ~553 ms |
| Time — spa-heavy              |     ~331 ms |              ~798 ms |
| Parallel N=8 (spa-light)      |     ~307 ms |              ~664 ms |
| Memory — static-small (peak)  |       64 MB |              328 MB  |
| Memory — spa-heavy (peak)     |      ~96 MB |              350 MB  |

### Axis 3 — Extraction quality (mean across 7 page-type fixtures)

| Metric      | servo-fetch (layout-aware) | Readability (DOM-only) |
| ----------- | -------------------------: | ---------------------: |
| Word-F1     |                      0.823 |                  0.723 |
| `with[]`    |                      88.6% |                  62.9% |
| `without[]` |                      96.4% |                  82.1% |

servo-fetch's advantage concentrates on boilerplate-heavy pages —
navbar/sidebar/footer removal (the `without[]` metric). Per-fixture
breakdown: [`results/published-extraction-layout.md`](results/published-extraction-layout.md).

## Quick start

Supported on macOS and Linux; Windows is not supported (POSIX
process groups and `/etc/hosts` are used). Time and extraction
benchmarks run identically on both. For accurate memory numbers of
Chromium-based peers, use Linux — on macOS, Playwright's detached
Chromium subprocess tree is not reliably captured (see
[Caveats](#caveats)).

```bash
cargo build --release                     # servo-fetch binary
brew install hyperfine uv                 # or apt / pkg equivalent
npm --prefix benchmarks/node ci           # playwright + readability

./benchmarks/bench setup                  # /etc/hosts alias (sudo, once)
./benchmarks/bench install-binaries all   # optional: Axis 1 peers
./benchmarks/bench all                    # every local benchmark (~25 min)
```

## Commands

| Command                            | Measures                                                      |
| ---------------------------------- | ------------------------------------------------------------- |
| `bench setup`                      | `/etc/hosts` alias `bench.servo-fetch` → 127.0.0.1            |
| `bench install-binaries [WHICH]`   | Download chrome-headless-shell and/or lightpanda              |
| `bench equivalence`                | Required substrings per runner × fixture                      |
| `bench time`                       | Wall-clock (hyperfine, warmup 3, min-runs 10)                 |
| `bench memory`                     | Peak RSS per runner × fixture (median of 5)                   |
| `bench parallel [--fixture NAME]`  | Throughput curve for N ∈ {1, 2, 4, 8}, multi-URL runners only |
| `bench size`                       | Binary size + cold-start latency                              |
| `bench extract layout`             | F1 + `with[]`/`without[]` on 7 page-type fixtures             |
| `bench extract dataset [DATASET]`  | F1 on scrapinghub/article-extraction (181 pages)              |
| `bench download [DATASET]`         | Fetch dataset into `data/` (no extra deps needed)             |
| `bench all`                        | Every benchmark that needs no external dataset                |

Outputs land in `results/` as Markdown + JSON. `bench all` takes ~25 min;
individual commands run 1–10 min each.

## Configuration

Defaults live in `src/servo_fetch_bench/config.py`; every field is
environment-overridable.

| Variable                    | Default                            | Meaning                          |
| --------------------------- | ---------------------------------- | -------------------------------- |
| `PORT`                      | `8731`                             | Fixture server port              |
| `BENCH_HOST`                | `bench.servo-fetch`                | Hostname used in URLs            |
| `WARMUP`                    | `3`                                | hyperfine warmup runs            |
| `MIN_RUNS`                  | `10`                               | hyperfine minimum runs           |
| `MEMORY_RUNS`               | `5`                                | Samples per memory measurement   |
| `PARALLEL_URLS`             | `1,2,4,8`                          | N values for parallel            |
| `FIXTURES`                  | `static-small spa-light spa-heavy` | Fixture subset for time / memory |
| `SERVO_FETCH_BIN`           | `../target/release/servo-fetch`    | Binary under test                |
| `NODE_BIN`                  | `node`                             | Node executable                  |
| `CHROME_HEADLESS_SHELL_BIN` | `./bin/chrome-headless-shell`      | Optional peer binary             |
| `LIGHTPANDA_BIN`            | `./bin/lightpanda`                 | Optional peer binary             |

```bash
PORT=9000 ./benchmarks/bench equivalence
FIXTURES="static-small" MEMORY_RUNS=10 ./benchmarks/bench memory
```

## Peer engines (Axis 1)

Direct-binary runners compared in Axis 1 (no Node.js wrapper). Install
with `bench install-binaries all`; missing binaries are skipped silently.

- **`chrome-headless-shell`** — slim Chromium (headless), BSD-style
- **`lightpanda`** — Zig-based minimal browser, AGPL-3.0¹, nightly
- **`curl`** — no-JS HTTP baseline, ships with macOS/Linux

¹ Invoked as an external subprocess (`fork` + `exec`); no Lightpanda
  code is linked or redistributed. servo-fetch remains MIT OR Apache-2.0.

## Caveats

These numbers are produced by servo-fetch's author and are biased
toward cases servo-fetch handles well. Every result file records the
exact machine, versions, and flags.

- **Memory on macOS has variance for Chromium-based tools.** Playwright
  launches Chromium with `detached: true` (separate process session).
  The default `tree` strategy (psutil polls parent + descendants)
  captures the subprocess tree in practice, but short-lived helpers
  can slip through. Linux + [`cgmemtime`](https://github.com/gsauthof/cgmemtime)
  via `bench memory --cgmemtime` uses kernel cgroup accounting and
  remains the most accurate method.
- **Loopback fixtures are artificial.** `127.0.0.1` eliminates DNS,
  TLS, HTTP/2, and cold-disk-cache overhead. Real-web wall-clock will
  be meaningfully higher for every tool.
- **Lightpanda ships nightly.** Reproducibility is limited by the
  commit installed on the day of the run.
- **Cloud CI is noisy for perf** ([Bheisler 2019](https://bheisler.github.io/post/benchmarking-in-the-cloud/)).
  Shared-tenant runners on GitHub Actions can swing wall-clock ±20%,
  so benchmarks are not run in CI. Published `results/` files are
  produced by running `./bench all` on the author's local machine;
  the spec is recorded in every result file. Re-run locally on your
  own hardware for comparable numbers.

Out of scope: tail-latency measurement, real-network timing,
long-running server workloads.

## Hostname alias

servo-fetch blocks `127.0.0.0/8` in production code as an SSRF guard.
The `/etc/hosts` alias `bench.servo-fetch → 127.0.0.1` keeps the guard
on while reaching the fixture server via a loopback entry.

## Development

```bash
cd benchmarks
uv sync --group dev
uv run pytest
uv run ruff check src tests tools
```
