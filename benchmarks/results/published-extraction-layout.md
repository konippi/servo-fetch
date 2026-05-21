# Environment
- date                  : 2026-05-21T05:52:36Z
- host OS               : macOS 26.3 (darwin 25.3.0)
- arch                  : arm64
- CPU                   : Apple M3 Pro (11P / 11L)
- memory                : 18.0 GiB
- hyperfine             : hyperfine 1.20.0
- node                  : v24.15.0
- servo-fetch           : servo-fetch 0.10.1
- Playwright            : 1.59.1
- chrome-headless-shell : Google Chrome for Testing 148.0.7778.96
- lightpanda            : unknown

# Extraction quality — page-type fixtures

_`visibility=off` disables flag-based filtering (baseline); `moderate` (default) strips CSS- and ARIA-hidden content; `strict` additionally drops screen-reader-only nodes._

## `article-footer-heavy`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.851 | 100% | 100% |
| servo-fetch (visibility=moderate) | 0.851 | 100% | 100% |
| servo-fetch (visibility=strict) | 0.851 | 100% | 100% |
| Readability (DOM-only) | 0.982 | 100% | 100% |

## `documentation-sidebar`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.858 | 100% | 100% |
| servo-fetch (visibility=moderate) | 0.858 | 100% | 100% |
| servo-fetch (visibility=strict) | 0.858 | 100% | 100% |
| Readability (DOM-only) | 0.984 | 100% | 100% |

## `service-multi-section`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.862 | 100% | 100% |
| servo-fetch (visibility=moderate) | 0.862 | 100% | 100% |
| servo-fetch (visibility=strict) | 0.862 | 100% | 100% |
| Readability (DOM-only) | 0.026 | 0% | 0% |

## `forum-thread`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.974 | 100% | 100% |
| servo-fetch (visibility=moderate) | 0.974 | 100% | 100% |
| servo-fetch (visibility=strict) | 0.974 | 100% | 100% |
| Readability (DOM-only) | 0.961 | 80% | 100% |

## `product-jsonld`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.312 | 20% | 75% |
| servo-fetch (visibility=moderate) | 0.312 | 20% | 75% |
| servo-fetch (visibility=strict) | 0.312 | 20% | 75% |
| Readability (DOM-only) | 0.136 | 0% | 75% |

## `collection-grid`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.932 | 100% | 100% |
| servo-fetch (visibility=moderate) | 0.932 | 100% | 100% |
| servo-fetch (visibility=strict) | 0.932 | 100% | 100% |
| Readability (DOM-only) | 0.977 | 80% | 100% |

## `listing-cards`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.974 | 100% | 100% |
| servo-fetch (visibility=moderate) | 0.974 | 100% | 100% |
| servo-fetch (visibility=strict) | 0.974 | 100% | 100% |
| Readability (DOM-only) | 0.992 | 80% | 100% |

## `visibility-spam`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.672 | 100% | 54% |
| servo-fetch (visibility=moderate) | 0.788 | 100% | 85% |
| servo-fetch (visibility=strict) | 0.820 | 100% | 100% |
| Readability (DOM-only) | 0.769 | 100% | 54% |

## Summary — mean across 8 fixtures

| Extractor | Mean F1 | Mean with[] | Mean without[] |
|-|-:|-:|-:|
| servo-fetch (visibility=off) | 0.804 | 90.0% | 91.1% |
| servo-fetch (visibility=moderate) | 0.819 | 90.0% | 95.0% |
| servo-fetch (visibility=strict) | 0.823 | 90.0% | 96.9% |
| Readability (DOM-only) | 0.728 | 67.5% | 78.6% |
