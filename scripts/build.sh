#!/usr/bin/env bash
set -e

echo "Building contracts..."
# wasm32v1-none is the Soroban target: core wasm 1.0 only, which the Soroban
# host validator accepts. wasm32-unknown-unknown on Rust 1.82+ emits
# reference-types/multi-value instructions that are rejected at deployment.
cargo build --target wasm32v1-none --release
echo "Build complete."
