//! F004 calls (FR-014, FR-015, FR-017–FR-020; contracts/collateral.md `issue_call`,
//! `check_coverage`, `cancel_call`; research R5).
//! - Part 1 (T030): delivery calls and re-checks without pledges.
//! - Part 2 (T042): with pledges and price moves.
//! - Part 3 (T051): cancellation.

use collateral::events::CoverageChecked;
use collateral::CollateralError;
use mmf_common::enums::CallStatus;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use tests::assert::{assert_anchor_error, assert_instruction_error};
use tests::collateral::{
    age_price, agreement_of, call_of, call_status, check, exposure, issue, r, Tracker,
};
use tests::events::one_cpi_event;
use tests::fixtures::{self, CollatCtx, Ready};
use tests::Harness;

const MILLION: u64 = 1_000_000_000_000;
const DELIVERY: u8 = 0;

fn ready(ready: Ready) -> (Harness, CollatCtx, Tracker) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund_opts(&mut h, ready);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    (h, c, t)
}

// ---- part 1 (T030) ------------------------------------------------------------------------

#[test]
fn a_new_call_covers_only_the_uncalled_shortfall() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    t.call(c.agr.call(&r(1)));
    // Everything is already demanded.
    assert_anchor_error(&issue(&mut h, &c, &c.taker, DELIVERY, r(2)), CollateralError::NothingToCall);
    t.check(&h);

    // A raise to $1.5M opens a second call for the extra $500 000 only.
    exposure(&mut h, &c, &c.taker, MILLION * 3 / 2, r(3)).expect("$1.5M");
    t.call(c.agr.call(&r(3)));
    let second = call_of(&h, &c, r(3));
    assert_eq!((second.seq, second.value_delta), (2, MILLION / 2));
    assert_eq!(
        second.units_required,
        mmf_common::collateral::units_for_value_up(MILLION / 2, 100_000_000, 200).unwrap()
    );
    let a = agreement_of(&h, &c);
    assert_eq!((a.called_outstanding, a.open_delivery_calls), (MILLION * 3 / 2, 2));
    t.check(&h);
}

#[test]
fn references_are_unique_per_agreement() {
    let (mut h, c, _) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    // A raise whose call would reuse a call's reference.
    assert_anchor_error(&exposure(&mut h, &c, &c.taker, 2 * MILLION, r(1)), CollateralError::CallRefInUse);
    // `issue_call` creates its account with Anchor `init`: the system program refuses.
    let res = issue(&mut h, &c, &c.taker, DELIVERY, r(1));
    assert_instruction_error(&res, InstructionError::Custom(0));
}

#[test]
fn delivery_calls_are_the_takers() {
    let (mut h, c, _) = ready(Ready::Standard);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, 2, r(1)), CollateralError::InvalidCallKind);
    assert_anchor_error(&issue(&mut h, &c, &c.giver, DELIVERY, r(2)), CollateralError::Unauthorized);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, DELIVERY, [0; 32]), CollateralError::ZeroExternalRef);
}

#[test]
fn issue_call_needs_a_price() {
    let (mut h, c, _) = ready(Ready::Unpriced);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, DELIVERY, r(1)), CollateralError::NoPrice);
    assert_anchor_error(&check(&mut h, &c, &c.taker), CollateralError::NoPrice);
}

#[test]
fn price_age_boundary() {
    let (mut h, c, _) = ready(Ready::Standard);
    let anyone = h.funded_keypair();
    age_price(&mut h, &c, 93_600);
    check(&mut h, &c, &anyone).expect("exactly the maximum age is fresh");
    age_price(&mut h, &c, 93_601);
    assert_anchor_error(&check(&mut h, &c, &anyone), CollateralError::StalePrice);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, DELIVERY, r(1)), CollateralError::StalePrice);
}

#[test]
fn check_coverage_is_anyones_and_decides_nothing() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    t.call(c.agr.call(&r(1)));
    let anyone = h.funded_keypair();
    let meta = check(&mut h, &c, &anyone).expect("anyone");
    assert!(meta.compute_units_consumed <= 80_000, "used {}", meta.compute_units_consumed);
    let first: CoverageChecked = one_cpi_event(&meta);
    let second: CoverageChecked = one_cpi_event(&check(&mut h, &c, &c.keeper).expect("keeper"));
    assert_eq!(first.triggered_by, anyone.pubkey());
    assert_eq!(first.snapshot, second.snapshot, "the outcome doesn't depend on who asked");
    assert!(first.snapshot.valued);
    assert_eq!(first.snapshot.coverage, -(MILLION as i128));
    assert_eq!(call_status(&h, &c.agr.call(&r(1))), CallStatus::Open);
    t.check(&h);
}

#[test]
fn proposed_agreement_takes_no_calls() {
    let (mut h, c, _) = ready(Ready::ProposedOnly);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, DELIVERY, r(1)), CollateralError::AgreementNotActive);
    let anyone = h.funded_keypair();
    assert_anchor_error(&check(&mut h, &c, &anyone), CollateralError::AgreementNotActive);
}

// ---- part 2 (T042): with pledges and price moves -----------------------------------------

use tests::collateral::{pledge, publish_next_day};

const UNITS_FOR_MILLION: u64 = 1_020_408_163_266;

fn units_for(value: u64, nav: u64) -> u64 {
    mmf_common::collateral::units_for_value_up(value, nav, 200).unwrap()
}

#[test]
fn open_calls_are_met_together() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    exposure(&mut h, &c, &c.taker, MILLION * 3 / 2, r(2)).expect("$1.5M");
    let (first, second) = (t.call(c.agr.call(&r(1))), t.call(c.agr.call(&r(2))));
    // Covering the first call's value alone leaves both open: coverage is still negative.
    pledge(&mut h, &c, &c.giver, UNITS_FOR_MILLION, r(10)).expect("partial");
    t.pledge(c.agr.pledge(&r(10)));
    assert_eq!(call_status(&h, &first), CallStatus::Open);
    assert_eq!(call_status(&h, &second), CallStatus::Open);
    assert_eq!(agreement_of(&h, &c).called_outstanding, MILLION / 2, "capped at the shortfall");
    t.check(&h);
    // Covering the rest meets both in the same transaction.
    pledge(&mut h, &c, &c.giver, units_for(MILLION / 2, 100_000_000), r(11)).expect("the rest");
    t.pledge(c.agr.pledge(&r(11)));
    assert_eq!(call_status(&h, &first), CallStatus::Met);
    assert_eq!(call_status(&h, &second), CallStatus::Met);
    let a = agreement_of(&h, &c);
    assert_eq!((a.called_outstanding, a.open_delivery_calls, a.met_below_seq), (0, 0, 3));
    t.check(&h);
}

#[test]
fn a_rise_after_a_partial_pledge_opens_a_new_call() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    t.call(c.agr.call(&r(1)));
    pledge(&mut h, &c, &c.giver, units_for(600_000_000_000, 100_000_000), r(10)).expect("$600k of value");
    t.pledge(c.agr.pledge(&r(10)));
    assert_eq!(agreement_of(&h, &c).called_outstanding, 400_000_000_000);
    // The $500 000 rise is not folded into the first call: it gets its own call and deadline.
    exposure(&mut h, &c, &c.taker, MILLION * 3 / 2, r(2)).expect("raise");
    t.call(c.agr.call(&r(2)));
    assert_eq!(call_of(&h, &c, r(2)).value_delta, 500_000_000_000);
    let a = agreement_of(&h, &c);
    assert_eq!((a.called_outstanding, a.open_delivery_calls), (900_000_000_000, 2));
    t.check(&h);
}

#[test]
fn price_moves_open_and_meet_calls() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    let first = t.call(c.agr.call(&r(1)));
    pledge(&mut h, &c, &c.giver, UNITS_FOR_MILLION, r(10)).expect("full cover");
    t.pledge(c.agr.pledge(&r(10)));
    assert_eq!(call_status(&h, &first), CallStatus::Met);

    // $0.99: the shortfall is exactly the MTA, so nothing can be called.
    publish_next_day(&mut h, &c, 99_000_000);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, DELIVERY, r(2)), CollateralError::NothingToCall);
    // $0.985: a $15 000 shortfall, called for 15 539.210609 units.
    publish_next_day(&mut h, &c, 98_500_000);
    let meta = issue(&mut h, &c, &c.taker, DELIVERY, r(3)).expect("call the drop");
    assert!(meta.compute_units_consumed <= 80_000, "used {}", meta.compute_units_consumed);
    let drop = t.call(c.agr.call(&r(3)));
    let call = call_of(&h, &c, r(3));
    assert_eq!((call.value_delta, call.units_required, call.nav_seq), (15_000_000_000, 15_539_210_609, 3));
    t.check(&h);
    // Back to $1.00: anyone's re-check finds the cover and meets the call.
    publish_next_day(&mut h, &c, 100_000_000);
    assert_eq!(call_status(&h, &drop), CallStatus::Open, "not yet observed");
    let anyone = h.funded_keypair();
    check(&mut h, &c, &anyone).expect("re-check");
    assert_eq!(call_status(&h, &drop), CallStatus::Met);
    t.check(&h);
}
