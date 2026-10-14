//! F004 US5, substitute (FR-030, FR-007; contracts/collateral.md `substitute`; research R9).
//! T057: written before the code.

use collateral::events::Substituted;
use collateral::CollateralError;
use compliance_hook::HookError;
use mmf_common::enums::{CallStatus, PledgeStatus};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::collateral::{
    agr, agreement_of, call_status, exposure, lock_of, pledge, pledge_of, pledge_on, propose, r,
    release, sign, substitute_on, suspend, Tracker,
};
use tests::events::one_cpi_event;
use tests::fixtures::{self, worked_terms, CollatCtx, GIVER_UNITS};
use tests::{clock, units, Harness};

const UNITS_FOR_MILLION: u64 = 1_020_408_163_266;

/// The fixture's agreement with pledge `r(9)` of 1 020 408.163266 units (no exposure).
fn pledged() -> (Harness, CollatCtx, Tracker) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    pledge(&mut h, &c, &c.giver, UNITS_FOR_MILLION, r(9)).expect("pledge");
    t.pledge(c.agr.pledge(&r(9)));
    (h, c, t)
}

#[test]
fn us5_substitute_with_a_larger_pledge() {
    let (mut h, c, mut t) = pledged();
    let before = units::balance(&h, &c.m(), &c.giver.pubkey());
    let now = clock::now(&h.svm);
    let meta = substitute_on(&mut h, &c.agr, &c.giver, r(9), r(10), 1_100_000_000_000).expect("substitute");
    t.pledge(c.agr.pledge(&r(10)));
    assert!(meta.compute_units_consumed <= 80_000, "used {}", meta.compute_units_consumed);

    let old = pledge_of(&h, &c, r(9));
    assert_eq!((old.status, old.released_units, old.closed_at), (PledgeStatus::Substituted.as_u8(), UNITS_FOR_MILLION, now));
    let new = pledge_of(&h, &c, r(10));
    assert_eq!((new.status, new.units, new.seq), (PledgeStatus::Active.as_u8(), 1_100_000_000_000, 2));
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()), (1_100_000_000_000, 1));
    assert_eq!(units::balance(&h, &c.m(), &c.giver.pubkey()), before, "no units move");
    assert_eq!(agreement_of(&h, &c).pledged_units, 1_100_000_000_000);
    let ev: Substituted = one_cpi_event(&meta);
    assert_eq!((ev.old_remaining, ev.new_units, ev.new_seq), (UNITS_FOR_MILLION, 1_100_000_000_000, 2));
    t.check(&h);

    // A smaller replacement is a release, not a substitution.
    assert_anchor_error(
        &substitute_on(&mut h, &c.agr, &c.giver, r(10), r(11), 1_000_000_000_000),
        CollateralError::SubstituteTooSmall,
    );
}

#[test]
fn an_equal_swap_changes_no_totals() {
    let (mut h, c, mut t) = pledged();
    substitute_on(&mut h, &c.agr, &c.giver, r(9), r(10), UNITS_FOR_MILLION).expect("equal swap");
    t.pledge(c.agr.pledge(&r(10)));
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()), (UNITS_FOR_MILLION, 1));
    assert_eq!(agreement_of(&h, &c).pledged_units, UNITS_FOR_MILLION);
    t.check(&h);
}

#[test]
fn substitution_rejections() {
    let (mut h, c, mut t) = pledged();
    // The net increase must be unlocked; the new total must fit the pledge limit (5 000 000).
    assert_anchor_error(
        &substitute_on(&mut h, &c.agr, &c.giver, r(9), r(10), GIVER_UNITS + 1),
        HookError::InsufficientUnlockedUnits,
    );
    assert_anchor_error(
        &substitute_on(&mut h, &c.agr, &c.giver, r(9), r(11), 5_000_000_000_001),
        CollateralError::PledgeLimitExceeded,
    );
    assert_anchor_error(&substitute_on(&mut h, &c.agr, &c.taker, r(9), r(12), UNITS_FOR_MILLION), CollateralError::Unauthorized);
    assert_anchor_error(&substitute_on(&mut h, &c.agr, &c.giver, r(9), [0; 32], UNITS_FOR_MILLION), CollateralError::ZeroPledgeRef);

    // A Substituted old pledge, and a Released one.
    substitute_on(&mut h, &c.agr, &c.giver, r(9), r(13), UNITS_FOR_MILLION).expect("swap");
    t.pledge(c.agr.pledge(&r(13)));
    assert_anchor_error(&substitute_on(&mut h, &c.agr, &c.giver, r(9), r(14), UNITS_FOR_MILLION), CollateralError::PledgeNotActive);
    release(&mut h, &c, &c.taker, r(13), None, UNITS_FOR_MILLION).expect("release everything");
    assert_anchor_error(&substitute_on(&mut h, &c.agr, &c.giver, r(13), r(15), UNITS_FOR_MILLION), CollateralError::PledgeNotActive);
    t.check(&h);

    // A pledge of another agreement.
    let other = agr(&c, 2);
    propose(&mut h, &c.m(), &c.taker, worked_terms(&c.giver.pubkey(), &c.taker.pubkey(), 2)).expect("propose");
    sign(&mut h, &c.giver, &other).expect("sign");
    t.agreement(other.agreement());
    pledge_on(&mut h, &other, &c.giver, 1_000, r(1)).expect("pledge on the other");
    t.pledge(other.pledge(&r(1)));
    let ix = mmf_sdk::ix_collateral::substitute(&c.giver.pubkey(), &h.payer.pubkey(), &c.agr, &other.pledge(&r(1)), r(16), 1_000);
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), CollateralError::PledgeNotActive);
    t.check(&h);
}

#[test]
fn suspended_giver_cannot_substitute() {
    let (mut h, c, _) = pledged();
    suspend(&mut h, &c, &c.giver.pubkey());
    assert_anchor_error(
        &substitute_on(&mut h, &c.agr, &c.giver, r(9), r(10), UNITS_FOR_MILLION),
        CollateralError::PartyNotEligible,
    );
}

#[test]
fn a_larger_swap_can_meet_the_call() {
    let (mut h, c, mut t) = pledged();
    // $1.1M exposure: the 1 020 408.163266 units ($1M) leave a $100 000 call open.
    exposure(&mut h, &c, &c.taker, 1_100_000_000_000, r(1)).expect("$1.1M");
    let call = t.call(c.agr.call(&r(1)));
    assert_eq!(call_status(&h, &call), CallStatus::Open);
    let units = mmf_common::collateral::units_for_value_up(1_100_000_000_000, 100_000_000, 200).unwrap();
    substitute_on(&mut h, &c.agr, &c.giver, r(9), r(10), units).expect("swap up");
    t.pledge(c.agr.pledge(&r(10)));
    assert_eq!(call_status(&h, &call), CallStatus::Met);
    t.check(&h);
}

#[test]
fn proposed_agreement_takes_no_substitution() {
    let (mut h, c, _) = pledged();
    let proposed = agr(&c, 2);
    propose(&mut h, &c.m(), &c.taker, worked_terms(&c.giver.pubkey(), &c.taker.pubkey(), 2)).expect("propose");
    let ix = mmf_sdk::ix_collateral::substitute(&c.giver.pubkey(), &h.payer.pubkey(), &proposed, &c.agr.pledge(&r(9)), r(10), UNITS_FOR_MILLION);
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), CollateralError::AgreementNotActive);
}
