//! Valuation, observation and party checks shared by the instructions (data-model.md "Units and
//! formulas", research R4, R5, R11). The arithmetic lives in `mmf_common::collateral`; this module
//! applies it to the accounts.

use anchor_lang::prelude::*;
use mmf_common::collateral::{self as math, Observed, PriceAge};
use mmf_common::enums::CallStatus;
use mmf_common::views::{FundStateView, HolderStateView};

use crate::errors::CollateralError;
use crate::events::Snapshot;
use crate::state::{CollateralCall, PledgeAgreement};

/// The Fund's latest price may value this agreement (`NoPrice` / `StalePrice` otherwise).
pub fn fresh(fund: &FundStateView, a: &PledgeAgreement, now: i64) -> Result<()> {
    match math::price_age(fund.nav_seq, fund.nav_ts, now, a.max_nav_age_secs) {
        PriceAge::Fresh => Ok(()),
        PriceAge::NoPrice => err!(CollateralError::NoPrice),
        PriceAge::Stale => err!(CollateralError::StalePrice),
    }
}

pub fn is_fresh(fund: &FundStateView, a: &PledgeAgreement, now: i64) -> bool {
    fresh(fund, a, now).is_ok()
}

/// Coverage with the given pledged units and exposure, at the Fund's latest price.
pub fn coverage_with(
    fund: &FundStateView,
    a: &PledgeAgreement,
    pledged_units: u64,
    required_value: u64,
) -> Result<i128> {
    math::coverage(pledged_units, fund.nav, a.haircut_bps, required_value)
        .ok_or_else(|| error!(CollateralError::Overflow))
}

/// The agreement's coverage at the Fund's latest price.
pub fn coverage(fund: &FundStateView, a: &PledgeAgreement) -> Result<i128> {
    coverage_with(fund, a, a.pledged_units, a.required_value)
}

fn apply(a: &mut PledgeAgreement, shortfall: u64) {
    let o = math::observe(
        Observed {
            called_outstanding: a.called_outstanding,
            open_delivery_calls: a.open_delivery_calls,
            met_below_seq: a.met_below_seq,
        },
        shortfall,
        a.call_count,
    );
    a.called_outstanding = o.called_outstanding;
    a.open_delivery_calls = o.open_delivery_calls;
    a.met_below_seq = o.met_below_seq;
}

/// Observe coverage at a fresh price (research R5): cap the demanded part at the shortfall, and
/// at zero shortfall make every delivery call issued so far Met. The caller has checked `fresh`.
pub fn observe(a: &mut PledgeAgreement, fund: &FundStateView) -> Result<()> {
    let s = math::shortfall(coverage(fund, a)?);
    apply(a, s);
    Ok(())
}

/// Observe a coverage known to be zero or more without a price (the required value is zero).
pub fn observe_covered(a: &mut PledgeAgreement) {
    apply(a, 0);
}

/// Store the derived status: a delivery call stored Open or Disputed below `met_below_seq` is
/// Met. Returns true if it changed the call.
pub fn settle_status(call: &mut CollateralCall, a: &PledgeAgreement, now: i64) -> bool {
    if call.is_delivery() && math::delivery_met(call.status, call.seq, a.met_below_seq) {
        call.status = CallStatus::Met.as_u8();
        call.closed_at = now;
        return true;
    }
    false
}

/// The agreement after a step, for its event. `valued` is false when no fresh price was used.
pub fn snapshot(a: &PledgeAgreement, fund: &FundStateView, valued: bool) -> Result<Snapshot> {
    Ok(Snapshot {
        valued,
        coverage: if valued { coverage(fund, a)? } else { 0 },
        nav_seq: fund.nav_seq,
        pledged_units: a.pledged_units,
        required_value: a.required_value,
        called_outstanding: a.called_outstanding,
        open_delivery_calls: a.open_delivery_calls,
        met_below_seq: a.met_below_seq,
    })
}

/// A party's holder record (`WrongHolder` if it isn't `owner`'s record for `mint`).
pub fn load_holder(info: &AccountInfo, mint: &Pubkey, owner: &Pubkey) -> Result<HolderStateView> {
    HolderStateView::load(info, mint, owner).map_err(|_| error!(CollateralError::WrongHolder))
}

/// Party check (research R11): an Approved Investor with valid KYC (`PartyNotEligible`).
pub fn require_eligible(holder: &HolderStateView, now: i64) -> Result<()> {
    require!(holder.eligible(now), CollateralError::PartyNotEligible);
    Ok(())
}

/// The agreement's FundState, checked against the address the agreement names (`FundMismatch`).
pub fn load_fund(info: &AccountInfo, a: &PledgeAgreement) -> Result<FundStateView> {
    FundStateView::load(info, &a.fund_state).map_err(|_| error!(CollateralError::FundMismatch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agreement() -> PledgeAgreement {
        PledgeAgreement {
            version: 1,
            fund_state: Pubkey::default(),
            giver: Pubkey::default(),
            taker: Pubkey::default(),
            nonce: 1,
            mode: 0,
            income_to: 0,
            haircut_bps: 200,
            max_nav_age_secs: 93_600,
            call_cure_secs: 14_400,
            enforcement_grace_secs: 7_200,
            min_transfer_value: 10_000_000_000,
            concentration_cap_units: 5_000_000_000_000,
            taker_may_call_unilaterally: true,
            agreement_hash: [7; 32],
            status: 1,
            giver_signed: true,
            taker_signed: true,
            pledged_units: 0,
            required_value: 1_000_000_000_000,
            pledge_count: 1,
            call_count: 2,
            created_ts: 0,
            activated_ts: 0,
            pending_raise_value: 0,
            pending_raise_ref: [0; 32],
            pending_raise_ts: 0,
            exposure_ref: [0; 32],
            exposure_updated_ts: 0,
            called_outstanding: 1_000_000_000_000,
            open_delivery_calls: 1,
            met_below_seq: 0,
            open_return_call: Pubkey::default(),
            bump: 0,
            _reserved: [0; 64],
        }
    }

    fn fund(nav: u64, nav_seq: u64, nav_ts: i64) -> FundStateView {
        FundStateView {
            mint: Pubkey::default(),
            role_registry: Pubkey::default(),
            status: 1,
            accrual_index: 0,
            cash_mint: Pubkey::default(),
            nav,
            nav_ts,
            nav_seq,
            nav_period_end_day: 0,
            units_outstanding: 0,
            cash_treasury: Pubkey::default(),
        }
    }

    #[test]
    fn freshness() {
        let a = agreement();
        assert!(fresh(&fund(100_000_000, 1, 0), &a, 93_600).is_ok());
        assert_eq!(
            fresh(&fund(100_000_000, 1, 0), &a, 93_601).unwrap_err(),
            error!(CollateralError::StalePrice)
        );
        assert_eq!(
            fresh(&fund(0, 0, 0), &a, 0).unwrap_err(),
            error!(CollateralError::NoPrice)
        );
    }

    #[test]
    fn observe_meets_once_covered() {
        let f = fund(100_000_000, 1, 0);
        let mut a = agreement();
        observe(&mut a, &f).unwrap();
        assert_eq!(
            a.called_outstanding, 1_000_000_000_000,
            "nothing pledged: unchanged"
        );
        a.pledged_units = 1_020_408_163_266;
        observe(&mut a, &f).unwrap();
        assert_eq!(
            (a.called_outstanding, a.open_delivery_calls, a.met_below_seq),
            (0, 0, 2)
        );
    }

    #[test]
    fn derived_status_is_stored() {
        let a = PledgeAgreement {
            met_below_seq: 2,
            ..agreement()
        };
        let mut call = CollateralCall {
            version: 1,
            agreement: Pubkey::default(),
            seq: 1,
            kind: 0,
            value_delta: 1,
            units_required: 1,
            nav_seq: 1,
            issued_at: 0,
            cure_deadline_ts: 0,
            enforce_after_ts: 0,
            status: CallStatus::Disputed.as_u8(),
            enforced_units: 0,
            enforced_value: 0,
            external_ref: [1; 32],
            units_released: 0,
            dispute_ref: [0; 32],
            disputed_at: 0,
            closed_at: 0,
            bump: 0,
            _reserved: [0; 64],
        };
        assert!(settle_status(&mut call, &a, 9));
        assert_eq!((call.status, call.closed_at), (CallStatus::Met.as_u8(), 9));
        assert!(!settle_status(&mut call, &a, 10), "already Met");
        call.kind = 1;
        call.status = CallStatus::Open.as_u8();
        assert!(
            !settle_status(&mut call, &a, 10),
            "return calls are never derived Met"
        );
    }
}
