//! T058, US4 Cancel, expire, reject (FR-023–FR-027): unfunded or stuck instructions end, and every
//! funded leg is earmarked for its funder; `return_leg` delivers it (research R3).

use dvp_settlement::events::{
    CancelRequestWithdrawn, CancelRequested, SettlementCancelled, SettlementExpired,
};
use dvp_settlement::DvpError;
use mmf_sdk::ix_dvp::{self, Deal};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::events::one_cpi_event;
use tests::fixtures::{self, SettleCtx};
use tests::settle::{self, red_args, sec_args, sub_args, BUYER, ONE, SELLER};
use tests::{clock, units, Harness};

const CASE: [u8; 32] = [0xC5; 32];

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

#[test]
fn spec_independent_test_cancel_expire_reject_and_everyone_is_restored() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let b = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 0);
    let c = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let start = (
        settle::cash_of(&h, &s, &a.pubkey()),
        units::balance(&h, &s.m(), &b.pubkey()),
        settle::cash_of(&h, &s, &c.pubkey()),
    );

    // 1. A subscription whose cash is in, cancelled by the investor.
    let d1 = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), 3 * ONE, 1));
    settle::fund_cash(&mut h, &s, &d1, &a).unwrap();
    // 2. A redemption whose units are in, left to expire.
    let d2 = settle::created(&mut h, &s, &b, red_args(&s, &b.pubkey(), 4 * ONE, 2));
    settle::fund_units(&mut h, &d2, &b).unwrap();
    // 3. A secondary transfer whose cash is in, rejected by compliance.
    let d3 = settle::created(
        &mut h,
        &s,
        &c,
        sec_args(&b.pubkey(), &c.pubkey(), ONE, 5 * ONE, 3),
    );
    settle::fund_cash(&mut h, &s, &d3, &c).unwrap();

    settle::cancel(&mut h, &d1, &a, BUYER).expect("cancel");
    settle::reject(&mut h, &s, &d3, 1).expect("reject");
    clock::advance(&mut h.svm, 86_400);
    settle::expire(&mut h, &d2, &s.keeper).expect("expire");

    settle::return_cash(&mut h, &s, &d1, &s.keeper).unwrap();
    settle::return_units(&mut h, &d2, &s.keeper).unwrap();
    settle::return_cash(&mut h, &s, &d3, &s.keeper).unwrap();

    let end = (
        settle::cash_of(&h, &s, &a.pubkey()),
        units::balance(&h, &s.m(), &b.pubkey()),
        settle::cash_of(&h, &s, &c.pubkey()),
    );
    assert_eq!(end, start, "every funder has exactly what it started with");
    assert_eq!(units::token_balance(&h, &s.unit_escrow), 0);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), 0);
    let statuses: Vec<u8> = [d1, d2, d3]
        .iter()
        .map(|d| settle::read_si(&h, d).status)
        .collect();
    assert_eq!(statuses, vec![5, 6, 7], "Cancelled, Expired, Rejected");
}

#[test]
fn either_side_cancels_before_both_legs_are_in() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let b = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);

    // Instructed, cancelled by the Fund side (transfer agent).
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let meta = settle::cancel(&mut h, &d, &s.transfer_agent, SELLER).expect("cancel");
    let si = settle::read_si(&h, &d);
    assert_eq!(
        (si.status, si.units_to_return, si.cash_to_return),
        (5, 0, 0)
    );
    assert_eq!(si.closed_at, clock::now(&h.svm));
    let ev = one_cpi_event::<SettlementCancelled>(&meta);
    assert_eq!(
        (ev.seq, ev.units_to_return, ev.cash_to_return, ev.signer),
        (si.seq, 0, 0, s.transfer_agent.pubkey())
    );

    // SellerFunded (units in), cancelled by the buyer: the units are earmarked.
    let d = settle::created(
        &mut h,
        &s,
        &a,
        sec_args(&b.pubkey(), &a.pubkey(), 2 * ONE, ONE, 2),
    );
    settle::fund_units(&mut h, &d, &b).unwrap();
    settle::cancel(&mut h, &d, &a, BUYER).expect("buyer cancels");
    let si = settle::read_si(&h, &d);
    assert_eq!(
        (si.status, si.units_to_return, si.cash_to_return),
        (5, 2 * ONE, 0)
    );

    // BuyerFunded (cash in), cancelled by the seller: the cash is earmarked.
    let d = settle::created(
        &mut h,
        &s,
        &a,
        sec_args(&b.pubkey(), &a.pubkey(), ONE, 3 * ONE, 3),
    );
    settle::fund_cash(&mut h, &s, &d, &a).unwrap();
    settle::cancel(&mut h, &d, &b, SELLER).expect("seller cancels");
    let si = settle::read_si(&h, &d);
    assert_eq!(
        (si.status, si.units_to_return, si.cash_to_return),
        (5, 0, 3 * ONE)
    );
}

#[test]
fn only_the_sides_own_party_cancels() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    // The investor isn't the Fund side; the keeper and compliance aren't parties.
    assert_anchor_error(
        &settle::cancel(&mut h, &d, &a, SELLER),
        DvpError::Unauthorized,
    );
    for who in [&s.keeper, &s.compliance, &s.issuer] {
        assert_anchor_error(
            &settle::cancel(&mut h, &d, who, BUYER),
            DvpError::Unauthorized,
        );
    }
    assert_anchor_error(&settle::cancel(&mut h, &d, &a, 2), DvpError::InvalidSide);
    assert_eq!(settle::read_si(&h, &d).status, 0);
}

fn affirmed_sub(h: &mut Harness, s: &SettleCtx, n: u8) -> (Deal, solana_keypair::Keypair) {
    let a = fixtures::investor_with_cash(h, s, 10 * ONE);
    let d = settle::affirmed_subscription(h, s, &a, ONE, n);
    (d, a)
}

#[test]
fn an_affirmed_instruction_is_cancelled_only_when_both_sides_ask() {
    let (mut h, s) = ready();
    let (d, a) = affirmed_sub(&mut h, &s, 1);

    let meta = settle::cancel(&mut h, &d, &a, BUYER).expect("first request");
    let ev = one_cpi_event::<CancelRequested>(&meta);
    assert_eq!((ev.side, ev.signer), (BUYER, a.pubkey()));
    let si = settle::read_si(&h, &d);
    assert_eq!(
        (si.status, si.cancel_requests),
        (3, 2),
        "still Affirmed, buyer bit set"
    );
    assert_anchor_error(
        &settle::cancel(&mut h, &d, &a, BUYER),
        DvpError::CancelAlreadyRequested,
    );

    // Withdrawn, then asked again, then the Fund side agrees.
    let meta = settle::withdraw_cancel(&mut h, &d, &a, BUYER).expect("withdraw");
    assert_eq!(one_cpi_event::<CancelRequestWithdrawn>(&meta).side, BUYER);
    assert_eq!(settle::read_si(&h, &d).cancel_requests, 0);
    assert_anchor_error(
        &settle::withdraw_cancel(&mut h, &d, &a, BUYER),
        DvpError::NoCancelRequest,
    );
    settle::cancel(&mut h, &d, &a, BUYER).unwrap();
    settle::cancel(&mut h, &d, &s.transfer_agent, SELLER).expect("both agree");
    let si = settle::read_si(&h, &d);
    assert_eq!((si.status, si.cash_to_return), (5, ONE));
    // Nothing to withdraw once it is Cancelled.
    assert_anchor_error(
        &settle::withdraw_cancel(&mut h, &d, &a, BUYER),
        DvpError::InstructionFinal,
    );
}

#[test]
fn a_single_cancel_request_does_not_block_settlement() {
    let (mut h, s) = ready();
    let (d, a) = affirmed_sub(&mut h, &s, 1);
    settle::cancel(&mut h, &d, &a, BUYER).unwrap();
    settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("settles anyway (FR-023)");
    assert_eq!(units::balance(&h, &s.m(), &a.pubkey()), ONE);
}

#[test]
fn withdrawing_needs_an_affirmed_instruction_with_your_own_request() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    assert_anchor_error(
        &settle::withdraw_cancel(&mut h, &d, &a, BUYER),
        DvpError::NoCancelRequest,
    );
    // Affirmed, the Fund side asked: the buyer has no request of its own to withdraw.
    let (d, buyer) = affirmed_sub(&mut h, &s, 2);
    settle::cancel(&mut h, &d, &s.transfer_agent, SELLER).unwrap();
    assert_anchor_error(
        &settle::withdraw_cancel(&mut h, &d, &buyer, BUYER),
        DvpError::NoCancelRequest,
    );
}

#[test]
fn cancelling_after_expiry_is_allowed() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    settle::fund_cash(&mut h, &s, &d, &a).unwrap();
    clock::advance(&mut h.svm, 2 * 86_400);
    settle::cancel(&mut h, &d, &a, BUYER).expect("cancel after expiry");
    assert_eq!(settle::read_si(&h, &d).status, 5);
}

#[test]
fn anyone_expires_at_or_after_expiry_even_while_paused() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let exp = settle::read_si(&h, &d).expires_at_ts;
    clock::set_unix_timestamp(&mut h.svm, exp - 1);
    assert_anchor_error(&settle::expire(&mut h, &d, &s.keeper), DvpError::NotExpired);

    h.send(
        &[mmf_sdk::ix_governance::pause_transfers(
            &s.compliance.pubkey(),
            &s.m(),
            3,
            CASE,
        )],
        &[&s.compliance],
    )
    .unwrap();
    clock::set_unix_timestamp(&mut h.svm, exp);
    let stranger = h.funded_keypair();
    let meta = settle::expire(&mut h, &d, &stranger).expect("anyone, at expiry, while paused");
    let si = settle::read_si(&h, &d);
    assert_eq!(
        (si.status, si.units_to_return, si.cash_to_return),
        (6, 0, 0)
    );
    assert_eq!(si.closed_at, exp);
    let ev = one_cpi_event::<SettlementExpired>(&meta);
    assert_eq!(
        (
            ev.seq,
            ev.triggered_by,
            ev.units_to_return,
            ev.cash_to_return
        ),
        (si.seq, stranger.pubkey(), 0, 0)
    );
}

#[test]
fn nothing_but_a_return_works_on_a_final_instruction() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    // One instruction in each final status.
    let (settled, _) = affirmed_sub(&mut h, &s, 1);
    settle::settle_sub(&mut h, &s, &settled, &s.keeper).unwrap();
    let cancelled = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 2));
    settle::cancel(&mut h, &cancelled, &a, BUYER).unwrap();
    let rejected = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 3));
    settle::reject(&mut h, &s, &rejected, 3).unwrap();
    let expired = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 4));
    clock::advance(&mut h.svm, 86_400);
    settle::expire(&mut h, &expired, &s.keeper).unwrap();
    clock::advance(&mut h.svm, -86_400);

    for d in [settled, cancelled, rejected, expired] {
        let ta = &s.transfer_agent;
        let ix = ix_dvp::affirm_fund_side(&ta.pubkey(), &d);
        assert_anchor_error(&h.send(&[ix], &[ta]), DvpError::InstructionFinal);
        assert_anchor_error(
            &settle::fund_cash(&mut h, &s, &d, &a),
            DvpError::InstructionFinal,
        );
        assert_anchor_error(
            &settle::settle_sub(&mut h, &s, &d, &s.keeper),
            DvpError::InstructionFinal,
        );
        assert_anchor_error(
            &settle::cancel(&mut h, &d, &a, BUYER),
            DvpError::InstructionFinal,
        );
        assert_anchor_error(
            &settle::withdraw_cancel(&mut h, &d, &a, BUYER),
            DvpError::InstructionFinal,
        );
        assert_anchor_error(
            &settle::expire(&mut h, &d, &s.keeper),
            DvpError::InstructionFinal,
        );
        assert_anchor_error(
            &settle::reject(&mut h, &s, &d, 1),
            DvpError::InstructionFinal,
        );
    }
}
