//! `cancel(side)` and `withdraw_cancel(side)` (contracts/dvp_settlement.md, FR-023, research R17).
//! A side's party cancels an instruction that isn't fully funded; an Affirmed one is cancelled
//! only once both sides have asked. Cancelling moves no tokens: funded legs are earmarked and
//! returned by `return_leg` (research R3).

use anchor_lang::prelude::*;
use mmf_common::enums::{SettlementStatus, Side};
use mmf_common::views::{FundStateView, RoleRegistryView};

use crate::errors::DvpError;
use crate::escrow;
use crate::events::{CancelRequestWithdrawn, CancelRequested, SettlementCancelled};
use crate::state::SettlementInstruction;

#[event_cpi]
#[derive(Accounts)]
pub struct Cancel<'info> {
    /// The side's party: the transfer agent for the Fund side, otherwise the seller or buyer.
    pub party: Signer<'info>,
    /// CHECK: decoded with `FundStateView`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at the fund's `role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// A genuine instruction (owner and discriminator checked by Anchor); the handler checks it
    /// belongs to `fund_state` (`FundMismatch`).
    #[account(mut)]
    pub settlement: Account<'info, SettlementInstruction>,
}

/// Checks 1–4, shared by `cancel` and `withdraw_cancel`. Returns the side.
fn check(ctx: &Context<Cancel>, side: u8) -> Result<Side> {
    let a = &ctx.accounts;
    let fund_key = a.fund_state.key();
    // 1. The instruction, its fund and the fund's role registry belong together.
    escrow::require_fund(&a.settlement, &fund_key)?;
    let fund = FundStateView::load(&a.fund_state, &fund_key)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    let roles = RoleRegistryView::load(&a.role_registry, &fund.role_registry)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    require_keys_eq!(roles.fund_state, fund_key, DvpError::FundMismatch);
    // 2–4.
    let side = Side::try_from(side).map_err(|_| error!(DvpError::InvalidSide))?;
    require!(a.settlement.status().is_open(), DvpError::InstructionFinal);
    require_keys_eq!(
        a.party.key(),
        a.settlement.side_party(side, &roles.transfer_agent),
        DvpError::Unauthorized
    );
    Ok(side)
}

pub fn handle_cancel(ctx: Context<Cancel>, side: u8) -> Result<()> {
    let side = check(&ctx, side)?;
    let clock = Clock::get()?;
    let fund = ctx.accounts.fund_state.key();
    let signer = ctx.accounts.party.key();
    let si = &mut ctx.accounts.settlement;
    let bit = SettlementInstruction::cancel_bit(side);

    // 5. An Affirmed instruction needs both sides' requests (research R17).
    if si.status() == SettlementStatus::Affirmed {
        require!(
            si.cancel_requests & bit == 0,
            DvpError::CancelAlreadyRequested
        );
        si.cancel_requests |= bit;
        if si.cancel_requests != 0b11 {
            emit_cpi!(CancelRequested {
                fund,
                seq: si.seq,
                side: side.as_u8(),
                signer,
                slot: clock.slot,
                unix_ts: clock.unix_timestamp,
            });
            return Ok(());
        }
    }

    escrow::earmark(si, SettlementStatus::Cancelled, clock.unix_timestamp);
    emit_cpi!(SettlementCancelled {
        fund,
        seq: si.seq,
        units_to_return: si.units_to_return,
        cash_to_return: si.cash_to_return,
        signer,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}

pub fn handle_withdraw_cancel(ctx: Context<Cancel>, side: u8) -> Result<()> {
    let side = check(&ctx, side)?;
    let si = &mut ctx.accounts.settlement;
    let bit = SettlementInstruction::cancel_bit(side);
    // 5. Only an Affirmed instruction's own pending request (the other side hasn't asked, or it
    // would already be Cancelled).
    require!(
        si.status() == SettlementStatus::Affirmed && si.cancel_requests == bit,
        DvpError::NoCancelRequest
    );
    si.cancel_requests = 0;
    let clock = Clock::get()?;
    emit_cpi!(CancelRequestWithdrawn {
        fund: ctx.accounts.fund_state.key(),
        seq: si.seq,
        side: side.as_u8(),
        signer: ctx.accounts.party.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
