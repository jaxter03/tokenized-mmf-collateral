//! Records (FR-033). Emitted with `emit_cpi!`; every payload carries `fund`, `slot`, `unix_ts`.

use anchor_lang::prelude::*;

#[event]
pub struct RolesInitialized {
    pub fund: Pubkey,
    pub issuer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct RolesAssigned {
    pub fund: Pubkey,
    pub issuer: Pubkey,
    pub fund_admin: Pubkey,
    pub transfer_agent: Pubkey,
    pub compliance: Pubkey,
    pub keeper: Pubkey,
    pub auditor: Pubkey,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}
