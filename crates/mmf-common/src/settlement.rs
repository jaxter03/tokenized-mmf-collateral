//! Settlement formulas (F003; data-model.md "Units and formulas", shared-model §8).
//!
//! One implementation shared by `dvp_settlement`, `fund` and the SDK; the tests' reference model
//! keeps its own copy (research R18). Every function is pure, uses checked `u128` math and
//! returns `None` on overflow, so callers fail closed (Constitution VII). Each division states
//! its rounding direction (Constitution IX).

use crate::accrual::{utc_day, SECONDS_PER_DAY};

/// Price scale: a price is cash per whole unit × 10^8 (shared-model §1).
pub const PRICE_SCALE: u128 = 100_000_000;

/// Basis points in 100 %.
pub const BPS: u128 = 10_000;

/// Default instruction expiry when `init_dvp` is given none: 24 hours (Clarification Q2).
pub const DEFAULT_EXPIRY_SECS: u32 = 86_400;

/// Cash a subscriber pays for `units`: `ceil(units × nav / 10^8)`. Rounds up, so the Fund is
/// never under-paid. `None` on overflow or if the result doesn't fit `u64`.
pub fn subscription_cash(units: u64, nav: u64) -> Option<u64> {
    let product = u128::from(units).checked_mul(u128::from(nav))?;
    let cash = product.div_ceil(PRICE_SCALE);
    u64::try_from(cash).ok()
}

/// Cash a redeemer receives for `units`: `floor(units × nav / 10^8)`. Rounds down, so the Fund
/// never over-pays. `None` on overflow or if the result doesn't fit `u64`.
pub fn redemption_cash(units: u64, nav: u64) -> Option<u64> {
    let product = u128::from(units).checked_mul(u128::from(nav))?;
    u64::try_from(product / PRICE_SCALE).ok()
}

/// Daily redemption limit (FR-021): `(redeemed_today + units) × 10_000 ≤ at_day_start × gate_bps`.
/// Cross-multiplied, so there is no division and a redemption exactly at the limit passes.
/// `None` on overflow.
pub fn redemption_ok(
    redeemed_today: u64,
    units: u64,
    at_day_start: u64,
    gate_bps: u16,
) -> Option<bool> {
    let after = u128::from(redeemed_today).checked_add(u128::from(units))?;
    let lhs = after.checked_mul(BPS)?;
    let rhs = u128::from(at_day_start).checked_mul(u128::from(gate_bps))?;
    Some(lhs <= rhs)
}

/// The latest price may be used (FR-007): a price exists, and its covered period ended no
/// earlier than yesterday (`nav_period_end_day + 1 ≥ today`).
pub fn price_fresh(nav_seq: u64, nav_period_end_day: u32, today: u32) -> bool {
    nav_seq > 0 && u64::from(nav_period_end_day) + 1 >= u64::from(today)
}

/// 00:00 UTC of the day containing `ts` (the redemption day's start, research R9). `None` before
/// 1970.
pub fn day_start(ts: i64) -> Option<i64> {
    i64::from(utc_day(ts)?).checked_mul(SECONDS_PER_DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_DOLLAR: u64 = 100_000_000;

    #[test]
    fn spec_amounts_at_one_dollar() {
        // US1: 2 000 000 units (6 dp) cost $2M (6 dp); US2: 500 000 units pay $500 000.
        assert_eq!(
            subscription_cash(2_000_000_000_000, ONE_DOLLAR),
            Some(2_000_000_000_000)
        );
        assert_eq!(
            redemption_cash(2_000_000_000_000, ONE_DOLLAR),
            Some(2_000_000_000_000)
        );
        assert_eq!(
            redemption_cash(500_000_000_000, ONE_DOLLAR),
            Some(500_000_000_000)
        );
    }

    #[test]
    fn rounding_favours_the_fund() {
        // 1 base unit at $0.99999999: product 99_999_999 < 10^8.
        assert_eq!(subscription_cash(1, 99_999_999), Some(1));
        assert_eq!(redemption_cash(1, 99_999_999), Some(0));
        // 1_000_001 × 99_999_999 = 100_000_098_999_999 → 1_000_000.98999999.
        assert_eq!(subscription_cash(1_000_001, 99_999_999), Some(1_000_001));
        assert_eq!(redemption_cash(1_000_001, 99_999_999), Some(1_000_000));
        // An exact product doesn't round.
        assert_eq!(subscription_cash(3, 200_000_000), Some(6));
        assert_eq!(redemption_cash(0, ONE_DOLLAR), Some(0));
    }

    #[test]
    fn cash_overflow_is_none() {
        // u64::MAX units at $2.00 is 2 × u64::MAX cash: doesn't fit u64.
        assert_eq!(subscription_cash(u64::MAX, 2 * ONE_DOLLAR), None);
        assert_eq!(redemption_cash(u64::MAX, 2 * ONE_DOLLAR), None);
        // u64::MAX at $1.00 still fits.
        assert_eq!(redemption_cash(u64::MAX, ONE_DOLLAR), Some(u64::MAX));
    }

    #[test]
    fn redemption_limit_boundary() {
        // 10 % of 1 000 000 = 100 000.
        assert_eq!(redemption_ok(0, 100_000, 1_000_000, 1_000), Some(true));
        assert_eq!(redemption_ok(0, 100_001, 1_000_000, 1_000), Some(false));
        assert_eq!(redemption_ok(60_000, 40_000, 1_000_000, 1_000), Some(true));
        assert_eq!(redemption_ok(60_000, 40_001, 1_000_000, 1_000), Some(false));
        // First day: nothing outstanding at the start of the day, so nothing can be redeemed.
        assert_eq!(redemption_ok(0, 1, 0, 10_000), Some(false));
        assert_eq!(redemption_ok(0, 0, 0, 10_000), Some(true));
        // 100 % limit allows everything outstanding at the day's start.
        assert_eq!(redemption_ok(0, u64::MAX, u64::MAX, 10_000), Some(true));
        assert_eq!(redemption_ok(u64::MAX, 1, u64::MAX, 10_000), Some(false));
    }

    #[test]
    fn price_freshness() {
        let today = 20_741;
        assert!(price_fresh(1, today, today), "covers today");
        assert!(price_fresh(1, today - 1, today), "ended yesterday");
        assert!(
            !price_fresh(1, today - 2, today),
            "ended the day before yesterday"
        );
        assert!(
            price_fresh(3, today + 2, today),
            "covers ahead (Friday's 3-day record)"
        );
        assert!(!price_fresh(0, today, today), "no publish yet");
        assert!(
            price_fresh(1, u32::MAX, 0),
            "no overflow at the top of the range"
        );
    }

    #[test]
    fn day_start_is_midnight_utc() {
        // 2026-10-13 00:00 UTC = day 20 739.
        let midnight = 20_739 * 86_400;
        assert_eq!(day_start(midnight), Some(midnight));
        assert_eq!(day_start(midnight + 86_399), Some(midnight));
        assert_eq!(day_start(midnight + 86_400), Some(midnight + 86_400));
        assert_eq!(day_start(-1), None);
    }
}
