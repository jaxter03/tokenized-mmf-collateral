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

// ---------------------------------------------------------------------------------------------
// F002 T034: SC-003 (publish cost) and SC-004 (settlement cost)
// ---------------------------------------------------------------------------------------------

use mmf_common::ids::FUND_PROGRAM_ID;
use mmf_sdk::pda;
use tests::clock;

/// Tuesday 2026-10-13.
const TUE: u32 = 20_739;
/// Value-dependent variation of the hook's u128 arithmetic (see the SC-004 assertion).
const SETTLEMENT_JITTER_CU: u64 = 16;

/// Compute units of the outermost `fund` invocation (its "consumed" line comes last: the
/// event's self-CPI logs its own line first).
fn fund_cu(meta: &TxMeta) -> u64 {
    let prefix = format!("Program {FUND_PROGRAM_ID} consumed ");
    let line = meta
        .logs
        .iter()
        .rev()
        .find(|l| l.starts_with(&prefix))
        .expect("fund ran");
    line[prefix.len()..]
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn publish_on(h: &mut Harness, ctx: &fixtures::FundCtx, day: u32) -> TxMeta {
    clock::set_utc_day(&mut h.svm, day, 15 * 3_600);
    fixtures::publish(h, ctx, 100_000_000, 123_287_671, 1).expect("publish")
}

#[test]
#[ignore = "slow: registers 10 000 holders; run in release"]
fn publish_and_settlement_cost_do_not_grow() {
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, TUE, 15 * 3_600);
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    let (a, b) = (alice.insecure_clone(), bob.pubkey());
    fixtures::transfer(&mut h, &ctx, &a, &b, 1).unwrap();
    register_many(&mut h, &ctx, SMALL - 2);

    // Snapshot for the publish A/B: the same record (seq 2, same day, same inputs) is published
    // again at the end with 10 000 holders. Each record's address has its own bump, so only the
    // same seq on the same fund compares like with like.
    publish_on(&mut h, &ctx, TUE);
    let fs_key = ctx.fund_state();
    let fs_snapshot = h.account(&fs_key).unwrap();
    let next = pda::nav_record(&fs_key, 2).0;
    let ix = mmf_sdk::ix_fund::publish_daily(
        &ctx.fund_admin.pubkey(),
        &h.payer.pubkey(),
        &ctx.m(),
        2,
        100_000_000,
        123_287_671,
        1,
    );
    assert_eq!(ix.accounts.len(), 8, "publish's account list is fixed");
    clock::set_utc_day(&mut h.svm, TUE + 1, 15 * 3_600);
    let small_pub = fund_cu(
        &h.send(std::slice::from_ref(&ix), &[&ctx.fund_admin])
            .unwrap(),
    );

    // Settlement, 10 holders: last settled 1 publish ago, then 90 publishes ago.
    let hook_small_1 = hook_cu(&fixtures::transfer(&mut h, &ctx, &a, &b, 1).unwrap());
    let mut day = TUE + 2;
    for _ in 0..90 {
        publish_on(&mut h, &ctx, day);
        day += 1;
    }
    let hook_small_90 = hook_cu(&fixtures::transfer(&mut h, &ctx, &a, &b, 1).unwrap());

    register_many(&mut h, &ctx, LARGE - SMALL);

    // Settlement, 10 000 holders.
    publish_on(&mut h, &ctx, day);
    day += 1;
    let hook_large_1 = hook_cu(&fixtures::transfer(&mut h, &ctx, &a, &b, 1).unwrap());
    for _ in 0..90 {
        publish_on(&mut h, &ctx, day);
        day += 1;
    }
    let hook_large_90 = hook_cu(&fixtures::transfer(&mut h, &ctx, &a, &b, 1).unwrap());

    // Publish, 10 000 holders: put the fund back exactly as it was before seq 2 (test scaffolding,
    // not a program state the chain can reach) and publish the same record again.
    h.svm.set_account(fs_key, fs_snapshot).unwrap();
    h.svm.set_account(next, Default::default()).unwrap();
    clock::set_utc_day(&mut h.svm, TUE + 1, 15 * 3_600);
    let large_pub = fund_cu(&h.send(&[ix], &[&ctx.fund_admin]).unwrap());

    println!(
        "publish compute units: {small_pub} with {SMALL} holders, {large_pub} with {LARGE} holders"
    );
    println!(
        "hook compute units: {hook_small_1} / {hook_small_90} (settled 1 / 90 publishes ago) with {SMALL} holders, \
         {hook_large_1} / {hook_large_90} with {LARGE} holders"
    );
    assert_eq!(
        small_pub, large_pub,
        "SC-003: publish cost must not depend on holders"
    );
    // Settlement does constant work (two u128 multiplications), but the SBF cost of u128
    // arithmetic moves by a unit or two with the values involved: with 2 holders and nothing else
    // changing it reads 21 959–21 961. Growth with holders or with publishes since the last
    // settlement would cost thousands (one more address-derivation round is ~1 500 CU), so a
    // 16-CU band proves SC-004 without depending on the stored amounts.
    let all = [hook_small_1, hook_small_90, hook_large_1, hook_large_90];
    let spread = all.iter().max().unwrap() - all.iter().min().unwrap();
    assert!(
        spread <= SETTLEMENT_JITTER_CU,
        "SC-004: settlement cost must not depend on holders or publishes since: {all:?}"
    );
    assert!(
        hook_large_90 <= HOOK_CU_CEILING,
        "hook used {hook_large_90} CU, ceiling {HOOK_CU_CEILING}"
    );
}

// ---------------------------------------------------------------------------------------------
// F003 T073: SC-007 (settlement cost doesn't grow with holders or open instructions)
// ---------------------------------------------------------------------------------------------

/// One subscription and one redemption by the same two investors, settled; returns their CU.
/// The same parties and accounts at both scales, so every address derivation costs the same.
fn settle_pair(
    h: &mut Harness,
    s: &fixtures::SettleCtx,
    buyer: &solana_keypair::Keypair,
    seller: &solana_keypair::Keypair,
    n: u8,
) -> (u64, u64) {
    use tests::settle;
    let d = settle::affirmed_subscription(h, s, buyer, settle::ONE, n);
    let sub = settle::settle_sub(h, s, &d, &s.keeper)
        .unwrap()
        .compute_units_consumed;
    let d = settle::affirmed_redemption(h, s, seller, settle::ONE, n + 1);
    let red = settle::settle_red(h, &d, &s.keeper)
        .unwrap()
        .compute_units_consumed;
    (sub, red)
}

#[test]
#[ignore = "slow: registers 10 000 holders and 1 000 instructions; run in release"]
fn settlement_cost_does_not_grow_with_holders_or_open_instructions() {
    use tests::settle::{self, ONE};
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let buyer = fixtures::investor_with_cash(&mut h, &s, 100 * ONE);
    // Plenty outstanding at the day's start, so the redemption limit never binds.
    let seller = fixtures::investor_with_units(&mut h, &s, 100_000 * ONE, 0);
    settle::fill_treasury(&mut h, &s, 100 * ONE);
    register_many(&mut h, &s, SMALL - 3);
    // Warm up: the buyer's first units and the day's first mint each write once (holder count,
    // redemption-day rollover); measure the steady state at both scales.
    settle_pair(&mut h, &s, &buyer, &seller, 10);
    let small = settle_pair(&mut h, &s, &buyer, &seller, 1);

    register_many(&mut h, &s, LARGE - SMALL);
    // 1 000 open instructions (created, never funded).
    let opener = fixtures::investor_with_cash(&mut h, &s, 0);
    for i in 0..1_000u32 {
        let mut args = settle::sub_args(&s, &opener.pubkey(), ONE, 1);
        args.external_ref = [0; 32];
        args.external_ref[..4].copy_from_slice(&(i + 1).to_le_bytes());
        settle::create(&mut h, &s, &opener, args).expect("open instruction");
    }
    let large = settle_pair(&mut h, &s, &buyer, &seller, 3);

    println!(
        "settle CU (subscription, redemption): {small:?} with {SMALL} holders and 1 open \
         instruction, {large:?} with {LARGE} holders and 1 000 open"
    );
    assert_eq!(
        small, large,
        "settlement cost must not depend on holders or open instructions"
    );
}
