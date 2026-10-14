//! T069, US5 Slow signers (FR-028, FR-029, SC-004; research R13): a step prepared on the
//! signer's durable nonce and signed hours later still succeeds while the instruction is open
//! and not expired, and a signed step takes effect at most once.

use dvp_settlement::DvpError;
use mmf_sdk::ix_dvp::{self, Deal};
use mmf_sdk::prepare;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;
use tests::assert::assert_anchor_error;
use tests::fixtures::{self, SettleCtx};
use tests::nonce::{self, create_nonce_account, expire_recent_blockhashes, sign_and_submit};
use tests::settle::{self, sub_args, ONE};
use tests::{clock, Harness};

const HOUR: i64 = 3_600;

fn ready() -> (Harness, SettleCtx) {
    let mut h = nonce::harness_with_blockhash_check();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

/// The investor's cash funding of `d`, prepared on the investor's nonce account; the investor
/// is both nonce authority and fee payer, as a custodian's own key would be.
fn prepare_funding(
    h: &Harness,
    s: &SettleCtx,
    d: &Deal,
    investor: &Keypair,
    nonce: &anchor_lang::prelude::Pubkey,
) -> Message {
    let source = fixtures::cash_ata(h, &investor.pubkey(), &s.cash_mint);
    let ix = ix_dvp::fund_cash(&investor.pubkey(), d, &source);
    let value = prepare::read_nonce(&h.account(nonce).unwrap().data).expect("nonce value");
    prepare::with_durable_nonce(&[ix], nonce, &investor.pubkey(), &investor.pubkey(), value)
}

#[test]
fn the_settlement_fixture_works_with_the_blockhash_check_on() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("settles");
}

#[test]
fn a_funding_signed_6_and_23_hours_after_preparation_succeeds() {
    for hours in [6, 23] {
        let (mut h, s) = ready();
        let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
        let nonce = create_nonce_account(&mut h, &a.pubkey());
        let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
        let message = prepare_funding(&h, &s, &d, &a, &nonce);

        clock::advance(&mut h.svm, hours * HOUR);
        expire_recent_blockhashes(&mut h);
        sign_and_submit(&mut h, message, &[&a])
            .unwrap_or_else(|e| panic!("signed after {hours} h: {:?}", e.err));
        assert_eq!(settle::read_si(&h, &d).status, 2, "BuyerFunded");
    }
}

#[test]
fn an_ordinary_transaction_of_the_same_step_expires() {
    // The control: without the nonce, a step signed against an old blockhash is refused by the
    // network before any program runs.
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let source = fixtures::cash_ata(&h, &a.pubkey(), &s.cash_mint);
    let old = h.svm.latest_blockhash();
    let msg = Message::new(
        &[ix_dvp::fund_cash(&a.pubkey(), &d, &source)],
        Some(&a.pubkey()),
    );
    clock::advance(&mut h.svm, 6 * HOUR);
    expire_recent_blockhashes(&mut h);
    let tx = Transaction::new(&[&a], msg, old);
    let res = h.svm.send_transaction(tx);
    assert_eq!(res.unwrap_err().err, TransactionError::BlockhashNotFound);
}

#[test]
fn a_prepared_step_is_judged_on_the_state_when_it_arrives() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let nonce = create_nonce_account(&mut h, &a.pubkey());
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let message = prepare_funding(&h, &s, &d, &a, &nonce);
    // While the custodian's approvals run, the transfer agent cancels the instruction.
    settle::cancel(&mut h, &d, &s.transfer_agent, settle::SELLER).unwrap();
    clock::advance(&mut h.svm, 2 * HOUR);
    expire_recent_blockhashes(&mut h);
    let res = sign_and_submit(&mut h, message, &[&a]);
    assert_anchor_error(&res, DvpError::InstructionFinal);
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), ONE);
}

#[test]
fn a_prepared_step_after_expiry_is_refused() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let nonce = create_nonce_account(&mut h, &a.pubkey());
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let message = prepare_funding(&h, &s, &d, &a, &nonce);
    clock::advance(&mut h.svm, 24 * HOUR);
    expire_recent_blockhashes(&mut h);
    assert_anchor_error(
        &sign_and_submit(&mut h, message, &[&a]),
        DvpError::InstructionExpired,
    );
}

#[test]
fn a_prepared_creation_survives_other_creations() {
    // Research R2: the address depends on the creator and its reference, not on the sequence
    // number, so another party's creation in the meantime doesn't invalidate it.
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let b = fixtures::investor_with_cash(&mut h, &s, ONE);
    let nonce = create_nonce_account(&mut h, &a.pubkey());
    let args = sub_args(&s, &a.pubkey(), ONE, 7);
    let ix = ix_dvp::create_instruction(&a.pubkey(), &a.pubkey(), &s.m(), args);
    let value = prepare::read_nonce(&h.account(&nonce).unwrap().data).unwrap();
    let message = prepare::with_durable_nonce(&[ix], &nonce, &a.pubkey(), &a.pubkey(), value);

    settle::created(&mut h, &s, &b, sub_args(&s, &b.pubkey(), ONE, 7));
    clock::advance(&mut h.svm, 5 * HOUR);
    expire_recent_blockhashes(&mut h);
    sign_and_submit(&mut h, message, &[&a]).expect("still valid");
    let si = settle::read_si(&h, &settle::deal(&s, &a.pubkey(), &args));
    assert_eq!(si.seq, 2, "numbered when it arrived");
}

#[test]
fn a_signed_step_takes_effect_at_most_once() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);
    let nonce = create_nonce_account(&mut h, &a.pubkey());
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let message = prepare_funding(&h, &s, &d, &a, &nonce);
    sign_and_submit(&mut h, message.clone(), &[&a]).expect("first");
    // The same signed message again: its nonce has moved on.
    let res = sign_and_submit(&mut h, message, &[&a]);
    assert_eq!(res.unwrap_err().err, TransactionError::BlockhashNotFound);
    // Prepared again on the new nonce value: the program refuses a second funding.
    let again = prepare_funding(&h, &s, &d, &a, &nonce);
    assert_anchor_error(
        &sign_and_submit(&mut h, again, &[&a]),
        DvpError::LegAlreadyFunded,
    );
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 9 * ONE, "paid once");
}
