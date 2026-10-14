//! F004 US2, exposure and the calls it opens (FR-011–FR-013, FR-007; contracts/collateral.md
//! `update_exposure`, `confirm_exposure`; research R7). T029: written before the code.

use collateral::events::ExposureUpdated;
use collateral::CollateralError;
use mmf_common::enums::{CallKind, CallStatus, ExposureOutcome};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::collateral::{
    age_price, agreement_of, call_of, call_status, confirm, exposure, r, suspend, Tracker,
};
use tests::events::one_cpi_event;
use tests::fixtures::{self, CollatCtx, Ready};
use tests::{clock, Harness};

const MILLION: u64 = 1_000_000_000_000;
const MTA: u64 = 10_000_000_000;

fn ready(ready: Ready) -> (Harness, CollatCtx, Tracker) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund_opts(&mut h, ready);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    (h, c, t)
}

#[test]
fn us2_one_million_opens_a_call_with_deadlines() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    let now = clock::now(&h.svm);
    let meta = exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("set exposure");
    t.call(c.agr.call(&r(1)));
    t.check(&h);

    let call = call_of(&h, &c, r(1));
    assert_eq!(call.agreement, c.agreement());
    assert_eq!((call.seq, call.kind), (1, CallKind::Delivery.as_u8()));
    assert_eq!((call.value_delta, call.units_required), (MILLION, 1_020_408_163_266));
    assert_eq!(call.nav_seq, 1);
    assert_eq!(call.issued_at, now);
    assert_eq!(call.cure_deadline_ts, now + 14_400);
    assert_eq!(call.enforce_after_ts, now + 21_600);
    assert_eq!(call.status, CallStatus::Open.as_u8());
    assert_eq!(call.external_ref, r(1));

    let a = agreement_of(&h, &c);
    assert_eq!((a.required_value, a.exposure_ref, a.exposure_updated_ts), (MILLION, r(1), now));
    assert_eq!((a.called_outstanding, a.open_delivery_calls, a.call_count), (MILLION, 1, 2));

    let ev: ExposureUpdated = one_cpi_event(&meta);
    assert_eq!(ev.outcome, ExposureOutcome::Applied.as_u8());
    assert_eq!((ev.required_value, ev.external_ref, ev.signer), (MILLION, r(1), c.taker.pubkey()));
    let opened = ev.opened_call.expect("a call opened");
    assert_eq!((opened.call, opened.seq, opened.value_delta), (c.agr.call(&r(1)), 1, MILLION));
    assert_eq!(opened.units_required, 1_020_408_163_266);
    assert!(ev.snapshot.valued);
    assert_eq!(ev.snapshot.coverage, -(MILLION as i128));
    assert!(meta.compute_units_consumed <= 80_000, "used {}", meta.compute_units_consumed);
}

#[test]
fn only_the_taker_with_a_reference() {
    let (mut h, c, _) = ready(Ready::Standard);
    assert_anchor_error(&exposure(&mut h, &c, &c.giver, MILLION, r(1)), CollateralError::Unauthorized);
    let outsider = fixtures::new_investor(&mut h, &c);
    assert_anchor_error(&exposure(&mut h, &c, &outsider, MILLION, r(1)), CollateralError::Unauthorized);
    assert_anchor_error(&exposure(&mut h, &c, &c.taker, MILLION, [0; 32]), CollateralError::ZeroExternalRef);
}

#[test]
fn shortfall_must_be_more_than_the_mta() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    // Nothing pledged: the shortfall is the exposure. Exactly the MTA opens nothing.
    exposure(&mut h, &c, &c.taker, MTA, r(1)).expect("at the MTA");
    assert!(!h.exists(&c.agr.call(&r(1))), "no call account");
    assert_eq!(agreement_of(&h, &c).call_count, 1);
    // One base unit more opens a call for the whole uncalled shortfall.
    exposure(&mut h, &c, &c.taker, MTA + 1, r(2)).expect("above the MTA");
    t.call(c.agr.call(&r(2)));
    assert_eq!(call_of(&h, &c, r(2)).value_delta, MTA + 1);
    t.check(&h);
}

#[test]
fn a_raise_needs_a_fresh_price_and_an_eligible_taker() {
    let (mut h, c, _) = ready(Ready::Standard);
    age_price(&mut h, &c, 93_601);
    assert_anchor_error(&exposure(&mut h, &c, &c.taker, MILLION, r(1)), CollateralError::StalePrice);
    age_price(&mut h, &c, 93_600);
    suspend(&mut h, &c, &c.taker.pubkey());
    assert_anchor_error(&exposure(&mut h, &c, &c.taker, MILLION, r(1)), CollateralError::PartyNotEligible);
}

#[test]
fn a_raise_on_an_unpriced_fund_is_refused() {
    let (mut h, c, _) = ready(Ready::Unpriced);
    assert_anchor_error(&exposure(&mut h, &c, &c.taker, MILLION, r(1)), CollateralError::NoPrice);
}

#[test]
fn lowering_applies_at_any_price_age_and_zero_meets_every_call() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("raise");
    t.call(c.agr.call(&r(1)));
    age_price(&mut h, &c, 200_000);
    // A suspended taker may still lower its exposure (FR-034).
    suspend(&mut h, &c, &c.taker.pubkey());
    exposure(&mut h, &c, &c.taker, MILLION / 2, r(2)).expect("lower on a stale price");
    let a = agreement_of(&h, &c);
    assert_eq!(a.required_value, MILLION / 2);
    assert_eq!(call_status(&h, &c.agr.call(&r(1))), CallStatus::Open, "no price: not observed");
    let meta = exposure(&mut h, &c, &c.taker, 0, r(3)).expect("lower to zero");
    assert_eq!(call_status(&h, &c.agr.call(&r(1))), CallStatus::Met);
    let a = agreement_of(&h, &c);
    assert_eq!((a.called_outstanding, a.open_delivery_calls, a.met_below_seq), (0, 0, 2));
    let ev: ExposureUpdated = one_cpi_event(&meta);
    assert!(ev.opened_call.is_none());
    t.check(&h);
}

#[test]
fn two_step_raise_waits_for_the_giver() {
    let (mut h, c, mut t) = ready(Ready::TwoStep);
    let meta = exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("raise");
    let ev: ExposureUpdated = one_cpi_event(&meta);
    assert_eq!(ev.outcome, ExposureOutcome::Pending.as_u8());
    let a = agreement_of(&h, &c);
    assert_eq!((a.required_value, a.pending_raise_value, a.pending_raise_ref), (0, MILLION, r(1)));
    assert!(!h.exists(&c.agr.call(&r(1))));

    // Confirmations that don't match the pending raise exactly.
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, MILLION + 1, r(1)), CollateralError::PendingRaiseMismatch);
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, MILLION, r(2)), CollateralError::PendingRaiseMismatch);
    assert_anchor_error(&confirm(&mut h, &c, &c.taker, MILLION, r(1)), CollateralError::Unauthorized);

    // The confirmation opens the call, with deadlines counted from the confirmation.
    clock::advance(&mut h.svm, 3_600);
    let now = clock::now(&h.svm);
    let meta = confirm(&mut h, &c, &c.giver, MILLION, r(1)).expect("confirm");
    t.call(c.agr.call(&r(1)));
    let ev: ExposureUpdated = one_cpi_event(&meta);
    assert_eq!(ev.outcome, ExposureOutcome::Confirmed.as_u8());
    let call = call_of(&h, &c, r(1));
    assert_eq!((call.issued_at, call.cure_deadline_ts), (now, now + 14_400));
    let a = agreement_of(&h, &c);
    assert_eq!((a.required_value, a.pending_raise_value), (MILLION, 0));
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, MILLION, r(1)), CollateralError::NoPendingRaise);
    t.check(&h);
}

#[test]
fn a_newer_raise_or_a_lower_value_replaces_the_pending_one() {
    let (mut h, c, _) = ready(Ready::TwoStep);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("raise");
    exposure(&mut h, &c, &c.taker, 2 * MILLION, r(2)).expect("newer raise");
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, MILLION, r(1)), CollateralError::PendingRaiseMismatch);
    // A lower (here: equal) value applies at once and clears the pending raise.
    exposure(&mut h, &c, &c.taker, 0, r(3)).expect("lower");
    assert_eq!(agreement_of(&h, &c).pending_raise_value, 0);
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, 2 * MILLION, r(2)), CollateralError::NoPendingRaise);
}

#[test]
fn confirmation_needs_a_fresh_price_and_an_eligible_giver() {
    let (mut h, c, _) = ready(Ready::TwoStep);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("raise");
    age_price(&mut h, &c, 93_601);
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, MILLION, r(1)), CollateralError::StalePrice);
    age_price(&mut h, &c, 60);
    suspend(&mut h, &c, &c.giver.pubkey());
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, MILLION, r(1)), CollateralError::PartyNotEligible);
}

#[test]
fn proposed_agreement_takes_no_exposure() {
    let (mut h, c, _) = ready(Ready::ProposedOnly);
    assert_anchor_error(&exposure(&mut h, &c, &c.taker, MILLION, r(1)), CollateralError::AgreementNotActive);
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, MILLION, r(1)), CollateralError::AgreementNotActive);
}
