# Environment
- date                  : 2026-05-06T06:02:38Z
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

# Parallel scalability

Fixture: `spa-light`.

## N = 1

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `servo-fetch (N=1)` | 251.5 ± 39.3 | 220.1 | 336.6 | 1.00 |
| `playwright:optimized (N=1)` | 615.9 ± 50.5 | 539.1 | 697.8 | 2.45 ± 0.43 |

## N = 2

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `servo-fetch (N=2)` | 275.5 ± 20.8 | 253.7 | 320.8 | 1.00 |
| `playwright:optimized (N=2)` | 574.6 ± 41.3 | 547.5 | 673.7 | 2.09 ± 0.22 |

## N = 4

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `servo-fetch (N=4)` | 274.3 ± 10.0 | 264.3 | 299.4 | 1.00 |
| `playwright:optimized (N=4)` | 592.5 ± 15.1 | 570.2 | 625.7 | 2.16 ± 0.10 |

## N = 8

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `servo-fetch (N=8)` | 306.6 ± 20.8 | 288.9 | 354.7 | 1.00 |
| `playwright:optimized (N=8)` | 664.2 ± 31.6 | 628.3 | 746.0 | 2.17 ± 0.18 |

