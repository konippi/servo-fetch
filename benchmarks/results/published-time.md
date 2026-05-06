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

# Time benchmarks

## static-small

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `servo-fetch` | 230.9 ± 16.7 | 213.9 | 271.4 | 1.00 |
| `chrome-headless-shell` | 273.6 ± 10.9 | 257.9 | 302.3 | 1.19 ± 0.10 |
| `lightpanda` | 295.2 ± 25.0 | 264.5 | 337.0 | 1.28 ± 0.14 |
| `playwright:optimized` | 645.3 ± 60.0 | 564.4 | 726.8 | 2.80 ± 0.33 |

## spa-light

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `servo-fetch` | 235.1 ± 12.8 | 215.8 | 266.0 | 1.00 |
| `chrome-headless-shell` | 279.6 ± 15.6 | 250.5 | 313.7 | 1.19 ± 0.09 |
| `lightpanda` | 270.5 ± 22.2 | 236.2 | 322.6 | 1.15 ± 0.11 |
| `playwright:optimized` | 552.5 ± 17.5 | 522.8 | 583.8 | 2.35 ± 0.15 |

## spa-heavy

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `servo-fetch` | 330.7 ± 119.9 | 255.7 | 613.5 | 1.05 ± 0.39 |
| `chrome-headless-shell` | 314.4 ± 16.6 | 301.3 | 360.1 | 1.00 |
| `lightpanda` | 465.0 ± 318.9 | 305.0 | 1340.7 | 1.48 ± 1.02 |
| `playwright:optimized` | 798.2 ± 156.5 | 588.7 | 1157.4 | 2.54 ± 0.52 |

