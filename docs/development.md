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

CI runs static checks, build and tests on macOS, Linux and Windows. Local checks do not establish hosted CI success. No publication or release workflow is installed.
