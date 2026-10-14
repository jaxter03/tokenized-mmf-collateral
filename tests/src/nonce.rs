//! Durable-nonce helpers for the slow-signer tests (F003 US5, research R13).
//!
//! By default the harness doesn't check blockhashes. These tests turn the check on, so an
//! ordinary transaction whose blockhash has expired fails with `BlockhashNotFound`, while one
//! built on a durable nonce (with `AdvanceNonceAccount` first) is accepted.

use anchor_lang::prelude::Pubkey;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::{Harness, TxResult};

/// Size of a nonce account (system program `nonce::state::Versions`).
const NONCE_ACCOUNT_LEN: usize = 80;

/// A harness that rejects transactions whose blockhash is no longer recent.
pub fn harness_with_blockhash_check() -> Harness {
    let mut h = Harness::new();
    let svm = std::mem::take(&mut h.svm);
    h.svm = svm.with_blockhash_check(true);
    h
}

/// Creates and initializes a nonce account with `authority`. Returns its address.
pub fn create_nonce_account(h: &mut Harness, authority: &Pubkey) -> Pubkey {
    let nonce = Keypair::new();
    let lamports = h.svm.minimum_balance_for_rent_exemption(NONCE_ACCOUNT_LEN);
    let ixs = solana_system_interface::instruction::create_nonce_account(
        &h.payer.pubkey(),
        &nonce.pubkey(),
        authority,
        lamports,
    );
    h.send(&ixs, &[&nonce]).expect("create nonce account");
    nonce.pubkey()
}

/// Makes every blockhash a prepared step could have used stale (LiteSVM keeps only the latest).
pub fn expire_recent_blockhashes(h: &mut Harness) {
    h.svm.expire_blockhash();
}

/// Signs `message` (prepared earlier) with `signers` now and submits it as is.
pub fn sign_and_submit(h: &mut Harness, message: Message, signers: &[&Keypair]) -> TxResult {
    let blockhash = message.recent_blockhash;
    let tx = Transaction::new(signers, message, blockhash);
    let res = h.svm.send_transaction(tx);
    h.svm.expire_blockhash();
    res
}
