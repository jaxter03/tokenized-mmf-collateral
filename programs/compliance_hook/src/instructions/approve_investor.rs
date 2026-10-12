//! `approve_investor` (FR-016, FR-017) and, from US3, `renew_investor` (FR-018): compliance
//! approves an investor with a provider-signed KYC statement (contracts/kyc-statement.md).

use anchor_lang::prelude::*;
use mmf_common::seeds;

use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
pub struct ApproveInvestor<'info> {
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
    /// CHECK: must be the Instructions sysvar; checked in the handler (a fake one is
    /// `StatementMissing`).
    pub instructions: UncheckedAccount<'info>,
}

pub fn handle_approve_investor(
    _ctx: Context<ApproveInvestor>,
    _attestation_hash: [u8; 32],
) -> Result<()> {
    // Implemented in T053.
    Ok(())
}
