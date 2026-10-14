//! F004 parties' status and paused transfers (FR-034, FR-035; research R11, R12). T067: steps
//! that add a lock or an obligation need an eligible actor; steps that only reduce the giver's
//! burden don't; a suspension never moves a deadline; locks work while transfers are paused.

use collateral::CollateralError;
use compliance_hook::HookError;
use mmf_common::enums::{CallStatus, ReasonCode};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::collateral::{
    call_of, call_status, cancel, confirm, exposure, issue, lock_of, pledge, r, reinstate,
    release, substitute_on, suspend, Tracker,
};
use tests::fixtures::{self, CollatCtx, Ready};
use tests::{clock, Harness};

const MILLION: u64 = 1_000_000_000_000;
const UNITS_FOR_MILLION: u64 = 1_020_408_163_266;
const DELIVERY: u8 = 0;
const RETURN: u8 = 1;

/// $1M covered by pledge `r(9)`.
fn covered(ready: Ready) -> (Harness, CollatCtx, Tracker) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund_opts(&mut h, ready);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    t.call(c.agr.call(&r(1)));
    pledge(&mut h, &c, &c.giver, UNITS_FOR_MILLION, r(9)).expect("pledge");
    t.pledge(c.agr.pledge(&r(9)));
    (h, c, t)
}

#[test]
fn a_suspended_giver_adds_nothing_but_can_still_reduce() {
    let (mut h, c, mut t) = covered(Ready::Standard);
    // An open delivery call (to dispute) and an open return call (to cancel), made beforehand.
    exposure(&mut h, &c, &c.taker, 2 * MILLION, r(2)).expect("raise: a second call");
    let open_call = t.call(c.agr.call(&r(2)));
    let deadlines = |h: &Harness| {
        let call = call_of(h, &c, r(2));
        (call.cure_deadline_ts, call.enforce_after_ts)
    };
    let before = deadlines(&h);

    suspend(&mut h, &c, &c.giver.pubkey());
    assert_anchor_error(&pledge(&mut h, &c, &c.giver, 1, r(10)), CollateralError::PartyNotEligible);
    assert_anchor_error(
        &substitute_on(&mut h, &c.agr, &c.giver, r(9), r(11), UNITS_FOR_MILLION),
        CollateralError::PartyNotEligible,
    );
    assert_anchor_error(&issue(&mut h, &c, &c.giver, RETURN, r(12)), CollateralError::PartyNotEligible);

    // Disputing stays open to a suspended giver.
    let ix = mmf_sdk::ix_collateral::dispute_call(&c.giver.pubkey(), &c.agr, &open_call, r(13));
    h.send(&[ix], &[&c.giver]).expect("dispute while suspended");
    assert_eq!(call_status(&h, &open_call), CallStatus::Disputed);
    // The taker can still lower the exposure and release.
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(3)).expect("lower");
    release(&mut h, &c, &c.taker, r(9), None, 100_000_000_000).expect("release to a suspended giver");
    t.check(&h);

    // Deadlines never move with a suspension or a reinstatement.
    assert_eq!(deadlines(&h), before);
    reinstate(&mut h, &c, &c.giver.pubkey());
    assert_eq!(deadlines(&h), before);

    // Cancelling its own return call is open to a suspended giver too.
    issue(&mut h, &c, &c.giver, RETURN, r(14)).expect("return call while approved");
    t.call(c.agr.call(&r(14)));
    suspend(&mut h, &c, &c.giver.pubkey());
    cancel(&mut h, &c, &c.giver, r(14)).expect("cancel while suspended");
    t.check(&h);
}

#[test]
fn a_kyc_expired_giver_is_refused_like_a_suspended_one() {
    let (mut h, c, _) = covered(Ready::Standard);
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(2)).expect("lower: an excess");
    let state: compliance_hook::HolderState =
        fixtures::read(&h, &mmf_sdk::pda::holder(&c.m(), &c.giver.pubkey()).0);
    clock::set_unix_timestamp(&mut h.svm, state.kyc_expiry_ts);
    assert_anchor_error(&pledge(&mut h, &c, &c.giver, 1, r(10)), CollateralError::PartyNotEligible);
    assert_anchor_error(
        &substitute_on(&mut h, &c.agr, &c.giver, r(9), r(11), UNITS_FOR_MILLION),
        CollateralError::PartyNotEligible,
    );
    assert_anchor_error(&issue(&mut h, &c, &c.giver, RETURN, r(12)), CollateralError::PartyNotEligible);
}

#[test]
fn a_suspended_giver_cannot_confirm_a_raise() {
    let (mut h, c, _) = covered(Ready::TwoStep);
    exposure(&mut h, &c, &c.taker, 2 * MILLION, r(2)).expect("pending raise");
    suspend(&mut h, &c, &c.giver.pubkey());
    assert_anchor_error(&confirm(&mut h, &c, &c.giver, 2 * MILLION, r(2)), CollateralError::PartyNotEligible);
}

#[test]
fn a_suspended_taker_can_only_reduce() {
    let (mut h, c, mut t) = covered(Ready::Standard);
    exposure(&mut h, &c, &c.taker, 3 * MILLION / 2, r(2)).expect("an open call");
    t.call(c.agr.call(&r(2)));
    suspend(&mut h, &c, &c.taker.pubkey());
    assert_anchor_error(&exposure(&mut h, &c, &c.taker, 2 * MILLION, r(3)), CollateralError::PartyNotEligible);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, DELIVERY, r(4)), CollateralError::PartyNotEligible);
    cancel(&mut h, &c, &c.taker, r(2)).expect("cancel while suspended");
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(5)).expect("lower while suspended");
    release(&mut h, &c, &c.taker, r(9), None, 100_000_000_000).expect("release while suspended");
    t.check(&h);
}

#[test]
fn locks_work_while_transfers_are_paused() {
    let (mut h, c, mut t) = covered(Ready::Standard);
    let ix = mmf_sdk::ix_governance::pause_transfers(
        &c.compliance.pubkey(),
        &c.m(),
        ReasonCode::OperationalError as u8,
        [0; 32],
    );
    h.send(&[ix], &[&c.compliance]).expect("pause");

    pledge(&mut h, &c, &c.giver, 1_000_000, r(10)).expect("pledge while paused");
    t.pledge(c.agr.pledge(&r(10)));
    substitute_on(&mut h, &c.agr, &c.giver, r(10), r(11), 2_000_000).expect("substitute while paused");
    t.pledge(c.agr.pledge(&r(11)));
    release(&mut h, &c, &c.taker, r(11), None, 2_000_000).expect("release while paused");
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()).0, UNITS_FOR_MILLION);
    // The pause still stops every transfer, including of unlocked units.
    let res = fixtures::transfer(&mut h, &c, &c.giver, &c.taker.pubkey(), 1);
    assert_anchor_error(&res, HookError::TransfersPaused);
    t.check(&h);
}
