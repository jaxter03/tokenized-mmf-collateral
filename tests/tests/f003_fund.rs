//! `affirm_and_fund` (FR-011–FR-014; contracts/dvp_settlement.md, checks in order). T030: the
//! cash leg and the Fund side's affirmation of a subscription. T042 (US2) adds the unit leg and
//! the redemption's cash leg.

use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_spl::token_2022::spl_token_2022::error::TokenError;
use dvp_settlement::events::LegFunded;
use dvp_settlement::DvpError;
use mmf_common::enums::ReasonCode;
use mmf_sdk::ix_dvp;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_token_error};
use tests::events::one_cpi_event;
use tests::fixtures::{self, SettleCtx};
use tests::settle::{self, sub_args, ONE};
use tests::{clock, Harness, Ix};

const CASE: [u8; 32] = [0xC5; 32];

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

/// Index of each `AffirmAndFund` account in the instruction.
const SOURCE: usize = 6;
const DESTINATION: usize = 7;
const LEG_MINT: usize = 8;

fn with_account(mut ix: Ix, index: usize, key: anchor_lang::prelude::Pubkey) -> Ix {
    let m = ix.accounts[index].clone();
    ix.accounts[index] = AccountMeta { pubkey: key, ..m };
    ix
}

#[test]
fn affirm_then_fund_and_fund_then_affirm() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);

    // Affirm first.
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), 2 * ONE, 1));
    let meta = settle::affirm(&mut h, &s, &d).expect("affirm");
    let si = settle::read_si(&h, &d);
    assert_eq!(si.status, 1, "SellerFunded");
    assert_eq!(si.seller_funded_at, clock::now(&h.svm));
    let ev = one_cpi_event::<LegFunded>(&meta);
    assert_eq!(
        (
            ev.seq,
            ev.side,
            ev.amount,
            ev.status_after,
            ev.signer,
            ev.external_ref
        ),
        (1, 0, 0, 1, s.transfer_agent.pubkey(), settle::ext(1))
    );
    assert_eq!(
        fixtures::cash_balance(&h, &s.cash_escrow),
        0,
        "affirming moves nothing"
    );

    clock::advance(&mut h.svm, 30);
    let meta = settle::fund_cash(&mut h, &s, &d, &a).expect("fund");
    let si = settle::read_si(&h, &d);
    assert_eq!(si.status, 3, "Affirmed");
    assert_eq!(si.buyer_funded_at, clock::now(&h.svm));
    let ev = one_cpi_event::<LegFunded>(&meta);
    assert_eq!(
        (ev.side, ev.amount, ev.status_after, ev.signer),
        (1, 2 * ONE, 3, a.pubkey())
    );
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 8 * ONE);
    assert_eq!(
        fixtures::cash_balance(&h, &s.cash_escrow),
        2 * ONE,
        "held in escrow"
    );

    // Fund first.
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 2));
    settle::fund_cash(&mut h, &s, &d, &a).unwrap();
    assert_eq!(settle::read_si(&h, &d).status, 2, "BuyerFunded");
    settle::affirm(&mut h, &s, &d).unwrap();
    assert_eq!(settle::read_si(&h, &d).status, 3, "Affirmed");
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), 3 * ONE);
}

#[test]
fn side_must_be_seller_or_buyer() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let mut ix = ix_dvp::affirm_fund_side(&s.transfer_agent.pubkey(), &d);
    *ix.data.last_mut().unwrap() = 2;
    let res = h.send(&[ix], &[&s.transfer_agent]);
    assert_anchor_error(&res, DvpError::InvalidSide);
}

#[test]
fn not_after_expiry_or_once_final() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let exp = settle::read_si(&h, &d).expires_at_ts;
    clock::set_unix_timestamp(&mut h.svm, exp);
    assert_anchor_error(
        &settle::affirm(&mut h, &s, &d),
        DvpError::InstructionExpired,
    );
    assert_anchor_error(
        &settle::fund_cash(&mut h, &s, &d, &a),
        DvpError::InstructionExpired,
    );

    // Settled is final (Cancelled, Expired and Rejected are covered with US4).
    clock::set_unix_timestamp(&mut h.svm, exp - 86_400);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 2);
    settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("settle");
    assert_anchor_error(&settle::affirm(&mut h, &s, &d), DvpError::InstructionFinal);
    assert_anchor_error(
        &settle::fund_cash(&mut h, &s, &d, &a),
        DvpError::InstructionFinal,
    );
}

#[test]
fn only_the_sides_own_party() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let b = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));

    // The investor can't affirm the Fund side.
    let ix = ix_dvp::affirm_fund_side(&a.pubkey(), &d);
    assert_anchor_error(&h.send(&[ix], &[&a]), DvpError::Unauthorized);
    // Nobody but the buyer funds its cash, not even the transfer agent.
    let ta_cash = fixtures::create_cash_ata(&mut h, &s.transfer_agent.pubkey(), &s.cash_mint);
    let ix = ix_dvp::fund_cash(&s.transfer_agent.pubkey(), &d, &ta_cash);
    assert_anchor_error(&h.send(&[ix], &[&s.transfer_agent]), DvpError::Unauthorized);
    assert_anchor_error(
        &settle::fund_cash(&mut h, &s, &d, &b),
        DvpError::Unauthorized,
    );
    assert_eq!(settle::read_si(&h, &d).status, 0, "still Instructed");
}

#[test]
fn each_leg_once() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    settle::affirm(&mut h, &s, &d).unwrap();
    assert_anchor_error(&settle::affirm(&mut h, &s, &d), DvpError::LegAlreadyFunded);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 2));
    settle::fund_cash(&mut h, &s, &d, &a).unwrap();
    assert_anchor_error(
        &settle::fund_cash(&mut h, &s, &d, &a),
        DvpError::LegAlreadyFunded,
    );
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 9 * ONE, "paid once");
}

#[test]
fn a_suspended_investor_cannot_fund() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let ix = mmf_sdk::ix_governance::suspend_holder(
        &s.compliance.pubkey(),
        &s.m(),
        &a.pubkey(),
        ReasonCode::SanctionsHit as u8,
        CASE,
    );
    h.send(&[ix], &[&s.compliance]).unwrap();
    assert_anchor_error(
        &settle::fund_cash(&mut h, &s, &d, &a),
        DvpError::PartyNotEligible,
    );
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), ONE, "no cash taken");
}

#[test]
fn cash_must_be_the_funds_cash_and_go_to_the_escrow() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let own = fixtures::cash_ata(&h, &a.pubkey(), &s.cash_mint);
    let base = || ix_dvp::fund_cash(&a.pubkey(), &d, &own);

    // A source account of another mint.
    let other_mint = fixtures::create_cash_mint(&mut h, fixtures::CASH_DECIMALS);
    fixtures::mint_cash(&mut h, &other_mint, &a.pubkey(), ONE);
    let foreign = fixtures::cash_ata(&h, &a.pubkey(), &other_mint);
    let res = h.send(&[with_account(base(), SOURCE, foreign)], &[&a]);
    assert_anchor_error(&res, DvpError::WrongAccount);
    // The other mint passed as the leg's mint.
    let res = h.send(&[with_account(base(), LEG_MINT, other_mint)], &[&a]);
    assert_anchor_error(&res, DvpError::WrongAccount);
    // A destination other than the cash escrow (here, the investor's own account).
    let res = h.send(&[with_account(base(), DESTINATION, own)], &[&a]);
    assert_anchor_error(&res, DvpError::WrongAccount);
    assert_eq!(settle::read_si(&h, &d).status, 0);
}

#[test]
fn not_enough_cash_changes_nothing() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE - 1);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    assert_token_error(
        &settle::fund_cash(&mut h, &s, &d, &a),
        TokenError::InsufficientFunds,
    );
    assert_eq!(settle::read_si(&h, &d).status, 0);
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), ONE - 1);
}

// ---- T042 (US2): the unit leg and the redemption's cash ----------------------------------------

use compliance_hook::{HolderState, HookError};
use tests::fixtures::{read, NAV_ONE, SETTLE_DAY};
use tests::settle::red_args;
use tests::units;

/// 4.5 % a year, per day, × 10^12.
const RATE: u64 = 123_287_671;

#[test]
fn units_move_into_escrow_through_the_hook() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    // A day at RATE while the investor holds all 10 units.
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    fixtures::publish(&mut h, &s, NAV_ONE, RATE as i64, 1).unwrap();

    let d = settle::created(&mut h, &s, &a, red_args(&s, &a.pubkey(), 4 * ONE, 1));
    let meta = settle::fund_units(&mut h, &d, &a).expect("fund units");
    assert_eq!(units::balance(&h, &s.m(), &a.pubkey()), 6 * ONE);
    assert_eq!(units::token_balance(&h, &s.unit_escrow), 4 * ONE);
    let si = settle::read_si(&h, &d);
    assert_eq!((si.status, si.seller_funded_at), (1, clock::now(&h.svm)));
    let ev = one_cpi_event::<LegFunded>(&meta);
    assert_eq!((ev.side, ev.amount, ev.status_after), (0, 4 * ONE, 1));

    // The hook settled the seller's interest on its old balance (10 units × one day).
    let rec: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &a.pubkey()).0);
    assert_eq!(
        rec.accrued_e18,
        u128::from(10 * ONE) * u128::from(RATE) * 1_000_000
    );
    assert_eq!(rec.last_balance, 6 * ONE);
    let esc: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &s.escrow).0);
    assert_eq!((esc.last_balance, esc.accrued_e18), (4 * ONE, 0));

    // The transfer agent funds the cash from the treasury: Affirmed.
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    settle::fund_redemption_cash(&mut h, &s, &d).expect("treasury cash");
    assert_eq!(settle::read_si(&h, &d).status, 3);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_treasury), 6 * ONE);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), 4 * ONE);
}

#[test]
fn unit_leg_refused_while_paused_or_suspended_or_short() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 2 * ONE, 0);
    let c = s.compliance.pubkey();
    let d = settle::created(&mut h, &s, &a, red_args(&s, &a.pubkey(), ONE, 1));

    h.send(
        &[mmf_sdk::ix_governance::pause_transfers(&c, &s.m(), 3, CASE)],
        &[&s.compliance],
    )
    .unwrap();
    assert_anchor_error(
        &settle::fund_units(&mut h, &d, &a),
        HookError::TransfersPaused,
    );
    h.send(
        &[mmf_sdk::ix_governance::unpause_transfers(
            &c,
            &s.m(),
            3,
            CASE,
        )],
        &[&s.compliance],
    )
    .unwrap();

    let short = settle::created(&mut h, &s, &a, red_args(&s, &a.pubkey(), 3 * ONE, 2));
    assert_token_error(
        &settle::fund_units(&mut h, &short, &a),
        TokenError::InsufficientFunds,
    );

    let ix = mmf_sdk::ix_governance::suspend_holder(
        &c,
        &s.m(),
        &a.pubkey(),
        ReasonCode::SanctionsHit as u8,
        CASE,
    );
    h.send(&[ix], &[&s.compliance]).unwrap();
    assert_anchor_error(
        &settle::fund_units(&mut h, &d, &a),
        DvpError::PartyNotEligible,
    );
    assert_eq!(
        units::balance(&h, &s.m(), &a.pubkey()),
        2 * ONE,
        "nothing moved"
    );
}

#[test]
fn redemption_cash_only_from_the_treasury_and_only_by_the_transfer_agent() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 2 * ONE, 0);
    settle::fill_treasury(&mut h, &s, 10 * ONE);
    let d = settle::created(&mut h, &s, &a, red_args(&s, &a.pubkey(), ONE, 1));

    // The transfer agent paying from any other cash account.
    let other = fixtures::create_cash_ata(&mut h, &s.keeper.pubkey(), &s.cash_mint);
    fixtures::mint_cash(&mut h, &s.cash_mint, &s.keeper.pubkey(), ONE);
    let ix = ix_dvp::fund_cash(&s.transfer_agent.pubkey(), &d, &other);
    let res = h.send(&[ix], &[&s.transfer_agent]);
    assert_anchor_error(&res, DvpError::WrongCashSource);

    // The investor can't fund the Fund's cash side; the transfer agent can't fund its units.
    let ix = ix_dvp::fund_redemption_cash(&a.pubkey(), &d, &s.cash_treasury);
    assert_anchor_error(&h.send(&[ix], &[&a]), DvpError::Unauthorized);
    let ix = ix_dvp::fund_units(&s.transfer_agent.pubkey(), &d);
    assert_anchor_error(&h.send(&[ix], &[&s.transfer_agent]), DvpError::Unauthorized);
    assert_eq!(settle::read_si(&h, &d).status, 0);
}

#[test]
fn units_may_be_funded_before_valid_from() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let mut args = red_args(&s, &a.pubkey(), ONE, 1);
    args.not_before_ts = clock::now(&h.svm) + 3_600;
    let d = settle::created(&mut h, &s, &a, args);
    settle::fund_units(&mut h, &d, &a).expect("early funding is allowed");
}

// US2 scenario 3 (locked units can't fund a redemption) is verified in F004, which builds hook
// step 5: pledge a holder's only unit, then `fund_units` must fail with
// `HookError::PledgedUnitsLocked` and move nothing. No placeholder test here: an ignored test that
// panics would break `cargo test -- --ignored`.
