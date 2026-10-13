//! Price override (contracts/governance.md, F002 FR-008): compliance approves or withdraws the one
//! price the next publish may use beyond the fund's daily move limit. Governance checks the role,
//! the fund and the reason, then CPIs into `fund` with its `["fund-caller"]` PDA. The record in
//! F002 is fund's event (shared-model I8); F007 adds an ActionRecord of kind NavOverride here.

use anchor_lang::prelude::*;
use fund::program::Fund;
use mmf_common::enums::{FundStatus, ReasonCode};
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::GovernanceError;
use crate::state::RoleRegistry;

#[derive(Accounts)]
pub struct NavOverrideAction<'info> {
    pub compliance: Signer<'info>,
    pub role_registry: Account<'info, RoleRegistry>,
    /// CHECK: the fund bound to this role registry; decoded with `FundStateView`, written by fund.
    #[account(mut, address = role_registry.fund_state)]
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: governance PDA `["fund-caller"]`; signs the CPI.
    #[account(seeds = [seeds::FUND_CALLER], bump)]
    pub fund_caller: UncheckedAccount<'info>,
    /// CHECK: fund's `["__event_authority"]` PDA (for its emit_cpi! event).
    pub fund_event_authority: UncheckedAccount<'info>,
    pub fund_program: Program<'info, Fund>,
}

/// Checks in contract order: Unauthorized → FundNotOpen → InvalidReason. The fund binding is
/// Anchor's `address = role_registry.fund_state` constraint: `["fund-caller"]` is shared by every
/// fund, so one fund's compliance must not reach another fund's state.
fn check(ctx: &Context<NavOverrideAction>, reason: u8) -> Result<()> {
    let a = &ctx.accounts;
    // 1. Only the compliance role (a Squads vault signs through Squads' execute).
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

    // 3. OperationalError or Other (research R5).
    let allowed = ReasonCode::try_from(reason)
        .map(|r| r.allowed_for_nav_override())
        .unwrap_or(false);
    require!(allowed, GovernanceError::InvalidReason);
    Ok(())
}

fn fund_accounts<'info>(
    a: &NavOverrideAction<'info>,
) -> fund::cpi::accounts::SetNavOverride<'info> {
    fund::cpi::accounts::SetNavOverride {
        fund_caller: a.fund_caller.to_account_info(),
        fund_state: a.fund_state.to_account_info(),
        event_authority: a.fund_event_authority.to_account_info(),
        program: a.fund_program.to_account_info(),
    }
}

pub fn handle_approve_nav_override(
    ctx: Context<NavOverrideAction>,
    nav: u64,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Result<()> {
    check(&ctx, reason)?;
    let a = &ctx.accounts;
    let bump = [ctx.bumps.fund_caller];
    let signer: &[&[&[u8]]] = &[&[seeds::FUND_CALLER, &bump]];
    fund::cpi::set_nav_override(
        CpiContext::new(a.fund_program.key(), fund_accounts(a)).with_signer(signer),
        nav,
        reason,
        case_ref_hash,
        a.compliance.key(),
    )
}

pub fn handle_withdraw_nav_override(
    ctx: Context<NavOverrideAction>,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Result<()> {
    check(&ctx, reason)?;
    let a = &ctx.accounts;
    let bump = [ctx.bumps.fund_caller];
    let signer: &[&[&[u8]]] = &[&[seeds::FUND_CALLER, &bump]];
    fund::cpi::clear_nav_override(
        CpiContext::new(a.fund_program.key(), fund_accounts(a)).with_signer(signer),
        reason,
        case_ref_hash,
        a.compliance.key(),
    )
}
