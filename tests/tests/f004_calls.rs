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
