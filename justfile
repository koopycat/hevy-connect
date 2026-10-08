# Canonical development and release tasks for hevy-axi.

# Show the available recipes.
default:
    @just --list

# Build the optimized single native binary at target/release/hevy-axi.
build:
    cargo build --locked --release

# Run the CLI from source.
run *ARGS:
    cargo run --locked --quiet -- {{ ARGS }}

# Run all unit and integration tests.
test:
    cargo test --locked --all-targets

# Run Clippy with warnings denied.
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Format Rust and the justfile.
format:
    just --fmt
    cargo fmt

# Verify Rust and justfile formatting without changing files.
format-check:
    just --fmt --check
    cargo fmt --check

# Refresh the Hevy OpenAPI capture from the live docs (network).
api-sync:
    cargo run --locked --example api-sync

# Report whether the committed capture differs from the live docs (network).
api-sync-check:
    cargo run --locked --example api-sync -- --check

# Install the release binary in the user-local bin directory.
install: build
    mkdir -p "${HOME}/.local/bin"
    install -m 0755 target/release/hevy-axi "${HOME}/.local/bin/hevy-axi"

# Remove Cargo build artifacts.
clean:
    cargo clean

# Run the complete local validation suite.
check: format-check lint test build
