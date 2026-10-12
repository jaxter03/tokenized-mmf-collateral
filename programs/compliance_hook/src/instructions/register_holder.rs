//! `register_holder` (contracts/compliance_hook.md, FR-013, FR-014): creates a Pending holder
//! record for an owner account.

use anchor_lang::prelude::*;
use mmf_common::seeds;

use crate::state::Registry;

#[event_cpi]
#[derive(Accounts)]
#[instruction(owner: Pubkey)]
pub struct RegisterHolder<'info> {
    /// The owner itself (Investor class only) or compliance (any class).
    pub authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub registry: Account<'info, Registry>,
    /// CHECK: decoded with `FundStateView` at `registry.fund_state`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at `registry.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: PDA `["holder", mint, owner]`; created by hand in the handler so an existing record
    /// gives `HolderAlreadyRegistered` and a pre-funded address can still be registered.
    #[account(mut, seeds = [seeds::HOLDER, registry.mint.as_ref(), owner.as_ref()], bump)]
    pub holder_state: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handle_register_holder(
    _ctx: Context<RegisterHolder>,
    _owner: Pubkey,
    _class: u8,
) -> Result<()> {
    // Implemented in T052.
    Ok(())
}
