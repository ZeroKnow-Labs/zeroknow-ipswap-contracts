#!/usr/bin/env bash
set -e

# Available contracts in the workspace
CONTRACTS=("ip_registry" "atomic_swap" "zk_verifier")

# Build all contracts by default
TARGET="${1:-all}"

# Function to build a specific contract
build_contract() {
    local contract="$1"
    local path="contracts/${contract}"
    
    if [ ! -d "$path" ]; then
        echo "Error: Contract '${contract}' not found in workspace."
        exit 1
    fi
    
    echo "Building ${contract}..."
    cargo build --target wasm32-unknown-unknown --release -p "${contract}"
    echo "${contract} build complete."
}

# Validate target contract if not "all"
if [ "$TARGET" != "all" ]; then
    valid=false
    for contract in "${CONTRACTS[@]}"; do
        if [ "$TARGET" == "$contract" ]; then
            valid=true
            break
        fi
    done
    
    if [ "$valid" == "false" ]; then
        echo "Error: Unknown contract '${TARGET}'"
        echo "Available contracts: ${CONTRACTS[*]}"
        exit 1
    fi
fi

# Build contracts
if [ "$TARGET" == "all" ]; then
    echo "Building all contracts..."
    cargo build --target wasm32-unknown-unknown --release
    echo "All contracts built successfully."
else
    build_contract "$TARGET"
fi
