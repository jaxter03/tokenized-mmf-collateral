//! `set_rules` (contracts/compliance_hook.md): the issuer sets the Register's holder rules while
//! the Fund is in Setup (FR-008, FR-009).

use anchor_lang::prelude::*;
use mmf_common::enums::FundStatus;
use mmf_common::jurisdiction::{self, SET_WORDS};
use mmf_common::views::{FundStateView, RoleRegistryView};

use crate::errors::HookError;
use crate::events::RegisterRulesSet;
use crate::state::Registry;

#[event_cpi]
#[derive(Accounts)]
pub struct SetRules<'info> {
    pub issuer: Signer<'info>,
    #[account(mut)]
    pub registry: Account<'info, Registry>,
    /// CHECK: decoded with `FundStateView` at `registry.fund_state`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at `registry.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
}

pub fn handle_set_rules(
    ctx: Context<SetRules>,
    kyc_providers: [Pubkey; 4],
    allowed_jurisdictions: [u64; SET_WORDS],
    max_holders: u32,
) -> Result<()> {
    let reg = &ctx.accounts.registry;

    // 1. Only the issuer.
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &reg.role_registry)
        .map_err(|_| error!(HookError::Unauthorized))?;
    require_keys_eq!(
        ctx.accounts.issuer.key(),
        roles.issuer,
        HookError::Unauthorized
    );

    // 2. Only in Setup; fixed once Open (FR-009).
    let in_setup = FundStateView::load(&ctx.accounts.fund_state, &reg.fund_state)
        .map(|f| f.status == FundStatus::Setup.as_u8())
        .unwrap_or(false);
    require!(in_setup, HookError::FundNotInSetup);

    // 3. Non-default providers are distinct; empty slots (default pubkey) may repeat.
    for (i, p) in kyc_providers.iter().enumerate() {
        if *p != Pubkey::default() {
            require!(
                !kyc_providers[i + 1..].contains(p),
                HookError::DuplicateProvider
            );
        }
    }

    // 4. Every allowed jurisdiction is an assigned ISO 3166-1 code; no bit ≥ 676.
    require!(
        jurisdiction::valid_set(&allowed_jurisdictions),
        HookError::UnknownJurisdiction
    );

    let fund = reg.fund_state;
    let reg = &mut ctx.accounts.registry;
    reg.kyc_providers = kyc_providers;
    reg.allowed_jurisdictions = allowed_jurisdictions;
    reg.max_holders = max_holders;

    let clock = Clock::get()?;
    emit_cpi!(RegisterRulesSet {
        fund,
        kyc_providers,
        allowed_jurisdictions,
        max_holders,
        signer: ctx.accounts.issuer.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
