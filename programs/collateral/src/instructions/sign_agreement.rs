//! `sign_agreement()` (contracts/collateral.md, FR-003, FR-004): the party that didn't propose
//! signs, and the agreement becomes Active.

use anchor_lang::prelude::*;
use mmf_common::enums::AgreementStatus;

use crate::errors::CollateralError;
use crate::events::AgreementActive;
use crate::state::PledgeAgreement;
use crate::valuation::{load_fund, load_holder, require_eligible};

#[event_cpi]
#[derive(Accounts)]
pub struct SignAgreement<'info> {
    pub signer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    /// CHECK: HolderState of `agreement.giver`, via `HolderStateView` (`WrongHolder`).
    pub giver_holder: UncheckedAccount<'info>,
    /// CHECK: HolderState of `agreement.taker`, via `HolderStateView` (`WrongHolder`).
    pub taker_holder: UncheckedAccount<'info>,
}

pub fn handle_sign_agreement(ctx: Context<SignAgreement>) -> Result<()> {
    let a = &ctx.accounts;
    let ag = &a.agreement;
    // 1. The agreement's own FundState.
    require_keys_eq!(
        a.fund_state.key(),
        ag.fund_state,
        CollateralError::FundMismatch
    );
    let fund = load_fund(&a.fund_state, ag)?;
    // 2–4. Proposed, signed by the other party, once.
    require!(
        ag.status == AgreementStatus::Proposed.as_u8(),
        CollateralError::AgreementNotProposed
    );
    let signer = a.signer.key();
    let is_giver = signer == ag.giver;
    require!(
        is_giver || signer == ag.taker,
        CollateralError::Unauthorized
    );
    let already = if is_giver {
        ag.giver_signed
    } else {
        ag.taker_signed
    };
    require!(!already, CollateralError::AlreadySigned);
    // 5. Both parties are still eligible (FR-004).
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let giver = load_holder(&a.giver_holder, &fund.mint, &ag.giver)?;
    let taker = load_holder(&a.taker_holder, &fund.mint, &ag.taker)?;
    require_eligible(&giver, now)?;
    require_eligible(&taker, now)?;

    let ag = &mut ctx.accounts.agreement;
    if is_giver {
        ag.giver_signed = true;
    } else {
        ag.taker_signed = true;
    }
    ag.status = AgreementStatus::Active.as_u8();
    ag.activated_ts = now;
    emit_cpi!(AgreementActive {
        fund: ag.fund_state,
        agreement: ag.key(),
        signer,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
