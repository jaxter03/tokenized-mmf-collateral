//! `issue_call(kind, external_ref)` (contracts/collateral.md, FR-014, FR-016): the taker demands
//! the uncalled shortfall (Delivery) or the giver asks for the excess back (Return).

use anchor_lang::prelude::*;
use mmf_common::collateral::shortfall;
use mmf_common::enums::CallKind;
use mmf_common::seeds;

use crate::call_account;
use crate::errors::CollateralError;
use crate::events::CallIssued;
use crate::state::{CollateralCall, PledgeAgreement};
use crate::valuation::{coverage, fresh, load_fund, load_holder, observe, require_eligible, snapshot};

#[event_cpi]
#[derive(Accounts)]
#[instruction(kind: u8, external_ref: [u8; 32])]
pub struct IssueCall<'info> {
    /// The taker for a Delivery call, the giver for a Return call.
    pub signer: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    /// CHECK: HolderState of the signer, via `HolderStateView`.
    pub signer_holder: UncheckedAccount<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + CollateralCall::INIT_SPACE,
        seeds = [seeds::CALL, agreement.key().as_ref(), &external_ref],
        bump
    )]
    pub call: Account<'info, CollateralCall>,
    pub system_program: Program<'info, System>,
}

pub fn handle_issue_call(ctx: Context<IssueCall>, kind: u8, external_ref: [u8; 32]) -> Result<()> {
    let agreement = ctx.accounts.agreement.key();
    let a = &ctx.accounts.agreement;
    // 1–2.
    let fund = load_fund(&ctx.accounts.fund_state, a)?;
    require!(a.is_active(), CollateralError::AgreementNotActive);
    // 3–4. A valid kind, signed by its party.
    let kind = CallKind::try_from(kind).map_err(|_| error!(CollateralError::InvalidCallKind))?;
    let party = match kind {
        CallKind::Delivery => a.taker,
        CallKind::Return => a.giver,
    };
    let signer = ctx.accounts.signer.key();
    require_keys_eq!(signer, party, CollateralError::Unauthorized);
    // 5–7.
    require!(external_ref != [0; 32], CollateralError::ZeroExternalRef);
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let holder = load_holder(&ctx.accounts.signer_holder, &fund.mint, &party)?;
    require_eligible(&holder, now)?;
    fresh(&fund, a, now)?;

    let a = &mut ctx.accounts.agreement;
    observe(a, &fund)?;
    let call = match kind {
        CallKind::Delivery => {
            // 8D. Only the uncalled shortfall, and only above the MTA.
            let uncalled = shortfall(coverage(&fund, a)?)
                .checked_sub(a.called_outstanding)
                .ok_or(error!(CollateralError::Overflow))?;
            require!(uncalled > a.min_transfer_value, CollateralError::NothingToCall);
            let call =
                call_account::delivery_call(a, agreement, &fund, now, uncalled, external_ref, ctx.bumps.call)?;
            call_account::count_delivery(a, uncalled)?;
            call
        }
        // T053: the Return branch replaces this placeholder.
        CallKind::Return => return err!(CollateralError::InvalidCallKind),
    };
    let opened = call_account::opened(&call, ctx.accounts.call.key());
    ctx.accounts.call.set_inner(call);

    let snap = snapshot(&ctx.accounts.agreement, &fund, true)?;
    emit_cpi!(CallIssued {
        fund: ctx.accounts.agreement.fund_state,
        agreement,
        call: opened,
        signer,
        snapshot: snap,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
