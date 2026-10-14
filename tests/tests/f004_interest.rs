//! F004: locked units earn interest exactly as unlocked units do (FR-026, SC-004; F002 US2
//! scenario 3, deferred to here). T041: written before the code.

use compliance_hook::HolderState;
use solana_signer::Signer;
use tests::collateral::{lock_of, pledge, r, Tracker};
use tests::fixtures::{self, read, NAV_ONE};
use tests::{clock, Harness};

const ONE_MILLION_UNITS: u64 = 1_000_000_000_000;

#[test]
fn locked_and_unlocked_units_earn_the_same() {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    let m = c.m();
    // G keeps 1 000 000 units and locks all of them; B holds 1 000 000 unlocked.
    let b = fixtures::new_investor(&mut h, &c);
    fixtures::transfer(&mut h, &c, &c.giver, &b.pubkey(), ONE_MILLION_UNITS).expect("split");
    pledge(&mut h, &c, &c.giver, ONE_MILLION_UNITS, r(1)).expect("lock everything");
    t.pledge(c.agr.pledge(&r(1)));
    assert_eq!(lock_of(&h, &m, &c.giver.pubkey()).0, ONE_MILLION_UNITS);

    for rate in [123_287_671i64, 100_000_000, 150_000_000, 90_000_000, 130_000_000] {
        clock::advance(&mut h.svm, 86_400);
        fixtures::publish(&mut h, &c, NAV_ONE, rate, 1).expect("publish");
        // Each holder touches its record with a zero-unit transfer, settling its interest.
        fixtures::transfer(&mut h, &c, &c.giver, &c.taker.pubkey(), 0).expect("touch G");
        fixtures::transfer(&mut h, &c, &b, &c.taker.pubkey(), 0).expect("touch B");
        let g: HolderState = read(&h, &mmf_sdk::pda::holder(&m, &c.giver.pubkey()).0);
        let u: HolderState = read(&h, &mmf_sdk::pda::holder(&m, &b.pubkey()).0);
        assert_eq!(g.accrued_e18, u.accrued_e18, "same interest after the publish at {rate}");
    }
    let g: HolderState = read(&h, &mmf_sdk::pda::holder(&m, &c.giver.pubkey()).0);
    assert!(g.accrued_e18 > 0, "interest was earned");
    // 1 000 000 units × Σ rates × 10^6 (exact, shared-model §8).
    let expected: u128 = u128::from(ONE_MILLION_UNITS)
        * (123_287_671u128 + 100_000_000 + 150_000_000 + 90_000_000 + 130_000_000)
        * 1_000_000;
    assert_eq!(g.accrued_e18, expected);
    t.check(&h);
}
