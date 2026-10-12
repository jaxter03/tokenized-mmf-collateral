//! `set_paused` (contracts/compliance_hook.md): pause or unpause all transfers. Called only by
//! governance (signer = governance PDA `["hook-admin"]`) on behalf of compliance.

use anchor_lang::prelude::*;

use crate::errors::HookError;
use crate::events::TransfersPaused;
use crate::instructions::set_status::require_hook_admin;
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
    ctx: Context<SetPaused>,
    paused: bool,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Result<()> {
    // 1. Only governance's hook-admin PDA.
    require_hook_admin(&ctx.accounts.hook_admin.key())?;

    // 2. A real change, so every record means something.
    let reg = &mut ctx.accounts.registry;
    if paused {
        require!(!reg.transfers_paused, HookError::AlreadyPaused);
    } else {
        require!(reg.transfers_paused, HookError::NotPaused);
    }
    reg.transfers_paused = paused;

    let clock = Clock::get()?;
    emit_cpi!(TransfersPaused {
        fund: reg.fund_state,
        paused,
        reason,
        case_ref_hash,
        actor,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
