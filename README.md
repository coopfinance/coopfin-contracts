# coopfin-contracts

Soroban smart contracts powering the CoopFinance cooperative finance platform.

[![Build](https://github.com/coopfinance/coopfin-contracts/actions/workflows/test.yml/badge.svg)](https://github.com/coopfinance/coopfin-contracts/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Stellar Wave](https://img.shields.io/badge/Stellar-Wave%20Program-blue)](https://drips.network/wave/stellar)

## Contracts

| Contract | Description | Status |
|----------|-------------|--------|
| `treasury` | Group wallet — contributions, withdrawals, balance tracking | ✅ Testnet |
| `loan` | Loan request, disbursement, and repayment lifecycle | ✅ Testnet |
| `voting` | On-chain proposals and member voting | ✅ Testnet |
| `governance` | Cooperative rules engine (interest rates, quorum, etc.) | ✅ Testnet |
| `dividend` | Proportional profit distribution to members | ✅ Testnet |

## Prerequisites

- Rust `>=1.74` with `wasm32-unknown-unknown` target
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/install-stellar-cli) `>=21.0`

```bash
# Install Rust target
rustup target add wasm32-unknown-unknown

# Install Stellar CLI
cargo install --locked stellar-cli --features opt
```

## Setup

```bash
git clone https://github.com/coopfinance/coopfin-contracts
cd coopfin-contracts
```

## Build

```bash
# Build all contracts
stellar contract build

# Or with cargo
cargo build --target wasm32-unknown-unknown --release
```

## Test

```bash
# Run all tests
cargo test

# Run tests for a specific contract
cargo test -p coopfin-treasury
cargo test -p coopfin-loan
cargo test -p coopfin-voting
```

## Deploy to Testnet

```bash
# Fund a deployer account
stellar keys generate deployer
stellar keys fund deployer --network testnet

# Run deploy script
chmod +x scripts/deploy.sh
./scripts/deploy.sh testnet
```

## Contract Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     GovernanceContract                          │
│  Rules: min_contribution, loan_multiplier, quorum, interest     │
└──────────────┬──────────────────────┬───────────────────────────┘
               │                      │
    ┌──────────▼──────────┐  ┌────────▼────────┐
    │  TreasuryContract   │  │  VotingContract  │
    │  - add_member       │  │  - create_proposal│
    │  - contribute       │  │  - vote           │
    │  - withdraw         │  │  - finalize       │
    └──────────┬──────────┘  └────────┬──────────┘
               │                      │
    ┌──────────▼──────────┐  ┌────────▼────────┐
    │    LoanContract     │  │ DividendContract │
    │  - request_loan     │  │  - distribute    │
    │  - approve_loan     │  │  (proportional)  │
    │  - repay            │  └─────────────────┘
    └─────────────────────┘
```

## Contract Addresses (Testnet)

> Deploy with `./scripts/deploy.sh testnet` and addresses are written to `deployments/testnet.json`.

## Contributing

This repo participates in the [Stellar Drips Wave](https://drips.network/wave/stellar) program.
Merged PRs may earn XLM rewards. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Security

- All contracts use `require_auth()` on every state-changing function
- No reentrancy risk (Soroban execution model is single-threaded)
- Formal audit planned for mainnet launch

## License

MIT
