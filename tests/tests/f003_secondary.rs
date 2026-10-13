//! T053, US3 Secondary transfer against cash: two approved holders swap existing units for cash,
//! both-or-neither. No units are created or destroyed.

use compliance_hook::{HolderState, HookError, Registry};
use dvp_settlement::events::Settled;
use dvp_settlement::DvpError;
use fund::FundState;
use mmf_common::enums::ReasonCode;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_i4};
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, NAV_ONE, SETTLE_DAY};
use tests::settle::{self, sec_args, ONE};
use tests::{clock, units, Harness};

const CASE: [u8; 32] = [0xC5; 32];
/// 4.5 % a year, per day, × 10^12.
const RATE: u64 = 123_287_671;

// Scenario 5 (same holder on both sides, zero cash) is covered by f003_create.rs
// (`zero_units_zero_reference_and_same_party`, `cash_amounts_round_in_the_funds_favour`).

#[test]
fn spec_independent_test_a_million_units_for_999_500_dollars() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let a = fixtures::investor_with_units(&mut h, &s, 1_000_000 * ONE, 0);
    let b = fixtures::investor_with_cash(&mut h, &s, 999_500 * ONE);
    let supply = units::supply(&h, &s.m());

    let d = settle::created(
        &mut h,
        &s,
        &a,
        sec_args(&a.pubkey(), &b.pubkey(), 1_000_000 * ONE, 999_500 * ONE, 1),
    );
    assert_eq!(settle::read_si(&h, &d).status, 0);
    settle::fund_units(&mut h, &d, &a).unwrap();
    settle::fund_cash(&mut h, &s, &d, &b).unwrap();
    let meta = settle::settle_sec(&mut h, &d, &s.keeper).expect("settle");

    assert_eq!(units::balance(&h, &s.m(), &b.pubkey()), 1_000_000 * ONE);
    assert_eq!(units::balance(&h, &s.m(), &a.pubkey()), 0);
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 999_500 * ONE);
    assert_eq!(settle::cash_of(&h, &s, &b.pubkey()), 0);
    assert_eq!(units::token_balance(&h, &s.unit_escrow), 0);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), 0);
    assert_eq!(
        units::supply(&h, &s.m()),
        supply,
        "no units created or destroyed"
    );
    let fs: FundState = read(&h, &s.fund_state());
    assert_eq!(fs.units_outstanding, supply);
    assert_i4(&h, &s.m());

    // I10: the seller left the holder count, the buyer joined it.
    let reg: Registry = read(&h, &s.registry());
    assert_eq!(reg.holder_count, 1);
    let ah: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &a.pubkey()).0);
    assert_eq!(ah.last_balance, 0);

    let ev = one_cpi_event::<Settled>(&meta);
    assert_eq!(
        (ev.kind, ev.units, ev.cash_amount, ev.seller, ev.buyer),
        (2, 1_000_000 * ONE, 999_500 * ONE, a.pubkey(), b.pubkey())
    );
}

#[test]
fn either_order_of_funding_and_nothing_until_both() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    let b = fixtures::investor_with_cash(&mut h, &s, 100 * ONE);

    // Cash first, created by the buyer.
    let d = settle::created(
        &mut h,
        &s,
        &b,
        sec_args(&a.pubkey(), &b.pubkey(), 2 * ONE, 3 * ONE, 1),
    );
    settle::fund_cash(&mut h, &s, &d, &b).unwrap();
    assert_eq!(settle::read_si(&h, &d).status, 2, "BuyerFunded");
    assert_anchor_error(
        &settle::settle_sec(&mut h, &d, &s.keeper),
        DvpError::NotAffirmed,
    );
    settle::fund_units(&mut h, &d, &a).unwrap();
    assert_eq!(settle::read_si(&h, &d).status, 3);
    settle::settle_sec(&mut h, &d, &s.keeper).unwrap();

    // Units first: only the seller funded.
    let d = settle::created(
        &mut h,
        &s,
        &b,
        sec_args(&a.pubkey(), &b.pubkey(), ONE, ONE, 2),
    );
    settle::fund_units(&mut h, &d, &a).unwrap();
    assert_eq!(settle::read_si(&h, &d).status, 1, "SellerFunded");
    assert_anchor_error(
        &settle::settle_sec(&mut h, &d, &s.keeper),
        DvpError::NotAffirmed,
    );
    assert_eq!(
        units::balance(&h, &s.m(), &b.pubkey()),
        2 * ONE,
        "only the first swap"
    );
}

#[test]
fn a_buyer_suspended_after_funding_receives_nothing() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    let b = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_secondary(&mut h, &s, &a, &b, ONE, ONE, 1);
    let ix = mmf_sdk::ix_governance::suspend_holder(
        &s.compliance.pubkey(),
        &s.m(),
        &b.pubkey(),
        ReasonCode::SanctionsHit as u8,
        CASE,
    );
    h.send(&[ix], &[&s.compliance]).unwrap();
    assert_anchor_error(
        &settle::settle_sec(&mut h, &d, &s.keeper),
        DvpError::PartyNotEligible,
    );
    assert_eq!(units::token_balance(&h, &s.unit_escrow), ONE);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), ONE);
}

#[test]
fn the_holder_cap_applies_to_the_buyer() {
    // Cap 2: the seller (keeping some units) and another holder already count.
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund_with_cap(&mut h, 2);
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    let _c = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let b = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_secondary(&mut h, &s, &a, &b, 4 * ONE, ONE, 1);
    assert_anchor_error(
        &settle::settle_sec(&mut h, &d, &s.keeper),
        HookError::HolderCapReached,
    );
    assert_eq!(units::balance(&h, &s.m(), &b.pubkey()), 0);
    assert_eq!(
        settle::cash_of(&h, &s, &a.pubkey()),
        0,
        "the seller isn't paid either"
    );
    assert_eq!(settle::read_si(&h, &d).status, 3);
}

#[test]
fn interest_for_a_day_in_escrow_goes_to_the_escrow_and_the_buyer_starts_fresh() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    let b = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let mut args = sec_args(&a.pubkey(), &b.pubkey(), 10 * ONE, 5 * ONE, 1);
    args.expires_at_ts = clock::now(&h.svm) + 2 * 86_400;
    let d = settle::created(&mut h, &s, &b, args);
    settle::fund_units(&mut h, &d, &a).unwrap();
    settle::fund_cash(&mut h, &s, &d, &b).unwrap();

    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    fixtures::publish(&mut h, &s, NAV_ONE, RATE as i64, 1).unwrap();
    settle::settle_sec(&mut h, &d, &s.keeper).unwrap();

    let fs: FundState = read(&h, &s.fund_state());
    let day = u128::from(RATE) * 1_000_000;
    let escrow: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &s.escrow).0);
    assert_eq!(escrow.accrued_e18, u128::from(10 * ONE) * day);
    let seller: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &a.pubkey()).0);
    assert_eq!(seller.accrued_e18, 0, "held nothing at the publish");
    let buyer: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &b.pubkey()).0);
    assert_eq!(
        (buyer.accrued_e18, buyer.index_at_last_touch),
        (0, fs.accrual_index)
    );
}
