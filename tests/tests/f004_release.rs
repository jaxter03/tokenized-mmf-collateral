//! F004 US4, release excess (FR-016, FR-027–FR-029, FR-007; contracts/collateral.md `issue_call`
//! Return, `release`; research R8). T050: written before the code.

use collateral::events::Released;
use collateral::{CollateralError, ProposeArgs};
use mmf_common::enums::{CallKind, CallStatus, PledgeStatus};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::collateral::{
    age_price, agr, agreement_of, call_of, call_status, cancel, exposure, issue, lock_of, pledge,
    pledge_of, pledge_on, propose, publish_next_day, r, release, sign, suspend, Tracker,
};
use tests::events::one_cpi_event;
use tests::fixtures::{self, worked_terms, CollatCtx, GIVER_UNITS};
use tests::{units, Harness};

const MILLION: u64 = 1_000_000_000_000;
const UNITS_FOR_MILLION: u64 = 1_020_408_163_266;
const RETURN: u8 = 1;

/// The fixture's agreement with $1M exposure covered exactly by pledge `r(9)`.
fn covered() -> (Harness, CollatCtx, Tracker) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    t.call(c.agr.call(&r(1)));
    pledge(&mut h, &c, &c.giver, UNITS_FOR_MILLION, r(9)).expect("pledge");
    t.pledge(c.agr.pledge(&r(9)));
    (h, c, t)
}

#[test]
fn us4_keeper_releases_the_excess_against_the_return_call() {
    let (mut h, c, mut t) = covered();
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(2)).expect("lower to $400 000");
    issue(&mut h, &c, &c.giver, RETURN, r(3)).expect("return call");
    let ret = t.call(c.agr.call(&r(3)));
    let call = call_of(&h, &c, r(3));
    assert_eq!(call.kind, CallKind::Return.as_u8());
    assert_eq!(
        (call.value_delta, call.units_required),
        (600_000_000_000, 612_244_897_959)
    );
    assert_eq!(
        call.enforce_after_ts, 0,
        "return calls have no enforcement time"
    );
    assert_eq!(agreement_of(&h, &c).open_return_call, ret);
    t.check(&h);

    let keeper = c.keeper.insecure_clone();
    let before = units::balance(&h, &c.m(), &c.giver.pubkey());
    let meta =
        release(&mut h, &c, &keeper, r(9), Some(r(3)), 612_244_897_959).expect("keeper releases");
    assert!(
        meta.compute_units_consumed <= 80_000,
        "used {}",
        meta.compute_units_consumed
    );
    assert_eq!(
        units::balance(&h, &c.m(), &c.giver.pubkey()),
        before,
        "no units move"
    );
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()), (408_163_265_307, 1));
    assert_eq!(call_status(&h, &ret), CallStatus::Met);
    let call = call_of(&h, &c, r(3));
    assert_eq!(call.units_released, 612_244_897_959);
    let a = agreement_of(&h, &c);
    assert_eq!(a.open_return_call, anchor_lang::prelude::Pubkey::default());
    let ev: Released = one_cpi_event(&meta);
    assert!(ev.return_call_met);
    assert_eq!(
        (ev.units, ev.pledge_remaining, ev.return_call),
        (612_244_897_959, 408_163_265_307, Some(ret))
    );
    assert_eq!(ev.snapshot.coverage, 0, "coverage is exactly zero");
    t.check(&h);
}

#[test]
fn return_call_rules() {
    let (mut h, c, mut t) = covered();
    // Excess exactly at the MTA.
    exposure(&mut h, &c, &c.taker, MILLION - 10_000_000_000, r(2)).expect("lower by the MTA");
    assert_anchor_error(
        &issue(&mut h, &c, &c.giver, RETURN, r(3)),
        CollateralError::NoExcess,
    );
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(4)).expect("lower further");
    assert_anchor_error(
        &issue(&mut h, &c, &c.taker, RETURN, r(5)),
        CollateralError::Unauthorized,
    );
    issue(&mut h, &c, &c.giver, RETURN, r(6)).expect("return call");
    t.call(c.agr.call(&r(6)));
    assert_anchor_error(
        &issue(&mut h, &c, &c.giver, RETURN, r(7)),
        CollateralError::ReturnCallAlreadyOpen,
    );
    t.check(&h);
}

#[test]
fn suspended_giver_cannot_open_a_return_call() {
    let (mut h, c, _) = covered();
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(2)).expect("lower");
    suspend(&mut h, &c, &c.giver.pubkey());
    assert_anchor_error(
        &issue(&mut h, &c, &c.giver, RETURN, r(3)),
        CollateralError::PartyNotEligible,
    );
}

#[test]
fn an_excess_below_one_unit_returns_nothing() {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    // An agreement with no haircut and no MTA; one base unit pledged.
    let args = ProposeArgs {
        haircut_bps: 0,
        min_transfer_value: 0,
        ..worked_terms(&c.giver.pubkey(), &c.taker.pubkey(), 2)
    };
    let a = agr(&c, 2);
    propose(&mut h, &c.m(), &c.taker, args).expect("propose");
    sign(&mut h, &c.giver, &a).expect("sign");
    pledge_on(&mut h, &a, &c.giver, 1_000_000, r(1)).expect("pledge one unit");
    // At $1.01 one unit is worth 1 010 000; an exposure of 1 009 999 leaves an excess of 1,
    // which buys floor(1 / 1.01) = 0 units.
    publish_next_day(&mut h, &c, 101_000_000);
    let ix = mmf_sdk::ix_collateral::update_exposure(
        &c.taker.pubkey(),
        &h.payer.pubkey(),
        &a,
        1_009_999,
        r(2),
    );
    h.send(&[ix], &[&c.taker]).expect("exposure");
    let ix =
        mmf_sdk::ix_collateral::issue_call(&c.giver.pubkey(), &h.payer.pubkey(), &a, RETURN, r(3));
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), CollateralError::ReturnTooSmall);
}

#[test]
fn taker_releases_and_unnamed_releases_dont_count() {
    let (mut h, c, mut t) = covered();
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(2)).expect("lower");
    // The taker may release without any return call.
    release(&mut h, &c, &c.taker, r(9), None, 100_000_000_000).expect("taker release");
    t.check(&h);
    issue(&mut h, &c, &c.giver, RETURN, r(3)).expect("return call");
    t.call(c.agr.call(&r(3)));
    // A release that doesn't name the open return call doesn't count towards it.
    release(&mut h, &c, &c.taker, r(9), None, 1_000_000).expect("unnamed release");
    assert_eq!(call_of(&h, &c, r(3)).units_released, 0);
    release(&mut h, &c, &c.taker, r(9), Some(r(3)), 1_000_000).expect("named release");
    assert_eq!(call_of(&h, &c, r(3)).units_released, 1_000_000);
    t.check(&h);
}

#[test]
fn keeper_rules() {
    let (mut h, c, mut t) = covered();
    let keeper = c.keeper.insecure_clone();
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(2)).expect("lower");
    assert_anchor_error(
        &release(&mut h, &c, &keeper, r(9), None, 1),
        CollateralError::KeeperNeedsReturnCall,
    );
    issue(&mut h, &c, &c.giver, RETURN, r(3)).expect("return call");
    t.call(c.agr.call(&r(3)));
    assert_anchor_error(
        &release(&mut h, &c, &keeper, r(9), Some(r(3)), 612_244_897_960),
        CollateralError::ExceedsReturnCall,
    );
    // A cancelled return call can't be released against.
    cancel(&mut h, &c, &c.giver, r(3)).expect("giver cancels");
    assert_anchor_error(
        &release(&mut h, &c, &keeper, r(9), Some(r(3)), 1),
        CollateralError::WrongReturnCall,
    );
    // Nor can another agreement's return call.
    let other = agr(&c, 2);
    propose(
        &mut h,
        &c.m(),
        &c.taker,
        worked_terms(&c.giver.pubkey(), &c.taker.pubkey(), 2),
    )
    .expect("propose");
    sign(&mut h, &c.giver, &other).expect("sign");
    t.agreement(other.agreement());
    pledge_on(&mut h, &other, &c.giver, 500_000_000_000, r(1)).expect("pledge");
    t.pledge(other.pledge(&r(1)));
    let ix = mmf_sdk::ix_collateral::issue_call(
        &c.giver.pubkey(),
        &h.payer.pubkey(),
        &other,
        RETURN,
        r(4),
    );
    h.send(&[ix], &[&c.giver])
        .expect("return call on the other agreement");
    t.call(other.call(&r(4)));
    let ix = mmf_sdk::ix_collateral::release(
        &keeper.pubkey(),
        &c.agr,
        &c.agr.pledge(&r(9)),
        Some(other.call(&r(4))),
        1,
    );
    assert_anchor_error(&h.send(&[ix], &[&keeper]), CollateralError::WrongReturnCall);
    t.check(&h);
}

#[test]
fn release_rejections() {
    let (mut h, c, t) = covered();
    assert_anchor_error(
        &release(&mut h, &c, &c.taker, r(9), None, 0),
        CollateralError::ZeroUnits,
    );
    assert_anchor_error(
        &release(&mut h, &c, &c.taker, r(9), None, UNITS_FOR_MILLION + 1),
        CollateralError::ExceedsPledgeRemaining,
    );
    // One base unit would leave the $1M exposure under-covered.
    assert_anchor_error(
        &release(&mut h, &c, &c.taker, r(9), None, 1),
        CollateralError::WouldUndercollateralise,
    );
    assert_anchor_error(
        &release(&mut h, &c, &c.giver, r(9), None, 1),
        CollateralError::Unauthorized,
    );
    let outsider = fixtures::new_investor(&mut h, &c);
    assert_anchor_error(
        &release(&mut h, &c, &outsider, r(9), None, 1),
        CollateralError::Unauthorized,
    );

    // Fully released pledges are no longer Active.
    exposure(&mut h, &c, &c.taker, 0, r(2)).expect("exposure to zero");
    release(&mut h, &c, &c.taker, r(9), None, UNITS_FOR_MILLION).expect("release everything");
    let p = pledge_of(&h, &c, r(9));
    assert_eq!(
        (p.status, p.released_units),
        (PledgeStatus::Released.as_u8(), UNITS_FOR_MILLION)
    );
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()), (0, 0));
    assert_anchor_error(
        &release(&mut h, &c, &c.taker, r(9), None, 1),
        CollateralError::PledgeNotActive,
    );
    t.check(&h);
}

#[test]
fn price_age_and_zero_exposure() {
    let (mut h, c, t) = covered();
    exposure(&mut h, &c, &c.taker, 400_000_000_000, r(2)).expect("lower");
    age_price(&mut h, &c, 93_601);
    assert_anchor_error(
        &release(&mut h, &c, &c.taker, r(9), None, 1),
        CollateralError::StalePrice,
    );
    // With nothing to protect, no price is needed (a missing publish never traps units).
    exposure(&mut h, &c, &c.taker, 0, r(3)).expect("exposure to zero");
    release(&mut h, &c, &c.taker, r(9), None, UNITS_FOR_MILLION).expect("release on a stale price");
    assert_eq!(units::balance(&h, &c.m(), &c.giver.pubkey()), GIVER_UNITS);
    t.check(&h);
}

#[test]
fn proposed_agreement_releases_nothing() {
    let (mut h, c, _) = covered();
    // A second agreement, never signed, named with a pledge of the first.
    let proposed = agr(&c, 2);
    propose(
        &mut h,
        &c.m(),
        &c.taker,
        worked_terms(&c.giver.pubkey(), &c.taker.pubkey(), 2),
    )
    .expect("propose");
    let ix = mmf_sdk::ix_collateral::release(
        &c.taker.pubkey(),
        &proposed,
        &c.agr.pledge(&r(9)),
        None,
        1,
    );
    assert_anchor_error(
        &h.send(&[ix], &[&c.taker]),
        CollateralError::AgreementNotActive,
    );
}
