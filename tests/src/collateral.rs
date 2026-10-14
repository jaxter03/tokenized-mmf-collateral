//! F004 assertion helpers (T018): invariants I1, I2, I15 and I16 (shared-model §7) checked
//! against the on-chain accounts, and the derived call status.
//!
//! LiteSVM can't list a program's accounts, so a test records the agreements, pledges and calls
//! it creates in a `Tracker`, which then reads every one of them.

use anchor_lang::prelude::Pubkey;
use collateral::{CollateralCall, Pledge, PledgeAgreement};
use compliance_hook::HolderState;
use mmf_common::enums::{CallKind, CallStatus, PledgeStatus};

use crate::fixtures::read;
use crate::Harness;

/// The derived status of a call (data-model.md): a Delivery call stored Open or Disputed below
/// its agreement's `met_below_seq` is Met. Written out here rather than through
/// `mmf_common::collateral`.
pub fn call_status(h: &Harness, call: &Pubkey) -> CallStatus {
    let c: CollateralCall = read(h, call);
    let a: PledgeAgreement = read(h, &c.agreement);
    let stored = CallStatus::try_from(c.status).expect("stored status");
    let unresolved = matches!(stored, CallStatus::Open | CallStatus::Disputed);
    if c.kind == CallKind::Delivery.as_u8() && unresolved && c.seq < a.met_below_seq {
        CallStatus::Met
    } else {
        stored
    }
}

/// The accounts a test has created, for the invariant checks.
#[derive(Default, Clone, Debug)]
pub struct Tracker {
    pub mint: Pubkey,
    pub agreements: Vec<Pubkey>,
    pub pledges: Vec<Pubkey>,
    pub calls: Vec<Pubkey>,
}

impl Tracker {
    pub fn new(mint: Pubkey) -> Self {
        Self {
            mint,
            ..Self::default()
        }
    }

    pub fn agreement(&mut self, key: Pubkey) -> Pubkey {
        if !self.agreements.contains(&key) {
            self.agreements.push(key);
        }
        key
    }

    pub fn pledge(&mut self, key: Pubkey) -> Pubkey {
        self.pledges.push(key);
        key
    }

    pub fn call(&mut self, key: Pubkey) -> Pubkey {
        self.calls.push(key);
        key
    }

    /// I1, I2, I15 and I16 over every tracked account (pledges and calls that don't exist, e.g.
    /// after a failed step, are skipped).
    #[track_caller]
    pub fn check(&self, h: &Harness) {
        let pledges: Vec<Pledge> = self
            .pledges
            .iter()
            .filter(|k| h.exists(k))
            .map(|k| read(h, k))
            .collect();
        let calls: Vec<CollateralCall> = self
            .calls
            .iter()
            .filter(|k| h.exists(k))
            .map(|k| read(h, k))
            .collect();
        let active = |p: &Pledge| p.status == PledgeStatus::Active.as_u8();
        let remaining = |p: &Pledge| p.units - p.released_units - p.enforced_units;

        let mut givers: Vec<Pubkey> = Vec::new();
        for key in &self.agreements {
            let a: PledgeAgreement = read(h, key);
            if !givers.contains(&a.giver) {
                givers.push(a.giver);
            }
            // I15.
            let total: u64 = pledges
                .iter()
                .filter(|p| p.agreement == *key && active(p))
                .map(remaining)
                .sum();
            assert_eq!(a.pledged_units, total, "I15: agreement {key} pledged_units");
            assert!(
                a.pledged_units <= a.concentration_cap_units,
                "I15: agreement {key} above its pledge limit"
            );
            // I16.
            let open: Vec<&CollateralCall> = calls
                .iter()
                .filter(|c| {
                    c.agreement == *key
                        && c.kind == CallKind::Delivery.as_u8()
                        && (c.status == CallStatus::Open.as_u8()
                            || c.status == CallStatus::Disputed.as_u8())
                        && c.seq >= a.met_below_seq
                })
                .collect();
            assert_eq!(
                usize::from(a.open_delivery_calls),
                open.len(),
                "I16: agreement {key} open_delivery_calls"
            );
            let values: u64 = open.iter().map(|c| c.value_delta).sum();
            assert!(
                a.called_outstanding <= values,
                "I16: agreement {key} demanded part {} > Σ open values {values}",
                a.called_outstanding
            );
            if open.is_empty() {
                assert_eq!(a.called_outstanding, 0, "I16: agreement {key} with no open calls");
            }
        }

        // I1 and I2 for every giver of a tracked agreement. Pledges of untracked agreements would
        // be missed, so every test tracks all the agreements it creates.
        for giver in givers {
            let state: HolderState = read(h, &mmf_sdk::pda::holder(&self.mint, &giver).0);
            let mine: Vec<&Pledge> = pledges
                .iter()
                .filter(|p| {
                    active(p) && {
                        let a: PledgeAgreement = read(h, &p.agreement);
                        a.giver == giver
                    }
                })
                .collect();
            let locked: u64 = mine.iter().map(|p| remaining(p)).sum();
            assert_eq!(state.pledged_amount, locked, "I1: {giver} pledged_amount");
            assert_eq!(
                usize::from(state.active_liens),
                mine.len(),
                "I1: {giver} active_liens"
            );
            let balance = crate::units::balance(h, &self.mint, &giver);
            assert!(
                balance >= state.pledged_amount,
                "I2: {giver} balance {balance} < locked {}",
                state.pledged_amount
            );
        }
    }
}

// ---- step helpers shared by the f004 test files -------------------------------------------

use mmf_sdk::ix_collateral::Agr;
use solana_keypair::Keypair;
use solana_signer::Signer;

use crate::fixtures::{CollatBase, FundCtx};
use crate::TxResult;

/// The agreement between the fixture's giver and taker with `nonce`.
pub fn agr(c: &CollatBase, nonce: u64) -> Agr {
    Agr {
        mint: c.m(),
        giver: c.giver.pubkey(),
        taker: c.taker.pubkey(),
        nonce,
    }
}

/// `propose_agreement` signed by `proposer`, paid by the harness payer.
pub fn propose(
    h: &mut Harness,
    mint: &Pubkey,
    proposer: &Keypair,
    args: collateral::ProposeArgs,
) -> TxResult {
    let ix = mmf_sdk::ix_collateral::propose_agreement(&proposer.pubkey(), &h.payer.pubkey(), mint, args);
    h.send(&[ix], &[proposer])
}

/// `sign_agreement` signed by `signer`.
pub fn sign(h: &mut Harness, signer: &Keypair, a: &Agr) -> TxResult {
    let ix = mmf_sdk::ix_collateral::sign_agreement(&signer.pubkey(), a);
    h.send(&[ix], &[signer])
}

/// Compliance suspends `owner` (reason 3, operational error).
pub fn suspend(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey) {
    let ix = mmf_sdk::ix_governance::suspend_holder(&ctx.compliance.pubkey(), &ctx.m(), owner, 3, [1; 32]);
    h.send(&[ix], &[&ctx.compliance]).expect("suspend");
}

/// Compliance reinstates `owner`.
pub fn reinstate(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey) {
    let ix = mmf_sdk::ix_governance::reinstate_holder(&ctx.compliance.pubkey(), &ctx.m(), owner, 3, [1; 32]);
    h.send(&[ix], &[&ctx.compliance]).expect("reinstate");
}

/// Compliance revokes `owner`.
pub fn revoke(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey) {
    let ix = mmf_sdk::ix_governance::revoke_holder(&ctx.compliance.pubkey(), &ctx.m(), owner, 3, [1; 32]);
    h.send(&[ix], &[&ctx.compliance]).expect("revoke");
}
