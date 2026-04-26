# Contributing to servo-fetch

Thank you for considering contributing to servo-fetch.

If your contribution is not straightforward, please open an issue first to discuss the change before submitting a PR.

## Development setup

Requires Rust **1.86.0+** (see `rust-version` in Cargo.toml).

```sh
git clone https://github.com/konippi/servo-fetch
cd servo-fetch
cargo build
```

> First build takes several minutes due to Servo compilation.

### Useful commands

```sh
cargo run -- "https://example.com"                          # Markdown output
cargo run -- "https://example.com" --json                   # JSON output
cargo run -- "https://example.com" --screenshot page.png    # Screenshot
cargo run -- "https://example.com" --js "document.title"    # JS execution
cargo test                                                  # Run tests
cargo test -- --ignored                                     # Run Servo+network tests (slow)
cargo clippy                                                # Lint (pedantic)
cargo fmt                                                   # Format
cargo deny check                                            # License & advisory check
typos                                                       # Spell check
```

### Coverage

```sh
cargo install cargo-llvm-cov
cargo llvm-cov --lib --tests
```

## Commit conventions

This project uses [Conventional Commits](https://www.conventionalcommits.org/).

```text
feat: add PDF output support
fix: handle empty body in extract
refactor: simplify bridge error handling
```

## Pull request guidelines

- Keep PRs focused on a single change
- Ensure `cargo clippy`, `cargo fmt --check`, and `cargo test` pass with zero warnings
- Run `cargo test -- --ignored` if your change affects Servo integration or network behavior
- Update documentation if behavior changes

## Reporting bugs

Please use the [bug report template](https://github.com/konippi/servo-fetch/issues/new?template=bug_report.md) and include:

- Steps to reproduce
- Expected vs actual behavior
- Output of `servo-fetch --version`
- OS info

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
