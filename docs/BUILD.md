# Build

## Prerequisites

- Rust stable with `rustfmt` and `clippy` components.
- Windows SDK and Visual Studio C++ build tools for future Windows crates.
- Docker Desktop for future control-plane integration tests.

## Verification commands

```text
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Verified baseline

On 2026-09-06, the repository passed:

- `cargo fmt --all -- --check`
- `cargo test --workspace --locked` (12 unit tests and all doc-test targets)
- `cargo clippy --workspace --all-targets --locked -- -D warnings`

The verification used Rust 1.98.1 with the MSVC 14.44 toolset and Windows SDK
10.0.26100.0.

## Dependency policy

Keep cryptographic and media dependencies pinned through the lockfile once the
first dependency resolution is run. Review licenses and security advisories
before accepting new dependencies.
