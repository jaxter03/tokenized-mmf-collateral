//! `approve_fund_account` (FR-019): compliance approves a fund-operated account without a KYC
//! statement, only when the system itself controls it.

use anchor_lang::prelude::*;
use mmf_common::seeds;

use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
pub struct ApproveFundAccount<'info> {
    pub compliance: Signer<'info>,
    pub registry: Account<'info, Registry>,
    /// CHECK: decoded with `FundStateView` at `registry.fund_state`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at `registry.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [seeds::HOLDER, registry.mint.as_ref(), holder_state.owner.as_ref()],
        bump = holder_state.bump
    )]
    pub holder_state: Account<'info, HolderState>,
}

pub fn handle_approve_fund_account(_ctx: Context<ApproveFundAccount>) -> Result<()> {
    // Implemented in T054.
    Ok(())
}
