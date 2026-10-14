//! `init_registry` (contracts/compliance_hook.md): creates the Register and the mint's
//! extra-account list while the Fund is in Setup.

use anchor_lang::prelude::*;
use mmf_common::enums::FundStatus;
use mmf_common::ids::{COLLATERAL_PROGRAM_ID, FUND_PROGRAM_ID, GOVERNANCE_PROGRAM_ID};
use mmf_common::seeds;
use mmf_common::views::{FundStateView, RoleRegistryView};

use crate::errors::HookError;
use crate::events::RegistryInitialized;
use crate::extra_metas::{extra_metas, extra_metas_space, write_extra_metas};
use crate::state::Registry;

#[event_cpi]
#[derive(Accounts)]
pub struct InitRegistry<'info> {
    /// Must equal `role_registry.issuer`; pays rent.
    #[account(mut)]
    pub issuer: Signer<'info>,
    /// CHECK: the fund mint; only its address is used (seeds, and the FundState PDA derivation).
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

pub fn handle_init_registry(ctx: Context<InitRegistry>) -> Result<()> {
    let mint = ctx.accounts.mint.key();
    let fund_key = Pubkey::find_program_address(&[seeds::FUND, mint.as_ref()], &FUND_PROGRAM_ID).0;
    // A fund that doesn't exist for this mint is not in Setup (fail closed).
    let fund = FundStateView::load(&ctx.accounts.fund_state, &fund_key)
        .map_err(|_| error!(HookError::FundNotInSetup))?;

    // 1. Only the issuer.
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &fund.role_registry)
        .map_err(|_| error!(HookError::Unauthorized))?;
    require_keys_eq!(
        ctx.accounts.issuer.key(),
        roles.issuer,
        HookError::Unauthorized
    );

    // 2. Only in Setup. (Unreachable after launch today, since an Open fund already has a registry.)
    require!(
        fund.status == FundStatus::Setup.as_u8(),
        HookError::FundNotInSetup
    );

    let registry_key = ctx.accounts.registry.key();
    let delegate =
        Pubkey::find_program_address(&[seeds::DELEGATE, mint.as_ref()], &GOVERNANCE_PROGRAM_ID).0;
    write_extra_metas(
        &ctx.accounts.extra_account_meta_list,
        &extra_metas(&registry_key, &fund_key, &delegate)?,
    )?;

    let reg = &mut ctx.accounts.registry;
    reg.version = Registry::VERSION;
    reg.mint = mint;
    reg.governance = GOVERNANCE_PROGRAM_ID;
    // F004 (research R2): the only program whose `["hook-caller"]` may change locked units. Set
    // here, never by a setter (a setter would be a rule-change power, Constitution VIII).
    reg.collateral_program = COLLATERAL_PROGRAM_ID;
    reg.role_registry = fund.role_registry;
    reg.fund_state = fund_key;
    reg.bump = ctx.bumps.registry;

    let clock = Clock::get()?;
    emit_cpi!(RegistryInitialized {
        fund: fund_key,
        signer: ctx.accounts.issuer.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
