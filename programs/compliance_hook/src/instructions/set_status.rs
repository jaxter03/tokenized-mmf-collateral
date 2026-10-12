//! `set_status` (contracts/compliance_hook.md): suspend, reinstate or revoke a holder. Called only
//! by governance (signer = governance PDA `["hook-admin"]`) on behalf of compliance.

use anchor_lang::prelude::*;
use mmf_common::seeds;

use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
pub struct SetStatus<'info> {
    /// Must be governance's `["hook-admin"]` PDA (checked in the handler: `UnauthorizedCaller`).
    pub hook_admin: Signer<'info>,
    pub registry: Account<'info, Registry>,
    #[account(
        mut,
        seeds = [seeds::HOLDER, registry.mint.as_ref(), holder_state.owner.as_ref()],
        bump = holder_state.bump
    )]
    pub holder_state: Account<'info, HolderState>,
}

pub fn handle_set_status(
    _ctx: Context<SetStatus>,
    _action: u8,
    _reason: u8,
    _case_ref_hash: [u8; 32],
    _actor: Pubkey,
) -> Result<()> {
    // Implemented in T065.
    Ok(())
}
