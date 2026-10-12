//! `init_roles` (contracts/governance.md): creates the role list before the Fund exists and fixes
//! the issuer (FR-002, research R10).

use anchor_lang::prelude::*;
use mmf_common::ids::FUND_PROGRAM_ID;
use mmf_common::seeds;

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

pub fn handle_init_roles(_ctx: Context<InitRoles>) -> Result<()> {
    // Implemented in T032.
    Ok(())
}
