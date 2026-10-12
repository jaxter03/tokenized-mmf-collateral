//! `launch` (contracts/fund.md): the issuer moves the Fund from Setup to Open (FR-005, FR-006).

use anchor_lang::prelude::*;
use mmf_common::enums::FundStatus;
use mmf_common::ids::COMPLIANCE_HOOK_PROGRAM_ID;
use mmf_common::jurisdiction;
use mmf_common::seeds;
use mmf_common::views::{RegistryView, RoleRegistryView};

use crate::errors::FundError;
use crate::events::FundLaunched;
use crate::state::FundState;

#[event_cpi]
#[derive(Accounts)]
pub struct Launch<'info> {
    pub issuer: Signer<'info>,
    #[account(mut)]
    pub fund_state: Account<'info, FundState>,
    /// CHECK: decoded with `RoleRegistryView` at `fund_state.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: decoded with `RegistryView` at PDA `["registry", mint]` of compliance_hook.
    pub registry: UncheckedAccount<'info>,
    /// CHECK: must be owned by compliance_hook at PDA `["extra-account-metas", mint]` and hold data.
    pub extra_account_meta_list: UncheckedAccount<'info>,
}

pub fn handle_launch(ctx: Context<Launch>) -> Result<()> {
    let fs = &ctx.accounts.fund_state;
    let fund_key = fs.key();

    // 1. Only the issuer (FR-005). A role registry that can't be verified fails closed.
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &fs.role_registry)
        .map_err(|_| error!(FundError::Unauthorized))?;
    require_keys_eq!(
        ctx.accounts.issuer.key(),
        roles.issuer,
        FundError::Unauthorized
    );

    // 2. Only from Setup (US1-6).
    require!(
        fs.status == FundStatus::Setup.as_u8(),
        FundError::FundNotInSetup
    );

    // 3. Every operating role assigned (FR-006).
    require!(roles.all_assigned(), FundError::RolesIncomplete);

    // 4. Register and extra-account list exist for this mint.
    let mint = fs.mint;
    let registry_key = Pubkey::find_program_address(
        &[seeds::REGISTRY, mint.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
    .0;
    let registry = RegistryView::load(&ctx.accounts.registry, &registry_key)
        .map_err(|_| error!(FundError::RegisterNotInitialized))?;
    require!(
        registry.mint == mint && registry.fund_state == fund_key,
        FundError::RegisterNotInitialized
    );
    let metas_key = Pubkey::find_program_address(
        &[seeds::EXTRA_ACCOUNT_METAS, mint.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
    .0;
    let metas = &ctx.accounts.extra_account_meta_list;
    require!(
        metas.key() == metas_key
            && metas.owner == &COMPLIANCE_HOOK_PROGRAM_ID
            && !metas.data_is_empty(),
        FundError::RegisterNotInitialized
    );

    // 5–6. At least one KYC provider and one allowed jurisdiction (FR-006).
    require!(registry.has_provider(), FundError::NoKycProvider);
    require!(
        !jurisdiction::is_empty(&registry.allowed_jurisdictions),
        FundError::NoAllowedJurisdiction
    );

    ctx.accounts.fund_state.status = FundStatus::Open.as_u8();
    let clock = Clock::get()?;
    emit_cpi!(FundLaunched {
        fund: fund_key,
        signer: ctx.accounts.issuer.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
