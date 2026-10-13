//! Daily price and interest formulas (F002; data-model.md "Units and formulas", shared-model §8).
//!
//! One implementation shared by `fund`, the transfer hook and the SDK (research R11). Every
//! function is pure, uses checked `u128` math and returns `None` on overflow, so callers fail
//! closed (Constitution VII). Nothing here divides except `utc_day`, so interest is exact: no
//! rounding direction to choose until payout (F006).

/// Seconds in a UTC day.
pub const SECONDS_PER_DAY: i64 = 86_400;

/// `daily_rate_e12 × INDEX_PER_RATE` is one day's index increment: the rate has scale 10^12 and
/// the index 10^18 (shared-model §1).
pub const INDEX_PER_RATE: u128 = 1_000_000;

/// UTC calendar day number, `ts.div_euclid(86_400)` (research R2). `None` before 1970 or past
/// `u32::MAX` days.
pub fn utc_day(ts: i64) -> Option<u32> {
    u32::try_from(ts.div_euclid(SECONDS_PER_DAY)).ok()
}

/// Index increment of one publish: `rate × 10^6 × days_covered`. Exact.
pub fn index_increment(daily_rate_e12: u64, days_covered: u16) -> Option<u128> {
    u128::from(daily_rate_e12)
        .checked_mul(INDEX_PER_RATE)?
        .checked_mul(u128::from(days_covered))
}

/// Settles a holder at `index` on the balance held since its last settlement:
/// returns `(accrued_e18 + pre_balance × (index − index_at_last_touch), index)`. Exact.
/// `None` on overflow, or if `index` is behind the holder's checkpoint (the index never
/// decreases, so that would be corrupt state).
pub fn settle(
    accrued_e18: u128,
    index_at_last_touch: u128,
    pre_balance: u64,
    index: u128,
) -> Option<(u128, u128)> {
    let delta = index.checked_sub(index_at_last_touch)?;
    let earned = u128::from(pre_balance).checked_mul(delta)?;
    Some((accrued_e18.checked_add(earned)?, index))
}

/// A holder's interest earned so far, computed on read without writing anything (FR-017):
/// stored `accrued_e18` plus what its `last_balance` has earned since its last settlement.
pub fn earned_e18(
    accrued_e18: u128,
    index_at_last_touch: u128,
    last_balance: u64,
    accrual_index: u128,
) -> Option<u128> {
    settle(
        accrued_e18,
        index_at_last_touch,
        last_balance,
        accrual_index,
    )
    .map(|(e, _)| e)
}

/// Typo guard (FR-007): `|nav − prev| × 10_000 ≤ prev × max_nav_move_bps`, cross-multiplied so
/// there is no division and no boundary rounding. A move exactly at the limit passes.
pub fn price_move_ok(prev: u64, nav: u64, max_nav_move_bps: u16) -> bool {
    let diff = u128::from(prev.abs_diff(nav));
    // Both products fit in u128: u64 × 10_000 and u64 × u16.
    diff * 10_000 <= u128::from(prev) * u128::from(max_nav_move_bps)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// $1.00 with 8 decimals.
    const ONE_DOLLAR: u64 = 100_000_000;
    /// 4.5 % a year ÷ 365 ≈ 0.000123287671 per day, × 10^12.
    const RATE_4_5_PCT: u64 = 123_287_671;

    #[test]
    fn encodings() {
        assert_eq!(ONE_DOLLAR, 100_000_000);
        // FR-005: the rate is stored and used without rounding.
        assert_eq!(index_increment(RATE_4_5_PCT, 1), Some(123_287_671_000_000));
        assert_eq!(index_increment(RATE_4_5_PCT, 3), Some(369_863_013_000_000));
        assert_eq!(index_increment(0, 3), Some(0));
        assert!(index_increment(u64::MAX, u16::MAX).is_some());
    }

    #[test]
    fn utc_days() {
        // 2026-10-13 00:00:00 UTC.
        assert_eq!(utc_day(1_791_849_600), Some(20_739));
        assert_eq!(utc_day(1_791_849_600 + SECONDS_PER_DAY - 1), Some(20_739));
        assert_eq!(utc_day(1_791_849_600 + SECONDS_PER_DAY), Some(20_740));
        assert_eq!(utc_day(0), Some(0));
        assert_eq!(utc_day(-1), None);
        assert_eq!(utc_day(i64::MAX), None);
    }

    #[test]
    fn price_move_limit() {
        // 50 bps from $1.00 allows ±$0.005 exactly.
        assert!(price_move_ok(ONE_DOLLAR, 99_500_000, 50));
        assert!(price_move_ok(ONE_DOLLAR, 100_500_000, 50));
        assert!(!price_move_ok(ONE_DOLLAR, 99_499_999, 50));
        assert!(!price_move_ok(ONE_DOLLAR, 100_500_001, 50));
        assert!(price_move_ok(ONE_DOLLAR, ONE_DOLLAR, 1));
        // Extremes don't overflow.
        assert!(price_move_ok(u64::MAX, u64::MAX, u16::MAX));
        assert!(!price_move_ok(1, u64::MAX, u16::MAX));
    }

    #[test]
    fn settle_is_exact() {
        let index = index_increment(RATE_4_5_PCT, 1).unwrap();
        // 1 000 000 units (6 dp) for one day at 4.5 %/yr = 123.287671 units, exactly.
        let (earned, at) = settle(0, 0, 1_000_000_000_000, index).unwrap();
        assert_eq!(earned, 1_000_000_000_000 * 123_287_671_000_000);
        assert_eq!(earned / 10u128.pow(18), 123_287_671);
        assert_eq!(at, index);
        // Earned so far plus more.
        let (again, _) = settle(earned, index, 2, index + 5).unwrap();
        assert_eq!(again, earned + 10);
    }

    #[test]
    fn zero_balance_only_moves_the_checkpoint() {
        assert_eq!(settle(7, 100, 0, 900), Some((7, 900)));
    }

    #[test]
    fn settle_fails_closed() {
        // Index behind the checkpoint: corrupt state.
        assert_eq!(settle(0, 10, 1, 9), None);
        // Product overflow.
        assert_eq!(settle(0, 0, u64::MAX, u128::MAX), None);
        // Sum overflow.
        assert_eq!(settle(u128::MAX, 0, 1, 1), None);
    }

    #[test]
    fn earned_on_read() {
        assert_eq!(earned_e18(5, 10, 3, 20), Some(35));
        assert_eq!(earned_e18(5, 20, 3, 10), None);
    }
}
