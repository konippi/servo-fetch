# Environment
- date                  : 2026-05-06T06:01:04Z
- host OS               : macOS 26.3 (darwin 25.3.0)
- arch                  : arm64
- CPU                   : Apple M3 Pro (11P / 11L)
- memory                : 18.0 GiB
- hyperfine             : hyperfine 1.20.0
- node                  : v24.15.0
- servo-fetch           : servo-fetch 0.7.1
- Playwright            : 1.59.1
- chrome-headless-shell : Google Chrome for Testing 148.0.7778.96
- lightpanda            : unknown

# Equivalence check

| Tool | Fixture | Status | Missing |
|---|---|---|---|
| `curl` | `static-small` | ✅ pass |  |
| `servo-fetch` | `static-small` | ✅ pass |  |
| `chrome-headless-shell` | `static-small` | ✅ pass |  |
| `lightpanda` | `static-small` | ⚠️ expected-fail | Static small fixture, fixed per-fetch overhead |
| `playwright:optimized` | `static-small` | ✅ pass |  |
| `curl` | `spa-light` | ⚠️ expected-fail | SPA light fixture, This fixture represents a minimal single-page application |
| `servo-fetch` | `spa-light` | ✅ pass |  |
| `chrome-headless-shell` | `spa-light` | ✅ pass |  |
| `lightpanda` | `spa-light` | ⚠️ expected-fail | SPA light fixture, This fixture represents a minimal single-page application |
| `playwright:optimized` | `spa-light` | ✅ pass |  |
| `curl` | `spa-heavy` | ⚠️ expected-fail | SPA heavy fixture, Item 1 · score, Item 1000 · score |
| `servo-fetch` | `spa-heavy` | ✅ pass |  |
| `chrome-headless-shell` | `spa-heavy` | ✅ pass |  |
| `lightpanda` | `spa-heavy` | ⚠️ expected-fail | SPA heavy fixture, Item 1 · score, Item 1000 · score |
| `playwright:optimized` | `spa-heavy` | ✅ pass |  |
