//! `cancel_call()` (contracts/collateral.md, FR-020): the taker withdraws a delivery call (Open
//! or Disputed), or the giver its open return call.

use anchor_lang::prelude::*;
use mmf_common::enums::{CallKind, CallStatus};

use crate::errors::CollateralError;
use crate::events::CallCancelled;
use crate::state::{CollateralCall, PledgeAgreement};
use crate::valuation::{load_fund, settle_status};

/// A delivery call leaves the agreement's open calls (cancelled, or cancelled on a dispute's
/// resolution): one fewer open call, and the demanded part falls by its value (not below zero),
/// to zero once none is left.
pub fn uncount_delivery(a: &mut PledgeAgreement, value: u64) -> Result<()> {
    a.open_delivery_calls = a
        .open_delivery_calls
        .checked_sub(1)
        .ok_or(error!(CollateralError::Overflow))?;
    a.called_outstanding -= a.called_outstanding.min(value);
    if a.open_delivery_calls == 0 {
        a.called_outstanding = 0;
    }
    Ok(())
}

#[event_cpi]
#[derive(Accounts)]
pub struct CancelCall<'info> {
    pub signer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    #[account(mut)]
    pub call: Account<'info, CollateralCall>,
}

pub fn handle_cancel_call(ctx: Context<CancelCall>) -> Result<()> {
    // 1. The agreement's FundState and one of its calls.
    load_fund(&ctx.accounts.fund_state, &ctx.accounts.agreement)?;
    require_keys_eq!(
        ctx.accounts.call.agreement,
        ctx.accounts.agreement.key(),
        CollateralError::FundMismatch
    );
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    // 2. A call already met by coverage is stored as Met first (the transaction rolls back if
    // a later check fails, so nothing is stored then).
    settle_status(&mut ctx.accounts.call, &ctx.accounts.agreement, now);
    let call = &ctx.accounts.call;
    let a = &ctx.accounts.agreement;
    // 3–4. Its party, while it is unresolved.
    let delivery = call.is_delivery();
    let party = if delivery { a.taker } else { a.giver };
    let signer = ctx.accounts.signer.key();
    require_keys_eq!(signer, party, CollateralError::Unauthorized);
    let open =
        call.status_is(CallStatus::Open) || (delivery && call.status_is(CallStatus::Disputed));
    require!(open, CollateralError::CallNotOpen);

    let (value, seq, kind) = (call.value_delta, call.seq, call.kind);
    let call = &mut ctx.accounts.call;
    call.status = CallStatus::Cancelled.as_u8();
    call.closed_at = now;
    let a = &mut ctx.accounts.agreement;
    if kind == CallKind::Delivery.as_u8() {
        uncount_delivery(a, value)?;
    } else {
        a.open_return_call = Pubkey::default();
    }
    emit_cpi!(CallCancelled {
        fund: a.fund_state,
        agreement: a.key(),
        call: ctx.accounts.call.key(),
        seq,
        kind,
        signer,
        called_outstanding: a.called_outstanding,
        open_delivery_calls: a.open_delivery_calls,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
