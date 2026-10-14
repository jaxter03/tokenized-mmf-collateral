//! Collateral valuation (F004; data-model.md "Units and formulas", shared-model §8, research R4,
//! R5).
//!
//! One implementation shared by `collateral` and the SDK; the tests' reference model keeps its
//! own copy (research R15). Every function is pure and uses checked `u128` math. A result that
//! overflows or doesn't fit returns `None`, so callers fail closed (Constitution VII). Each
//! division states its rounding direction (Constitution IX).

use crate::settlement::{BPS, PRICE_SCALE};

/// `CollateralCall.status` values the derived-Met rule looks at (shared-model §4.4).
const CALL_OPEN: u8 = 0;
const CALL_DISPUTED: u8 = 2;

/// `nav × (10_000 − haircut_bps)`, the value of one base unit after the haircut, scaled by
/// `10^8 × 10_000`. `None` for a haircut of 100 % or more, or a zero price.
fn haircut_price(nav: u64, haircut_bps: u16) -> Option<u128> {
    let keep = BPS.checked_sub(u128::from(haircut_bps))?;
    let p = u128::from(nav).checked_mul(keep)?;
    (p > 0).then_some(p)
}

/// Collateral value of `units` at `nav` after the haircut:
/// `floor(units × nav × (10_000 − h) / (10^8 × 10_000))`. Rounds down, so the taker never
/// over-counts collateral. Taken once on an agreement's total pledged units.
pub fn collateral_value(units: u64, nav: u64, haircut_bps: u16) -> Option<u64> {
    let keep = BPS.checked_sub(u128::from(haircut_bps))?;
    let product = u128::from(units)
        .checked_mul(u128::from(nav))?
        .checked_mul(keep)?;
    u64::try_from(product / (PRICE_SCALE * BPS)).ok()
}

/// Units a delivery call requires for `value`:
/// `ceil(value × 10^8 × 10_000 / (nav × (10_000 − h)))`. Rounds up, so the giver always posts
/// enough.
pub fn units_for_value_up(value: u64, nav: u64, haircut_bps: u16) -> Option<u64> {
    let numerator = u128::from(value).checked_mul(PRICE_SCALE * BPS)?;
    u64::try_from(numerator.div_ceil(haircut_price(nav, haircut_bps)?)).ok()
}

/// Units a return call releases for `value`: the same formula rounded down, so the taker keeps
/// enough.
pub fn units_for_value_down(value: u64, nav: u64, haircut_bps: u16) -> Option<u64> {
    let numerator = u128::from(value).checked_mul(PRICE_SCALE * BPS)?;
    u64::try_from(numerator / haircut_price(nav, haircut_bps)?).ok()
}

/// `collateral_value(pledged_units) − required_value`: negative is a shortfall, positive an
/// excess.
pub fn coverage(pledged_units: u64, nav: u64, haircut_bps: u16, required_value: u64) -> Option<i128> {
    let value = collateral_value(pledged_units, nav, haircut_bps)?;
    i128::from(value).checked_sub(i128::from(required_value))
}

/// `max(0, −coverage)`.
pub fn shortfall(coverage: i128) -> u64 {
    u64::try_from(coverage.min(0).unsigned_abs()).unwrap_or(u64::MAX)
}

/// `max(0, coverage)`.
pub fn excess(coverage: i128) -> u64 {
    u64::try_from(coverage.max(0)).unwrap_or(u64::MAX)
}

/// Whether the Fund's latest price may value an agreement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceAge {
    Fresh,
    /// No price has been published yet.
    NoPrice,
    /// Published more than the agreement's maximum price age ago.
    Stale,
}

/// Fresh iff a price exists and `now − nav_ts ≤ max_age_secs` (exactly at the maximum age is
/// fresh).
pub fn price_age(nav_seq: u64, nav_ts: i64, now: i64, max_age_secs: u32) -> PriceAge {
    if nav_seq == 0 {
        return PriceAge::NoPrice;
    }
    match now.checked_sub(nav_ts) {
        Some(age) if age <= i64::from(max_age_secs) => PriceAge::Fresh,
        _ => PriceAge::Stale,
    }
}

/// The agreement's call bookkeeping that `observe` updates (shared-model §4.4, invariant I16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observed {
    /// The demanded part of the shortfall.
    pub called_outstanding: u64,
    pub open_delivery_calls: u16,
    pub met_below_seq: u64,
}

/// Observe coverage (research R5): the demanded part never exceeds the current shortfall `S`,
/// and once `S == 0` every delivery call issued so far is Met.
pub fn observe(state: Observed, shortfall: u64, call_count: u64) -> Observed {
    if shortfall == 0 {
        return Observed {
            called_outstanding: 0,
            open_delivery_calls: 0,
            met_below_seq: call_count,
        };
    }
    Observed {
        called_outstanding: state.called_outstanding.min(shortfall),
        ..state
    }
}

/// Derived status: a delivery call stored Open or Disputed whose `seq` is below
/// `met_below_seq` is Met.
pub fn delivery_met(stored_status: u8, seq: u64, met_below_seq: u64) -> bool {
    matches!(stored_status, CALL_OPEN | CALL_DISPUTED) && seq < met_below_seq
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_DOLLAR: u64 = 100_000_000;
    const H: u16 = 200;

    #[test]
    fn worked_figures() {
        // data-model "Worked checks": $1M at $1.00 and 2 % needs 1,020,408.163266 units.
        assert_eq!(
            units_for_value_up(1_000_000_000_000, ONE_DOLLAR, H),
            Some(1_020_408_163_266)
        );
        assert_eq!(
            collateral_value(1_020_408_163_266, ONE_DOLLAR, H),
            Some(1_000_000_000_000)
        );
        // US4: a $600 000 excess returns 612,244.897959 units; 408,163.265307 remain, worth $400 000.
        assert_eq!(
            units_for_value_down(600_000_000_000, ONE_DOLLAR, H),
            Some(612_244_897_959)
        );
        assert_eq!(1_020_408_163_266u64 - 612_244_897_959, 408_163_265_307);
        assert_eq!(
            collateral_value(408_163_265_307, ONE_DOLLAR, H),
            Some(400_000_000_000)
        );
        // Price drops: $0.99 leaves exactly the MTA short; $0.985 leaves $15 000 short.
        assert_eq!(
            coverage(1_020_408_163_266, 99_000_000, H, 1_000_000_000_000),
            Some(-10_000_000_000)
        );
        assert_eq!(
            coverage(1_020_408_163_266, 98_500_000, H, 1_000_000_000_000),
            Some(-15_000_000_000)
        );
        assert_eq!(
            units_for_value_up(15_000_000_000, 98_500_000, H),
            Some(15_539_210_609)
        );
    }

    #[test]
    fn rounding_directions() {
        // An exact multiple doesn't round (haircut 0).
        assert_eq!(units_for_value_up(1_000, ONE_DOLLAR, 0), Some(1_000));
        assert_eq!(units_for_value_down(1_000, ONE_DOLLAR, 0), Some(1_000));
        // One base unit of value at 2 %: 1 / 0.98 = 1.0204… → up 2, down 1.
        assert_eq!(units_for_value_up(1, ONE_DOLLAR, H), Some(2));
        assert_eq!(units_for_value_down(1, ONE_DOLLAR, H), Some(1));
        // Value rounds down: 1 unit at 2 % is worth 0.98 → 0.
        assert_eq!(collateral_value(1, ONE_DOLLAR, H), Some(0));
        // Above $1.00 with no haircut, one base unit of value buys less than one unit.
        assert_eq!(units_for_value_down(1, 101_000_000, 0), Some(0));
        assert_eq!(units_for_value_up(1, 101_000_000, 0), Some(1));
    }

    #[test]
    fn haircut_bounds_and_overflow() {
        assert_eq!(units_for_value_up(1, ONE_DOLLAR, 9_999), Some(10_000));
        assert_eq!(collateral_value(10_000, ONE_DOLLAR, 9_999), Some(1));
        assert_eq!(units_for_value_up(1, ONE_DOLLAR, 10_000), None);
        assert_eq!(units_for_value_down(1, ONE_DOLLAR, 10_001), None);
        assert_eq!(collateral_value(1, ONE_DOLLAR, 10_001), None);
        assert_eq!(units_for_value_up(1, 0, H), None);
        // u64::MAX of value at a tiny price needs more units than u64 holds.
        assert_eq!(units_for_value_up(u64::MAX, 1, H), None);
        // u64::MAX units at $2.00 are worth more than u64 holds.
        assert_eq!(collateral_value(u64::MAX, 2 * ONE_DOLLAR, 0), None);
        assert_eq!(collateral_value(u64::MAX, ONE_DOLLAR, 0), Some(u64::MAX));
    }

    #[test]
    fn shortfall_and_excess() {
        assert_eq!(shortfall(-5), 5);
        assert_eq!(shortfall(0), 0);
        assert_eq!(shortfall(7), 0);
        assert_eq!(excess(7), 7);
        assert_eq!(excess(-7), 0);
        assert_eq!(shortfall(i128::from(u64::MAX) * -2), u64::MAX);
    }

    #[test]
    fn price_age_boundaries() {
        let max = 93_600;
        assert_eq!(price_age(0, 0, 100, max), PriceAge::NoPrice);
        assert_eq!(price_age(1, 1_000, 1_000 + 93_600, max), PriceAge::Fresh);
        assert_eq!(price_age(1, 1_000, 1_000 + 93_601, max), PriceAge::Stale);
        assert_eq!(price_age(1, 1_000, 500, max), PriceAge::Fresh, "clock behind the publish");
        assert_eq!(price_age(1, i64::MIN, i64::MAX, max), PriceAge::Stale, "no overflow");
    }

    #[test]
    fn observe_caps_and_meets() {
        let before = Observed {
            called_outstanding: 1_000,
            open_delivery_calls: 2,
            met_below_seq: 1,
        };
        // Shortfall above the demanded part: unchanged.
        assert_eq!(observe(before, 1_500, 3), before);
        // Equal: unchanged.
        assert_eq!(observe(before, 1_000, 3), before);
        // Below: capped, calls stay open.
        assert_eq!(
            observe(before, 400, 3),
            Observed {
                called_outstanding: 400,
                ..before
            }
        );
        // Covered: every call issued so far is Met.
        assert_eq!(
            observe(before, 0, 3),
            Observed {
                called_outstanding: 0,
                open_delivery_calls: 0,
                met_below_seq: 3
            }
        );
    }

    #[test]
    fn derived_met() {
        for (status, met) in [(0, true), (1, false), (2, true), (3, false), (4, false), (5, false)] {
            assert_eq!(delivery_met(status, 1, 2), met, "status {status} below");
        }
        assert!(!delivery_met(0, 2, 2), "seq at the boundary is not covered");
        assert!(!delivery_met(0, 5, 2));
    }
}
