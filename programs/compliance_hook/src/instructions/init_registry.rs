//! `init_registry` (contracts/compliance_hook.md): creates the Register and the mint's
//! extra-account list while the Fund is in Setup.

use anchor_lang::prelude::*;
use mmf_common::seeds;

use crate::extra_metas::extra_metas_space;
use crate::state::Registry;

#[event_cpi]
#[derive(Accounts)]
pub struct InitRegistry<'info> {
    /// Must equal `role_registry.issuer`; pays rent.
    #[account(mut)]
    pub issuer: Signer<'info>,
    /// CHECK: the fund mint; only its address is used (seeds, and checked against `fund_state.mint`).
    pub mint: UncheckedAccount<'info>,
    /// CHECK: decoded with `FundStateView` at PDA `["fund", mint]` of the fund program.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at `fund_state.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    #[account(
        init,
        payer = issuer,
        space = 8 + Registry::INIT_SPACE,
        seeds = [seeds::REGISTRY, mint.key().as_ref()],
        bump
    )]
    pub registry: Account<'info, Registry>,
    /// CHECK: created here with the five entries; read by Token-2022 on every transfer.
    #[account(
        init,
        payer = issuer,
        space = extra_metas_space(),
        seeds = [seeds::EXTRA_ACCOUNT_METAS, mint.key().as_ref()],
        bump
    )]
    pub extra_account_meta_list: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handle_init_registry(_ctx: Context<InitRegistry>) -> Result<()> {
    // Implemented in T037.
    Ok(())
}
