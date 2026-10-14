//! `approve_fund_account` (FR-019): compliance approves a fund-operated account without a KYC
//! statement, only when the system itself controls it.

use anchor_lang::prelude::*;
use mmf_common::enums::HolderStatus;
use mmf_common::seeds;

use crate::errors::HookError;
use crate::events::HolderApproved;
use crate::instructions::common::{require_compliance, require_open, require_status};
use crate::instructions::register_holder::system_owner;
use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
pub struct ApproveFundAccount<'info> {
    pub compliance: Signer<'info>,
    pub registry: Account<'info, Registry>,
    /// CHECK: decoded with `FundStateView` at `registry.fund_state`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at `registry.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [seeds::HOLDER, registry.mint.as_ref(), holder_state.owner.as_ref()],
        bump = holder_state.bump
    )]
    pub holder_state: Account<'info, HolderState>,
}

pub fn handle_approve_fund_account(ctx: Context<ApproveFundAccount>) -> Result<()> {
    let a = &ctx.accounts;
    require_compliance(&a.registry, &a.role_registry, &a.compliance.key())?;
    require_open(&a.registry, &a.fund_state)?;
    require_status(&a.holder_state, HolderStatus::Pending)?;
    // 5. A fund-operated class: the fund treasury, or (from F003) the settlement escrow.
    let class = a
        .holder_state
        .class()
        .filter(|c| c.is_fund_operated())
        .ok_or(error!(HookError::NotFundAccount))?;
    // 6. Still owned by the system's own PDA for that class (re-checked; registration already did).
    let expected =
        system_owner(class, &a.registry.fund_state).ok_or(error!(HookError::NotFundAccount))?;
    require_keys_eq!(
        a.holder_state.owner,
        expected,
        HookError::NotSystemControlled
    );

    let fund = a.registry.fund_state;
    let signer = a.compliance.key();
    let h = &mut ctx.accounts.holder_state;
    h.status = HolderStatus::Approved.as_u8();
    h.kyc_expiry_ts = i64::MAX; // fund-operated accounts never expire
    h.jurisdiction = [0; 2];
    h.investor_type = 0;
    h.attestation_hash = [0; 32];
    h.attested_by = Pubkey::default();

    let clock = Clock::get()?;
    emit_cpi!(HolderApproved {
        fund,
        owner: h.owner,
        class: h.class,
        jurisdiction: h.jurisdiction,
        investor_type: 0,
        kyc_expiry_ts: h.kyc_expiry_ts,
        attestation_hash: h.attestation_hash,
        attested_by: h.attested_by,
        renewal: false,
        signer,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
