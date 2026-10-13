//! Reading a holder's interest without a transaction (F002 FR-017).

use compliance_hook::HolderState;
use fund::FundState;

/// Interest `holder` has earned so far, in base units × 10^18: what is stored plus what its
/// recorded balance has earned since its last settlement. Writes nothing. `None` only for
/// inconsistent accounts (the holder's checkpoint ahead of the fund's counter) or overflow.
pub fn earned_e18(holder: &HolderState, fund: &FundState) -> Option<u128> {
    mmf_common::accrual::earned_e18(
        holder.accrued_e18,
        holder.index_at_last_touch,
        holder.last_balance,
        fund.accrual_index,
    )
}
