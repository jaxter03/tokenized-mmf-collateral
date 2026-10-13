//! Ending an instruction without settling it (research R3): the funded legs are earmarked for
//! their funders in the same step, and `return_leg` delivers them later. The signed token
//! movements themselves live in `token.rs`.

use anchor_lang::prelude::*;
use mmf_common::enums::SettlementStatus;

use crate::errors::DvpError;
use crate::state::SettlementInstruction;

/// The instruction belongs to `fund_state` (`FundMismatch`). Anchor has already checked it is a
/// genuine SettlementInstruction; only this program writes its `fund_state`.
pub fn require_fund(si: &SettlementInstruction, fund_state: &Pubkey) -> Result<()> {
    require_keys_eq!(si.fund_state, *fund_state, DvpError::FundMismatch);
    Ok(())
}

/// Moves an open instruction to `status` (Cancelled, Expired or Rejected), earmarking exactly what
/// it holds in escrow (data-model "Escrow held by an instruction"), computed before the status
/// changes.
pub fn earmark(si: &mut SettlementInstruction, status: SettlementStatus, now: i64) {
    debug_assert!(status.is_returnable() && si.status().is_open());
    si.units_to_return = si.units_held();
    si.cash_to_return = si.cash_held();
    si.status = status.as_u8();
    si.closed_at = now;
}
