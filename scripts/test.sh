#!/usr/bin/env bash
set -e

echo "Building release WASM artifacts..."
cargo build --target wasm32-unknown-unknown --release

echo "Running unit tests..."
cargo test

echo "Running E2E tests against the release WASM..."
cargo test -p e2e-tests

echo "All tests passed."
