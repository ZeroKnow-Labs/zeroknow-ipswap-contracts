# Atomic IP Marketplace — Contracts

Soroban smart contracts for atomic IP swaps on Stellar.

## Contracts

- **`contracts/atomic_swap`** — Atomic swaps with USDC, fee handling, pause/cancel.
- **`contracts/ip_registry`** — On-chain IP asset registration with TTL.
- **`contracts/zk_verifier`** — Merkle-tree ZK proof verification with TTL.

## Prerequisites

- Rust (stable) with `wasm32-unknown-unknown` target
- Stellar CLI: `cargo install --locked stellar-cli --features opt`

## Build

```bash
./scripts/build.sh          # all contracts
./scripts/build.sh atomic_swap   # single contract
```

## Test

```bash
./scripts/test.sh
# runs: cargo test --locked --workspace
```

## Deploy (Testnet)

```bash
cp .env.example .env   # fill in keys / network vars
./scripts/deploy_testnet.sh
```

## Environment Variables

| Variable | Description |
|---|---|
| `STELLAR_NETWORK` | `testnet` / `mainnet` / `local` |
| `STELLAR_RPC_URL` | Soroban RPC endpoint |
| `ATOMIC_SWAP_ADMIN` | Admin address |
| `ATOMIC_SWAP_FEE_RECIPIENT` | Fee recipient address |
| `ATOMIC_SWAP_FEE_BPS` | Fee in basis points |
| `ATOMIC_SWAP_CANCEL_DELAY_SECS` | Cancel delay in seconds |

## License

Apache License 2.0 — see [LICENSE](./LICENSE).
