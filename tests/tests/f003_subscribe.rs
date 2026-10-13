//! T031, US1 Subscribe (FR-015–FR-019): units come into existence only as their cash is paid to
//! the treasury, both in one step.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use compliance_hook::HookError;
use dvp_settlement::events::Settled;
use dvp_settlement::DvpError;
use fund::events::SubscriptionMinted;
use fund::FundState;
use mmf_common::enums::ReasonCode;
use mmf_sdk::{ix_dvp, ix_governance};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_i4};
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, SettleCtx};
use tests::settle::{self, sub_args, ONE};
use tests::{clock, units, Harness, Ix};

const CASE: [u8; 32] = [0xC5; 32];
const SANCTIONS: u8 = ReasonCode::SanctionsHit as u8;

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

fn supply(h: &Harness, s: &SettleCtx) -> u64 {
    units::supply(h, &s.m())
}

fn outstanding(h: &Harness, s: &SettleCtx) -> u64 {
    read::<FundState>(h, &s.fund_state()).units_outstanding
}

fn with_account(mut ix: Ix, key_now: Pubkey, key_new: Pubkey) -> Ix {
    let at = ix
        .accounts
        .iter()
        .position(|m| m.pubkey == key_now)
        .expect("account present");
    let m = ix.accounts[at].clone();
    ix.accounts[at] = AccountMeta {
        pubkey: key_new,
        ..m
    };
    ix
}

#[test]
fn spec_independent_test_two_million_at_one_dollar() {
    let (mut h, s) = ready();
    let investor = fixtures::investor_with_cash(&mut h, &s, 2_000_000 * ONE);
    let before = supply(&h, &s);

    let d = settle::created(
        &mut h,
        &s,
        &investor,
        sub_args(&s, &investor.pubkey(), 2_000_000 * ONE, 1),
    );
    settle::affirm(&mut h, &s, &d).unwrap();
    settle::fund_cash(&mut h, &s, &d, &investor).unwrap();
    assert_eq!(supply(&h, &s), before, "no unit exists before settlement");

    let meta = settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("settle");

    assert_eq!(
        units::balance(&h, &s.m(), &investor.pubkey()),
        2_000_000 * ONE
    );
    assert_eq!(settle::cash_of(&h, &s, &investor.pubkey()), 0);
    assert_eq!(
        fixtures::cash_balance(&h, &s.cash_treasury),
        2_000_000 * ONE
    );
    assert_eq!(units::token_balance(&h, &s.unit_escrow), 0);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), 0);
    assert_eq!(outstanding(&h, &s), before + 2_000_000 * ONE);
    assert_i4(&h, &s.m());

    let si = settle::read_si(&h, &d);
    assert_eq!(si.status, 4, "Settled");
    assert_eq!(si.closed_at, clock::now(&h.svm));
    assert_eq!((si.units_to_return, si.cash_to_return), (0, 0));

    // I5: the mint and the treasury's cash are in the same transaction.
    let minted = one_cpi_event::<SubscriptionMinted>(&meta);
    assert_eq!(
        (
            minted.fund,
            minted.settlement,
            minted.units,
            minted.units_outstanding
        ),
        (
            s.fund_state(),
            d.settlement(),
            2_000_000 * ONE,
            before + 2_000_000 * ONE
        )
    );
    let ev = one_cpi_event::<Settled>(&meta);
    assert_eq!(
        (ev.seq, ev.kind, ev.units, ev.cash_amount, ev.nav_seq),
        (1, 0, 2_000_000 * ONE, 2_000_000 * ONE, 1)
    );
    assert_eq!(
        (ev.seller, ev.buyer, ev.triggered_by, ev.external_ref),
        (
            s.fund_state(),
            investor.pubkey(),
            s.keeper.pubkey(),
            settle::ext(1)
        )
    );
}

#[test]
fn nothing_settles_until_both_sides_are_in() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);

    // Instructed, Fund side only, cash only: NotAffirmed and nothing is created.
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        DvpError::NotAffirmed,
    );
    settle::affirm(&mut h, &s, &d).unwrap();
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        DvpError::NotAffirmed,
    );
    let d2 = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 2));
    settle::fund_cash(&mut h, &s, &d2, &a).unwrap();
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d2, &s.keeper),
        DvpError::NotAffirmed,
    );
    assert_eq!(supply(&h, &s), 0);
}

#[test]
fn settle_only_inside_its_window() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let now = clock::now(&h.svm);
    let mut args = sub_args(&s, &a.pubkey(), ONE, 1);
    args.not_before_ts = now + 600;
    args.expires_at_ts = now + 1_200;
    let d = settle::created(&mut h, &s, &a, args);
    settle::affirm(&mut h, &s, &d).unwrap();
    settle::fund_cash(&mut h, &s, &d, &a).expect("funding before valid-from is allowed");

    clock::set_unix_timestamp(&mut h.svm, now + 599);
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        DvpError::NotYetValid,
    );
    clock::set_unix_timestamp(&mut h.svm, now + 1_200);
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        DvpError::InstructionExpired,
    );
    clock::set_unix_timestamp(&mut h.svm, now + 600);
    settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("at valid-from");
}

#[test]
fn not_while_paused() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    let c = s.compliance.pubkey();
    h.send(
        &[ix_governance::pause_transfers(&c, &s.m(), 3, CASE)],
        &[&s.compliance],
    )
    .unwrap();
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        DvpError::TransfersPaused,
    );
    assert_eq!(supply(&h, &s), 0);
    h.send(
        &[ix_governance::unpause_transfers(&c, &s.m(), 3, CASE)],
        &[&s.compliance],
    )
    .unwrap();
    settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("after unpause");
}

#[test]
fn an_investor_suspended_after_funding_blocks_settlement() {
    // US1 scenario 4 and the spec's edge case: nothing moves, the instruction stays Affirmed.
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    let c = s.compliance.pubkey();
    h.send(
        &[ix_governance::suspend_holder(
            &c,
            &s.m(),
            &a.pubkey(),
            SANCTIONS,
            CASE,
        )],
        &[&s.compliance],
    )
    .unwrap();
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        DvpError::PartyNotEligible,
    );
    assert_eq!(settle::read_si(&h, &d).status, 3, "still Affirmed");
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), ONE);
    assert_eq!(supply(&h, &s), 0);

    h.send(
        &[ix_governance::reinstate_holder(
            &c,
            &s.m(),
            &a.pubkey(),
            SANCTIONS,
            CASE,
        )],
        &[&s.compliance],
    )
    .unwrap();
    settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("after reinstatement");
}

#[test]
fn a_failed_delivery_creates_nothing() {
    // Holder cap 1: one investor already holds units, so delivering to a new one fails at the
    // hook. The mint in the same instruction is undone with it (FR-017).
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund_with_cap(&mut h, 1);
    let _holder = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let newcomer = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &newcomer, ONE, 1);
    let before = supply(&h, &s);
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        HookError::HolderCapReached,
    );
    assert_eq!(supply(&h, &s), before, "nothing minted");
    assert_eq!(outstanding(&h, &s), before);
    assert_eq!(
        fixtures::cash_balance(&h, &s.cash_escrow),
        ONE,
        "cash still in escrow"
    );
    assert_eq!(settle::read_si(&h, &d).status, 3);
}

#[test]
fn units_and_cash_go_only_to_the_parties_accounts() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let thief = fixtures::investor_with_cash(&mut h, &s, 0);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    let base = || ix_dvp::settle_subscription(&s.keeper.pubkey(), &d, &s.cash_treasury);

    let thief_cash = fixtures::cash_ata(&h, &thief.pubkey(), &s.cash_mint);
    let res = h.send(
        &[with_account(base(), s.cash_treasury, thief_cash)],
        &[&s.keeper],
    );
    assert_anchor_error(&res, DvpError::WrongAccount);

    let buyer_units = mmf_sdk::pda::holding_account(&a.pubkey(), &s.m());
    let thief_units = mmf_sdk::pda::holding_account(&thief.pubkey(), &s.m());
    let res = h.send(
        &[with_account(base(), buyer_units, thief_units)],
        &[&s.keeper],
    );
    assert_anchor_error(&res, DvpError::WrongAccount);
    assert_eq!(supply(&h, &s), 0);
}

#[test]
fn settle_twice_is_rejected() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    settle::settle_sub(&mut h, &s, &d, &s.keeper).unwrap();
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        DvpError::InstructionFinal,
    );
    assert_eq!(
        units::balance(&h, &s.m(), &a.pubkey()),
        ONE,
        "delivered once"
    );
    assert_i4(&h, &s.m());
}

#[test]
fn anyone_can_trigger_with_the_same_result() {
    // FR-015: the keeper, a stranger and the investor itself.
    let (mut h, s) = ready();
    let stranger: Keypair = h.funded_keypair();
    let mut results = Vec::new();
    for (i, who) in [0u8, 1, 2].into_iter().enumerate() {
        let inv = fixtures::investor_with_cash(&mut h, &s, 3 * ONE);
        let d = settle::affirmed_subscription(&mut h, &s, &inv, 3 * ONE, i as u8 + 1);
        let by: &Keypair = match who {
            0 => &s.keeper,
            1 => &stranger,
            _ => &inv,
        };
        settle::settle_sub(&mut h, &s, &d, by).expect("settle");
        results.push((
            units::balance(&h, &s.m(), &inv.pubkey()),
            settle::cash_of(&h, &s, &inv.pubkey()),
            settle::read_si(&h, &d).status,
        ));
    }
    assert!(results.iter().all(|r| *r == (3 * ONE, 0, 4)), "{results:?}");
    assert_eq!(fixtures::cash_balance(&h, &s.cash_treasury), 9 * ONE);
}

#[test]
fn the_instruction_keeps_its_original_price() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, 5 * ONE, 1);
    // Next day, a new price before settlement.
    clock::set_utc_day(&mut h.svm, fixtures::SETTLE_DAY + 1, 60);
    fixtures::publish(&mut h, &s, 100_400_000, 0, 1).unwrap();
    settle::settle_sub(&mut h, &s, &d, &s.keeper).unwrap();
    assert_eq!(units::balance(&h, &s.m(), &a.pubkey()), 5 * ONE);
    assert_eq!(
        settle::cash_of(&h, &s, &a.pubkey()),
        5 * ONE,
        "paid $1.00, not $1.004"
    );
    assert_eq!(settle::read_si(&h, &d).nav_seq, 1);
}

#[test]
fn a_subscription_settles_within_its_compute_budget() {
    // Research R15: ≤ 200 000 CU (mint with accrue, hook-checked delivery, cash payment).
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    let meta = settle::settle_sub(&mut h, &s, &d, &s.keeper).unwrap();
    println!("subscription settle: {} CU", meta.compute_units_consumed);
    assert!(
        meta.compute_units_consumed <= 200_000,
        "{} CU",
        meta.compute_units_consumed
    );
}
