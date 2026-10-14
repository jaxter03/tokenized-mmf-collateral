//! `propose_agreement(args)` (contracts/collateral.md, FR-001–FR-006): the giver or the taker
//! records the terms of their legal collateral agreement; the proposal counts as its signature.

use anchor_lang::prelude::*;
use mmf_common::enums::{AgreementStatus, FundStatus, IncomeTo, PledgeMode};
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::CollateralError;
use crate::events::AgreementProposed;
use crate::state::{PledgeAgreement, ProposeArgs};
use crate::valuation::{load_holder, require_eligible};

#[event_cpi]
#[derive(Accounts)]
#[instruction(args: ProposeArgs)]
pub struct ProposeAgreement<'info> {
    /// The giver or the taker.
    pub proposer: Signer<'info>,
    /// Pays the agreement's rent; may differ from the proposer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView` (owner, discriminator, address).
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: HolderState of `args.giver`, via `HolderStateView` (`WrongHolder`).
    pub giver_holder: UncheckedAccount<'info>,
    /// CHECK: HolderState of `args.taker`, via `HolderStateView` (`WrongHolder`).
    pub taker_holder: UncheckedAccount<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + PledgeAgreement::INIT_SPACE,
        seeds = [
            seeds::AGREEMENT,
            fund_state.key().as_ref(),
            args.giver.as_ref(),
            args.taker.as_ref(),
            &args.nonce.to_le_bytes(),
        ],
        bump
    )]
    pub agreement: Account<'info, PledgeAgreement>,
    pub system_program: Program<'info, System>,
}

/// Checks 5–11: the terms the system can enforce (FR-002).
fn check_terms(args: &ProposeArgs) -> Result<()> {
    require!(
        args.mode == PledgeMode::Lien.as_u8(),
        CollateralError::UnsupportedPledgeMode
    );
    require!(
        args.income_to == IncomeTo::Giver.as_u8(),
        CollateralError::UnsupportedIncomeRouting
    );
    require!(args.haircut_bps < 10_000, CollateralError::InvalidHaircut);
    require!(args.call_cure_secs > 0, CollateralError::InvalidCureTime);
    require!(
        args.concentration_cap_units > 0,
        CollateralError::InvalidPledgeLimit
    );
    require!(args.max_nav_age_secs > 0, CollateralError::InvalidPriceAge);
    require!(
        args.agreement_hash != [0; 32],
        CollateralError::EmptyAgreementHash
    );
    Ok(())
}

pub fn handle_propose_agreement(ctx: Context<ProposeAgreement>, args: ProposeArgs) -> Result<()> {
    let a = &ctx.accounts;
    // 1–2. A FundState, Open.
    let fund = FundStateView::load(&a.fund_state, &a.fund_state.key())
        .map_err(|_| error!(CollateralError::FundMismatch))?;
    require!(
        fund.status == FundStatus::Open.as_u8(),
        CollateralError::FundNotOpen
    );
    // 3–4. The proposer is one of two different parties.
    let proposer = a.proposer.key();
    require!(
        proposer == args.giver || proposer == args.taker,
        CollateralError::Unauthorized
    );
    require!(args.giver != args.taker, CollateralError::SameParty);
    // 5–11. Terms.
    check_terms(&args)?;
    // 12. Both parties are Approved investors with valid KYC (FR-004).
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let giver = load_holder(&a.giver_holder, &fund.mint, &args.giver)?;
    let taker = load_holder(&a.taker_holder, &fund.mint, &args.taker)?;
    require_eligible(&giver, now)?;
    require_eligible(&taker, now)?;

    let ag = &mut ctx.accounts.agreement;
    ag.set_inner(PledgeAgreement {
        version: PledgeAgreement::VERSION,
        fund_state: ctx.accounts.fund_state.key(),
        giver: args.giver,
        taker: args.taker,
        nonce: args.nonce,
        mode: args.mode,
        income_to: args.income_to,
        haircut_bps: args.haircut_bps,
        max_nav_age_secs: args.max_nav_age_secs,
        call_cure_secs: args.call_cure_secs,
        enforcement_grace_secs: args.enforcement_grace_secs,
        min_transfer_value: args.min_transfer_value,
        concentration_cap_units: args.concentration_cap_units,
        taker_may_call_unilaterally: args.taker_may_call_unilaterally,
        agreement_hash: args.agreement_hash,
        status: AgreementStatus::Proposed.as_u8(),
        giver_signed: proposer == args.giver,
        taker_signed: proposer == args.taker,
        pledged_units: 0,
        required_value: 0,
        pledge_count: 1,
        call_count: 1,
        created_ts: now,
        activated_ts: 0,
        pending_raise_value: 0,
        pending_raise_ref: [0; 32],
        pending_raise_ts: 0,
        exposure_ref: [0; 32],
        exposure_updated_ts: 0,
        called_outstanding: 0,
        open_delivery_calls: 0,
        met_below_seq: 0,
        open_return_call: Pubkey::default(),
        bump: ctx.bumps.agreement,
        _reserved: [0; 64],
    });
    emit_cpi!(AgreementProposed {
        fund: ctx.accounts.fund_state.key(),
        agreement: ctx.accounts.agreement.key(),
        proposer,
        args,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
