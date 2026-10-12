//! `approve_investor` (FR-016, FR-017) and, from US3, `renew_investor` (FR-018): compliance
//! approves an investor with a provider-signed KYC statement (contracts/kyc-statement.md).

use anchor_lang::prelude::*;
use mmf_common::enums::HolderStatus;
use mmf_common::seeds;

use crate::errors::HookError;
use crate::events::HolderApproved;
use crate::instructions::common::{require_compliance, require_open, require_status};
use crate::state::{HolderState, Registry};
use crate::statement;

#[event_cpi]
#[derive(Accounts)]
pub struct ApproveInvestor<'info> {
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
    /// CHECK: must be the Instructions sysvar; checked in the handler (a fake one is
    /// `StatementMissing`).
    pub instructions: UncheckedAccount<'info>,
}

pub fn handle_approve_investor(
    ctx: Context<ApproveInvestor>,
    attestation_hash: [u8; 32],
) -> Result<()> {
    approve(ctx, attestation_hash, false)
}

/// `renew_investor` (FR-018): an Approved investor with a statement whose expiry is later than
/// the recorded one (research R14); jurisdiction and investor type may change and are re-checked.
pub fn handle_renew_investor(
    ctx: Context<ApproveInvestor>,
    attestation_hash: [u8; 32],
) -> Result<()> {
    approve(ctx, attestation_hash, true)
}

/// Shared by approval (Pending → Approved) and renewal (Approved → Approved, later expiry).
pub fn approve(
    ctx: Context<ApproveInvestor>,
    attestation_hash: [u8; 32],
    renewal: bool,
) -> Result<()> {
    let a = &ctx.accounts;
    // 1–2. Compliance, fund Open.
    require_compliance(&a.registry, &a.role_registry, &a.compliance.key())?;
    require_open(&a.registry, &a.fund_state)?;
    // 3–4. Not revoked; Pending to approve, Approved to renew.
    let expected = if renewal {
        HolderStatus::Approved
    } else {
        HolderStatus::Pending
    };
    require_status(&a.holder_state, expected)?;
    // 5. Statements are for investors; fund-operated accounts use approve_fund_account.
    require!(a.holder_state.is_investor(), HookError::NotInvestor);
    // 6–17. The statement (contracts/kyc-statement.md checks 1–12).
    let now = Clock::get()?.unix_timestamp;
    let v = statement::verify(
        &a.instructions,
        &a.registry,
        &a.holder_state,
        &attestation_hash,
        now,
        renewal,
    )?;

    let fund = a.registry.fund_state;
    let signer = a.compliance.key();
    let h = &mut ctx.accounts.holder_state;
    h.status = HolderStatus::Approved.as_u8();
    h.jurisdiction = v.statement.jurisdiction;
    h.investor_type = v.statement.investor_type;
    h.kyc_expiry_ts = v.statement.expiry_ts;
    h.attestation_hash = v.attestation_hash;
    h.attested_by = v.provider;

    let clock = Clock::get()?;
    emit_cpi!(HolderApproved {
        fund,
        owner: h.owner,
        class: h.class,
        jurisdiction: h.jurisdiction,
        investor_type: h.investor_type,
        kyc_expiry_ts: h.kyc_expiry_ts,
        attestation_hash: h.attestation_hash,
        attested_by: h.attested_by,
        renewal,
        signer,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
