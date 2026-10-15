//! F004 slow signers (FR-036, SC-008; research R5, R6): a step prepared on the signer's durable
//! nonce and signed hours later is judged on the state when it arrives. Pledges and releases
//! don't depend on the list of open calls, so calls opened in the meantime don't invalidate them.
//! T068.

use anchor_lang::prelude::Pubkey;
use collateral::CollateralError;
use mmf_common::enums::CallStatus;
use mmf_sdk::{ix_collateral, prepare};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;
use tests::assert::assert_anchor_error;
use tests::collateral::{agreement_of, call_status, exposure, issue, lock_of, r, Tracker};
use tests::fixtures::{self, CollatCtx, Ready};
use tests::nonce::{self, create_nonce_account, expire_recent_blockhashes, sign_and_submit};
use tests::{clock, Harness};

const HOUR: i64 = 3_600;
const MILLION: u64 = 1_000_000_000_000;
const UNITS_FOR_MILLION: u64 = 1_020_408_163_266;

fn ready(ready: Ready) -> (Harness, CollatCtx, Tracker) {
    let mut h = nonce::harness_with_blockhash_check();
    let c = fixtures::collateral_ready_fund_opts(&mut h, ready);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    (h, c, t)
}

/// `ix` prepared on `signer`'s nonce account; `signer` is nonce authority and fee payer, as a
/// custodian's own key would be.
fn prepare(
    h: &Harness,
    ix: anchor_lang::solana_program::instruction::Instruction,
    signer: &Keypair,
    nonce: &Pubkey,
) -> Message {
    let value = prepare::read_nonce(&h.account(nonce).unwrap().data).expect("nonce value");
    prepare::with_durable_nonce(&[ix], nonce, &signer.pubkey(), &signer.pubkey(), value)
}

#[test]
fn a_pledge_signed_six_hours_later_succeeds() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    let call = t.call(c.agr.call(&r(1)));
    let nonce = create_nonce_account(&mut h, &c.giver.pubkey());
    let ix = ix_collateral::pledge(
        &c.giver.pubkey(),
        &c.giver.pubkey(),
        &c.agr,
        UNITS_FOR_MILLION,
        r(9),
    );
    let message = prepare(&h, ix, &c.giver, &nonce);

    clock::advance(&mut h.svm, 6 * HOUR);
    expire_recent_blockhashes(&mut h);
    sign_and_submit(&mut h, message.clone(), &[&c.giver]).expect("signed 6 h later");
    t.pledge(c.agr.pledge(&r(9)));
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()).0, UNITS_FOR_MILLION);
    assert_eq!(call_status(&h, &call), CallStatus::Met);
    t.check(&h);

    // The same signed message again: its nonce has moved on, so nothing happens twice.
    let res = sign_and_submit(&mut h, message, &[&c.giver]);
    assert_eq!(res.unwrap_err().err, TransactionError::BlockhashNotFound);
    assert_eq!(
        lock_of(&h, &c.m(), &c.giver.pubkey()),
        (UNITS_FOR_MILLION, 1)
    );
}

#[test]
fn an_ordinary_transaction_of_the_same_pledge_expires() {
    let (mut h, c, _) = ready(Ready::Standard);
    let old = h.svm.latest_blockhash();
    let ix = ix_collateral::pledge(&c.giver.pubkey(), &c.giver.pubkey(), &c.agr, 1, r(9));
    let msg = Message::new(&[ix], Some(&c.giver.pubkey()));
    clock::advance(&mut h.svm, 6 * HOUR);
    expire_recent_blockhashes(&mut h);
    let res = h
        .svm
        .send_transaction(Transaction::new(&[&c.giver], msg, old));
    assert_eq!(res.unwrap_err().err, TransactionError::BlockhashNotFound);
}

#[test]
fn calls_opened_meanwhile_dont_invalidate_prepared_steps() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M");
    t.call(c.agr.call(&r(1)));
    // G prepares a pledge; before it is signed, T opens a second call.
    let g_nonce = create_nonce_account(&mut h, &c.giver.pubkey());
    let ix = ix_collateral::pledge(
        &c.giver.pubkey(),
        &c.giver.pubkey(),
        &c.agr,
        UNITS_FOR_MILLION,
        r(9),
    );
    let pledge_msg = prepare(&h, ix, &c.giver, &g_nonce);
    exposure(&mut h, &c, &c.taker, 2 * MILLION, r(2)).expect("second call");
    t.call(c.agr.call(&r(2)));
    clock::advance(&mut h.svm, 3 * HOUR);
    expire_recent_blockhashes(&mut h);
    sign_and_submit(&mut h, pledge_msg, &[&c.giver]).expect("the prepared pledge still lands");
    t.pledge(c.agr.pledge(&r(9)));
    t.check(&h);

    // T prepares a release; before it is signed, G opens a return call.
    exposure(&mut h, &c, &c.taker, 0, r(3)).expect("no exposure");
    let t_nonce = create_nonce_account(&mut h, &c.taker.pubkey());
    let ix = ix_collateral::release(
        &c.taker.pubkey(),
        &c.agr,
        &c.agr.pledge(&r(9)),
        None,
        1_000_000,
    );
    let release_msg = prepare(&h, ix, &c.taker, &t_nonce);
    issue(&mut h, &c, &c.giver, 1, r(4)).expect("return call");
    t.call(c.agr.call(&r(4)));
    clock::advance(&mut h.svm, 2 * HOUR);
    expire_recent_blockhashes(&mut h);
    sign_and_submit(&mut h, release_msg, &[&c.taker]).expect("the prepared release still lands");
    assert_eq!(
        lock_of(&h, &c.m(), &c.giver.pubkey()).0,
        UNITS_FOR_MILLION - 1_000_000
    );
    t.check(&h);
}

#[test]
fn a_confirmation_for_a_replaced_raise_is_refused() {
    let (mut h, c, _) = ready(Ready::TwoStep);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("pending raise");
    let nonce = create_nonce_account(&mut h, &c.giver.pubkey());
    let ix = ix_collateral::confirm_exposure(
        &c.giver.pubkey(),
        &c.giver.pubkey(),
        &c.agr,
        MILLION,
        r(1),
    );
    let message = prepare(&h, ix, &c.giver, &nonce);
    // While the giver's custodian approves, the taker replaces the raise.
    exposure(&mut h, &c, &c.taker, 2 * MILLION, r(2)).expect("newer raise");
    clock::advance(&mut h.svm, 4 * HOUR);
    expire_recent_blockhashes(&mut h);
    assert_anchor_error(
        &sign_and_submit(&mut h, message, &[&c.giver]),
        CollateralError::PendingRaiseMismatch,
    );
    assert_eq!(agreement_of(&h, &c).pending_raise_value, 2 * MILLION);
}
