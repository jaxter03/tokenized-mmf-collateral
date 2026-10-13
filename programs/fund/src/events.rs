//! Records (F001 FR-033, F002 FR-020). Emitted with `emit_cpi!`; every payload carries `fund`,
//! `slot`, `unix_ts`.

use anchor_lang::prelude::*;

use crate::state::FundTerms;

#[event]
pub struct FundCreated {
    pub fund: Pubkey,
    pub mint: Pubkey,
    pub issuer: Pubkey,
    pub terms: FundTerms,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct FundLaunched {
    pub fund: Pubkey,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

/// A price and rate were published (F002).
#[event]
pub struct NavPublished {
    pub fund: Pubkey,
    pub seq: u64,
    pub nav: u64,
    pub daily_rate_e12: u64,
    pub days_covered: u16,
    pub period_start_day: u32,
    pub index_after: u128,
    pub publisher: Pubkey,
    /// The price passed only because the pending override named it.
    pub override_used: bool,
    /// The pending override this publish cleared, used or not; 0 if none was pending.
    pub override_cleared_nav: u64,
    pub slot: u64,
    pub unix_ts: i64,
}

/// Compliance approved a price override, through governance (F002).
#[event]
pub struct NavOverrideApproved {
    pub fund: Pubkey,
    pub nav: u64,
    pub reason: u8,
    pub case_ref_hash: [u8; 32],
    /// The compliance key that signed the governance instruction.
    pub actor: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

/// Compliance withdrew a pending price override before use (F002).
#[event]
pub struct NavOverrideWithdrawn {
    pub fund: Pubkey,
    /// The override that was withdrawn.
    pub nav: u64,
    pub reason: u8,
    pub case_ref_hash: [u8; 32],
    pub actor: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}
