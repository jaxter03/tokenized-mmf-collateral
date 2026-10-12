//! Records (FR-033). Emitted with `emit_cpi!`; every payload carries `fund`, `slot`, `unix_ts`.

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
