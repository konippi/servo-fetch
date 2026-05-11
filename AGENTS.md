# AGENTS.md

Instructions for AI coding agents working on servo-fetch. This file complements, not replaces, [`CONTRIBUTING.md`](CONTRIBUTING.md) (human-oriented) and [`skills/servo-fetch/SKILL.md`](skills/servo-fetch/SKILL.md) (consumer-oriented). Treat this file as living documentation; update it when conventions change.

## Project overview

servo-fetch is a Rust workspace with two crates:

- **`servo-fetch`** (library): wraps the [Servo](https://servo.org) browser engine to fetch, render, and extract web content. Main entry points: `fetch()`, `crawl()` / `crawl_each()` (streaming), `map()` (sitemap discovery).
- **`servo-fetch-cli`** (binary): CLI + HTTP API + MCP server. Depends on `servo-fetch`.

Toolchain pinned in `rust-toolchain.toml`; MSRV declared as `rust-version` in `Cargo.toml` — do not use features stabilized after the MSRV. Workspace-wide version bumping via `[workspace.package].version`.

The Servo dependency dominates build time (several minutes on first build, ~30-40 minutes for release). Plan long-running commands accordingly.

## Setup and commands

Run these from the workspace root unless noted. Copy-paste ready:

```sh
cargo build                                             # Debug build
cargo build --release                                   # Release build (slow)
cargo fmt --all                                         # Format
cargo fmt --all -- --check                              # CI fmt check
cargo clippy --workspace --all-targets -- -D warnings   # Lint (pedantic, warnings are errors)
cargo test --workspace                                  # Unit + integration tests (fast)
cargo test -- --ignored                                 # Include Servo-backed e2e tests (slow)
cargo deny check                                        # License + advisory audit
taplo check                                             # TOML lint (install: cargo install taplo-cli --locked)
taplo fmt                                               # TOML format
typos                                                   # Spell check
```

Nextest profiles (preferred over `cargo test` in CI):

```sh
cargo nextest run --workspace --profile ci                                   # Unit + integration, CI output
cargo nextest run -p servo-fetch-cli --run-ignored only --profile ci-e2e     # E2E, retries on flake
```

Ignored tests require a working Servo environment. Servo uses `SoftwareRenderingContext` internally, so no X server is needed in CI (the apt install list in `release.yml` includes X11 libs for linking, not for runtime). If an `ignored` test fails locally, first retry; if it still fails, check whether the test hits the network or a site that requires `--settle`.

## Code conventions

### Rust style

- **`cargo clippy` is a gate.** Use `-D warnings` in CI; keep it passing locally before pushing.
- **Inline `format!` args** — `format!("x={x}")`, not `format!("x={}", x)`. See the clippy rule `uninlined_format_args`.
- **`#[non_exhaustive]` on public structs/enums** that we expect to grow (e.g., `Page`, `CrawlResult`, `ConsoleMessage`). Do not add new public enum variants or struct fields without it unless the type is intentionally sealed.
- **Fields `pub(crate)` + builder methods** for option structs like `CrawlOptions`, `FetchOptions`, `MapOptions`. Callers construct via `Options::new(url)` and chain `.method(value)`. Adding a new `pub(crate)` field + builder method is always non-breaking.
- **`Result<T, Error>` end-to-end** — crate-local `error::Result`. No `unwrap()` in non-test code except where a panic is documented as the only sane behaviour (e.g., poisoned mutex).
- **Single-concern modules over strict line counts.** Large files are acceptable when they remain one semantic concern (e.g., `crawl.rs` owns all crawl logic). Split when a second concern emerges, not when LoC crosses a threshold.
- **No `#[async_trait]`** — prefer native RPITIT (`fn foo(&self, ...) -> impl Future<Output = T> + Send`) when the return type is nameable.

### Commits

Conventional Commits, **subject-only** by default. Use the body only when critical context cannot fit in the subject. Scopes used in this repo:

- `chore(deps)` — external dependencies the **product ships** (cargo crates, docker base image).
- `chore(ci)` — CI/workflow configuration, github-actions entries, Dependabot config itself.
- `fix(<area>)` — bug fixes; `<area>` is typically a module (`crawl`, `map`, `build`, `tests`) or `ci`.
- `feat(<area>)` — user-facing additions.
- `refactor(<area>)`, `test(<area>)`, `docs`, `perf`, `build`.

Examples: `fix(build): drop +crt-static on Windows to match mozjs-sys CRT`, `chore(deps): bump rustls`.

### Branch names

Kebab-case under a type prefix: `fix/windows-crt-mismatch`, `refactor/wiremock-body-mime-consistency`, `ci/trivy-image-scan`.

### GitHub Actions

**All third-party actions must be SHA-pinned** with a version comment: `uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2`. This is enforced for supply-chain hygiene (see the March 2026 `trivy-action` compromise). Dependabot (`chore(ci)`) keeps SHAs current.

## Testing idioms

### wiremock mocks

**Always use `set_body_raw(body, mime)`**, never `set_body_string` + `insert_header("content-type", ...)`. `set_body_string` unconditionally overwrites the content-type to `text/plain`, regardless of chain order, and the resulting mock silently serves the wrong MIME — which breaks browser-level HTML parsing (Servo receives `text/plain` and wraps the body in `<pre>`).

```rust
// WRONG — insert_header is overridden by set_body_string's forced text/plain
ResponseTemplate::new(200)
    .insert_header("content-type", "text/html")
    .set_body_string(html)

// RIGHT
ResponseTemplate::new(200).set_body_raw(html.into_bytes(), "text/html; charset=utf-8")
```

Shared helper: `crates/servo-fetch-cli/tests/common.rs::mock_page`.

### E2E tests

Mark Servo-backed tests with `#[ignore = "e2e: requires Servo engine"]` and run them via the `ci-e2e` nextest profile, which enables retries with a 2-second backoff. Keep the mock HTML minimal but syntactically valid (`<!doctype html><html>...</html>`).

### Assertions

Compare whole values with `assert_eq!` over field-by-field checks. Failing diffs are easier to read when the entire structure is dumped.

## When contributing

- Read [`CONTRIBUTING.md`](CONTRIBUTING.md) for the human-oriented workflow and the AI usage policy.
- Read [`SECURITY.md`](SECURITY.md) before changing anything SSRF-, TLS-, or signing-related.
- User-facing behavior changes must update:
  - `README.md` (CLI + library usage)
  - `crates/servo-fetch-cli/README.md` (CLI flags reference)
  - `skills/servo-fetch/SKILL.md` and `skills/servo-fetch/references/guide.md` if behavior or flags change
  - `SECURITY.md` if policy defaults change
- PR template: `.github/pull_request_template.md`. Keep the Summary concise; explain *why* where non-obvious, not *what* (the diff shows what).
- Do not submit autonomously-generated PRs. Human-in-the-loop is a hard requirement (see CONTRIBUTING.md § Use of AI).
