# Environment
- date                  : 2026-05-06T06:03:35Z
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

# Extraction quality — page-type fixtures

## `article-footer-heavy`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.851 | 100% | 100% |
| Readability (DOM-only) | 0.982 | 100% | 100% |

## `documentation-sidebar`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.858 | 100% | 100% |
| Readability (DOM-only) | 0.984 | 100% | 100% |

## `service-multi-section`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.862 | 100% | 100% |
| Readability (DOM-only) | 0.026 | 0% | 0% |

## `forum-thread`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.974 | 100% | 100% |
| Readability (DOM-only) | 0.961 | 80% | 100% |

## `product-jsonld`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.312 | 20% | 75% |
| Readability (DOM-only) | 0.136 | 0% | 75% |

## `collection-grid`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.932 | 100% | 100% |
| Readability (DOM-only) | 0.977 | 80% | 100% |

## `listing-cards`

| Extractor | F1 | with[] | without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.974 | 100% | 100% |
| Readability (DOM-only) | 0.992 | 80% | 100% |

## Summary — mean across 7 fixtures

| Extractor | Mean F1 | Mean with[] | Mean without[] |
|-|-:|-:|-:|
| servo-fetch (layout-aware) | 0.823 | 88.6% | 96.4% |
| Readability (DOM-only) | 0.723 | 62.9% | 82.1% |
