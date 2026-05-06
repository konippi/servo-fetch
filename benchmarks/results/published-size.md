# Environment
- date                  : 2026-05-06T06:04:02Z
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

# Binary footprint

| Tool | Binary size | Non-system deps¹ | Path |
|-|-:|-:|-|
| `servo-fetch` | 71.6 MiB | 0 | /Users/konippi/dev/oss/servo-fetch/target/release/servo-fetch |
| `chrome-headless-shell` | 150.5 MiB | 0 | /Users/konippi/dev/oss/servo-fetch/benchmarks/bin/chrome-headless-shell-mac-arm64/chrome-headless-shell |
| `lightpanda` | 62.4 MiB | 0 | /Users/konippi/dev/oss/servo-fetch/benchmarks/bin/lightpanda |
| `curl` | 0.5 MiB | 0 | /usr/bin/curl |
| `node` | 6.0 MiB | 0 | /Users/konippi/.vite-plus/bin/node |

¹ Dynamic libraries outside `/System/Library`, `/usr/lib`, `/lib`, `/lib64`. System stdlib doesn't count against a zero-deps claim — it's on every installed OS already.
