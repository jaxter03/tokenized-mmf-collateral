//! `set_nav_override` / `clear_nav_override` (contracts/fund.md): called only by governance,
//! signed by its `["fund-caller"]` PDA, after compliance approved or withdrew a price override
//! (F002 FR-008). Governance has already checked the compliance role, the fund binding and the
//! reason; `fund` checks the caller and its own state.

use anchor_lang::prelude::*;
use mmf_common::enums::FundStatus;
use mmf_common::ids::GOVERNANCE_PROGRAM_ID;
use mmf_common::seeds;

use crate::errors::FundError;
use crate::events::{NavOverrideApproved, NavOverrideWithdrawn};
use crate::state::FundState;

#[event_cpi]
#[derive(Accounts)]
pub struct SetNavOverride<'info> {
    /// Must be governance's `["fund-caller"]` PDA (checked in the handler).
    pub fund_caller: Signer<'info>,
    #[account(mut)]
    pub fund_state: Account<'info, FundState>,
}

/// 1. Only governance's `["fund-caller"]` PDA; 2. only while Open.
fn check_caller_and_status(ctx: &Context<SetNavOverride>) -> Result<()> {
    let expected = Pubkey::find_program_address(&[seeds::FUND_CALLER], &GOVERNANCE_PROGRAM_ID).0;
    require_keys_eq!(
        ctx.accounts.fund_caller.key(),
        expected,
        FundError::UnauthorizedCaller
    );
    require!(
        ctx.accounts.fund_state.status == FundStatus::Open.as_u8(),
        FundError::FundNotOpen
    );
    Ok(())
}

pub fn handle_set_nav_override(
    ctx: Context<SetNavOverride>,
    nav: u64,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Result<()> {
    check_caller_and_status(&ctx)?;
    require!(nav > 0, FundError::ZeroPrice);
    // One at a time: replacing one is an explicit, recorded withdrawal first (research R5).
    require!(
        ctx.accounts.fund_state.pending_override_nav == 0,
        FundError::OverrideAlreadyPending
    );

    ctx.accounts.fund_state.pending_override_nav = nav;
    let clock = Clock::get()?;
    emit_cpi!(NavOverrideApproved {
        fund: ctx.accounts.fund_state.key(),
        nav,
        reason,
        case_ref_hash,
        actor,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}

pub fn handle_clear_nav_override(
    ctx: Context<SetNavOverride>,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Result<()> {
    check_caller_and_status(&ctx)?;
    let nav = ctx.accounts.fund_state.pending_override_nav;
    require!(nav != 0, FundError::NoOverridePending);

    ctx.accounts.fund_state.pending_override_nav = 0;
    let clock = Clock::get()?;
    emit_cpi!(NavOverrideWithdrawn {
        fund: ctx.accounts.fund_state.key(),
        nav,
        reason,
        case_ref_hash,
        actor,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
