//! `set_rules` (contracts/compliance_hook.md): the issuer sets the Register's holder rules while
//! the Fund is in Setup (FR-008, FR-009).

use anchor_lang::prelude::*;
use mmf_common::jurisdiction::SET_WORDS;

use crate::state::Registry;

#[event_cpi]
#[derive(Accounts)]
pub struct SetRules<'info> {
    pub issuer: Signer<'info>,
    #[account(mut)]
    pub registry: Account<'info, Registry>,
    /// CHECK: decoded with `FundStateView` at `registry.fund_state`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at `registry.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
}

pub fn handle_set_rules(
    _ctx: Context<SetRules>,
    _kyc_providers: [Pubkey; 4],
    _allowed_jurisdictions: [u64; SET_WORDS],
    _max_holders: u32,
) -> Result<()> {
    // Implemented in T038.
    Ok(())
}
