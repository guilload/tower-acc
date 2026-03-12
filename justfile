fix-all:
    cargo fmt --all
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty --allow-staged

test-all:
    cargo check --workspace --all-features
    cargo test --all-features
