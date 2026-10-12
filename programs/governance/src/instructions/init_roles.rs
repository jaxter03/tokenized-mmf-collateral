//! `init_roles` (contracts/governance.md): creates the role list before the Fund exists and fixes
//! the issuer (FR-002, research R10).

use anchor_lang::prelude::*;
use mmf_common::ids::FUND_PROGRAM_ID;
use mmf_common::seeds;

use crate::errors::GovernanceError;
use crate::events::RolesInitialized;
use crate::state::RoleRegistry;

#[event_cpi]
#[derive(Accounts)]
pub struct InitRoles<'info> {
    /// Becomes `role_registry.issuer`; pays rent.
    #[account(mut)]
    pub issuer: Signer<'info>,
    /// The keypair `fund::create_fund` will use for the mint. Its signature proves the caller
    /// controls that address, so nobody else can claim the issuer seat first.
    pub mint: Signer<'info>,
    /// CHECK: must be PDA `["fund", mint]` of the fund program (seeds constraint) and must not exist
    /// yet (checked in the handler).
    #[account(seeds = [seeds::FUND, mint.key().as_ref()], bump, seeds::program = FUND_PROGRAM_ID)]
    pub fund_state: UncheckedAccount<'info>,
    #[account(
        init,
        payer = issuer,
        space = 8 + RoleRegistry::INIT_SPACE,
        seeds = [seeds::ROLES, fund_state.key().as_ref()],
        bump
    )]
    pub role_registry: Account<'info, RoleRegistry>,
    pub system_program: Program<'info, System>,
}

pub fn handle_init_roles(ctx: Context<InitRoles>) -> Result<()> {
    // The fund must not exist yet. Unreachable through instructions today (a fund always has a role
    // registry, so the `init` above fails first), but kept as a defensive check.
    let fund_state = &ctx.accounts.fund_state;
    require!(
        fund_state.data_is_empty() && fund_state.lamports() == 0,
        GovernanceError::FundAlreadyExists
    );

    let issuer = ctx.accounts.issuer.key();
    let roles = &mut ctx.accounts.role_registry;
    roles.version = RoleRegistry::VERSION;
    roles.fund_state = fund_state.key();
    roles.issuer = issuer;
    roles.bump = ctx.bumps.role_registry;

    let clock = Clock::get()?;
    emit_cpi!(RolesInitialized {
        fund: fund_state.key(),
        issuer,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
