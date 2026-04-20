# SolSwap

An on-chain Automated Market Maker (AMM) built on Solana using the Anchor framework. SolSwap implements the constant-product invariant (`x * y = k`) with LP token minting/burning, configurable protocol fees, and slippage protection.

## AMM Math

SolSwap uses the constant-product formula pioneered by Uniswap v2:

```
x * y = k
```

Where `x` and `y` are the reserves of token A and token B respectively, and `k` is a constant that must not decrease after any swap.

### Swap Output Formula

Given an input amount `Δx` (after fee deduction):

```
Δy = y * Δx_fee / (x + Δx_fee)

where:
  Δx_fee = Δx * (10_000 - fee_bps) / 10_000
```

Example (0.3% fee, reserves A=1000, B=2000, swap 100 A):
```
Δx_fee = 100 * 9970 / 10000 = 99.7
Δy     = 2000 * 99.7 / (1000 + 99.7) ≈ 181.4 B
```

### LP Token Minting

On the **first deposit**, LP tokens are minted equal to the geometric mean of the deposit amounts:
```
LP = sqrt(amount_a * amount_b)
```

On **subsequent deposits**, tokens are accepted proportionally to the current reserve ratio, and LP tokens are minted proportionally:
```
LP = min(amount_a / reserve_a, amount_b / reserve_b) * total_lp_supply
```

### LP Token Redemption

Burning `lp_burn` LP tokens returns:
```
amount_a = lp_burn * reserve_a / total_lp_supply
amount_b = lp_burn * reserve_b / total_lp_supply
```

## Architecture

```
sol_swap/
├── programs/
│   └── sol_swap/
│       └── src/
│           ├── lib.rs        # Program entry point, instructions
│           ├── state.rs      # Pool and UserPosition account definitions
│           └── errors.rs     # Custom error codes
├── tests/
│   └── sol_swap.ts           # TypeScript integration tests
├── Anchor.toml               # Anchor workspace config
├── Cargo.toml                # Rust workspace
├── package.json              # Node.js deps
└── tsconfig.json
```

### Key Accounts

| Account | Type | Description |
|---------|------|-------------|
| `Pool` | PDA `[b"pool", mint_a, mint_b]` | Stores reserves, fee config, LP mint reference |
| `UserPosition` | PDA `[b"position", pool, user]` | Tracks per-user LP holdings |
| `pool_authority` | PDA `[b"authority", mint_a, mint_b]` | Signs CPI calls; owns reserves and LP mint |

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) >= 1.75
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) >= 1.18
- [Anchor CLI](https://www.anchor-lang.com/docs/installation) >= 0.29
- Node.js >= 18 and Yarn

```bash
# Install Anchor CLI
cargo install --git https://github.com/coral-xyz/anchor avm --locked --force
avm install 0.29.0
avm use 0.29.0
```

## Quickstart

```bash
# 1. Clone and install JS deps
git clone https://github.com/nikhil-ghind/SolSwap
cd SolSwap
yarn install

# 2. Build the program
anchor build

# 3. Run integration tests against a local validator
anchor test
```

## Program Instructions

### `initialize_pool(fee_bps: u64)`

Creates a new AMM pool for two SPL token mints.

| Parameter | Type | Description |
|-----------|------|-------------|
| `fee_bps` | `u64` | Protocol fee in basis points (30 = 0.30%, max 10000) |

Required accounts: `payer`, `token_mint_a`, `token_mint_b`, `pool`, `pool_authority`, `lp_mint`, `token_a_reserve`, `token_b_reserve`

### `add_liquidity(amount_a, amount_b, min_lp)`

Deposit tokens A and B; receive LP tokens proportionally.

| Parameter | Type | Description |
|-----------|------|-------------|
| `amount_a` | `u64` | Maximum token A to deposit |
| `amount_b` | `u64` | Maximum token B to deposit |
| `min_lp` | `u64` | Minimum LP tokens to receive (slippage guard) |

### `remove_liquidity(lp_amount, min_a, min_b)`

Burn LP tokens and withdraw underlying reserves.

| Parameter | Type | Description |
|-----------|------|-------------|
| `lp_amount` | `u64` | LP tokens to burn |
| `min_a` | `u64` | Minimum token A to receive |
| `min_b` | `u64` | Minimum token B to receive |

### `swap(amount_in, min_amount_out, a_to_b)`

Execute a constant-product swap.

| Parameter | Type | Description |
|-----------|------|-------------|
| `amount_in` | `u64` | Exact input amount |
| `min_amount_out` | `u64` | Minimum output (slippage protection) |
| `a_to_b` | `bool` | `true` = A→B, `false` = B→A |

## Events

| Event | Fields | Emitted by |
|-------|--------|------------|
| `PoolInitialized` | pool, mint_a, mint_b, lp_mint, fee_bps | `initialize_pool` |
| `LiquidityAdded` | pool, user, amount_a, amount_b, lp_minted | `add_liquidity` |
| `LiquidityRemoved` | pool, user, lp_burned, amount_a, amount_b | `remove_liquidity` |
| `SwapEvent` | pool, user, amount_in, amount_out, a_to_b | `swap` |

## Deployment to Devnet

```bash
# 1. Configure CLI for devnet
solana config set --url devnet

# 2. Fund your wallet
solana airdrop 2

# 3. Build with devnet cluster
anchor build

# 4. Get the deployed program ID
solana address -k target/deploy/sol_swap-keypair.json

# 5. Update declare_id! in lib.rs and [programs.devnet] in Anchor.toml with that address

# 6. Deploy
anchor deploy --provider.cluster devnet

# 7. Run tests against devnet
anchor test --provider.cluster devnet
```

## Security Notes

- All arithmetic uses checked operations; overflow reverts the transaction.
- Slippage guards (`min_lp`, `min_a`, `min_b`, `min_amount_out`) protect users from sandwich attacks and price movements.
- The pool authority PDA is the sole signer for reserve transfers and LP minting, preventing unauthorized withdrawals.
- Identical mint addresses are rejected at pool initialization.

## License

MIT
