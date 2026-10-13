//! T043, US2 Redeem (FR-016–FR-022): units are destroyed only as their cash is paid, within the
//! daily redemption limit. The fixture's limit is 10 % (`standard_terms` `redemption_gate_bps`
//! = 1 000) of the units outstanding at the start of the UTC day.

use dvp_settlement::events::Settled;
use dvp_settlement::DvpError;
use fund::events::RedemptionBurned;
use fund::{FundError, FundState};
use mmf_common::enums::ReasonCode;
use mmf_sdk::ix_governance;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_i4};
use tests::clock::{self, DAY};
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, SettleCtx, SETTLE_DAY};
use tests::settle::{self, red_args, ONE};
use tests::{units, Harness};

const CASE: [u8; 32] = [0xC5; 32];

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

fn fs(h: &Harness, s: &SettleCtx) -> FundState {
    read(h, &s.fund_state())
}

#[test]
fn gate_bps_is_what_these_tests_assume() {
    let (h, s) = ready();
    assert_eq!(fs(&h, &s).terms.redemption_gate_bps, 1_000);
}

#[test]
fn spec_independent_test_redeem_half_a_million() {
    let (mut h, s) = ready();
    // 5M outstanding at the start of the day, so 500 000 is exactly the 10 % limit.
    let investor = fixtures::investor_with_units(&mut h, &s, 500_000 * ONE, 0);
    let _other = fixtures::investor_with_units(&mut h, &s, 4_500_000 * ONE, 0);
    settle::fill_treasury(&mut h, &s, 500_000 * ONE);

    let d = settle::affirmed_redemption(&mut h, &s, &investor, 500_000 * ONE, 1);
    let meta = settle::settle_red(&mut h, &d, &s.keeper).expect("settle");

    assert_eq!(units::balance(&h, &s.m(), &investor.pubkey()), 0);
    assert_eq!(settle::cash_of(&h, &s, &investor.pubkey()), 500_000 * ONE);
    assert_eq!(units::token_balance(&h, &s.unit_escrow), 0, "destroyed");
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), 0);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_treasury), 0);
    let f = fs(&h, &s);
    assert_eq!(f.units_outstanding, 4_500_000 * ONE);
    assert_eq!(f.redeemed_today, 500_000 * ONE);
    assert_eq!(f.units_at_day_start, 5_000_000 * ONE);
    assert_eq!(f.day_start_ts, i64::from(SETTLE_DAY) * DAY);
    assert_i4(&h, &s.m());
    assert_eq!(settle::read_si(&h, &d).status, 4);

    let burned = one_cpi_event::<RedemptionBurned>(&meta);
    assert_eq!(
        (burned.settlement, burned.units, burned.units_outstanding),
        (d.settlement(), 500_000 * ONE, 4_500_000 * ONE)
    );
    assert_eq!(
        (burned.redeemed_today, burned.units_at_day_start),
        (500_000 * ONE, 5_000_000 * ONE)
    );
    let ev = one_cpi_event::<Settled>(&meta);
    assert_eq!(
        (ev.kind, ev.units, ev.cash_amount),
        (1, 500_000 * ONE, 500_000 * ONE)
    );
    assert_eq!((ev.seller, ev.buyer), (investor.pubkey(), s.fund_state()));
}

#[test]
fn cash_is_rounded_down_at_a_price_below_one_dollar() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 0);
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    fixtures::publish(&mut h, &s, 99_999_999, 0, 1).unwrap();
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_redemption(&mut h, &s, &a, 1_000_001, 1);
    assert_eq!(settle::read_si(&h, &d).cash_amount, 1_000_000);
    settle::settle_red(&mut h, &d, &s.keeper).unwrap();
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 1_000_000);
}

#[test]
fn the_daily_limit_holds_exactly_and_resets_at_utc_midnight() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 1_000 * ONE, 0);
    settle::fill_treasury(&mut h, &s, 1_000 * ONE);

    // 10 % of 1 000 = 100: exactly the limit settles.
    let d1 = settle::affirmed_redemption(&mut h, &s, &a, 100 * ONE, 1);
    settle::settle_red(&mut h, &d1, &s.keeper).expect("exactly at the limit");

    // One more unit today is refused, and nothing moves. It can still settle tomorrow (it needs
    // an expiry past the 24 h default to still be open then).
    let mut args = red_args(&s, &a.pubkey(), 1, 2);
    args.expires_at_ts = clock::now(&h.svm) + 2 * DAY;
    let d2 = settle::created(&mut h, &s, &a, args);
    settle::fund_units(&mut h, &d2, &a).unwrap();
    settle::fund_redemption_cash(&mut h, &s, &d2).unwrap();
    let escrow_before = units::token_balance(&h, &s.unit_escrow);
    assert_anchor_error(
        &settle::settle_red(&mut h, &d2, &s.keeper),
        FundError::RedemptionLimitExceeded,
    );
    assert_eq!(settle::read_si(&h, &d2).status, 3, "still Affirmed");
    assert_eq!(units::token_balance(&h, &s.unit_escrow), escrow_before);
    assert_eq!(fs(&h, &s).redeemed_today, 100 * ONE);

    // Next UTC day: a new base (900 units at the start of the day) and a fresh counter.
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    settle::settle_red(&mut h, &d2, &s.keeper).expect("after midnight");
    let f = fs(&h, &s);
    assert_eq!(f.units_at_day_start, 900 * ONE);
    assert_eq!(f.redeemed_today, 1);
    assert_eq!(f.day_start_ts, i64::from(SETTLE_DAY + 1) * DAY);
    assert_i4(&h, &s.m());
}

#[test]
fn subscriptions_during_the_day_do_not_raise_the_limit() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 1_000 * ONE, 0);
    let b = fixtures::investor_with_cash(&mut h, &s, 1_000 * ONE);
    settle::fill_treasury(&mut h, &s, 1_000 * ONE);
    // The day's first mint: the day starts with 1 000 outstanding, then 1 000 more arrive.
    let d = settle::affirmed_subscription(&mut h, &s, &b, 1_000 * ONE, 1);
    settle::settle_sub(&mut h, &s, &d, &s.keeper).unwrap();
    assert_eq!(fs(&h, &s).units_at_day_start, 1_000 * ONE);

    let d = settle::affirmed_redemption(&mut h, &s, &a, 100 * ONE + 1, 2);
    assert_anchor_error(
        &settle::settle_red(&mut h, &d, &s.keeper),
        FundError::RedemptionLimitExceeded,
    );
    let d = settle::affirmed_redemption(&mut h, &s, &a, 100 * ONE, 3);
    settle::settle_red(&mut h, &d, &s.keeper).expect("10 % of the day's starting units");
}

#[test]
fn nothing_can_be_redeemed_on_the_day_a_fund_first_has_units() {
    // Research R9: units created today weren't outstanding at the start of the day.
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, 10 * ONE, 1);
    settle::settle_sub(&mut h, &s, &d, &s.keeper).unwrap();
    let d = settle::affirmed_redemption(&mut h, &s, &a, 1, 2);
    assert_anchor_error(
        &settle::settle_red(&mut h, &d, &s.keeper),
        FundError::RedemptionLimitExceeded,
    );
}

#[test]
fn not_while_paused_even_though_a_burn_runs_no_hook() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 0);
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_redemption(&mut h, &s, &a, ONE, 1);
    h.send(
        &[ix_governance::pause_transfers(
            &s.compliance.pubkey(),
            &s.m(),
            3,
            CASE,
        )],
        &[&s.compliance],
    )
    .unwrap();
    assert_anchor_error(
        &settle::settle_red(&mut h, &d, &s.keeper),
        DvpError::TransfersPaused,
    );
    assert_eq!(units::token_balance(&h, &s.unit_escrow), ONE, "not burned");
}

#[test]
fn a_redeemer_suspended_after_funding_is_not_paid() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 0);
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_redemption(&mut h, &s, &a, ONE, 1);
    let ix = ix_governance::suspend_holder(
        &s.compliance.pubkey(),
        &s.m(),
        &a.pubkey(),
        ReasonCode::SanctionsHit as u8,
        CASE,
    );
    h.send(&[ix], &[&s.compliance]).unwrap();
    assert_anchor_error(
        &settle::settle_red(&mut h, &d, &s.keeper),
        DvpError::PartyNotEligible,
    );
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 0);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), ONE);
}

#[test]
fn a_seller_without_a_cash_account_is_paid_once_one_exists() {
    // Research R14: payment goes only to the seller's own cash ATA, which anyone may create.
    let (mut h, s) = ready();
    let a = fixtures::new_investor(&mut h, &s);
    units::seed_units(&mut h, &s.m(), &a.pubkey(), 100 * ONE);
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_redemption(&mut h, &s, &a, ONE, 1);
    assert!(settle::settle_red(&mut h, &d, &s.keeper).is_err());
    assert_eq!(
        units::token_balance(&h, &s.unit_escrow),
        ONE,
        "nothing burned"
    );
    assert_eq!(settle::read_si(&h, &d).status, 3);

    fixtures::create_cash_ata(&mut h, &a.pubkey(), &s.cash_mint);
    settle::settle_red(&mut h, &d, &s.keeper).expect("now payable");
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), ONE);
}

#[test]
fn a_redemption_settles_once() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 0);
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_redemption(&mut h, &s, &a, ONE, 1);
    settle::settle_red(&mut h, &d, &s.keeper).unwrap();
    assert_anchor_error(
        &settle::settle_red(&mut h, &d, &s.keeper),
        DvpError::InstructionFinal,
    );
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), ONE, "paid once");
    assert_i4(&h, &s.m());
}

#[test]
fn redemption_steps_fit_their_compute_budgets() {
    // Research R15: funding units ≤ 90 000 CU, settling a redemption ≤ 120 000 CU.
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 0);
    settle::fill_treasury(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, red_args(&s, &a.pubkey(), ONE, 1));
    let fund = settle::fund_units(&mut h, &d, &a)
        .unwrap()
        .compute_units_consumed;
    settle::fund_redemption_cash(&mut h, &s, &d).unwrap();
    let settle = settle::settle_red(&mut h, &d, &s.keeper)
        .unwrap()
        .compute_units_consumed;
    println!("fund units: {fund} CU; settle redemption: {settle} CU");
    assert!(fund <= 90_000, "{fund}");
    assert!(settle <= 120_000, "{settle}");
}
