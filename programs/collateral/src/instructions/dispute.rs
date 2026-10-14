//! Disputes (contracts/collateral.md, FR-031–FR-033, research R10): the giver disputes a delivery
//! call and may withdraw the dispute; compliance resolves it through governance, which signs
//! `resolve_dispute` with its `["collateral-caller"]` PDA.

use anchor_lang::prelude::*;
use mmf_common::enums::CallStatus;
use mmf_common::ids::GOVERNANCE_PROGRAM_ID;
use mmf_common::seeds;

use crate::errors::CollateralError;
use crate::events::{CallDisputed, DisputeResolved, DisputeWithdrawn};
use crate::instructions::cancel_call::uncount_delivery;
use crate::state::{CollateralCall, PledgeAgreement};
use crate::valuation::{load_fund, settle_status};

/// The call belongs to the agreement, whose FundState is the one passed (`FundMismatch`).
fn bound(fund_state: &AccountInfo, a: &Account<PledgeAgreement>, call: &CollateralCall) -> Result<()> {
    load_fund(fund_state, a)?;
    require_keys_eq!(call.agreement, a.key(), CollateralError::FundMismatch);
    Ok(())
}

#[event_cpi]
#[derive(Accounts)]
pub struct DisputeCall<'info> {
    /// Must be the agreement's giver.
    pub signer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    pub agreement: Account<'info, PledgeAgreement>,
    #[account(mut)]
    pub call: Account<'info, CollateralCall>,
}

#[event_cpi]
#[derive(Accounts)]
pub struct ResolveDispute<'info> {
    /// Must be governance's `["collateral-caller"]` PDA (`UnauthorizedCaller`).
    pub collateral_caller: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    #[account(mut)]
    pub call: Account<'info, CollateralCall>,
}

pub fn handle_dispute_call(ctx: Context<DisputeCall>, dispute_ref: [u8; 32]) -> Result<()> {
    let a = &ctx.accounts.agreement;
    // 1–3. The agreement's delivery call, disputed by its giver.
    bound(&ctx.accounts.fund_state, a, &ctx.accounts.call)?;
    require_keys_eq!(ctx.accounts.signer.key(), a.giver, CollateralError::Unauthorized);
    require!(ctx.accounts.call.is_delivery(), CollateralError::WrongCallKind);
    // 4. Still Open, after storing a Met that coverage already reached (rolled back on failure).
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    settle_status(&mut ctx.accounts.call, a, now);
    require!(ctx.accounts.call.status_is(CallStatus::Open), CollateralError::CallNotOpen);
    // 5.
    require!(dispute_ref != [0; 32], CollateralError::ZeroExternalRef);

    // Deadlines are unchanged: a dispute only blocks enforcement (F005).
    let call = &mut ctx.accounts.call;
    call.status = CallStatus::Disputed.as_u8();
    call.dispute_ref = dispute_ref;
    call.disputed_at = now;
    emit_cpi!(CallDisputed {
        fund: a.fund_state,
        agreement: a.key(),
        call: call.key(),
        seq: call.seq,
        dispute_ref,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}

pub fn handle_withdraw_dispute(ctx: Context<DisputeCall>) -> Result<()> {
    let a = &ctx.accounts.agreement;
    // 1–3. The giver withdraws its own dispute.
    bound(&ctx.accounts.fund_state, a, &ctx.accounts.call)?;
    require_keys_eq!(ctx.accounts.signer.key(), a.giver, CollateralError::Unauthorized);
    let clock = Clock::get()?;
    settle_status(&mut ctx.accounts.call, a, clock.unix_timestamp);
    require!(ctx.accounts.call.status_is(CallStatus::Disputed), CollateralError::NotDisputed);

    let call = &mut ctx.accounts.call;
    call.status = CallStatus::Open.as_u8();
    emit_cpi!(DisputeWithdrawn {
        fund: a.fund_state,
        agreement: a.key(),
        call: call.key(),
        seq: call.seq,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}

pub fn handle_resolve_dispute(
    ctx: Context<ResolveDispute>,
    outcome: u8,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Result<()> {
    // 1. Only governance (which has checked compliance, the fund and the reason).
    let caller = Pubkey::find_program_address(&[seeds::COLLATERAL_CALLER], &GOVERNANCE_PROGRAM_ID).0;
    require_keys_eq!(
        ctx.accounts.collateral_caller.key(),
        caller,
        CollateralError::UnauthorizedCaller
    );
    // 2–4.
    bound(&ctx.accounts.fund_state, &ctx.accounts.agreement, &ctx.accounts.call)?;
    require!(
        outcome == CallStatus::Open.as_u8() || outcome == CallStatus::Cancelled.as_u8(),
        CollateralError::InvalidOutcome
    );
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    settle_status(&mut ctx.accounts.call, &ctx.accounts.agreement, now);
    require!(ctx.accounts.call.status_is(CallStatus::Disputed), CollateralError::NotDisputed);

    let call = &mut ctx.accounts.call;
    call.status = outcome;
    let a = &mut ctx.accounts.agreement;
    if outcome == CallStatus::Cancelled.as_u8() {
        call.closed_at = now;
        uncount_delivery(a, call.value_delta)?;
    }
    emit_cpi!(DisputeResolved {
        fund: a.fund_state,
        agreement: a.key(),
        call: call.key(),
        seq: call.seq,
        outcome,
        reason,
        case_ref_hash,
        actor,
        called_outstanding: a.called_outstanding,
        open_delivery_calls: a.open_delivery_calls,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
