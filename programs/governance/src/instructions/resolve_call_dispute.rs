//! `resolve_call_dispute` (contracts/governance.md, F004 FR-032, research R10): compliance ends a
//! dispute on a delivery call, reopening or cancelling it. Governance checks the role, the fund
//! and the reason, then CPIs into `collateral::resolve_dispute` with its `["collateral-caller"]`
//! PDA. The record in F004 is collateral's `DisputeResolved` event; F007 adds an ActionRecord of
//! kind ResolveDispute here.

use anchor_lang::prelude::*;
use collateral::program::Collateral;
use mmf_common::enums::{FundStatus, ReasonCode};
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::GovernanceError;
use crate::state::RoleRegistry;

#[derive(Accounts)]
pub struct ResolveCallDispute<'info> {
    pub compliance: Signer<'info>,
    pub role_registry: Account<'info, RoleRegistry>,
    /// CHECK: the fund bound to this role registry; decoded with `FundStateView`.
    #[account(address = role_registry.fund_state)]
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: the PledgeAgreement; collateral checks it belongs to this fund.
    #[account(mut)]
    pub agreement: UncheckedAccount<'info>,
    /// CHECK: the CollateralCall; collateral checks it belongs to the agreement.
    #[account(mut)]
    pub call: UncheckedAccount<'info>,
    /// CHECK: governance PDA `["collateral-caller"]`; signs the CPI.
    #[account(seeds = [seeds::COLLATERAL_CALLER], bump)]
    pub collateral_caller: UncheckedAccount<'info>,
    /// CHECK: collateral's `["__event_authority"]` PDA (for its emit_cpi! event).
    pub collateral_event_authority: UncheckedAccount<'info>,
    pub collateral_program: Program<'info, Collateral>,
}

pub fn handle_resolve_call_dispute(
    ctx: Context<ResolveCallDispute>,
    outcome: u8,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Result<()> {
    let a = &ctx.accounts;
    // 1. Only the compliance role (a Squads vault signs through Squads' execute). The fund
    // binding is Anchor's `address = role_registry.fund_state`; collateral checks the agreement
    // belongs to that fund.
    require_keys_eq!(
        a.compliance.key(),
        a.role_registry.compliance,
        GovernanceError::Unauthorized
    );
    // 2. Only on an Open fund (a missing fund fails closed).
    let fund = FundStateView::load(&a.fund_state, &a.role_registry.fund_state)
        .map_err(|_| error!(GovernanceError::FundNotOpen))?;
    require!(
        fund.status == FundStatus::Open.as_u8(),
        GovernanceError::FundNotOpen
    );
    // 3. The F001 reasons (research R10).
    let allowed = ReasonCode::try_from(reason)
        .map(|r| r.allowed_for_resolve_dispute())
        .unwrap_or(false);
    require!(allowed, GovernanceError::InvalidReason);

    let bump = [ctx.bumps.collateral_caller];
    let signer: &[&[&[u8]]] = &[&[seeds::COLLATERAL_CALLER, &bump]];
    collateral::cpi::resolve_dispute(
        CpiContext::new_with_signer(
            a.collateral_program.key(),
            collateral::cpi::accounts::ResolveDispute {
                collateral_caller: a.collateral_caller.to_account_info(),
                fund_state: a.fund_state.to_account_info(),
                agreement: a.agreement.to_account_info(),
                call: a.call.to_account_info(),
                event_authority: a.collateral_event_authority.to_account_info(),
                program: a.collateral_program.to_account_info(),
            },
            signer,
        ),
        outcome,
        reason,
        case_ref_hash,
        a.compliance.key(),
    )
}
