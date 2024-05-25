use anchor_lang::prelude::*;

#[error_code]
pub enum SolSwapError {
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,

    #[msg("Insufficient liquidity in the pool")]
    InsufficientLiquidity,

    #[msg("Invalid fee basis points (must be <= 10000)")]
    InvalidFee,

    #[msg("Amount must be greater than zero")]
    ZeroAmount,

    #[msg("Pool already initialized")]
    PoolAlreadyInitialized,

    #[msg("Invalid token mint")]
    InvalidTokenMint,

    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,

    #[msg("Insufficient LP tokens")]
    InsufficientLpTokens,

    #[msg("Pool reserves are empty")]
    EmptyReserves,

    #[msg("Identical token mints not allowed")]
    IdenticalMints,
}
