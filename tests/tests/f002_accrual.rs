//! F002 US2: fair interest for every holder (T026–T028; contracts/compliance_hook.md step 8).
//!
//! Expected amounts are computed in each test with plain loops (`balance × rate × 10^6` per
//! covered day), never with `mmf_common::accrual` (research R11).

use std::collections::HashMap;

use anchor_lang::prelude::Pubkey;
use compliance_hook::events::TransferValidated;
use compliance_hook::{HolderState, HookError};
use fund::FundState;
use mmf_common::enums::ReasonCode;
use mmf_sdk::{ix_governance, ix_hook, pda};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::events::log_events;
use tests::fixtures::{self, publish, read, read_nav_record, FundCtx};
use tests::rebuild::{rebuild, Record};
use tests::{clock, units, Harness, TxMeta, TxResult};

/// Tuesday 2026-10-13.
const TUE: u32 = 20_739;
const ONE: u64 = 100_000_000;
const REASON: u8 = ReasonCode::OperationalError as u8;

/// 20 distinct daily rates (× 10^12), one of them zero.
const RATES: [i64; 20] = [
    123_287_671,
    120_000_000,
    125_500_000,
    119_178_082,
    130_000_001,
    0,
    121_212_121,
    118_000_000,
    127_000_000,
    124_657_534,
    122_000_003,
    126_027_397,
    117_808_219,
    128_767_123,
    120_547_945,
    131_506_849,
    116_438_356,
    129_315_068,
    119_999_999,
    123_000_000,
];

/// What `units` held for one covered day at `rate` earns, in base units × 10^18.
fn day(units: u64, rate: i64) -> u128 {
    u128::from(units) * rate as u128 * 1_000_000
}

fn fund_on(d: u32) -> (Harness, FundCtx) {
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, d, 15 * 3_600);
    let ctx = fixtures::launched_fund(&mut h, 0);
    (h, ctx)
}

fn at(h: &mut Harness, d: u32) {
    clock::set_utc_day(&mut h.svm, d, 15 * 3_600);
}

fn ok(res: TxResult) -> TxMeta {
    match res {
        Ok(m) => m,
        Err(f) => panic!(
            "expected success, got {:?}; logs:\n{}",
            f.err,
            f.meta.logs.join("\n")
        ),
    }
}

fn holder(h: &Harness, ctx: &FundCtx, owner: &Pubkey) -> HolderState {
    read(h, &pda::holder(&ctx.m(), owner).0)
}

fn accrued(h: &Harness, ctx: &FundCtx, owner: &Pubkey) -> u128 {
    holder(h, ctx, owner).accrued_e18
}

fn fund_state(h: &Harness, ctx: &FundCtx) -> FundState {
    read(h, &ctx.fund_state())
}

/// Interest earned so far, read without a transaction (FR-017).
fn earned_now(h: &Harness, ctx: &FundCtx, owner: &Pubkey) -> u128 {
    mmf_sdk::accrual::earned_e18(&holder(h, ctx, owner), &fund_state(h, ctx)).unwrap()
}

/// A 0-unit transfer: runs the hook, which settles both sides, and moves nothing.
fn settle(h: &mut Harness, ctx: &FundCtx, from: &Keypair, to: &Pubkey) -> TxMeta {
    ok(fixtures::transfer(h, ctx, from, to, 0))
}

/// Publishes one day at `rate` on `d`.
fn publish_on(h: &mut Harness, ctx: &FundCtx, d: u32, rate: i64) {
    at(h, d);
    ok(publish(h, ctx, ONE, rate, 1));
}

// ---------------------------------------------------------------------------------------------
// T026: the spec's independent test (US2-1, US2-2, US2-4)
// ---------------------------------------------------------------------------------------------

#[test]
fn a_holder_who_sends_half_midway_earns_exactly_day_by_day() {
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    let full: u64 = 1_000_000_000_000; // 1 000 000 units, 6 decimals
    let half = full / 2;
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), full);

    for (i, rate) in RATES[..10].iter().enumerate() {
        publish_on(&mut h, &ctx, TUE + i as u32, *rate);
    }
    ok(fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), half));
    for (i, rate) in RATES[10..].iter().enumerate() {
        publish_on(&mut h, &ctx, TUE + 10 + i as u32, *rate);
    }
    settle(&mut h, &ctx, &a, &b.pubkey());

    let mut want_a: u128 = 0;
    let mut want_b: u128 = 0;
    for (i, rate) in RATES.iter().enumerate() {
        if i < 10 {
            want_a += day(full, *rate);
        } else {
            want_a += day(half, *rate);
            want_b += day(half, *rate);
        }
    }
    assert_eq!(accrued(&h, &ctx, &a.pubkey()), want_a, "A, exactly");
    assert_eq!(accrued(&h, &ctx, &b.pubkey()), want_b, "B, exactly");
    // To the smallest unit, as the spec states it.
    assert_eq!(
        accrued(&h, &ctx, &a.pubkey()) / 10u128.pow(18),
        want_a / 10u128.pow(18)
    );
}

// ---------------------------------------------------------------------------------------------
// T027: ordering and the accrual rules
// ---------------------------------------------------------------------------------------------

/// Transfer 400 000 of A's 1 000 000 either before or after the second publish.
fn ordering(transfer_first: bool, new_slot: bool) {
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 1_000_000);
    let (r0, r1) = (RATES[0], RATES[1]);
    publish_on(&mut h, &ctx, TUE, r0);
    at(&mut h, TUE + 1);
    let step = |h: &mut Harness| {
        if new_slot {
            let s = h.svm.get_sysvar::<anchor_lang::prelude::Clock>().slot;
            h.svm.warp_to_slot(s + 3);
            at(h, TUE + 1);
        }
    };
    if transfer_first {
        ok(fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), 400_000));
        step(&mut h);
        ok(publish(&mut h, &ctx, ONE, r1, 1));
    } else {
        ok(publish(&mut h, &ctx, ONE, r1, 1));
        step(&mut h);
        ok(fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), 400_000));
    }
    settle(&mut h, &ctx, &b, &a.pubkey());
    let (want_a, want_b) = if transfer_first {
        (day(1_000_000, r0) + day(600_000, r1), day(400_000, r1))
    } else {
        (day(1_000_000, r0) + day(1_000_000, r1), 0)
    };
    assert_eq!(accrued(&h, &ctx, &a.pubkey()), want_a);
    assert_eq!(accrued(&h, &ctx, &b.pubkey()), want_b);
}

#[test]
fn whoever_holds_at_the_publish_earns_it() {
    // US2-6, FR-013: same slot and different slots, both orders.
    ordering(true, false);
    ordering(false, false);
    ordering(true, true);
    ordering(false, true);
}

#[test]
fn an_unchanged_balance_earns_balance_times_the_sum_of_rates() {
    // US2-1 with periods of 1, 3 and 2 days.
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    let bal = 7_000_000;
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), bal);
    let periods = [
        (TUE, 1u16, RATES[0]),
        (TUE + 1, 3, RATES[1]),
        (TUE + 4, 2, RATES[2]),
    ];
    let mut want = 0;
    for (d, days, rate) in periods {
        at(&mut h, d);
        ok(publish(&mut h, &ctx, ONE, rate, days));
        for _ in 0..days {
            want += day(bal, rate);
        }
    }
    assert_eq!(
        earned_now(&h, &ctx, &a.pubkey()),
        want,
        "read without a transaction"
    );
    settle(&mut h, &ctx, &a, &b.pubkey());
    assert_eq!(accrued(&h, &ctx, &a.pubkey()), want);
}

#[test]
fn a_late_holder_earns_nothing_for_earlier_publishes() {
    // FR-016, US2-5.
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 1_000_000);
    for i in 0..5 {
        publish_on(&mut h, &ctx, TUE + i, RATES[i as usize]);
    }
    let c = fixtures::new_investor(&mut h, &ctx);
    ok(fixtures::transfer(&mut h, &ctx, &a, &c.pubkey(), 250_000));
    let hc = holder(&h, &ctx, &c.pubkey());
    assert_eq!(hc.accrued_e18, 0);
    assert_eq!(hc.index_at_last_touch, fund_state(&h, &ctx).accrual_index);

    publish_on(&mut h, &ctx, TUE + 5, RATES[5 + 1]);
    settle(&mut h, &ctx, &c, &a.pubkey());
    assert_eq!(accrued(&h, &ctx, &c.pubkey()), day(250_000, RATES[6]));
}

#[test]
fn a_suspended_holder_keeps_earning() {
    // FR-015: status doesn't change interest; settlement happens at the next transfer.
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 3_000_000);
    let c = ctx.compliance.pubkey();
    ok(h.send(
        &[ix_governance::suspend_holder(
            &c,
            &ctx.m(),
            &a.pubkey(),
            REASON,
            [0; 32],
        )],
        &[&ctx.compliance],
    ));
    for i in 0..3 {
        publish_on(&mut h, &ctx, TUE + i, RATES[i as usize]);
    }
    ok(h.send(
        &[ix_governance::reinstate_holder(
            &c,
            &ctx.m(),
            &a.pubkey(),
            REASON,
            [0; 32],
        )],
        &[&ctx.compliance],
    ));
    settle(&mut h, &ctx, &a, &b.pubkey());
    let want: u128 = RATES[..3].iter().map(|r| day(3_000_000, *r)).sum();
    assert_eq!(accrued(&h, &ctx, &a.pubkey()), want);
}

#[test]
fn a_holder_with_expired_kyc_keeps_earning() {
    // FR-015: KYC lapses mid-way, publishes continue, the holder is renewed and then settles.
    let (mut h, ctx) = fund_on(TUE);
    let b = fixtures::new_investor(&mut h, &ctx);
    let a = h.funded_keypair();
    let o = a.pubkey();
    ok(h.send(&[ix_hook::register_holder(&o, &o, &ctx.m(), &o, 0)], &[&a]));
    let expiry = clock::now(&h.svm) + 2 * clock::DAY;
    let signed = fixtures::signed_statement(&ctx, &o, *b"US", expiry);
    let ixs = ix_hook::onboarding_tx(
        &h.payer.pubkey(),
        &ctx.compliance.pubkey(),
        &ctx.m(),
        &o,
        &signed,
    );
    ok(h.send(&ixs, &[&ctx.compliance]));
    units::seed_units(&mut h, &ctx.m(), &o, 2_000_000);

    for i in 0..4 {
        publish_on(&mut h, &ctx, TUE + i, RATES[i as usize]);
    }
    assert!(
        !holder(&h, &ctx, &o).approved(clock::now(&h.svm)),
        "KYC expired by now"
    );
    let renewal = fixtures::signed_statement(&ctx, &o, *b"US", clock::now(&h.svm) + fixtures::YEAR);
    let renew = ix_hook::renew_investor(
        &ctx.compliance.pubkey(),
        &ctx.m(),
        &o,
        renewal.attestation_hash,
    );
    ok(h.send(&[renewal.ix.clone(), renew], &[&ctx.compliance]));
    settle(&mut h, &ctx, &a, &b.pubkey());
    let want: u128 = RATES[..4].iter().map(|r| day(2_000_000, *r)).sum();
    assert_eq!(accrued(&h, &ctx, &o), want);
}

#[test]
fn holders_keep_earning_while_transfers_are_paused() {
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 1_000_000);
    units::seed_units(&mut h, &ctx.m(), &b.pubkey(), 3_000_000);
    let c = ctx.compliance.pubkey();
    ok(h.send(
        &[ix_governance::pause_transfers(
            &c,
            &ctx.m(),
            REASON,
            [0; 32],
        )],
        &[&ctx.compliance],
    ));
    publish_on(&mut h, &ctx, TUE, RATES[0]);
    publish_on(&mut h, &ctx, TUE + 1, RATES[1]);
    ok(h.send(
        &[ix_governance::unpause_transfers(
            &c,
            &ctx.m(),
            REASON,
            [0; 32],
        )],
        &[&ctx.compliance],
    ));
    settle(&mut h, &ctx, &a, &b.pubkey());
    assert_eq!(
        accrued(&h, &ctx, &a.pubkey()),
        day(1_000_000, RATES[0]) + day(1_000_000, RATES[1])
    );
    assert_eq!(
        accrued(&h, &ctx, &b.pubkey()),
        day(3_000_000, RATES[0]) + day(3_000_000, RATES[1])
    );
}

#[test]
fn the_fund_treasury_earns_like_any_holder() {
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let treasury = fixtures::onboard_treasury(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &treasury, 5_000);
    publish_on(&mut h, &ctx, TUE, RATES[0]);
    publish_on(&mut h, &ctx, TUE + 1, RATES[1]);
    settle(&mut h, &ctx, &a, &treasury);
    assert_eq!(
        accrued(&h, &ctx, &treasury),
        day(5_000, RATES[0]) + day(5_000, RATES[1])
    );
}

#[test]
fn earned_interest_can_be_read_without_a_transaction() {
    // FR-017: the SDK's read equals what a settlement then stores.
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 9_000_000);
    publish_on(&mut h, &ctx, TUE, RATES[3]);
    publish_on(&mut h, &ctx, TUE + 1, RATES[4]);
    let want = day(9_000_000, RATES[3]) + day(9_000_000, RATES[4]);
    let before = h.data(&pda::holder(&ctx.m(), &a.pubkey()).0);
    assert_eq!(earned_now(&h, &ctx, &a.pubkey()), want);
    assert_eq!(
        h.data(&pda::holder(&ctx.m(), &a.pubkey()).0),
        before,
        "reading writes nothing"
    );
    settle(&mut h, &ctx, &a, &b.pubkey());
    assert_eq!(accrued(&h, &ctx, &a.pubkey()), want);
    assert_eq!(earned_now(&h, &ctx, &a.pubkey()), want);
}

#[test]
fn a_rejected_transfer_settles_nothing() {
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 1_000_000);
    publish_on(&mut h, &ctx, TUE, RATES[0]);
    let c = ctx.compliance.pubkey();
    ok(h.send(
        &[ix_governance::suspend_holder(
            &c,
            &ctx.m(),
            &b.pubkey(),
            REASON,
            [0; 32],
        )],
        &[&ctx.compliance],
    ));
    let keys = [
        pda::holder(&ctx.m(), &a.pubkey()).0,
        pda::holder(&ctx.m(), &b.pubkey()).0,
    ];
    let before: Vec<Vec<u8>> = keys.iter().map(|k| h.data(k)).collect();
    let res = fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), 10);
    assert_anchor_error(&res, HookError::ReceiverNotApproved);
    let after: Vec<Vec<u8>> = keys.iter().map(|k| h.data(k)).collect();
    assert_eq!(before, after);
}

#[test]
fn the_transfer_record_carries_the_settled_interest() {
    // FR-020: a settlement's record is the transfer's record.
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 1_000_000);
    units::seed_units(&mut h, &ctx.m(), &b.pubkey(), 2_000_000);
    publish_on(&mut h, &ctx, TUE, RATES[0]);
    let meta = ok(fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), 100));
    let ev = log_events::<TransferValidated>(&meta);
    assert_eq!(ev.len(), 1);
    let e = &ev[0];
    assert_eq!(e.accrual_index, fund_state(&h, &ctx).accrual_index);
    assert_eq!(e.source_accrued_e18, day(1_000_000, RATES[0]));
    assert_eq!(e.destination_accrued_e18, day(2_000_000, RATES[0]));
    assert_eq!(e.source_accrued_e18, accrued(&h, &ctx, &a.pubkey()));
    assert_eq!(e.destination_accrued_e18, accrued(&h, &ctx, &b.pubkey()));
}

// ---------------------------------------------------------------------------------------------
// T028: an auditor rebuilds every holder's interest from the records alone (FR-021, SC-007)
// ---------------------------------------------------------------------------------------------

#[test]
fn every_holders_interest_can_be_rebuilt_from_the_records() {
    let (mut h, ctx) = fund_on(TUE);
    let holders: Vec<Keypair> = (0..4)
        .map(|_| fixtures::new_investor(&mut h, &ctx))
        .collect();
    let keys: Vec<Pubkey> = holders.iter().map(|k| k.pubkey()).collect();
    // Starting balances, held before the first publish (in F003 they arrive by recorded mint).
    let start: HashMap<Pubkey, u64> = [
        (keys[0], 1_000_000_000),
        (keys[1], 500_000_000),
        (keys[2], 0),
        (keys[3], 0),
    ]
    .into();
    for (k, b) in &start {
        if *b > 0 {
            units::seed_units(&mut h, &ctx.m(), k, *b);
        }
    }
    let mut balances = start.clone();
    let schedule = [
        (0, 1, 100_000_000),
        (1, 2, 50_000_000),
        (2, 3, 10_000_000),
        (0, 3, 200_000_000),
        (3, 0, 5_000_000),
    ];
    let mut records = vec![];
    let mut d = TUE;
    for i in 0..15usize {
        at(&mut h, d);
        let days: u16 = if i % 5 == 3 { 3 } else { 1 };
        ok(publish(&mut h, &ctx, ONE, RATES[i], days));
        d += u32::from(days); // the next publish starts after the covered period
        let seq = fund_state(&h, &ctx).nav_seq;
        records.push(Record::Nav {
            index_after: read_nav_record(&h, &ctx, seq).index_after,
        });
        let (from, to, amount) = schedule[i % schedule.len()];
        if balances[&keys[from]] >= amount {
            let meta = ok(fixtures::transfer(
                &mut h,
                &ctx,
                &holders[from],
                &keys[to],
                amount,
            ));
            *balances.get_mut(&keys[from]).unwrap() -= amount;
            *balances.get_mut(&keys[to]).unwrap() += amount;
            for e in log_events::<TransferValidated>(&meta) {
                records.push(Record::Transfer {
                    source: e.source_owner,
                    destination: e.destination_owner,
                    amount: e.amount,
                    source_accrued_after: e.source_accrued_e18,
                    destination_accrued_after: e.destination_accrued_e18,
                });
            }
        }
    }
    let rebuilt = rebuild(&start, &records);
    for k in &keys {
        assert_eq!(rebuilt[k], earned_now(&h, &ctx, k), "holder {k}");
    }
}
