use anchor_lang::prelude::*;

/// AMM pool state account — stores reserves, fee config, and LP mint reference.
#[account]
#[derive(Default)]
pub struct Pool {
    /// Bump seed for the pool PDA
    pub bump: u8,
    /// Token mint A
    pub token_mint_a: Pubkey,
    /// Token mint B
    pub token_mint_b: Pubkey,
    /// Pool's token A reserve account (ATA owned by pool authority)
    pub token_a_reserve: Pubkey,
    /// Pool's token B reserve account (ATA owned by pool authority)
    pub token_b_reserve: Pubkey,
    /// LP token mint (minted by the pool authority)
    pub lp_mint: Pubkey,
    /// Fee in basis points (e.g. 30 = 0.30%)
    pub fee_bps: u64,
    /// Current amount of token A in reserve
    pub reserve_a: u64,
    /// Current amount of token B in reserve
    pub reserve_b: u64,
    /// Total LP tokens in circulation
    pub total_lp_supply: u64,
    /// Authority (PDA) that controls reserves and LP mint
    pub authority: Pubkey,
    /// Authority bump
    pub authority_bump: u8,
}

impl Pool {
    pub const LEN: usize = 8   // discriminator
        + 1   // bump
        + 32  // token_mint_a
        + 32  // token_mint_b
        + 32  // token_a_reserve
        + 32  // token_b_reserve
        + 32  // lp_mint
        + 8   // fee_bps
        + 8   // reserve_a
        + 8   // reserve_b
        + 8   // total_lp_supply
        + 32  // authority
        + 1;  // authority_bump
}

/// Tracks an individual LP provider's share in a pool.
#[account]
#[derive(Default)]
pub struct UserPosition {
    /// Bump seed for the user position PDA
    pub bump: u8,
    /// The pool this position belongs to
    pub pool: Pubkey,
    /// The user's wallet address
    pub owner: Pubkey,
    /// Total LP tokens the user holds from this pool
    pub lp_tokens: u64,
}

impl UserPosition {
    pub const LEN: usize = 8   // discriminator
        + 1   // bump
        + 32  // pool
        + 32  // owner
        + 8;  // lp_tokens
}
