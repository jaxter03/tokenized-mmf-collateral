//! `FundState` and `FundTerms` in their final layout (docs/shared-model.md §4.2). Fields used by
//! later features exist from the start and stay zero in F001.

use anchor_lang::prelude::*;

/// Fund terms, set once by `create_fund` (FR-001) and never changed in F001 (FR-009).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct FundTerms {
    /// Fund identifier; must not be all zero.
    pub fund_id: [u8; 12],
    /// Cash asset used for settlement: an SPL Token or Token-2022 mint with 6 decimals.
    pub cash_mint: Pubkey,
    /// Target price of one whole unit in cash, 8 decimals ($1.00 = 100_000_000); > 0.
    pub target_nav: u64,
    /// Largest allowed daily price move, in basis points; > 0.
    pub max_nav_move_bps: u16,
    /// Daily redemption limit as a share of units outstanding, in basis points; 1 ..= 10 000.
    pub redemption_gate_bps: u16,
    /// 0 Reinvest (units), 1 Cash.
    pub distribution_mode: u8,
    /// Distribution frequency in days; > 0.
    pub distribution_cadence_days: u16,
    /// Fingerprint of the legal documents; must not be all zero.
    pub terms_hash: [u8; 32],
    /// Link to the legal documents, UTF-8, zero-padded; at least one non-zero byte.
    pub terms_uri: [u8; 128],
}

/// PDA `["fund", mint]`; also the mint authority, metadata authority, close and burn authority.
#[account]
#[derive(Debug, InitSpace)]
pub struct FundState {
    pub version: u8,
    pub mint: Pubkey,
    /// PDA `["roles", fund_state]` of governance.
    pub role_registry: Pubkey,
    pub terms: FundTerms,
    /// `mmf_common::enums::FundStatus`; F001 uses 0 Setup and 1 Open.
    pub status: u8,
    // --- F002: daily price and accrual (zero in F001) ---
    pub nav: u64,
    pub nav_ts: i64,
    pub nav_seq: u64,
    pub accrual_index: u128,
    pub daily_rate_e12: u64,
    // --- F003: subscriptions and redemptions (zero in F001) ---
    pub units_outstanding: u64,
    pub redeemed_today: u64,
    pub day_start_ts: i64,
    pub cash_treasury: Pubkey,
    // --- F006: distributions (zero in F001) ---
    pub current_cycle: u32,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl FundState {
    pub const VERSION: u8 = 1;
}
