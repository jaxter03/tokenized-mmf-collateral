//! `FundState`, `FundTerms` and `NavRecord` (docs/shared-model.md §4.2). Fields used by later
//! features exist from the start and stay zero until that feature ships.

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
    /// Equals the mint's supply (invariant I4).
    pub units_outstanding: u64,
    /// Units burned by redemptions since `day_start_ts`.
    pub redeemed_today: u64,
    /// 00:00 UTC of the current redemption day; 0 = never rolled over.
    pub day_start_ts: i64,
    /// Token account of `terms.cash_mint`, set once by `set_cash_treasury`.
    pub cash_treasury: Pubkey,
    // --- F006: distributions (zero in F001) ---
    pub current_cycle: u32,
    pub bump: u8,
    // --- F002, taken from the reserved bytes after `bump` so F001 offsets don't move
    // (research R10) ---
    /// Last UTC day covered by any publish; 0 = none.
    pub nav_period_end_day: u32,
    /// Price allowed by an approved override at the next publish; 0 = none.
    pub pending_override_nav: u64,
    // --- F003, also from the reserved bytes (research R9) ---
    /// `units_outstanding` when the redemption day last rolled over: the daily limit's base.
    pub units_at_day_start: u64,
    pub _reserved: [u8; 44],
}

impl FundState {
    pub const VERSION: u8 = 1;
}

/// PDA `["nav", fund_state, seq.to_le_bytes()]`: one per publish, written once by
/// `publish_daily` and never modified or closed (FR-003). No `_reserved`: it is immutable
/// (research R9).
#[account]
#[derive(Debug, InitSpace)]
pub struct NavRecord {
    pub version: u8,
    /// 1, 2, 3, … (`FundState.nav_seq` after this publish).
    pub seq: u64,
    /// Price of one whole unit in cash, 8 decimals.
    pub nav: u64,
    /// Units earned per unit held per day × 10^12, as published.
    pub daily_rate_e12: u64,
    /// ≥ 1.
    pub days_covered: u16,
    /// First UTC day this record pays for.
    pub period_start_day: u32,
    /// `FundState.accrual_index` after this publish.
    pub index_after: u128,
    /// Clock time of the publish.
    pub as_of_ts: i64,
    /// The fund admin that signed.
    pub published_by: Pubkey,
    /// The price passed only because an approved override named it.
    pub override_used: bool,
    pub bump: u8,
}

impl NavRecord {
    pub const VERSION: u8 = 1;
}
