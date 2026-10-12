//! T076 — SC-006: the rules behave the same with 10 000 registered holders as with 10, and the
//! effort to check one transfer doesn't grow with the number of holders.
//!
//! Run with: `cargo test -p tests --release --test scale -- --ignored --nocapture`

use anchor_lang::prelude::Pubkey;
use mmf_common::ids::COMPLIANCE_HOOK_PROGRAM_ID;
use mmf_sdk::ix_hook;
use solana_signer::Signer;
use tests::fixtures;
use tests::{Harness, TxMeta};

/// CI ceiling for the hook's own compute use on one transfer.
const HOOK_CU_CEILING: u64 = 60_000;
const SMALL: usize = 10;
const LARGE: usize = 10_000;
/// Registrations per transaction (fits the 1 232-byte limit).
const PER_TX: usize = 3;

/// Compute units the hook itself consumed in a transfer (from its "consumed" log line).
fn hook_cu(meta: &TxMeta) -> u64 {
    let prefix = format!("Program {COMPLIANCE_HOOK_PROGRAM_ID} consumed ");
    let line = meta
        .logs
        .iter()
        .find(|l| l.starts_with(&prefix))
        .expect("hook ran");
    line[prefix.len()..]
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn register_many(h: &mut Harness, ctx: &fixtures::FundCtx, n: usize) {
    let c = ctx.compliance.pubkey();
    let payer = h.payer.pubkey(); // ~28 SOL of rent for 10 000 records; compliance stays the authority
    let comp = ctx.compliance.insecure_clone();
    let mut left = n;
    while left > 0 {
        let k = left.min(PER_TX);
        let ixs: Vec<_> = (0..k)
            .map(|_| ix_hook::register_holder(&c, &payer, &ctx.m(), &Pubkey::new_unique(), 0))
            .collect();
        h.send(&ixs, &[&comp]).expect("register holders");
        left -= k;
    }
}

#[test]
#[ignore = "slow: registers 10 000 holders; run in release"]
fn hook_cost_does_not_grow_with_holders() {
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1).unwrap(); // both now hold units
    register_many(&mut h, &ctx, SMALL - 2);

    // Steady state: both already hold units, so the holder count doesn't change.
    let small = hook_cu(&fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1).unwrap());

    register_many(&mut h, &ctx, LARGE - SMALL);
    let large = hook_cu(&fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1).unwrap());

    println!("hook compute units: {small} with {SMALL} holders, {large} with {LARGE} holders");
    assert_eq!(
        small, large,
        "the hook's cost must not depend on the number of holders"
    );
    assert!(
        large <= HOOK_CU_CEILING,
        "hook used {large} CU, ceiling {HOOK_CU_CEILING}"
    );
}
