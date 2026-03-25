#!/usr/bin/env bash
set -e
cargo build --target wasm32-unknown-unknown --release

echo "Optimizing WASM artifacts..."
for wasm in target/wasm32-unknown-unknown/release/ip_registry.wasm \
            target/wasm32-unknown-unknown/release/atomic_swap.wasm \
            target/wasm32-unknown-unknown/release/zk_verifier.wasm; do
  if [[ -f "$wasm" ]]; then
    stellar contract optimize --wasm "$wasm"
    echo "  Optimized: $wasm"
  fi
done

echo "Build complete."
