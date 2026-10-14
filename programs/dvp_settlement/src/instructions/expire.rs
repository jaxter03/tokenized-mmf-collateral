//! `expire()` (contracts/dvp_settlement.md, FR-024): anyone ends an open instruction at or after
//! its expiry. Works whatever the fund's status or the pause flag; funded legs are earmarked and
//! returned by `return_leg` (research R3).

use anchor_lang::prelude::*;
use mmf_common::enums::SettlementStatus;

use crate::errors::DvpError;
use crate::escrow;
use crate::events::SettlementExpired;
use crate::state::SettlementInstruction;

#[event_cpi]
#[derive(Accounts)]
pub struct Expire<'info> {
    /// Anyone; recorded only.
    pub triggered_by: Signer<'info>,
    /// CHECK: the instruction's fund (bound by the settlement's seeds and its `fund_state`).
    pub fund_state: UncheckedAccount<'info>,
    /// A genuine instruction (owner and discriminator checked by Anchor); the handler checks it
    /// belongs to `fund_state` (`FundMismatch`).
    #[account(mut)]
    pub settlement: Account<'info, SettlementInstruction>,
}

pub fn handle_expire(ctx: Context<Expire>) -> Result<()> {
    let fund = ctx.accounts.fund_state.key();
    // 1–3.
    escrow::require_fund(&ctx.accounts.settlement, &fund)?;
    require!(
        ctx.accounts.settlement.status().is_open(),
        DvpError::InstructionFinal
    );
    let clock = Clock::get()?;
    require!(
        clock.unix_timestamp >= ctx.accounts.settlement.expires_at_ts,
        DvpError::NotExpired
    );

    let si = &mut ctx.accounts.settlement;
    escrow::earmark(si, SettlementStatus::Expired, clock.unix_timestamp);
    emit_cpi!(SettlementExpired {
        fund,
        seq: si.seq,
        units_to_return: si.units_to_return,
        cash_to_return: si.cash_to_return,
        triggered_by: ctx.accounts.triggered_by.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
