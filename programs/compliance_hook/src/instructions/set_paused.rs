//! `set_paused` (contracts/compliance_hook.md): pause or unpause all transfers. Called only by
//! governance (signer = governance PDA `["hook-admin"]`) on behalf of compliance.

use anchor_lang::prelude::*;

use crate::state::Registry;

#[event_cpi]
#[derive(Accounts)]
pub struct SetPaused<'info> {
    /// Must be governance's `["hook-admin"]` PDA (checked in the handler: `UnauthorizedCaller`).
    pub hook_admin: Signer<'info>,
    #[account(mut)]
    pub registry: Account<'info, Registry>,
}

pub fn handle_set_paused(
    _ctx: Context<SetPaused>,
    _paused: bool,
    _reason: u8,
    _case_ref_hash: [u8; 32],
    _actor: Pubkey,
) -> Result<()> {
    // Implemented in T066.
    Ok(())
}
