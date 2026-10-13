//! `reject_settlement` (contracts/governance.md, F003 FR-025, research R11): compliance rejects an
//! open settlement instruction. Governance checks the role, the fund and the reason, then CPIs into
//! `dvp_settlement::reject` with its `["dvp-caller"]` PDA. The record in F003 is dvp's
//! `SettlementRejected` event (shared-model I8); F007 adds an ActionRecord of kind
//! RejectSettlement here.

use anchor_lang::prelude::*;
use dvp_settlement::program::DvpSettlement;
use mmf_common::enums::{FundStatus, ReasonCode};
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::GovernanceError;
use crate::state::RoleRegistry;

#[derive(Accounts)]
pub struct RejectSettlement<'info> {
    pub compliance: Signer<'info>,
    pub role_registry: Account<'info, RoleRegistry>,
    /// CHECK: the fund bound to this role registry; decoded with `FundStateView`.
    #[account(address = role_registry.fund_state)]
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: the SettlementInstruction; dvp_settlement checks it belongs to this fund.
    #[account(mut)]
    pub settlement: UncheckedAccount<'info>,
    /// CHECK: governance PDA `["dvp-caller"]`; signs the CPI.
    #[account(seeds = [seeds::DVP_CALLER], bump)]
    pub dvp_caller: UncheckedAccount<'info>,
    /// CHECK: dvp_settlement's `["__event_authority"]` PDA (for its emit_cpi! event).
    pub dvp_event_authority: UncheckedAccount<'info>,
    pub dvp_settlement_program: Program<'info, DvpSettlement>,
}

pub fn handle_reject_settlement(
    ctx: Context<RejectSettlement>,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Result<()> {
    let a = &ctx.accounts;
    // 1. Only the compliance role (a Squads vault signs through Squads' execute). The fund
    // binding is Anchor's `address = role_registry.fund_state`: `["dvp-caller"]` is shared by
    // every fund, and dvp_settlement checks the instruction belongs to that fund.
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
    // 3. The F001 reasons (research R11).
    let allowed = ReasonCode::try_from(reason)
        .map(|r| r.allowed_for_reject_settlement())
        .unwrap_or(false);
    require!(allowed, GovernanceError::InvalidReason);

    let bump = [ctx.bumps.dvp_caller];
    let signer: &[&[&[u8]]] = &[&[seeds::DVP_CALLER, &bump]];
    dvp_settlement::cpi::reject(
        CpiContext::new_with_signer(
            a.dvp_settlement_program.key(),
            dvp_settlement::cpi::accounts::Reject {
                dvp_caller: a.dvp_caller.to_account_info(),
                fund_state: a.fund_state.to_account_info(),
                settlement: a.settlement.to_account_info(),
                event_authority: a.dvp_event_authority.to_account_info(),
                program: a.dvp_settlement_program.to_account_info(),
            },
            signer,
        ),
        reason,
        case_ref_hash,
        a.compliance.key(),
    )
}
