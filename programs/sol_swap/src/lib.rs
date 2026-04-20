use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, MintTo, Token, TokenAccount, Transfer};

pub mod errors;
pub mod state;

use errors::SolSwapError;
use state::{Pool, UserPosition};

declare_id!("SWPabc1234567890abcdef1234567890abcdef12345");

// ─────────────────────────────────────────────
//  Events
// ─────────────────────────────────────────────

#[event]
pub struct PoolInitialized {
    pub pool: Pubkey,
    pub token_mint_a: Pubkey,
    pub token_mint_b: Pubkey,
    pub lp_mint: Pubkey,
    pub fee_bps: u64,
}

#[event]
pub struct LiquidityAdded {
    pub pool: Pubkey,
    pub user: Pubkey,
    pub amount_a: u64,
    pub amount_b: u64,
    pub lp_minted: u64,
}

#[event]
pub struct LiquidityRemoved {
    pub pool: Pubkey,
    pub user: Pubkey,
    pub lp_burned: u64,
    pub amount_a: u64,
    pub amount_b: u64,
}

#[event]
pub struct SwapEvent {
    pub pool: Pubkey,
    pub user: Pubkey,
    pub amount_in: u64,
    pub amount_out: u64,
    pub a_to_b: bool,
}

// ─────────────────────────────────────────────
//  Program
// ─────────────────────────────────────────────

#[program]
pub mod sol_swap {
    use super::*;

    /// Create a new AMM pool for two token mints.
    ///
    /// `fee_bps` — protocol fee expressed in basis points (e.g. 30 = 0.30 %).
    pub fn initialize_pool(ctx: Context<InitializePool>, fee_bps: u64) -> Result<()> {
        require!(fee_bps <= 10_000, SolSwapError::InvalidFee);
        require!(
            ctx.accounts.token_mint_a.key() != ctx.accounts.token_mint_b.key(),
            SolSwapError::IdenticalMints
        );

        let pool = &mut ctx.accounts.pool;
        pool.bump = ctx.bumps.pool;
        pool.authority = ctx.accounts.pool_authority.key();
        pool.authority_bump = ctx.bumps.pool_authority;
        pool.token_mint_a = ctx.accounts.token_mint_a.key();
        pool.token_mint_b = ctx.accounts.token_mint_b.key();
        pool.token_a_reserve = ctx.accounts.token_a_reserve.key();
        pool.token_b_reserve = ctx.accounts.token_b_reserve.key();
        pool.lp_mint = ctx.accounts.lp_mint.key();
        pool.fee_bps = fee_bps;
        pool.reserve_a = 0;
        pool.reserve_b = 0;
        pool.total_lp_supply = 0;

        emit!(PoolInitialized {
            pool: pool.key(),
            token_mint_a: pool.token_mint_a,
            token_mint_b: pool.token_mint_b,
            lp_mint: pool.lp_mint,
            fee_bps,
        });

        Ok(())
    }

    /// Deposit token A and token B into the pool; receive LP tokens.
    ///
    /// On first deposit the ratio is set by the depositor. On subsequent
    /// deposits tokens are accepted proportionally to the current reserves.
    /// `min_lp` is a slippage guard — the transaction reverts if fewer LP
    /// tokens would be minted.
    pub fn add_liquidity(
        ctx: Context<AddLiquidity>,
        amount_a: u64,
        amount_b: u64,
        min_lp: u64,
    ) -> Result<()> {
        require!(amount_a > 0 && amount_b > 0, SolSwapError::ZeroAmount);

        let pool = &mut ctx.accounts.pool;

        // ── compute LP to mint ──────────────────────────────────────────
        let lp_to_mint: u64;
        let actual_a: u64;
        let actual_b: u64;

        if pool.total_lp_supply == 0 {
            // First deposit — use geometric mean as initial LP supply.
            let lp = integer_sqrt(
                (amount_a as u128)
                    .checked_mul(amount_b as u128)
                    .ok_or(SolSwapError::ArithmeticOverflow)?,
            );
            lp_to_mint = lp as u64;
            actual_a = amount_a;
            actual_b = amount_b;
        } else {
            // Subsequent deposits — scale to current ratio.
            // lp = min( amount_a/reserve_a, amount_b/reserve_b ) * total_lp
            let lp_a = (amount_a as u128)
                .checked_mul(pool.total_lp_supply as u128)
                .ok_or(SolSwapError::ArithmeticOverflow)?
                / pool.reserve_a as u128;

            let lp_b = (amount_b as u128)
                .checked_mul(pool.total_lp_supply as u128)
                .ok_or(SolSwapError::ArithmeticOverflow)?
                / pool.reserve_b as u128;

            lp_to_mint = lp_a.min(lp_b) as u64;

            // Actual tokens deposited proportionally
            actual_a = (lp_to_mint as u128)
                .checked_mul(pool.reserve_a as u128)
                .ok_or(SolSwapError::ArithmeticOverflow)?
                .checked_div(pool.total_lp_supply as u128)
                .ok_or(SolSwapError::ArithmeticOverflow)? as u64;

            actual_b = (lp_to_mint as u128)
                .checked_mul(pool.reserve_b as u128)
                .ok_or(SolSwapError::ArithmeticOverflow)?
                .checked_div(pool.total_lp_supply as u128)
                .ok_or(SolSwapError::ArithmeticOverflow)? as u64;
        }

        require!(lp_to_mint >= min_lp, SolSwapError::SlippageExceeded);
        require!(lp_to_mint > 0, SolSwapError::ZeroAmount);

        // ── transfer tokens from user to reserves ──────────────────────
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token_a.to_account_info(),
                    to: ctx.accounts.token_a_reserve.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            actual_a,
        )?;

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token_b.to_account_info(),
                    to: ctx.accounts.token_b_reserve.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            actual_b,
        )?;

        // ── mint LP tokens to user ──────────────────────────────────────
        let authority_seeds: &[&[u8]] = &[
            b"authority",
            pool.token_mint_a.as_ref(),
            pool.token_mint_b.as_ref(),
            &[pool.authority_bump],
        ];
        let signer_seeds = &[authority_seeds];

        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.lp_mint.to_account_info(),
                    to: ctx.accounts.user_lp_account.to_account_info(),
                    authority: ctx.accounts.pool_authority.to_account_info(),
                },
                signer_seeds,
            ),
            lp_to_mint,
        )?;

        // ── update pool state ───────────────────────────────────────────
        pool.reserve_a = pool
            .reserve_a
            .checked_add(actual_a)
            .ok_or(SolSwapError::ArithmeticOverflow)?;
        pool.reserve_b = pool
            .reserve_b
            .checked_add(actual_b)
            .ok_or(SolSwapError::ArithmeticOverflow)?;
        pool.total_lp_supply = pool
            .total_lp_supply
            .checked_add(lp_to_mint)
            .ok_or(SolSwapError::ArithmeticOverflow)?;

        // ── update user position ────────────────────────────────────────
        let position = &mut ctx.accounts.user_position;
        position.bump = ctx.bumps.user_position;
        position.pool = pool.key();
        position.owner = ctx.accounts.user.key();
        position.lp_tokens = position
            .lp_tokens
            .checked_add(lp_to_mint)
            .ok_or(SolSwapError::ArithmeticOverflow)?;

        emit!(LiquidityAdded {
            pool: pool.key(),
            user: ctx.accounts.user.key(),
            amount_a: actual_a,
            amount_b: actual_b,
            lp_minted: lp_to_mint,
        });

        Ok(())
    }

    /// Burn LP tokens and withdraw the proportional share of both reserves.
    pub fn remove_liquidity(
        ctx: Context<RemoveLiquidity>,
        lp_amount: u64,
        min_a: u64,
        min_b: u64,
    ) -> Result<()> {
        require!(lp_amount > 0, SolSwapError::ZeroAmount);

        let pool = &mut ctx.accounts.pool;
        require!(pool.total_lp_supply > 0, SolSwapError::EmptyReserves);
        require!(
            ctx.accounts.user_position.lp_tokens >= lp_amount,
            SolSwapError::InsufficientLpTokens
        );

        // ── compute underlying amounts ──────────────────────────────────
        let amount_a = (lp_amount as u128)
            .checked_mul(pool.reserve_a as u128)
            .ok_or(SolSwapError::ArithmeticOverflow)?
            .checked_div(pool.total_lp_supply as u128)
            .ok_or(SolSwapError::ArithmeticOverflow)? as u64;

        let amount_b = (lp_amount as u128)
            .checked_mul(pool.reserve_b as u128)
            .ok_or(SolSwapError::ArithmeticOverflow)?
            .checked_div(pool.total_lp_supply as u128)
            .ok_or(SolSwapError::ArithmeticOverflow)? as u64;

        require!(amount_a >= min_a, SolSwapError::SlippageExceeded);
        require!(amount_b >= min_b, SolSwapError::SlippageExceeded);
        require!(amount_a > 0 && amount_b > 0, SolSwapError::InsufficientLiquidity);

        // ── burn LP tokens ──────────────────────────────────────────────
        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: ctx.accounts.lp_mint.to_account_info(),
                    from: ctx.accounts.user_lp_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            lp_amount,
        )?;

        // ── transfer tokens back to user ────────────────────────────────
        let authority_seeds: &[&[u8]] = &[
            b"authority",
            pool.token_mint_a.as_ref(),
            pool.token_mint_b.as_ref(),
            &[pool.authority_bump],
        ];
        let signer_seeds = &[authority_seeds];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.token_a_reserve.to_account_info(),
                    to: ctx.accounts.user_token_a.to_account_info(),
                    authority: ctx.accounts.pool_authority.to_account_info(),
                },
                signer_seeds,
            ),
            amount_a,
        )?;

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.token_b_reserve.to_account_info(),
                    to: ctx.accounts.user_token_b.to_account_info(),
                    authority: ctx.accounts.pool_authority.to_account_info(),
                },
                signer_seeds,
            ),
            amount_b,
        )?;

        // ── update pool state ───────────────────────────────────────────
        pool.reserve_a = pool
            .reserve_a
            .checked_sub(amount_a)
            .ok_or(SolSwapError::ArithmeticOverflow)?;
        pool.reserve_b = pool
            .reserve_b
            .checked_sub(amount_b)
            .ok_or(SolSwapError::ArithmeticOverflow)?;
        pool.total_lp_supply = pool
            .total_lp_supply
            .checked_sub(lp_amount)
            .ok_or(SolSwapError::ArithmeticOverflow)?;

        // ── update user position ────────────────────────────────────────
        let position = &mut ctx.accounts.user_position;
        position.lp_tokens = position
            .lp_tokens
            .checked_sub(lp_amount)
            .ok_or(SolSwapError::ArithmeticOverflow)?;

        emit!(LiquidityRemoved {
            pool: pool.key(),
            user: ctx.accounts.user.key(),
            lp_burned: lp_amount,
            amount_a,
            amount_b,
        });

        Ok(())
    }

    /// Constant-product swap (x * y = k).
    ///
    /// `amount_in`     — exact input amount (before fee deduction).
    /// `min_amount_out` — minimum acceptable output (slippage protection).
    /// `a_to_b`        — direction: true = A→B, false = B→A.
    ///
    /// Fee formula: amount_in_after_fee = amount_in * (10_000 - fee_bps) / 10_000
    /// Output:       dy = y * dx_fee / (x + dx_fee)
    pub fn swap(
        ctx: Context<Swap>,
        amount_in: u64,
        min_amount_out: u64,
        a_to_b: bool,
    ) -> Result<()> {
        require!(amount_in > 0, SolSwapError::ZeroAmount);

        let pool = &mut ctx.accounts.pool;
        require!(
            pool.reserve_a > 0 && pool.reserve_b > 0,
            SolSwapError::InsufficientLiquidity
        );

        let fee_bps = pool.fee_bps;
        let (reserve_in, reserve_out) = if a_to_b {
            (pool.reserve_a, pool.reserve_b)
        } else {
            (pool.reserve_b, pool.reserve_a)
        };

        // ── apply fee ──────────────────────────────────────────────────
        let amount_in_after_fee = (amount_in as u128)
            .checked_mul((10_000 - fee_bps) as u128)
            .ok_or(SolSwapError::ArithmeticOverflow)?
            / 10_000u128;

        // ── constant-product: dy = y * dx / (x + dx) ──────────────────
        let numerator = amount_in_after_fee
            .checked_mul(reserve_out as u128)
            .ok_or(SolSwapError::ArithmeticOverflow)?;

        let denominator = (reserve_in as u128)
            .checked_add(amount_in_after_fee)
            .ok_or(SolSwapError::ArithmeticOverflow)?;

        let amount_out = (numerator / denominator) as u64;

        require!(amount_out >= min_amount_out, SolSwapError::SlippageExceeded);
        require!(amount_out > 0, SolSwapError::InsufficientLiquidity);
        require!(amount_out < reserve_out, SolSwapError::InsufficientLiquidity);

        // ── transfer token_in from user to reserve ──────────────────────
        let authority_seeds: &[&[u8]] = &[
            b"authority",
            pool.token_mint_a.as_ref(),
            pool.token_mint_b.as_ref(),
            &[pool.authority_bump],
        ];
        let signer_seeds = &[authority_seeds];

        if a_to_b {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.user_token_in.to_account_info(),
                        to: ctx.accounts.reserve_in.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                amount_in,
            )?;

            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.reserve_out.to_account_info(),
                        to: ctx.accounts.user_token_out.to_account_info(),
                        authority: ctx.accounts.pool_authority.to_account_info(),
                    },
                    signer_seeds,
                ),
                amount_out,
            )?;

            pool.reserve_a = pool
                .reserve_a
                .checked_add(amount_in)
                .ok_or(SolSwapError::ArithmeticOverflow)?;
            pool.reserve_b = pool
                .reserve_b
                .checked_sub(amount_out)
                .ok_or(SolSwapError::ArithmeticOverflow)?;
        } else {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.user_token_in.to_account_info(),
                        to: ctx.accounts.reserve_in.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                amount_in,
            )?;

            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.reserve_out.to_account_info(),
                        to: ctx.accounts.user_token_out.to_account_info(),
                        authority: ctx.accounts.pool_authority.to_account_info(),
                    },
                    signer_seeds,
                ),
                amount_out,
            )?;

            pool.reserve_b = pool
                .reserve_b
                .checked_add(amount_in)
                .ok_or(SolSwapError::ArithmeticOverflow)?;
            pool.reserve_a = pool
                .reserve_a
                .checked_sub(amount_out)
                .ok_or(SolSwapError::ArithmeticOverflow)?;
        }

        emit!(SwapEvent {
            pool: pool.key(),
            user: ctx.accounts.user.key(),
            amount_in,
            amount_out,
            a_to_b,
        });

        Ok(())
    }
}

// ─────────────────────────────────────────────
//  Instruction Contexts
// ─────────────────────────────────────────────

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: Just used as a seed differentiator
    pub token_mint_a: Account<'info, Mint>,
    /// CHECK: Just used as a seed differentiator
    pub token_mint_b: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        space = Pool::LEN,
        seeds = [b"pool", token_mint_a.key().as_ref(), token_mint_b.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,

    /// PDA that acts as mint authority for the LP mint and owner of reserves
    /// CHECK: PDA verified by seeds
    #[account(
        seeds = [b"authority", token_mint_a.key().as_ref(), token_mint_b.key().as_ref()],
        bump
    )]
    pub pool_authority: UncheckedAccount<'info>,

    #[account(
        init,
        payer = payer,
        mint::decimals = 6,
        mint::authority = pool_authority,
    )]
    pub lp_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        token::mint = token_mint_a,
        token::authority = pool_authority,
    )]
    pub token_a_reserve: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = payer,
        token::mint = token_mint_b,
        token::authority = pool_authority,
    )]
    pub token_b_reserve: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct AddLiquidity<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.token_mint_a.as_ref(), pool.token_mint_b.as_ref()],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA verified by seeds
    #[account(
        seeds = [b"authority", pool.token_mint_a.as_ref(), pool.token_mint_b.as_ref()],
        bump = pool.authority_bump
    )]
    pub pool_authority: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = user,
        space = UserPosition::LEN,
        seeds = [b"position", pool.key().as_ref(), user.key().as_ref()],
        bump
    )]
    pub user_position: Account<'info, UserPosition>,

    #[account(mut, constraint = user_token_a.mint == pool.token_mint_a)]
    pub user_token_a: Account<'info, TokenAccount>,

    #[account(mut, constraint = user_token_b.mint == pool.token_mint_b)]
    pub user_token_b: Account<'info, TokenAccount>,

    #[account(mut, constraint = token_a_reserve.key() == pool.token_a_reserve)]
    pub token_a_reserve: Account<'info, TokenAccount>,

    #[account(mut, constraint = token_b_reserve.key() == pool.token_b_reserve)]
    pub token_b_reserve: Account<'info, TokenAccount>,

    #[account(mut, constraint = lp_mint.key() == pool.lp_mint)]
    pub lp_mint: Account<'info, Mint>,

    #[account(mut, constraint = user_lp_account.mint == pool.lp_mint)]
    pub user_lp_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct RemoveLiquidity<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.token_mint_a.as_ref(), pool.token_mint_b.as_ref()],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA verified by seeds
    #[account(
        seeds = [b"authority", pool.token_mint_a.as_ref(), pool.token_mint_b.as_ref()],
        bump = pool.authority_bump
    )]
    pub pool_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"position", pool.key().as_ref(), user.key().as_ref()],
        bump = user_position.bump,
        constraint = user_position.owner == user.key()
    )]
    pub user_position: Account<'info, UserPosition>,

    #[account(mut, constraint = user_token_a.mint == pool.token_mint_a)]
    pub user_token_a: Account<'info, TokenAccount>,

    #[account(mut, constraint = user_token_b.mint == pool.token_mint_b)]
    pub user_token_b: Account<'info, TokenAccount>,

    #[account(mut, constraint = token_a_reserve.key() == pool.token_a_reserve)]
    pub token_a_reserve: Account<'info, TokenAccount>,

    #[account(mut, constraint = token_b_reserve.key() == pool.token_b_reserve)]
    pub token_b_reserve: Account<'info, TokenAccount>,

    #[account(mut, constraint = lp_mint.key() == pool.lp_mint)]
    pub lp_mint: Account<'info, Mint>,

    #[account(mut, constraint = user_lp_account.mint == pool.lp_mint)]
    pub user_lp_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.token_mint_a.as_ref(), pool.token_mint_b.as_ref()],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA verified by seeds
    #[account(
        seeds = [b"authority", pool.token_mint_a.as_ref(), pool.token_mint_b.as_ref()],
        bump = pool.authority_bump
    )]
    pub pool_authority: UncheckedAccount<'info>,

    /// User's source token account (A when a_to_b=true, B when false)
    #[account(mut)]
    pub user_token_in: Account<'info, TokenAccount>,

    /// User's destination token account
    #[account(mut)]
    pub user_token_out: Account<'info, TokenAccount>,

    /// Pool's source reserve
    #[account(mut)]
    pub reserve_in: Account<'info, TokenAccount>,

    /// Pool's destination reserve
    #[account(mut)]
    pub reserve_out: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

// ─────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────

/// Integer square root via Newton's method (rounds down).
fn integer_sqrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
