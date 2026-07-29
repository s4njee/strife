# Linux Cross-Compilation

Strife's primary runtime is ARM64 Gentoo Linux, and x86-64 Linux is also supported. Native builds on each Linux architecture are the reference validation path.

## Native Gentoo ARM64

Install a stable Rust toolchain with `rustfmt` and `clippy`, then run:

```sh
cargo fmt --check
cargo clippy-all
cargo check-all
cargo build --workspace --target aarch64-unknown-linux-gnu
```

## Native x86-64 Linux

Install the same Rust components, then run:

```sh
cargo fmt --check
cargo clippy-all
cargo check-all
cargo build --workspace --target x86_64-unknown-linux-gnu
```

## Cross-Building from macOS

The Rust standard-library targets alone are insufficient because Linux linking also needs a target C toolchain. Install the Rust targets and appropriate GNU cross-linkers, configure Cargo's target linker entries locally, then use the workspace aliases:

```sh
rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu
cargo build-arm64-linux
cargo build-x86-64-linux
```

Alternatively, use `cross`, which supplies containerized target toolchains:

```sh
cargo install cross --locked
cross build --workspace --target aarch64-unknown-linux-gnu
cross build --workspace --target x86_64-unknown-linux-gnu
```

The CI workflow validates x86-64 Linux. ARM64 native validation runs on the Raspberry Pi until an ARM64 CI runner is configured.
