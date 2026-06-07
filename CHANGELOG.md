# Changelog

All notable changes to this project will be documented in this file.

## [0.12.1](https://github.com/konippi/servo-fetch/compare/0.12.0..0.12.1) - 2026-06-07

### Features

- *(cookies)* Add session-cookie injection from cookies.txt (#266)

## [0.12.0](https://github.com/konippi/servo-fetch/compare/0.11.4..0.12.0) - 2026-05-31

### Breaking Changes

- *(api)* Split sync and async ([#258](https://github.com/konippi/servo-fetch/pull/258))

### Migration Guide

The top-level synchronous helpers `fetch`, `markdown`, `text`, `extract_json`,
`crawl`, `crawl_each`, and `map` have been moved to `servo_fetch::blocking::*`.
Names, arguments, and behavior are unchanged.

### Features

- *(api)* Split sync and async (#258)

## [0.11.4](https://github.com/konippi/servo-fetch/compare/0.11.3..0.11.4) - 2026-05-28

### Features

- Preserve source chain and restructure variants ([#249](https://github.com/konippi/servo-fetch/pull/249))

## [0.11.3](https://github.com/konippi/servo-fetch/compare/0.11.2..0.11.3) - 2026-05-27

### Features

- *(cli)* Replace --screenshot with --format png ([#246](https://github.com/konippi/servo-fetch/pull/246))

### Miscellaneous

- Consolidate all external deps into workspace.dependencies ([#244](https://github.com/konippi/servo-fetch/pull/244))

## [0.11.2](https://github.com/konippi/servo-fetch/compare/0.11.1..0.11.2) - 2026-05-26

### Features

- *(cli)* Add healthcheck subcommand for orchestrator probes ([#239](https://github.com/konippi/servo-fetch/pull/239))

### Bug Fixes

- *(cli)* Align package.metadata.binstall with release archive layout ([#237](https://github.com/konippi/servo-fetch/pull/237))

## [0.11.1](https://github.com/konippi/servo-fetch/compare/0.11.0..0.11.1) - 2026-05-24

### Features

- *(cli)* Add -o/--output and --output-dir for file output ([#207](https://github.com/konippi/servo-fetch/pull/207))
- *(cli)* Unify output flags under --format ([#206](https://github.com/konippi/servo-fetch/pull/206))

### Bug Fixes

- *(bridge)* Bail out on timeout to avoid Servo parser race ([#208](https://github.com/konippi/servo-fetch/pull/208))

### Refactor

- *(test)* Run CLI tests in-process instead of spawning binary ([#211](https://github.com/konippi/servo-fetch/pull/211))

### Miscellaneous

- Enforce canonical imports via pinned nightly rustfmt ([#210](https://github.com/konippi/servo-fetch/pull/210))

## [0.11.0](https://github.com/konippi/servo-fetch/compare/v0.10.1..v0.11.0) - 2026-05-21

### Features

- *(crawl)* Type-tagged NDJSON with stats trailer and fetched_at ([#202](https://github.com/konippi/servo-fetch/pull/202))
- *(extract)* Visibility-aware filtering ([#200](https://github.com/konippi/servo-fetch/pull/200))

### Bug Fixes

- *(docker)* Bump debian:trixie-slim digest to 13.5 ([#203](https://github.com/konippi/servo-fetch/pull/203))

### Testing

- Add edge-case HTML fixtures for extraction tests ([#198](https://github.com/konippi/servo-fetch/pull/198))

## [0.10.1](https://github.com/konippi/servo-fetch/compare/v0.10.0..v0.10.1) - 2026-05-17

### Performance

- *(ci)* Share cargo-test build cache and parallelize e2e ([#195](https://github.com/konippi/servo-fetch/pull/195))

### Refactor

- *(test)* Replace super::super::* wildcards with crate paths ([#193](https://github.com/konippi/servo-fetch/pull/193))

### Documentation

- Use block comments in readme code snippets ([#185](https://github.com/konippi/servo-fetch/pull/185))
- Add python to readme intro and install table ([#184](https://github.com/konippi/servo-fetch/pull/184))
- *(python)* Add usage examples ([#183](https://github.com/konippi/servo-fetch/pull/183))

### Testing

- *(python)* Add stubtest CI gate and fix discovered stub drift ([#194](https://github.com/konippi/servo-fetch/pull/194))
- Cover Page::from_servo and pdf::probe HTTP paths ([#192](https://github.com/konippi/servo-fetch/pull/192))

### CI

- *(ci)* Bump actions/download-artifact from 4.3.0 to 8.0.1 ([#187](https://github.com/konippi/servo-fetch/pull/187))
- *(ci)* Bump astral-sh/setup-uv from 6.8.0 to 8.1.0 ([#190](https://github.com/konippi/servo-fetch/pull/190))
- *(ci)* Bump DavidAnson/markdownlint-cli2-action ([#188](https://github.com/konippi/servo-fetch/pull/188))
- *(ci)* Bump crate-ci/typos from 1.46.0 to 1.46.1 ([#189](https://github.com/konippi/servo-fetch/pull/189))
- *(ci)* Bump taiki-e/install-action from 2.76.0 to 2.77.3 ([#186](https://github.com/konippi/servo-fetch/pull/186))

### Dependencies

- *(deps)* Bump the patch-updates group with 3 updates ([#191](https://github.com/konippi/servo-fetch/pull/191))

## [0.10.0](https://github.com/konippi/servo-fetch/compare/v0.9.1..v0.10.0) - 2026-05-15

### Features

- *(release)* Add aarch64 Linux wheel build ([#179](https://github.com/konippi/servo-fetch/pull/179))
- *(python)* Add python sdk ([#170](https://github.com/konippi/servo-fetch/pull/170))
- *(schema)* Add CSS-selector schema extraction ([#169](https://github.com/konippi/servo-fetch/pull/169))

### Bug Fixes

- *(release)* Add license-files to pyproject.toml for sdist ([#182](https://github.com/konippi/servo-fetch/pull/182))
- *(release)* Remove dnf clang install (shadows container's configured clang) ([#180](https://github.com/konippi/servo-fetch/pull/180))
- *(release)* Remove twine check (incompatible with PEP 639 metadata) ([#178](https://github.com/konippi/servo-fetch/pull/178))
- *(release)* Use Docker manylinux for x86_64, defer aarch64 Linux ([#177](https://github.com/konippi/servo-fetch/pull/177))
- *(release)* Separate platform jobs for Python wheel builds ([#176](https://github.com/konippi/servo-fetch/pull/176))
- *(release)* Separate platform jobs for Python wheel builds ([#175](https://github.com/konippi/servo-fetch/pull/175))
- *(release)* Use gcc-toolset-15 for SpiderMonkey on manylinux ([#174](https://github.com/konippi/servo-fetch/pull/174))
- *(release)* Detect package manager for cross-platform Linux builds ([#173](https://github.com/konippi/servo-fetch/pull/173))
- *(release)* Use yum in manylinux container ([#172](https://github.com/konippi/servo-fetch/pull/172))

### Documentation

- Add AGENTS.md for AI coding agents ([#166](https://github.com/konippi/servo-fetch/pull/166))

## [0.9.1](https://github.com/konippi/servo-fetch/compare/v0.9.0..v0.9.1) - 2026-05-11

### Features

- *(crawl)* Add configurable concurrency and delay to crawler ([#156](https://github.com/konippi/servo-fetch/pull/156))

### Bug Fixes

- *(build)* Drop +crt-static on Windows to match mozjs-sys CRT ([#165](https://github.com/konippi/servo-fetch/pull/165))
- *(crawl)* Wire --selector and --json through to CrawlOptions ([#159](https://github.com/konippi/servo-fetch/pull/159))
- *(crypto)* Align rustls features and guard provider install ([#158](https://github.com/konippi/servo-fetch/pull/158))
- *(ci)* Update smoke test expectations for new AddressNotAllowed message ([#144](https://github.com/konippi/servo-fetch/pull/144))

### Refactor

- *(tests)* Use set_body_raw for explicit mime types in map tests ([#160](https://github.com/konippi/servo-fetch/pull/160))
- *(engine)* Split engine.rs into feature modules ([#157](https://github.com/konippi/servo-fetch/pull/157))

### CI

- Scan published Docker image with Trivy ([#163](https://github.com/konippi/servo-fetch/pull/163))
- *(ci)* Bump taiki-e/install-action from 2.75.29 to 2.76.0 ([#162](https://github.com/konippi/servo-fetch/pull/162))
- *(ci)* Let Dependabot update Dockerfile base image digest ([#161](https://github.com/konippi/servo-fetch/pull/161))
- *(taplo)* Add TOML formatter config and workflow ([#155](https://github.com/konippi/servo-fetch/pull/155))
- *(nextest)* Add repository config and wire profiles into CI ([#154](https://github.com/konippi/servo-fetch/pull/154))
- *(ci)* Bump actions/upload-artifact from 5.0.0 to 7.0.1 ([#150](https://github.com/konippi/servo-fetch/pull/150))
- *(ci)* Bump actions/download-artifact from 6.0.0 to 8.0.1 ([#147](https://github.com/konippi/servo-fetch/pull/147))
- *(ci)* Bump taiki-e/install-action from 2.75.23 to 2.75.29 ([#146](https://github.com/konippi/servo-fetch/pull/146))
- *(ci)* Bump DavidAnson/markdownlint-cli2-action ([#148](https://github.com/konippi/servo-fetch/pull/148))
- *(ci)* Bump crate-ci/typos from 1.45.2 to 1.46.0 ([#149](https://github.com/konippi/servo-fetch/pull/149))

### Dependencies

- *(deps)* Bump rmcp from 1.5.0 to 1.6.0 ([#152](https://github.com/konippi/servo-fetch/pull/152))
- *(deps)* Bump rustls in the patch-updates group ([#151](https://github.com/konippi/servo-fetch/pull/151))

### Miscellaneous

- *(build)* Tune cargo profiles and linker config ([#153](https://github.com/konippi/servo-fetch/pull/153))
- Refresh tagline across README, crate docs, and Cargo.toml ([#145](https://github.com/konippi/servo-fetch/pull/145))

## [0.9.0](https://github.com/konippi/servo-fetch/compare/v0.8.1..v0.9.0) - 2026-05-09

### Features

- *(ci)* Publish signed multi-arch Docker image on release ([#138](https://github.com/konippi/servo-fetch/pull/138))
- *(cli)* Add `serve` HTTP API server ([#135](https://github.com/konippi/servo-fetch/pull/135))
- *(map)* Add URL discovery via sitemaps without rendering ([#125](https://github.com/konippi/servo-fetch/pull/125))

### Bug Fixes

- *(ci)* Stop silencing smoke failures and drop dead xvfb ([#136](https://github.com/konippi/servo-fetch/pull/136))
- *(robots)* Pass caller timeout to robots.txt fetch ([#129](https://github.com/konippi/servo-fetch/pull/129))
- *(ci)* Use !cancelled() instead of always() for gate job ([#128](https://github.com/konippi/servo-fetch/pull/128))

### Refactor

- *(net)* Introduce NetworkPolicy for address validation ([#132](https://github.com/konippi/servo-fetch/pull/132))
- Introduce PageFetcher trait and check-pattern tests for crawl ([#130](https://github.com/konippi/servo-fetch/pull/130))
- Extract scope and robots modules from crawl ([#126](https://github.com/konippi/servo-fetch/pull/126))

### Documentation

- Add map command to README and CLI reference ([#127](https://github.com/konippi/servo-fetch/pull/127))

### Testing

- *(e2e)* Migrate parallel tests to wiremock and share mock helper ([#134](https://github.com/konippi/servo-fetch/pull/134))
- *(e2e)* Add CLI and MCP E2E tests with wiremock ([#133](https://github.com/konippi/servo-fetch/pull/133))
- *(map)* Add run() integration tests with wiremock ([#131](https://github.com/konippi/servo-fetch/pull/131))

### CI

- *(ci)* Suppress zizmor cache-poisoning for sccache inline ([#142](https://github.com/konippi/servo-fetch/pull/142))

### Miscellaneous

- Streamline PR template with related-issues and docs sections ([#137](https://github.com/konippi/servo-fetch/pull/137))

## [0.8.1](https://github.com/konippi/servo-fetch/compare/v0.8.0..v0.8.1) - 2026-05-07

### Bug Fixes

- *(cli)* Reject empty --selector at argument parsing ([#120](https://github.com/konippi/servo-fetch/pull/120))
- *(extract)* Return error on invalid CSS selector instead of crashing ([#119](https://github.com/konippi/servo-fetch/pull/119))
- *(cli)* Flush stderr on progress clear to prevent output interleaving ([#118](https://github.com/konippi/servo-fetch/pull/118))

### Refactor

- *(bridge)* Use lazy evaluation for default user-agent fallback ([#121](https://github.com/konippi/servo-fetch/pull/121))

## [0.8.0](https://github.com/konippi/servo-fetch/compare/v0.7.1..v0.8.0) - 2026-05-07

### Features

- *(engine)* Add selector-aware extraction on Page ([#115](https://github.com/konippi/servo-fetch/pull/115))
- *(benchmarks)* Add Python harness with three-axis measurement and baseline ([#105](https://github.com/konippi/servo-fetch/pull/105))

### Bug Fixes

- *(crawl)* Apply robots.txt per RFC 9309 with per-request UA ([#113](https://github.com/konippi/servo-fetch/pull/113))
- *(runtime)* Memoize tokio runtime and fail-fast on async callers ([#111](https://github.com/konippi/servo-fetch/pull/111))
- *(sys)* Filter Apple Silicon OpenGL driver noise from stderr ([#110](https://github.com/konippi/servo-fetch/pull/110))

### Refactor

- *(engine)* [**breaking**] Remove unused CrawlStatus from public API ([#116](https://github.com/konippi/servo-fetch/pull/116))
- *(lib)* Restrict some modules to crate visibility ([#112](https://github.com/konippi/servo-fetch/pull/112))

### Documentation

- *(security)* Refresh supported versions policy ([#114](https://github.com/konippi/servo-fetch/pull/114))
- *(contributing)* Document benchmark workflow and refresh label taxonomy ([#109](https://github.com/konippi/servo-fetch/pull/109))
- *(benchmarks)* Add Python / uv / Ruff badges ([#107](https://github.com/konippi/servo-fetch/pull/107))
- Add servo-fetch gif ([#106](https://github.com/konippi/servo-fetch/pull/106))

### Testing

- *(cli)* Add mcp help smoke test ([#102](https://github.com/konippi/servo-fetch/pull/102))

### Miscellaneous

- *(labels)* Add type: test label for test-only changes ([#108](https://github.com/konippi/servo-fetch/pull/108))
- Add Contributor Covenant Code of Conduct ([#104](https://github.com/konippi/servo-fetch/pull/104))

## [0.7.1](https://github.com/konippi/servo-fetch/compare/v0.7.0..v0.7.1) - 2026-05-04

### Bug Fixes

- Recover page content on load timeout instead of returning empty ([#98](https://github.com/konippi/servo-fetch/pull/98))
- Enable servo experimental preferences for SPA compatibility ([#97](https://github.com/konippi/servo-fetch/pull/97))

### Documentation

- Update readmes ([#99](https://github.com/konippi/servo-fetch/pull/99))

## [0.7.0](https://github.com/konippi/servo-fetch/compare/v0.6.1..v0.7.0) - 2026-05-04

### Features

- Support custom User-Agent via CLI flag and library API ([#94](https://github.com/konippi/servo-fetch/pull/94))
- Model CrawlResult outcome as Result<CrawlPage, CrawlError> ([#93](https://github.com/konippi/servo-fetch/pull/93))
- Derive PartialEq and Eq for ConsoleMessage ([#85](https://github.com/konippi/servo-fetch/pull/85))
- Implement Display and as_str() for ConsoleLevel ([#84](https://github.com/konippi/servo-fetch/pull/84))

### Bug Fixes

- Move libc dependency to all platforms for _exit ([#96](https://github.com/konippi/servo-fetch/pull/96))
- Correct URL validation error routing and trailing-dot bypass ([#91](https://github.com/konippi/servo-fetch/pull/91))
- Initialize CryptoProvider in crawl_each ([#90](https://github.com/konippi/servo-fetch/pull/90))
- Use _exit on all platforms to avoid SpiderMonkey destructor race ([#89](https://github.com/konippi/servo-fetch/pull/89))
- Warn when --selector matches no elements ([#88](https://github.com/konippi/servo-fetch/pull/88))
- Remove duplicate URL validation in PDF probe ([#87](https://github.com/konippi/servo-fetch/pull/87))
- Respect --json flag in crawl output ([#86](https://github.com/konippi/servo-fetch/pull/86))

### Documentation

- Clarify optional JSON fields as omitted when not detected ([#92](https://github.com/konippi/servo-fetch/pull/92))

## [0.6.1](https://github.com/konippi/servo-fetch/compare/v0.6.0..v0.6.1) - 2026-05-03

### Bug Fixes

- *(ci)* Update LICENSE paths in release archive ([#83](https://github.com/konippi/servo-fetch/pull/83))
- Auto-initialize rustls CryptoProvider in library API ([#77](https://github.com/konippi/servo-fetch/pull/77))

### Documentation

- Add library examples ([#78](https://github.com/konippi/servo-fetch/pull/78))

### CI

- Add cargo-shear for unused dependency detection ([#75](https://github.com/konippi/servo-fetch/pull/75))

### Miscellaneous

- Dual-license under MIT OR Apache-2.0 ([#76](https://github.com/konippi/servo-fetch/pull/76))

## [0.6.0](https://github.com/konippi/servo-fetch/compare/v0.5.0..v0.6.0) - 2026-05-02

### Features

- Split into workspace with library and cli crates ([#73](https://github.com/konippi/servo-fetch/pull/73))

### Refactor

- *(main)* Thin dispatch with structured CLI args ([#71](https://github.com/konippi/servo-fetch/pull/71))

### CI

- Add declarative label sync ([#69](https://github.com/konippi/servo-fetch/pull/69))

## [0.5.0](https://github.com/konippi/servo-fetch/compare/v0.4.0..v0.5.0) - 2026-05-01

### Features

- Add crawl subcommand and mcp tool ([#66](https://github.com/konippi/servo-fetch/pull/66))

### Refactor

- *(mcp)* Restructure module layout and improve error handling ([#67](https://github.com/konippi/servo-fetch/pull/67))

### Miscellaneous

- Add resource benchmarks and improve README ([#65](https://github.com/konippi/servo-fetch/pull/65))

## [0.4.0](https://github.com/konippi/servo-fetch/compare/v0.3.0..v0.4.0) - 2026-04-30

### Features

- Multi-WebView parallel fetch and batch support (#63) ([#63](https://github.com/konippi/servo-fetch/pull/63))

### Documentation

- Fix tagline line wrapping ([#62](https://github.com/konippi/servo-fetch/pull/62))
- Rewrite README with clearer positioning and complete MCP tool reference ([#61](https://github.com/konippi/servo-fetch/pull/61))

## [0.3.0](https://github.com/konippi/servo-fetch/compare/v0.2.2..v0.3.0) - 2026-04-29

### Features

- Add full-page screenshot support ([#57](https://github.com/konippi/servo-fetch/pull/57))

### Bug Fixes

- *(bridge)* Wait for document.readyState after LoadStatus::Complete ([#52](https://github.com/konippi/servo-fetch/pull/52))

### Refactor

- Extract screenshot module from bridge ([#58](https://github.com/konippi/servo-fetch/pull/58))
- Consolidate fetch parameters and tighten public docs ([#53](https://github.com/konippi/servo-fetch/pull/53))

### Documentation

- Document --full-page screenshot option ([#59](https://github.com/konippi/servo-fetch/pull/59))

### Miscellaneous

- Update .gitignore ([#56](https://github.com/konippi/servo-fetch/pull/56))
- Add recommended VS Code extensions ([#55](https://github.com/konippi/servo-fetch/pull/55))
- Add rust-src component to rust-toolchain.toml ([#54](https://github.com/konippi/servo-fetch/pull/54))

## [0.2.2](https://github.com/konippi/servo-fetch/compare/v0.2.1..v0.2.2) - 2026-04-28

### Bug Fixes

- *(bridge)* Use UserStyleSheet::new for noise removal stylesheet ([#41](https://github.com/konippi/servo-fetch/pull/41))
- *(release)* Pass --repo to gh release edit so checkout is not required ([#40](https://github.com/konippi/servo-fetch/pull/40))

### Performance

- *(ci)* Parallelize clippy/test, adopt nextest and fast ci profile ([#50](https://github.com/konippi/servo-fetch/pull/50))

### Documentation

- Refresh issue/PR templates and SECURITY ([#43](https://github.com/konippi/servo-fetch/pull/43))
- *(README)* Restructure sections and collapse platform-specific install ([#42](https://github.com/konippi/servo-fetch/pull/42))

### CI

- *(ci)* Bump crate-ci/typos from 1.45.1 to 1.45.2 ([#45](https://github.com/konippi/servo-fetch/pull/45))
- *(ci)* Bump dorny/paths-filter from 3.0.2 to 4.0.1 ([#44](https://github.com/konippi/servo-fetch/pull/44))
- *(ci)* Add 7-day cooldown to dependabot updates ([#47](https://github.com/konippi/servo-fetch/pull/47))

### Miscellaneous

- Tighten project configs and migrate lints to Cargo.toml ([#49](https://github.com/konippi/servo-fetch/pull/49))

## [0.2.1](https://github.com/konippi/servo-fetch/compare/v0.2.0..v0.2.1) - 2026-04-27

### Bug Fixes

- *(ci)* Tighten ci-passed gate against silent skipped jobs ([#33](https://github.com/konippi/servo-fetch/pull/33))
- *(release)* Set shell bash on Build release and Strip binary ([#39](https://github.com/konippi/servo-fetch/pull/39))
- *(release)* Publish crate before marking release as non-draft ([#36](https://github.com/konippi/servo-fetch/pull/36))
- *(linux)* Skip atexit on shutdown to avoid SpiderMonkey mutex race ([#37](https://github.com/konippi/servo-fetch/pull/37))
- *(ci)* Guard XVFB array expansion for macOS bash 3.2 ([#38](https://github.com/konippi/servo-fetch/pull/38))
- *(ci)* Install llvm for mozjs_sys aarch64 Linux source build ([#31](https://github.com/konippi/servo-fetch/pull/31))
- *(release)* Bundle ANGLE DLLs on Windows, add aarch64-linux, document Linux runtime ([#25](https://github.com/konippi/servo-fetch/pull/25))
- Install rustls CryptoProvider to prevent startup crash ([#23](https://github.com/konippi/servo-fetch/pull/23))

### Documentation

- Use tree URL for npx skills add to avoid full repo clone ([#22](https://github.com/konippi/servo-fetch/pull/22))
- Update Agent Skills to match implementation and add npx install ([#21](https://github.com/konippi/servo-fetch/pull/21))

### CI

- Add actionlint and zizmor workflow linters ([#35](https://github.com/konippi/servo-fetch/pull/35))
- Workflow hardening (permissions, env-based templates, toolchain pin) ([#30](https://github.com/konippi/servo-fetch/pull/30))
- Add smoke-test workflow with L1/L2/L3 tiered scenarios ([#29](https://github.com/konippi/servo-fetch/pull/29))
- Print stderr on scenario failure ([#28](https://github.com/konippi/servo-fetch/pull/28))
- Capture stderr in release-verify scenarios ([#27](https://github.com/konippi/servo-fetch/pull/27))
- *(dev)* Release-verify workflow for manual dispatch ([#26](https://github.com/konippi/servo-fetch/pull/26))

### Miscellaneous

- Tighten lint config and remove unused committed.toml ([#20](https://github.com/konippi/servo-fetch/pull/20))

## [0.2.0](https://github.com/konippi/servo-fetch/compare/v0.1.0..v0.2.0) - 2026-04-26

### Features

- Add accessibility tree, user stylesheets, and console capture ([#15](https://github.com/konippi/servo-fetch/pull/15))

### Refactor

- Improve type safety and test coverage ([#17](https://github.com/konippi/servo-fetch/pull/17))

### Documentation

- Simplify Why section in README ([#18](https://github.com/konippi/servo-fetch/pull/18))
- Update all files for accessibility tree, user stylesheets, and console capture ([#16](https://github.com/konippi/servo-fetch/pull/16))
- Rebrand README around embedded browser engine positioning ([#13](https://github.com/konippi/servo-fetch/pull/13))

### CI

- Migrate crates.io publish to trusted publishing (OIDC) ([#12](https://github.com/konippi/servo-fetch/pull/12))
- Skip verify on cargo publish to avoid redundant Servo rebuild ([#10](https://github.com/konippi/servo-fetch/pull/10))

### Miscellaneous

- Align Cargo.toml description with README ([#14](https://github.com/konippi/servo-fetch/pull/14))
- Rename workflows to .yml and remove pre-commit config ([#11](https://github.com/konippi/servo-fetch/pull/11))

## [0.1.0] - 2026-04-26

### Bug Fixes

- Harden input validation and document security limitations ([#7](https://github.com/konippi/servo-fetch/pull/7))

### Documentation

- Update README install section and migrate to macos-15-intel runner ([#8](https://github.com/konippi/servo-fetch/pull/8))
- Sync README with implemented features and fix CLI value names ([#5](https://github.com/konippi/servo-fetch/pull/5))

### Build

- *(deps)* Bump libc in the patch-updates group across 1 directory ([#2](https://github.com/konippi/servo-fetch/pull/2))
- *(deps)* Bump taiki-e/install-action from 2.42.4 to 2.75.21 ([#1](https://github.com/konippi/servo-fetch/pull/1))

### CI

- Skip expensive builds for docs-only changes ([#9](https://github.com/konippi/servo-fetch/pull/9))
- Harden release workflow with attestation ([#6](https://github.com/konippi/servo-fetch/pull/6))
- Remove flaky integration job and increase build timeout
- Replace rust-cache with sccache for faster builds
- Add explicit toolchain version to rust-toolchain action

### Styling

- Apply rustfmt formatting
<!-- generated by git-cliff -->
