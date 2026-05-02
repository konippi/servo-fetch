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

## Use of AI

AI tools (e.g. Claude Code, Kiro) can be useful for generating code. However, you remain responsible for any code you publish, and we are responsible for any code we merge and release. A few expectations:

- **Human in the loop.** Do not submit pull requests created autonomously by AI agents. We will close any PR we believe was created without a human author who understands the change.
- **Write PR descriptions and replies yourself.** Describe the change and reply to review comments in your own words. Do not paste AI output as a reply to maintainers. We may hide comments we believe are AI-generated.
- **Disclose AI context when you quote it.** If you paste output from an AI tool into an issue or PR, put it in a `>` quote block and add your own commentary explaining why it is relevant.

## Issue labels

We use two label axes plus status:

- **`type: *`** — what kind of work, aligned with [Conventional Commits](https://www.conventionalcommits.org/): `type: bug`, `type: feature`, `type: docs`, `type: refactor`, `type: perf`, `type: deps`, `type: ci`, `type: build`, `type: security`
- **surface** — which user-facing interface: `cli`, `mcp`, `skill`
- **status** — workflow state: `good first issue`, `help wanted`, `needs triage`, `needs info`

Start with [`good first issue`](https://github.com/konippi/servo-fetch/labels/good%20first%20issue) if you are new to the project.

Labels are declared in [`.github/labels.yml`](./.github/labels.yml) and synced by the `Sync Labels` workflow. To propose a new label, open a PR editing that file.

## Reporting bugs

Please use the [bug report template](https://github.com/konippi/servo-fetch/issues/new?template=bug_report.yml) and include:

- Steps to reproduce
- Expected vs actual behavior
- Output of `servo-fetch --version`
- OS info

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
