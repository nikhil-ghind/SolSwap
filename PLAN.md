# Sol Swap

## Project Overview

An on-chain automated market maker (AMM) implemented as a Solana program using the Anchor framework. Implements constant-product pricing (x*y=k), liquidity pool management with LP token minting/burning, configurable protocol fee collection, and swap execution. A TypeScript client SDK and integration test suite exercise all program instructions.

**Key Goals:**
- Constant-product (x*y=k) pricing curve for token swaps
- Liquidity pool creation with arbitrary SPL token pairs
- LP token minting on deposit and burning on withdrawal (proportional share)
- Protocol fee collection on swaps (configurable basis points)
- Slippage protection for swaps and liquidity operations
- TypeScript client for integration testing and SDK usage

## Tech Stack

- Rust (2021 edition)
- Anchor Framework 0.30+
- Solana CLI 1.18+
- SPL Token Program / Token-2022
- TypeScript 5.x
- @solana/web3.js 1.9+
- @coral-xyz/anchor (TypeScript client)
- Mocha + Chai (TypeScript tests)
- solana-test-validator (local testing)

## Architecture Overview

```
[Solana Program (on-chain)]
  |
  +-- InitializePool instruction
  |     Creates pool state, token vaults, LP mint
  |
  +-- AddLiquidity instruction
  |     Deposits token A + B, mints LP tokens
  |
  +-- RemoveLiquidity instruction
  |     Burns LP tokens, withdraws token A + B
  |
  +-- Swap instruction
  |     Swaps token A <-> B using x*y=k formula
  |
  +-- CollectFees instruction
        Withdraws accumulated protocol fees

[TypeScript Client SDK]
  |
  +-- AmmClient class wrapping all instructions
  +-- Integration tests (Mocha)
```

**Key Accounts:**
- `Pool` - PDA storing pool state (token mints, vault addresses, fee config, invariant k)
- `VaultA` / `VaultB` - Token accounts holding pooled liquidity (owned by pool PDA)
- `LPMint` - SPL mint for LP tokens (mint authority = pool PDA)
- `FeeVaultA` / `FeeVaultB` - Token accounts accumulating protocol fees

---

## Phase 1: Project Scaffolding and Pool Initialization

**Goal:** Set up the Anchor project and implement the pool initialization instruction.

### Tasks

1. Initialize Anchor project:
   - Command: `anchor init solana-amm --template=multiple`
   - This creates: `Anchor.toml`, `programs/solana-amm/`, `tests/`, `migrations/`.

2. Configure `Anchor.toml`:
   - Set `[programs.localnet]` program ID.
   - Set `[provider]` cluster to localnet, wallet path.
   - Set `[scripts]` test command: `yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts`.

3. Create `programs/solana-amm/src/lib.rs`:
   - Declare program ID with `declare_id!()`.
   - Define module with `#[program]` attribute.
   - Implement `initialize_pool` instruction:
     - Creates `Pool` PDA account (seeds: [b"pool", token_a_mint, token_b_mint]).
     - Creates `VaultA` and `VaultB` token accounts (owned by pool PDA).
     - Creates `LPMint` (mint authority = pool PDA, decimals = 6).
     - Creates `FeeVaultA` and `FeeVaultB` token accounts.
     - Sets initial pool state: fee_bps (e.g., 30 = 0.30%), protocol_fee_bps, authority.

4. Create `programs/solana-amm/src/state.rs`:
   - `#[account] pub struct Pool`:
     - `token_a_mint: Pubkey`
     - `token_b_mint: Pubkey`
     - `vault_a: Pubkey`
     - `vault_b: Pubkey`
     - `lp_mint: Pubkey`
     - `fee_vault_a: Pubkey`
     - `fee_vault_b: Pubkey`
     - `fee_bps: u16` (swap fee in basis points)
     - `protocol_fee_bps: u16` (portion of fee going to protocol)
     - `authority: Pubkey` (admin who can collect fees, update config)
     - `bump: u8` (PDA bump seed)
     - `total_lp_supply: u64` (tracked on-chain for proportional math)

5. Create `programs/solana-amm/src/instructions/mod.rs`:
   - Re-export instruction modules: initialize_pool, add_liquidity, remove_liquidity, swap, collect_fees.

6. Create `programs/solana-amm/src/instructions/initialize_pool.rs`:
   - `#[derive(Accounts)] pub struct InitializePool<'info>`:
     - `#[account(init, payer = authority, space = 8 + Pool::LEN, seeds = [...], bump)] pool: Account<'info, Pool>`
     - `#[account(init, ...)] vault_a: Account<'info, TokenAccount>`
     - `#[account(init, ...)] vault_b: Account<'info, TokenAccount>`
     - `#[account(init, ...)] lp_mint: Account<'info, Mint>`
     - `fee_vault_a`, `fee_vault_b` token accounts
     - `token_a_mint`, `token_b_mint`: Account<'info, Mint>
     - `authority: Signer<'info>`
     - `system_program`, `token_program`, `rent`
   - Handler function: validate mints are different, initialize Pool fields.

7. Create `programs/solana-amm/src/errors.rs`:
   - `#[error_code] pub enum AmmError`:
     - `InvalidFee` - fee_bps > 10000
     - `SameMint` - token_a_mint == token_b_mint
     - `SlippageExceeded` - output less than minimum
     - `InsufficientLiquidity` - pool has zero liquidity
     - `ZeroAmount` - deposit/swap amount is zero
     - `MathOverflow` - arithmetic overflow

8. Build and verify:
   - Command: `anchor build`
   - Command: `anchor keys list` (note program ID, update declare_id and Anchor.toml)

---

## Phase 2: Add Liquidity

**Goal:** Implement the add_liquidity instruction that deposits tokens and mints LP tokens.

### Tasks

1. Create `programs/solana-amm/src/instructions/add_liquidity.rs`:
   - `#[derive(Accounts)] pub struct AddLiquidity<'info>`:
     - `#[account(mut, seeds = [...], bump = pool.bump)] pool`
     - `#[account(mut)] vault_a`, `#[account(mut)] vault_b`
     - `#[account(mut)] lp_mint`
     - `#[account(mut)] user_token_a` (user's token A account)
     - `#[account(mut)] user_token_b` (user's token B account)
     - `#[account(mut)] user_lp` (user's LP token account)
     - `user: Signer<'info>`
     - `token_program`
   - Handler: `add_liquidity(ctx, amount_a: u64, amount_b: u64, min_lp_tokens: u64)`:
     - If pool is empty (first deposit): LP tokens minted = sqrt(amount_a * amount_b) (use integer sqrt). Lock minimum liquidity (MINIMUM_LIQUIDITY = 1000 lamports burned to zero address).
     - If pool has liquidity: compute LP tokens = min(amount_a * total_lp / vault_a_balance, amount_b * total_lp / vault_b_balance). This enforces proportional deposits.
     - Check lp_tokens >= min_lp_tokens (slippage protection).
     - Transfer amount_a from user to vault_a, amount_b from user to vault_b.
     - Mint lp_tokens to user_lp using pool PDA as mint authority (CPI to token program).
     - Update pool.total_lp_supply.

2. Create `programs/solana-amm/src/math.rs`:
   - Function: `pub fn integer_sqrt(n: u128) -> u64` - Newton's method integer square root.
   - Function: `pub fn checked_mul_div(a: u64, b: u64, c: u64) -> Result<u64>` - (a * b) / c with u128 intermediate to prevent overflow.

3. Write unit test (Rust):
   - In `programs/solana-amm/src/math.rs` add `#[cfg(test)] mod tests`:
     - Test integer_sqrt with known values (0, 1, 4, 9, 10000).
     - Test checked_mul_div with known values and edge cases.

4. Build: `anchor build`.

---

## Phase 3: Remove Liquidity

**Goal:** Implement the remove_liquidity instruction that burns LP tokens and returns deposited tokens.

### Tasks

1. Create `programs/solana-amm/src/instructions/remove_liquidity.rs`:
   - `#[derive(Accounts)] pub struct RemoveLiquidity<'info>`:
     - Same account structure as AddLiquidity (pool, vaults, mints, user accounts).
   - Handler: `remove_liquidity(ctx, lp_amount: u64, min_amount_a: u64, min_amount_b: u64)`:
     - Compute proportional share: amount_a = lp_amount * vault_a_balance / total_lp_supply, amount_b = lp_amount * vault_b_balance / total_lp_supply.
     - Check amount_a >= min_amount_a and amount_b >= min_amount_b (slippage protection).
     - Burn lp_amount from user_lp (CPI to token program with pool PDA authority).
     - Transfer amount_a from vault_a to user_token_a (CPI with pool PDA signer seeds).
     - Transfer amount_b from vault_b to user_token_b.
     - Update pool.total_lp_supply.

2. Build: `anchor build`.

---

## Phase 4: Swap with Constant-Product Pricing

**Goal:** Implement the swap instruction using x*y=k constant-product formula with fees.

### Tasks

1. Create `programs/solana-amm/src/instructions/swap.rs`:
   - `#[derive(Accounts)] pub struct Swap<'info>`:
     - `#[account(mut)] pool`
     - `#[account(mut)] vault_in` (source vault)
     - `#[account(mut)] vault_out` (destination vault)
     - `#[account(mut)] fee_vault` (fee vault for input token)
     - `#[account(mut)] user_token_in` (user sends this token)
     - `#[account(mut)] user_token_out` (user receives this token)
     - `user: Signer<'info>`
     - `token_program`
   - Handler: `swap(ctx, amount_in: u64, minimum_amount_out: u64)`:
     - Compute fee: `fee = amount_in * pool.fee_bps / 10000`.
     - Compute protocol_fee: `protocol_fee = fee * pool.protocol_fee_bps / 10000`.
     - Compute amount_in_after_fee: `amount_in - fee`.
     - Apply constant product: `amount_out = (vault_out_balance * amount_in_after_fee) / (vault_in_balance + amount_in_after_fee)`.
     - Verify: `amount_out >= minimum_amount_out` (slippage protection).
     - Verify: `amount_out < vault_out_balance` (cannot drain pool).
     - Transfer amount_in from user_token_in to vault_in.
     - Transfer protocol_fee from user_token_in to fee_vault (or from vault_in post-deposit).
     - Transfer amount_out from vault_out to user_token_out (CPI with PDA signer).

2. Add swap math functions to `math.rs`:
   - Function: `pub fn compute_swap_output(amount_in: u64, reserve_in: u64, reserve_out: u64, fee_bps: u16) -> Result<(u64, u64)>` - returns (amount_out, fee_amount). Uses u128 intermediates.
   - Function: `pub fn compute_swap_input(amount_out: u64, reserve_in: u64, reserve_out: u64, fee_bps: u16) -> Result<(u64, u64)>` - inverse: given desired output, compute required input.

3. Write math unit tests:
   - Test compute_swap_output with known values.
   - Test that k (product of reserves) does not decrease after swap.
   - Test fee computation.
   - Test edge case: amount_in = 0 returns error.

4. Build: `anchor build`.

---

## Phase 5: Collect Fees and Admin Functions

**Goal:** Implement protocol fee collection and admin configuration.

### Tasks

1. Create `programs/solana-amm/src/instructions/collect_fees.rs`:
   - `#[derive(Accounts)] pub struct CollectFees<'info>`:
     - `#[account(mut, has_one = authority)] pool`
     - `#[account(mut)] fee_vault_a`, `#[account(mut)] fee_vault_b`
     - `#[account(mut)] recipient_a`, `#[account(mut)] recipient_b` (authority's token accounts)
     - `authority: Signer<'info>`
     - `token_program`
   - Handler: `collect_fees(ctx)`:
     - Transfer full balance of fee_vault_a to recipient_a (CPI with PDA signer).
     - Transfer full balance of fee_vault_b to recipient_b.

2. Create `programs/solana-amm/src/instructions/update_fees.rs`:
   - `#[derive(Accounts)] pub struct UpdateFees<'info>`:
     - `#[account(mut, has_one = authority)] pool`
     - `authority: Signer<'info>`
   - Handler: `update_fees(ctx, new_fee_bps: u16, new_protocol_fee_bps: u16)`:
     - Validate new_fee_bps <= 10000 and new_protocol_fee_bps <= 10000.
     - Update pool.fee_bps and pool.protocol_fee_bps.

3. Wire all instructions in `lib.rs`:
   - Import and expose: initialize_pool, add_liquidity, remove_liquidity, swap, collect_fees, update_fees.

4. Build: `anchor build`.

---

## Phase 6: TypeScript Client SDK

**Goal:** Build a TypeScript client that wraps all program instructions for easy interaction and testing.

### Tasks

1. Create `app/src/amm-client.ts`:
   - Class `AmmClient`:
     - Constructor: accepts `Program<SolanaAmm>`, `Connection`, `Wallet`.
     - Method: `async initializePool(tokenAMint, tokenBMint, feeBps, protocolFeeBps) -> { pool, vaultA, vaultB, lpMint, txSig }`:
       - Derives pool PDA from seeds.
       - Creates associated token accounts for vaults and fee vaults.
       - Calls program.methods.initializePool().
     - Method: `async addLiquidity(pool, amountA, amountB, minLpTokens) -> { lpTokensMinted, txSig }`.
     - Method: `async removeLiquidity(pool, lpAmount, minAmountA, minAmountB) -> { amountA, amountB, txSig }`.
     - Method: `async swap(pool, amountIn, minimumAmountOut, swapAToB: boolean) -> { amountOut, txSig }`.
     - Method: `async collectFees(pool) -> { feeA, feeB, txSig }`.
     - Method: `async getPoolState(pool) -> PoolState` - fetches and deserializes pool account.

2. Create `app/src/utils.ts`:
   - Function: `derivePoolPda(programId, tokenAMint, tokenBMint) -> [PublicKey, number]`.
   - Function: `computeExpectedOutput(amountIn, reserveIn, reserveOut, feeBps) -> number` (client-side quote).
   - Function: `createMintAndFundAccount(connection, payer, decimals, amount) -> { mint, tokenAccount }` (test helper).

3. Create `app/src/types.ts`:
   - Interface `PoolState`: mirrors on-chain Pool struct.
   - Interface `SwapQuote`: { amountOut, fee, priceImpact }.

4. Install dependencies:
   - Command: `yarn add @coral-xyz/anchor @solana/web3.js @solana/spl-token bn.js`
   - Command: `yarn add -D typescript ts-mocha @types/mocha @types/chai chai`

5. Create `tsconfig.json`:
   - Target: ES2020, module: commonjs, strict: true, esModuleInterop: true.

---

## Phase 7: Integration Tests

**Goal:** Write comprehensive integration tests exercising all program instructions against a local validator.

### Tasks

1. Create `tests/amm.test.ts`:
   - Before all: start solana-test-validator (via Anchor), create two SPL token mints (Token A, Token B), fund test wallets.

2. Test suite: Pool Initialization
   - Test: initialize pool successfully, verify on-chain state (mints, fee config, authority).
   - Test: fail to initialize with same mint for A and B.
   - Test: fail to initialize with fee > 10000 bps.

3. Test suite: Add Liquidity
   - Test: first deposit mints correct LP tokens (sqrt(a*b) - MINIMUM_LIQUIDITY).
   - Test: subsequent deposit mints proportional LP tokens.
   - Test: fail when min_lp_tokens exceeds computed amount (slippage).
   - Test: fail with zero amounts.

4. Test suite: Swap
   - Test: swap A->B returns correct amount per x*y=k formula.
   - Test: swap B->A works symmetrically.
   - Test: fee is correctly deducted.
   - Test: k does not decrease after swap (invariant check).
   - Test: fail when minimum_amount_out exceeds computed output (slippage).
   - Test: multiple sequential swaps maintain invariant.

5. Test suite: Remove Liquidity
   - Test: withdraw proportional amounts for LP tokens burned.
   - Test: full withdrawal drains pool (except MINIMUM_LIQUIDITY).
   - Test: fail when min amounts exceed proportional share (slippage).

6. Test suite: Fees
   - Test: protocol fees accumulate in fee vaults after swaps.
   - Test: collect_fees transfers fee vault balances to authority.
   - Test: non-authority cannot collect fees.
   - Test: update_fees changes fee configuration.

7. Run tests:
   - Command: `anchor test`
   - This starts local validator, deploys program, runs TypeScript tests.
