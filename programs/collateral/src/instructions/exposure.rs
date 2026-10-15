//! `update_exposure` (taker) and `confirm_exposure` (giver) (contracts/collateral.md,
//! FR-011–FR-013, research R7). A raise that takes effect opens the delivery call it creates in
//! the same instruction: the named Constitution IV exception.

use anchor_lang::prelude::*;
use mmf_common::collateral::shortfall;
use mmf_common::enums::ExposureOutcome;
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::call_account;
use crate::errors::CollateralError;
use crate::events::{CallOpened, ExposureUpdated};
use crate::state::PledgeAgreement;
use crate::valuation::{
    coverage, fresh, is_fresh, load_fund, load_holder, observe, observe_covered, require_eligible,
    snapshot,
};

#[event_cpi]
#[derive(Accounts)]
#[instruction(required_value: u64, external_ref: [u8; 32])]
pub struct UpdateExposure<'info> {
    /// Must be the agreement's taker.
    pub signer: Signer<'info>,
    /// Pays for a call account if one opens.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    /// CHECK: HolderState of `agreement.taker`, via `HolderStateView`.
    pub signer_holder: UncheckedAccount<'info>,
    /// CHECK: PDA `["call", agreement, external_ref]`; created only if a call opens (R7).
    #[account(mut, seeds = [seeds::CALL, agreement.key().as_ref(), &external_ref], bump)]
    pub call: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[event_cpi]
#[derive(Accounts)]
#[instruction(required_value: u64, external_ref: [u8; 32])]
pub struct ConfirmExposure<'info> {
    /// Must be the agreement's giver.
    pub signer: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    /// CHECK: HolderState of `agreement.giver`, via `HolderStateView`.
    pub signer_holder: UncheckedAccount<'info>,
    /// CHECK: PDA `["call", agreement, external_ref]`; created only if a call opens.
    #[account(mut, seeds = [seeds::CALL, agreement.key().as_ref(), &external_ref], bump)]
    pub call: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Checks 1–4 shared by both instructions: the agreement's FundState, Active, the right party.
fn bound(
    fund_state: &AccountInfo,
    a: &PledgeAgreement,
    signer: Pubkey,
    party: Pubkey,
) -> Result<FundStateView> {
    let fund = load_fund(fund_state, a)?;
    require!(a.is_active(), CollateralError::AgreementNotActive);
    require_keys_eq!(signer, party, CollateralError::Unauthorized);
    Ok(fund)
}

fn clear_pending(a: &mut PledgeAgreement) {
    a.pending_raise_value = 0;
    a.pending_raise_ref = [0; 32];
    a.pending_raise_ts = 0;
}

/// A raise taking effect at a fresh price (the caller checked `fresh`): observe first, apply the
/// exposure, then open a Delivery call for the uncalled shortfall if it is more than the MTA.
#[allow(clippy::too_many_arguments)]
fn apply_raise<'info>(
    a: &mut PledgeAgreement,
    agreement: Pubkey,
    fund: &FundStateView,
    call_info: &AccountInfo<'info>,
    call_bump: u8,
    payer: &AccountInfo<'info>,
    system: &AccountInfo<'info>,
    required_value: u64,
    external_ref: [u8; 32],
    now: i64,
) -> Result<Option<CallOpened>> {
    observe(a, fund)?;
    a.required_value = required_value;
    a.exposure_ref = external_ref;
    a.exposure_updated_ts = now;
    clear_pending(a);
    let s = shortfall(coverage(fund, a)?);
    let uncalled = s
        .checked_sub(a.called_outstanding)
        .ok_or(error!(CollateralError::Overflow))?;
    if uncalled <= a.min_transfer_value {
        return Ok(None);
    }
    let call =
        call_account::delivery_call(a, agreement, fund, now, uncalled, external_ref, call_bump)?;
    call_account::create(call_info, payer, system, &agreement, &call)?;
    call_account::count_delivery(a, uncalled)?;
    Ok(Some(call_account::opened(&call, call_info.key())))
}

pub fn handle_update_exposure(
    ctx: Context<UpdateExposure>,
    required_value: u64,
    external_ref: [u8; 32],
) -> Result<()> {
    let agreement = ctx.accounts.agreement.key();
    let taker = ctx.accounts.agreement.taker;
    // 1–4.
    let fund = bound(
        &ctx.accounts.fund_state,
        &ctx.accounts.agreement,
        ctx.accounts.signer.key(),
        taker,
    )?;
    require!(external_ref != [0; 32], CollateralError::ZeroExternalRef);
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;

    let (outcome, opened_call, valued) = if required_value <= ctx.accounts.agreement.required_value
    {
        // 5. Lower or equal: at once, at any price age; never opens a call.
        let valued = is_fresh(&fund, &ctx.accounts.agreement, now);
        let a = &mut ctx.accounts.agreement;
        a.required_value = required_value;
        a.exposure_ref = external_ref;
        a.exposure_updated_ts = now;
        clear_pending(a);
        if valued {
            observe(a, &fund)?;
        } else if required_value == 0 {
            observe_covered(a);
        }
        (ExposureOutcome::Applied, None, valued)
    } else {
        // 5a. A raise needs an eligible taker.
        let holder = load_holder(&ctx.accounts.signer_holder, &fund.mint, &taker)?;
        require_eligible(&holder, now)?;
        if ctx.accounts.agreement.taker_may_call_unilaterally {
            // 5b–5c.
            fresh(&fund, &ctx.accounts.agreement, now)?;
            let call_info = ctx.accounts.call.to_account_info();
            let payer = ctx.accounts.payer.to_account_info();
            let system = ctx.accounts.system_program.to_account_info();
            let opened = apply_raise(
                &mut ctx.accounts.agreement,
                agreement,
                &fund,
                &call_info,
                ctx.bumps.call,
                &payer,
                &system,
                required_value,
                external_ref,
                now,
            )?;
            (ExposureOutcome::Applied, opened, true)
        } else {
            let a = &mut ctx.accounts.agreement;
            a.pending_raise_value = required_value;
            a.pending_raise_ref = external_ref;
            a.pending_raise_ts = now;
            (ExposureOutcome::Pending, None, false)
        }
    };

    let snap = snapshot(&ctx.accounts.agreement, &fund, valued)?;
    emit_cpi!(ExposureUpdated {
        fund: ctx.accounts.agreement.fund_state,
        agreement,
        required_value,
        external_ref,
        outcome: outcome.as_u8(),
        signer: taker,
        opened_call,
        snapshot: snap,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}

pub fn handle_confirm_exposure(
    ctx: Context<ConfirmExposure>,
    required_value: u64,
    external_ref: [u8; 32],
) -> Result<()> {
    let agreement = ctx.accounts.agreement.key();
    let giver = ctx.accounts.agreement.giver;
    // 1–3.
    let fund = bound(
        &ctx.accounts.fund_state,
        &ctx.accounts.agreement,
        ctx.accounts.signer.key(),
        giver,
    )?;
    // 4–5. Exactly the pending raise.
    let a = &ctx.accounts.agreement;
    require!(a.pending_raise_value != 0, CollateralError::NoPendingRaise);
    require!(
        required_value == a.pending_raise_value && external_ref == a.pending_raise_ref,
        CollateralError::PendingRaiseMismatch
    );
    // 6–7.
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let holder = load_holder(&ctx.accounts.signer_holder, &fund.mint, &giver)?;
    require_eligible(&holder, now)?;
    fresh(&fund, a, now)?;
    // 8 and the effect: as the unilateral raise, counted from now.
    let call_info = ctx.accounts.call.to_account_info();
    let payer = ctx.accounts.payer.to_account_info();
    let system = ctx.accounts.system_program.to_account_info();
    let opened_call = apply_raise(
        &mut ctx.accounts.agreement,
        agreement,
        &fund,
        &call_info,
        ctx.bumps.call,
        &payer,
        &system,
        required_value,
        external_ref,
        now,
    )?;

    let snap = snapshot(&ctx.accounts.agreement, &fund, true)?;
    emit_cpi!(ExposureUpdated {
        fund: ctx.accounts.agreement.fund_state,
        agreement,
        required_value,
        external_ref,
        outcome: ExposureOutcome::Confirmed.as_u8(),
        signer: giver,
        opened_call,
        snapshot: snap,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
