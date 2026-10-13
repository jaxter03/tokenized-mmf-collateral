//! T060 (FR-026, FR-027; research R3): every earmarked leg goes back to its funder through
//! `return_leg`, which anyone can trigger and retry. A return that can't be delivered yet fails
//! and leaves the earmark in place; the instruction is already final.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use compliance_hook::HookError;
use dvp_settlement::events::LegReturned;
use dvp_settlement::DvpError;
use mmf_common::enums::ReasonCode;
use mmf_sdk::{ix_dvp, ix_governance};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::events::one_cpi_event;
use tests::fixtures::{self, SettleCtx};
use tests::settle::{self, red_args, sec_args, sub_args, BUYER, ONE, SELLER};
use tests::{clock, units, Harness, Ix};

const CASE: [u8; 32] = [0xC5; 32];
const SANCTIONS: u8 = ReasonCode::SanctionsHit as u8;

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

fn with_account(mut ix: Ix, key_now: Pubkey, key_new: Pubkey) -> Ix {
    let at = ix
        .accounts
        .iter()
        .position(|m| m.pubkey == key_now)
        .unwrap();
    let m = ix.accounts[at].clone();
    ix.accounts[at] = AccountMeta {
        pubkey: key_new,
        ..m
    };
    ix
}

#[test]
fn both_legs_of_every_kind_go_back_after_cancel_expire_or_reject() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 100 * ONE);
    let b = fixtures::investor_with_units(&mut h, &s, 100 * ONE, 100 * ONE);
    settle::fill_treasury(&mut h, &s, 100 * ONE);
    let start = (
        units::balance(&h, &s.m(), &a.pubkey()),
        settle::cash_of(&h, &s, &a.pubkey()),
        units::balance(&h, &s.m(), &b.pubkey()),
        settle::cash_of(&h, &s, &b.pubkey()),
        fixtures::cash_balance(&h, &s.cash_treasury),
    );

    // Affirmed subscription → mutual cancel; Affirmed redemption → reject; Affirmed secondary →
    // expire.
    let sub = settle::affirmed_subscription(&mut h, &s, &a, 3 * ONE, 1);
    settle::cancel(&mut h, &sub, &a, BUYER).unwrap();
    settle::cancel(&mut h, &sub, &s.transfer_agent, SELLER).unwrap();
    let red = settle::affirmed_redemption(&mut h, &s, &b, 4 * ONE, 2);
    settle::reject(&mut h, &s, &red, SANCTIONS).unwrap();
    let sec = settle::affirmed_secondary(&mut h, &s, &a, &b, 5 * ONE, 6 * ONE, 3);
    clock::advance(&mut h.svm, 86_400);
    settle::expire(&mut h, &sec, &s.keeper).unwrap();

    let si = settle::read_si(&h, &red);
    assert_eq!((si.units_to_return, si.cash_to_return), (4 * ONE, 4 * ONE));
    let meta = settle::return_cash(&mut h, &s, &sub, &s.keeper).unwrap();
    let ev = one_cpi_event::<LegReturned>(&meta);
    assert_eq!(
        (ev.seq, ev.leg, ev.amount, ev.recipient, ev.triggered_by),
        (
            settle::read_si(&h, &sub).seq,
            1,
            3 * ONE,
            fixtures::cash_ata(&h, &a.pubkey(), &s.cash_mint),
            s.keeper.pubkey()
        )
    );
    let meta = settle::return_units(&mut h, &red, &s.keeper).unwrap();
    assert_eq!(one_cpi_event::<LegReturned>(&meta).amount, 4 * ONE);
    // A redemption's cash goes back to the treasury it came from.
    let meta = settle::return_cash(&mut h, &s, &red, &s.keeper).unwrap();
    assert_eq!(
        one_cpi_event::<LegReturned>(&meta).recipient,
        s.cash_treasury
    );
    settle::return_units(&mut h, &sec, &s.keeper).unwrap();
    settle::return_cash(&mut h, &s, &sec, &s.keeper).unwrap();

    let end = (
        units::balance(&h, &s.m(), &a.pubkey()),
        settle::cash_of(&h, &s, &a.pubkey()),
        units::balance(&h, &s.m(), &b.pubkey()),
        settle::cash_of(&h, &s, &b.pubkey()),
        fixtures::cash_balance(&h, &s.cash_treasury),
    );
    assert_eq!(end, start, "every funder restored exactly");
    assert_eq!(units::token_balance(&h, &s.unit_escrow), 0);
    assert_eq!(fixtures::cash_balance(&h, &s.cash_escrow), 0);
    for d in [sub, red, sec] {
        let si = settle::read_si(&h, &d);
        assert_eq!((si.units_to_return, si.cash_to_return), (0, 0));
    }
    assert_eq!(
        units::supply(&h, &s.m()),
        200 * ONE,
        "nothing created or destroyed"
    );
}

#[test]
fn a_return_needs_a_final_unsettled_instruction_and_something_earmarked() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    // Open: not returnable.
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    settle::fund_cash(&mut h, &s, &d, &a).unwrap();
    assert_anchor_error(
        &settle::return_cash(&mut h, &s, &d, &s.keeper),
        DvpError::NotReturnable,
    );
    // Settled: not returnable.
    let settled = settle::affirmed_subscription(&mut h, &s, &a, ONE, 2);
    settle::settle_sub(&mut h, &s, &settled, &s.keeper).unwrap();
    assert_anchor_error(
        &settle::return_cash(&mut h, &s, &settled, &s.keeper),
        DvpError::NotReturnable,
    );
    // Cancelled: the cash comes back once; the unfunded unit leg has nothing.
    settle::cancel(&mut h, &d, &a, BUYER).unwrap();
    assert_anchor_error(
        &settle::return_units(&mut h, &d, &s.keeper),
        DvpError::NothingToReturn,
    );
    settle::return_cash(&mut h, &s, &d, &s.keeper).unwrap();
    assert_anchor_error(
        &settle::return_cash(&mut h, &s, &d, &s.keeper),
        DvpError::NothingToReturn,
    );
    // An unknown leg.
    let mut ix = ix_dvp::return_cash(&s.keeper.pubkey(), &d, &s.cash_treasury);
    *ix.data.last_mut().unwrap() = 2;
    assert_anchor_error(&h.send(&[ix], &[&s.keeper]), DvpError::InvalidLeg);
}

#[test]
fn a_return_goes_only_to_the_funder() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 10 * ONE);
    let thief = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_secondary(&mut h, &s, &a, &thief, ONE, ONE, 1);
    settle::reject(&mut h, &s, &d, SANCTIONS).unwrap();
    // The seller's units to the buyer, and the buyer's cash to the seller: both refused.
    let seller_units = mmf_sdk::pda::holding_account(&a.pubkey(), &s.m());
    let thief_units = mmf_sdk::pda::holding_account(&thief.pubkey(), &s.m());
    let ix = with_account(
        ix_dvp::return_units(&s.keeper.pubkey(), &d),
        seller_units,
        thief_units,
    );
    assert_anchor_error(&h.send(&[ix], &[&s.keeper]), DvpError::WrongAccount);
    let buyer_cash = fixtures::cash_ata(&h, &thief.pubkey(), &s.cash_mint);
    let seller_cash = fixtures::cash_ata(&h, &a.pubkey(), &s.cash_mint);
    let ix = with_account(
        ix_dvp::return_cash(&s.keeper.pubkey(), &d, &s.cash_treasury),
        buyer_cash,
        seller_cash,
    );
    assert_anchor_error(&h.send(&[ix], &[&s.keeper]), DvpError::WrongAccount);
}

#[test]
fn an_undeliverable_return_keeps_its_earmark_until_it_can_be_delivered() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    let c = s.compliance.pubkey();
    let d = settle::created(&mut h, &s, &a, red_args(&s, &a.pubkey(), 3 * ONE, 1));
    settle::fund_units(&mut h, &d, &a).unwrap();

    // The seller is suspended; compliance rejects; the units can't go back yet.
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
    settle::reject(&mut h, &s, &d, SANCTIONS).expect("rejection never depends on the return");
    assert_anchor_error(
        &settle::return_units(&mut h, &d, &s.keeper),
        HookError::ReceiverNotApproved,
    );
    assert_eq!(
        settle::read_si(&h, &d).units_to_return,
        3 * ONE,
        "earmark kept"
    );
    assert_eq!(units::token_balance(&h, &s.unit_escrow), 3 * ONE);

    // Reinstated, but transfers are paused: still not yet.
    h.send(
        &[
            ix_governance::reinstate_holder(&c, &s.m(), &a.pubkey(), SANCTIONS, CASE),
            ix_governance::pause_transfers(&c, &s.m(), 3, CASE),
        ],
        &[&s.compliance],
    )
    .unwrap();
    assert_anchor_error(
        &settle::return_units(&mut h, &d, &s.keeper),
        HookError::TransfersPaused,
    );
    h.send(
        &[ix_governance::unpause_transfers(&c, &s.m(), 3, CASE)],
        &[&s.compliance],
    )
    .unwrap();
    settle::return_units(&mut h, &d, &s.keeper).expect("delivered at last");
    assert_eq!(units::balance(&h, &s.m(), &a.pubkey()), 10 * ONE);
    assert_eq!(settle::read_si(&h, &d).units_to_return, 0);
}

#[test]
fn a_buyer_without_its_cash_account_is_refunded_once_it_has_one_again() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    settle::fund_cash(&mut h, &s, &d, &a).unwrap();
    // The buyer closes its now-empty cash account.
    let ata = fixtures::cash_ata(&h, &a.pubkey(), &s.cash_mint);
    let close = anchor_spl::token_2022::spl_token_2022::instruction::close_account(
        &s.cash_program,
        &ata,
        &a.pubkey(),
        &a.pubkey(),
        &[],
    )
    .unwrap();
    h.send(&[close], &[&a]).unwrap();
    settle::cancel(&mut h, &d, &a, BUYER).unwrap();
    assert!(settle::return_cash(&mut h, &s, &d, &s.keeper).is_err());
    assert_eq!(settle::read_si(&h, &d).cash_to_return, ONE);
    fixtures::create_cash_ata(&mut h, &a.pubkey(), &s.cash_mint);
    settle::return_cash(&mut h, &s, &d, &s.keeper).expect("refunded");
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), ONE);
}

#[test]
fn the_keeper_expires_and_returns_in_one_transaction_or_falls_back() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    let b = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let k = s.keeper.pubkey();

    // All together, atomically.
    let d = settle::affirmed_secondary(&mut h, &s, &a, &b, ONE, ONE, 1);
    clock::advance(&mut h.svm, 86_400);
    let ixs = [
        ix_dvp::expire(&k, &d),
        ix_dvp::return_units(&k, &d),
        ix_dvp::return_cash(&k, &d, &s.cash_treasury),
    ];
    h.send(&ixs, &[&s.keeper]).expect("expire and both returns");
    assert_eq!(units::balance(&h, &s.m(), &a.pubkey()), 10 * ONE);
    assert_eq!(settle::cash_of(&h, &s, &b.pubkey()), 10 * ONE);

    // The seller is suspended: the bundle fails as a whole, so the keeper expires and returns the
    // cash alone, and the units stay earmarked.
    let mut args = sec_args(&a.pubkey(), &b.pubkey(), ONE, ONE, 2);
    args.expires_at_ts = clock::now(&h.svm) + 3_600;
    let d = settle::created(&mut h, &s, &b, args);
    settle::fund_units(&mut h, &d, &a).unwrap();
    settle::fund_cash(&mut h, &s, &d, &b).unwrap();
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
    clock::advance(&mut h.svm, 3_600);
    let ixs = [
        ix_dvp::expire(&k, &d),
        ix_dvp::return_units(&k, &d),
        ix_dvp::return_cash(&k, &d, &s.cash_treasury),
    ];
    assert!(h.send(&ixs, &[&s.keeper]).is_err());
    assert_eq!(settle::read_si(&h, &d).status, 3, "nothing happened");
    h.send(
        &[
            ix_dvp::expire(&k, &d),
            ix_dvp::return_cash(&k, &d, &s.cash_treasury),
        ],
        &[&s.keeper],
    )
    .expect("fallback");
    let si = settle::read_si(&h, &d);
    assert_eq!(
        (si.status, si.units_to_return, si.cash_to_return),
        (6, ONE, 0)
    );
    assert_eq!(settle::cash_of(&h, &s, &b.pubkey()), 10 * ONE);
}
