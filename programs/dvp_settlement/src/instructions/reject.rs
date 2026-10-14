//! `reject(reason, case_ref_hash, actor)` (contracts/dvp_settlement.md, FR-025, research R11).
//! Called only by `governance::reject_settlement`, signed by governance's `["dvp-caller"]` PDA,
//! after governance has checked compliance's role, the fund and the reason.

use anchor_lang::prelude::*;
use mmf_common::enums::SettlementStatus;
use mmf_common::ids::GOVERNANCE_PROGRAM_ID;
use mmf_common::seeds;

use crate::errors::DvpError;
use crate::escrow;
use crate::events::SettlementRejected;
use crate::state::SettlementInstruction;

#[event_cpi]
#[derive(Accounts)]
pub struct Reject<'info> {
    /// Must be governance's `["dvp-caller"]` PDA (`UnauthorizedCaller`).
    pub dvp_caller: Signer<'info>,
    /// CHECK: the instruction's fund.
    pub fund_state: UncheckedAccount<'info>,
    /// A genuine instruction (owner and discriminator checked by Anchor); the handler checks it
    /// belongs to `fund_state` (`FundMismatch`).
    #[account(mut)]
    pub settlement: Account<'info, SettlementInstruction>,
}

pub fn handle_reject(
    ctx: Context<Reject>,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Result<()> {
    // 1. Only governance (which has checked compliance, the fund and the reason).
    let caller = Pubkey::find_program_address(&[seeds::DVP_CALLER], &GOVERNANCE_PROGRAM_ID).0;
    require_keys_eq!(
        ctx.accounts.dvp_caller.key(),
        caller,
        DvpError::UnauthorizedCaller
    );
    // 2–3.
    let fund = ctx.accounts.fund_state.key();
    escrow::require_fund(&ctx.accounts.settlement, &fund)?;
    require!(
        ctx.accounts.settlement.status().is_open(),
        DvpError::InstructionFinal
    );

    let clock = Clock::get()?;
    let si = &mut ctx.accounts.settlement;
    escrow::earmark(si, SettlementStatus::Rejected, clock.unix_timestamp);
    emit_cpi!(SettlementRejected {
        fund,
        seq: si.seq,
        reason,
        case_ref_hash,
        actor,
        units_to_return: si.units_to_return,
        cash_to_return: si.cash_to_return,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
