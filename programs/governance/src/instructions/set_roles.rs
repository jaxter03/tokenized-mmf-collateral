//! `set_roles` (contracts/governance.md): the issuer assigns the five operating roles while the
//! Fund is in Setup (FR-010).

use anchor_lang::prelude::*;

use crate::state::RoleRegistry;

#[event_cpi]
#[derive(Accounts)]
pub struct SetRoles<'info> {
    pub issuer: Signer<'info>,
    #[account(mut)]
    pub role_registry: Account<'info, RoleRegistry>,
    /// CHECK: decoded with `FundStateView`, which checks owner, address and discriminator.
    pub fund_state: UncheckedAccount<'info>,
}

pub fn handle_set_roles(
    _ctx: Context<SetRoles>,
    _fund_admin: Pubkey,
    _transfer_agent: Pubkey,
    _compliance: Pubkey,
    _keeper: Pubkey,
    _auditor: Pubkey,
) -> Result<()> {
    // Implemented in T033.
    Ok(())
}
