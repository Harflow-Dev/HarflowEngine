# Development

Run from this repository, independently of sibling checkouts.

```sh
cargo fmt --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/check-architecture.py
python3 scripts/check-legacy-names.py
python3 -m unittest discover -s tests
cargo run --locked -p hf-host --bin harflow-engine-scaffold
```

## Shared checkout across macOS accounts

The workspace can be used from more than one macOS account. Home directories, toolchains and permissions are per account, and a `target/` directory created by another account must not be reused: `libsqlite3-sys`'s build script fails with `EPERM`, and the failed copy can truncate a file in that account's cache. Use a per-account target directory and `CARGO_HOME`, and limit build jobs so builds can run alongside other work:

```sh
export CARGO_TARGET_DIR="$WORK_DIR/target"
export CARGO_HOME="$WORK_DIR/cargo-home"
export CARGO_BUILD_JOBS=2
```

`WORK_DIR` is any writable directory outside the repository. On an account with its own Rust toolchain, nothing else is needed. Otherwise, without modifying the account that owns the toolchain, read its installation and work offline from a copy of its registry:

```sh
export RUSTUP_HOME="$TOOLCHAIN_ACCOUNT_HOME/.rustup"
export PATH="$TOOLCHAIN_ACCOUNT_HOME/.cargo/bin:$PATH"
export CARGO_NET_OFFLINE=true
mkdir -p "$CARGO_HOME"
cp -R "$TOOLCHAIN_ACCOUNT_HOME/.cargo/registry" "$CARGO_HOME/registry"
```

Yield during full strict Mac runs when a coordinator asks. Keep durable evidence out of `/private/tmp`.

CI runs static checks, build and tests on macOS, Linux and Windows. Local checks do not establish hosted CI success. No publication or release workflow is installed.
