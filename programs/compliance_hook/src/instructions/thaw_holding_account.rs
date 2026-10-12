//! `thaw_holding_account` — permissionless and state-checked (contracts/compliance_hook.md,
//! research R9): thaws an Approved owner's one holding account, its Token-2022 ATA.

use anchor_lang::prelude::*;
use anchor_spl::token_2022::Token2022;
use mmf_common::seeds;

use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
pub struct ThawHoldingAccount<'info> {
    /// Anyone; adds no discretion (Constitution III).
    pub payer: Signer<'info>,
    /// The mint's freeze authority; signs the thaw CPI.
    pub registry: Account<'info, Registry>,
    #[account(
        seeds = [seeds::HOLDER, registry.mint.as_ref(), holder_state.owner.as_ref()],
        bump = holder_state.bump
    )]
    pub holder_state: Account<'info, HolderState>,
    /// CHECK: must be the owner's ATA for the fund mint; checked in the handler.
    #[account(mut)]
    pub token_account: UncheckedAccount<'info>,
    /// CHECK: the fund mint (`registry.mint`).
    #[account(address = registry.mint)]
    pub mint: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, Token2022>,
}

pub fn handle_thaw_holding_account(_ctx: Context<ThawHoldingAccount>) -> Result<()> {
    // Implemented in T055.
    Ok(())
}
