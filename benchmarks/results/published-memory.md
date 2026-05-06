# Environment
- date                  : 2026-05-06T06:02:37Z
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

# Memory benchmarks

Median of 5 runs. Measurement: psutil tree polling (parent + all descendants, 10ms cadence).

| Tool × Fixture | min | median | max |
|---|---|---|---|
| curl / static-small | 4.1 MB | 4.7 MB | 4.7 MB |
| servo-fetch / static-small | 51.9 MB | 53.2 MB | 63.4 MB |
| chrome-headless-shell / static-small | 301.3 MB | 317.3 MB | 324.8 MB |
| lightpanda / static-small | 23.1 MB | 23.2 MB | 23.3 MB |
| playwright:optimized / static-small | 300.7 MB | 314.7 MB | 328.0 MB |
| curl / spa-light | 4.1 MB | 4.5 MB | 4.5 MB |
| servo-fetch / spa-light | 51.3 MB | 63.4 MB | 63.8 MB |
| chrome-headless-shell / spa-light | 299.4 MB | 311.1 MB | 317.2 MB |
| lightpanda / spa-light | 25.0 MB | 25.0 MB | 25.0 MB |
| playwright:optimized / spa-light | 313.3 MB | 314.4 MB | 327.7 MB |
| curl / spa-heavy | 4.5 MB | 4.7 MB | 4.7 MB |
| servo-fetch / spa-heavy | 82.6 MB | 93.1 MB | 98.2 MB |
| chrome-headless-shell / spa-heavy | 336.0 MB | 336.1 MB | 349.1 MB |
| lightpanda / spa-heavy | 36.2 MB | 36.4 MB | 36.8 MB |
| playwright:optimized / spa-heavy | 334.8 MB | 343.4 MB | 349.4 MB |
