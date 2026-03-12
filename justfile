fix-all:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

test-all:
    cargo check --workspace
    cargo test
    cargo test --features loom
