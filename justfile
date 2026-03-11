test-all:
    cargo check --workspace
    cargo test
    cargo test --features loom
