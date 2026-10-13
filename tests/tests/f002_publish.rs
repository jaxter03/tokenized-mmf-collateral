//! F002 US1: publish the daily price and rate (T018–T020; contracts/fund.md `publish_daily`).
//!
//! Every rejection asserts the exact error and that nothing changed: FundState byte-identical and
//! no NavRecord created (Constitution VI, VII).

use anchor_lang::prelude::Pubkey;
use anchor_lang::{Discriminator, InstructionData};
use fund::events::NavPublished;
use fund::{FundError, FundState, NavRecord};
use mmf_sdk::{ix_fund, ix_governance, pda};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::events::cpi_events;
use tests::fixtures::{self, check_i12, publish, read, read_nav_record, FundCtx};
use tests::{clock, units, Harness, Ix, TxResult};

/// Tuesday 2026-10-13.
const TUE: u32 = 20_739;
/// Friday 2026-10-16.
const FRI: u32 = 20_742;
/// Monday 2026-10-19.
const MON: u32 = 20_745;
const ONE: u64 = 100_000_000;
/// 4.5 % a year.
const RATE: i64 = 123_287_671;

/// A launched fund with the clock at 15:00 UTC on `day`.
fn fund_on(day: u32) -> (Harness, FundCtx) {
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, day, 15 * 3_600);
    let ctx = fixtures::launched_fund(&mut h, 0);
    (h, ctx)
}

fn at(h: &mut Harness, day: u32) {
    clock::set_utc_day(&mut h.svm, day, 15 * 3_600);
}

fn fund_state(h: &Harness, ctx: &FundCtx) -> FundState {
    read(h, &ctx.fund_state())
}

fn nav_record_exists(h: &Harness, ctx: &FundCtx, seq: u64) -> bool {
    h.exists(&pda::nav_record(&ctx.fund_state(), seq).0)
}

/// Sends `ix` signed by `signer` and asserts it fails with `err` leaving FundState unchanged and
/// no new NavRecord.
#[track_caller]
fn reject<E: Into<u32> + core::fmt::Debug + Copy>(
    h: &mut Harness,
    ctx: &FundCtx,
    ix: Ix,
    signers: &[&Keypair],
    err: E,
) {
    let before = h.data(&ctx.fund_state());
    let next = fund_state(h, ctx).nav_seq.wrapping_add(1);
    let res = h.send(&[ix], signers);
    assert_anchor_error(&res, err);
    assert_eq!(h.data(&ctx.fund_state()), before, "FundState unchanged");
    assert!(!nav_record_exists(h, ctx, next), "no NavRecord created");
}

/// A `publish_daily` for `ctx`'s next record, with `fund_admin` as the signer account.
fn publish_ix(
    h: &Harness,
    ctx: &FundCtx,
    fund_admin: &Pubkey,
    nav: u64,
    rate: i64,
    days: u16,
) -> Ix {
    let next = fund_state(h, ctx).nav_seq.wrapping_add(1);
    ix_fund::publish_daily(
        fund_admin,
        &h.payer.pubkey(),
        &ctx.m(),
        next,
        nav,
        rate,
        days,
    )
}

fn reject_publish<E: Into<u32> + core::fmt::Debug + Copy>(
    h: &mut Harness,
    ctx: &FundCtx,
    nav: u64,
    rate: i64,
    days: u16,
    err: E,
) {
    let ix = publish_ix(h, ctx, &ctx.fund_admin.pubkey(), nav, rate, days);
    let admin = ctx.fund_admin.insecure_clone();
    reject(h, ctx, ix, &[&admin], err);
}

fn ok(res: TxResult) -> tests::TxMeta {
    match res {
        Ok(m) => m,
        Err(f) => panic!(
            "expected success, got {:?}; logs:\n{}",
            f.err,
            f.meta.logs.join("\n")
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// T018: happy path (FR-002–FR-004, US1-1, US1-5, SC-005)
// ---------------------------------------------------------------------------------------------

#[test]
fn three_days_are_published_and_read_back_by_sequence() {
    let (mut h, ctx) = fund_on(TUE);
    let days = [
        (TUE, ONE, RATE),
        (TUE + 1, 100_010_000, 125_000_000),
        (TUE + 2, 99_995_000, 119_178_082),
    ];
    let mut index: u128 = 0;
    for (i, (day, nav, rate)) in days.iter().enumerate() {
        at(&mut h, *day);
        let meta = ok(publish(&mut h, &ctx, *nav, *rate, 1));
        index += *rate as u128 * 1_000_000;
        let now = clock::now(&h.svm);

        let seq = i as u64 + 1;
        let r = read_nav_record(&h, &ctx, seq);
        assert_eq!(r.version, NavRecord::VERSION);
        assert_eq!(r.seq, seq);
        assert_eq!(r.nav, *nav);
        assert_eq!(r.daily_rate_e12, *rate as u64);
        assert_eq!(r.days_covered, 1);
        assert_eq!(r.period_start_day, *day);
        assert_eq!(r.index_after, index);
        assert_eq!(r.as_of_ts, now);
        assert_eq!(r.published_by, ctx.fund_admin.pubkey());
        assert!(!r.override_used);

        let fs = fund_state(&h, &ctx);
        assert_eq!(fs.nav, *nav);
        assert_eq!(fs.nav_ts, now);
        assert_eq!(fs.nav_seq, seq);
        assert_eq!(fs.daily_rate_e12, *rate as u64);
        assert_eq!(fs.accrual_index, index);
        assert_eq!(fs.nav_period_end_day, *day);
        assert_eq!(fs.pending_override_nav, 0);

        let ev = cpi_events::<NavPublished>(&meta);
        assert_eq!(ev.len(), 1, "one NavPublished");
        let e = &ev[0];
        assert_eq!(e.fund, ctx.fund_state());
        assert_eq!(
            (e.seq, e.nav, e.daily_rate_e12, e.days_covered),
            (seq, *nav, *rate as u64, 1)
        );
        assert_eq!((e.period_start_day, e.index_after), (*day, index));
        assert_eq!(e.publisher, ctx.fund_admin.pubkey());
        assert!(!e.override_used);
        assert_eq!(e.override_cleared_nav, 0);
        assert_eq!(e.unix_ts, now);
    }
    assert_eq!(fund_state(&h, &ctx).nav_period_end_day, TUE + 2);
    check_i12(&h, &ctx);

    // Records are permanent: 90 more publishes leave records 1–3 byte-for-byte unchanged.
    let keys: Vec<Pubkey> = (1..=3)
        .map(|s| pda::nav_record(&ctx.fund_state(), s).0)
        .collect();
    let before: Vec<Vec<u8>> = keys.iter().map(|k| h.data(k)).collect();
    for d in 3..93 {
        at(&mut h, TUE + d);
        ok(publish(&mut h, &ctx, ONE, RATE, 1));
    }
    let after: Vec<Vec<u8>> = keys.iter().map(|k| h.data(k)).collect();
    assert_eq!(before, after, "records 1–3 unchanged");
    assert_eq!(fund_state(&h, &ctx).nav_seq, 93);
    check_i12(&h, &ctx);
}

// ---------------------------------------------------------------------------------------------
// T019: rejections, in contract order
// ---------------------------------------------------------------------------------------------

#[test]
fn role_registry_must_be_this_funds() {
    let (mut h, ctx) = fund_on(TUE);
    let other = fixtures::launched_fund(&mut h, 0);
    let admin = ctx.fund_admin.insecure_clone();

    // Another fund's role registry (whose fund admin is also a signer here).
    let mut ix = publish_ix(&h, &ctx, &admin.pubkey(), ONE, RATE, 1);
    ix.accounts[3].pubkey = pda::role_registry(&other.fund_state()).0;
    reject(&mut h, &ctx, ix, &[&admin], FundError::RoleRegistryMismatch);

    // An account that isn't governance's at all.
    let mut ix = publish_ix(&h, &ctx, &admin.pubkey(), ONE, RATE, 1);
    ix.accounts[3].pubkey = Pubkey::new_unique();
    reject(&mut h, &ctx, ix, &[&admin], FundError::RoleRegistryMismatch);
}

#[test]
fn only_the_fund_admin_publishes() {
    let (mut h, ctx) = fund_on(TUE);
    let stranger = h.funded_keypair();
    for who in [
        &ctx.issuer,
        &ctx.compliance,
        &ctx.keeper,
        &ctx.transfer_agent,
        &ctx.auditor,
        &stranger,
    ] {
        let ix = publish_ix(&h, &ctx, &who.pubkey(), ONE, RATE, 1);
        let k = who.insecure_clone();
        reject(&mut h, &ctx, ix, &[&k], FundError::Unauthorized);
    }
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
}

#[test]
fn a_fund_in_setup_cannot_publish() {
    let mut h = Harness::new();
    at(&mut h, TUE);
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    let admin = h.funded_keypair();
    let roles = ix_governance::Roles {
        fund_admin: admin.pubkey(),
        transfer_agent: Pubkey::new_unique(),
        compliance: Pubkey::new_unique(),
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    };
    h.send(
        &[ix_governance::set_roles(
            &issuer.pubkey(),
            &mint.pubkey(),
            &roles,
        )],
        &[&issuer],
    )
    .unwrap();
    let fs_key = pda::fund_state(&mint.pubkey()).0;
    let before = h.data(&fs_key);
    let ix = ix_fund::publish_daily(
        &admin.pubkey(),
        &h.payer.pubkey(),
        &mint.pubkey(),
        1,
        ONE,
        RATE,
        1,
    );
    let res = h.send(&[ix], &[&admin]);
    assert_anchor_error(&res, FundError::FundNotOpen);
    assert_eq!(h.data(&fs_key), before);
    assert!(!h.exists(&pda::nav_record(&fs_key, 1).0));
}

#[test]
fn price_must_be_positive() {
    let (mut h, ctx) = fund_on(TUE);
    reject_publish(&mut h, &ctx, 0, RATE, 1, FundError::ZeroPrice);
}

#[test]
fn rate_must_not_be_negative() {
    let (mut h, ctx) = fund_on(TUE);
    reject_publish(&mut h, &ctx, ONE, -1, 1, FundError::NegativeRate);
    reject_publish(&mut h, &ctx, ONE, i64::MIN, 1, FundError::NegativeRate);

    // A client that encodes the rate as u64::MAX (e.g. −1 mis-encoded) sends exactly these bytes
    // (research R6): rejected, not accepted as a huge rate.
    let mut ix = publish_ix(&h, &ctx, &ctx.fund_admin.pubkey(), ONE, 0, 1);
    let mut data = fund::instruction::PublishDaily::DISCRIMINATOR.to_vec();
    data.extend_from_slice(&ONE.to_le_bytes());
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        data.len(),
        fund::instruction::PublishDaily {
            nav: ONE,
            daily_rate_e12: 0,
            days_covered: 1
        }
        .data()
        .len()
    );
    ix.data = data;
    let admin = ctx.fund_admin.insecure_clone();
    reject(&mut h, &ctx, ix, &[&admin], FundError::NegativeRate);
}

#[test]
fn a_publish_covers_at_least_one_day() {
    let (mut h, ctx) = fund_on(TUE);
    reject_publish(&mut h, &ctx, ONE, RATE, 0, FundError::ZeroDaysCovered);
}

#[test]
fn a_period_that_has_not_begun_cannot_be_published() {
    // A second publish on a day already covered.
    let (mut h, ctx) = fund_on(TUE);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    clock::advance(&mut h.svm, 3_600);
    reject_publish(&mut h, &ctx, ONE, RATE, 1, FundError::PeriodNotStarted);

    // Saturday after a Friday publish that already covers the weekend.
    let (mut h, ctx) = fund_on(FRI);
    ok(publish(&mut h, &ctx, ONE, RATE, 3));
    at(&mut h, FRI + 1);
    reject_publish(&mut h, &ctx, ONE, RATE, 1, FundError::PeriodNotStarted);
    at(&mut h, FRI + 2);
    reject_publish(&mut h, &ctx, ONE, RATE, 1, FundError::PeriodNotStarted);
    at(&mut h, MON);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
}

#[test]
fn a_price_beyond_the_daily_move_limit_is_rejected() {
    // First publish: measured from the $1.00 target, limit 50 bps (standard terms).
    let (mut h, ctx) = fund_on(TUE);
    reject_publish(
        &mut h,
        &ctx,
        100_500_001,
        RATE,
        1,
        FundError::PriceMoveTooLarge,
    );
    reject_publish(
        &mut h,
        &ctx,
        99_499_999,
        RATE,
        1,
        FundError::PriceMoveTooLarge,
    );

    // Later publishes: measured from the previous price, up and down. 50 bps of 1.004 = 0.00502.
    ok(publish(&mut h, &ctx, 100_400_000, RATE, 1));
    at(&mut h, TUE + 1);
    reject_publish(
        &mut h,
        &ctx,
        100_902_001,
        RATE,
        1,
        FundError::PriceMoveTooLarge,
    );
    reject_publish(
        &mut h,
        &ctx,
        99_897_999,
        RATE,
        1,
        FundError::PriceMoveTooLarge,
    );
    ok(publish(&mut h, &ctx, 100_902_000, RATE, 1));
    at(&mut h, TUE + 2);
    // 50 bps of 1.00902 = 0.0050451: 1.00902 − 0.0050451 = 1.0039749.
    reject_publish(
        &mut h,
        &ctx,
        100_397_489,
        RATE,
        1,
        FundError::PriceMoveTooLarge,
    );
    ok(publish(&mut h, &ctx, 100_397_490, RATE, 1));
    check_i12(&h, &ctx);
}

#[test]
fn counter_and_sequence_overflow_fail_closed() {
    // Unreachable through instructions (~5.6 × 10^8 maximum publishes), so the state is patched
    // directly. This is one of F002's few `set_account` patches; see quickstart.md.
    let (mut h, ctx) = fund_on(TUE);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    fixtures::patch::<FundState>(&mut h, &ctx.fund_state(), |fs| {
        fs.accrual_index = u128::MAX - 1
    });
    at(&mut h, TUE + 1);
    reject_publish(&mut h, &ctx, ONE, 1, 1, FundError::Overflow);

    let (mut h, ctx) = fund_on(TUE);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    fixtures::patch::<FundState>(&mut h, &ctx.fund_state(), |fs| fs.nav_seq = u64::MAX);
    at(&mut h, TUE + 1);
    // The record address wraps to seq 0 (no panic in the account checks); the handler's checked
    // add reports Overflow.
    reject_publish(&mut h, &ctx, ONE, RATE, 1, FundError::Overflow);
}

#[test]
fn the_record_address_must_be_the_next_sequence() {
    let (mut h, ctx) = fund_on(TUE);
    let ix = ix_fund::publish_daily(
        &ctx.fund_admin.pubkey(),
        &h.payer.pubkey(),
        &ctx.m(),
        2,
        ONE,
        RATE,
        1,
    );
    let admin = ctx.fund_admin.insecure_clone();
    let before = h.data(&ctx.fund_state());
    let res = h.send(&[ix], &[&admin]);
    assert_anchor_error(&res, anchor_lang::error::ErrorCode::ConstraintSeeds);
    assert_eq!(h.data(&ctx.fund_state()), before);
    assert!(!nav_record_exists(&h, &ctx, 2));

    // A record can't be written twice: once seq 1 exists, its address is never the next one.
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    at(&mut h, TUE + 1);
    let ix = ix_fund::publish_daily(
        &ctx.fund_admin.pubkey(),
        &h.payer.pubkey(),
        &ctx.m(),
        1,
        ONE,
        RATE,
        1,
    );
    let res = h.send(&[ix], &[&admin]);
    assert_anchor_error(&res, anchor_lang::error::ErrorCode::ConstraintSeeds);
}

// ---------------------------------------------------------------------------------------------
// T020: coverage and acceptance (FR-007, FR-009, FR-009b, FR-010, I12)
// ---------------------------------------------------------------------------------------------

#[test]
fn a_move_exactly_at_the_limit_is_accepted() {
    for nav in [100_500_000, 99_500_000] {
        let (mut h, ctx) = fund_on(TUE);
        ok(publish(&mut h, &ctx, nav, RATE, 1));
        assert_eq!(read_nav_record(&h, &ctx, 1).nav, nav);
        assert_eq!(fund_state(&h, &ctx).nav, nav);
        check_i12(&h, &ctx);
    }
}

#[test]
fn friday_covers_the_weekend_then_monday_follows() {
    let (mut h, ctx) = fund_on(FRI);
    ok(publish(&mut h, &ctx, ONE, RATE, 3));
    let r = read_nav_record(&h, &ctx, 1);
    assert_eq!((r.period_start_day, r.days_covered), (FRI, 3));
    assert_eq!(r.index_after, RATE as u128 * 1_000_000 * 3);
    assert_eq!(fund_state(&h, &ctx).nav_period_end_day, FRI + 2);
    at(&mut h, MON);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    assert_eq!(read_nav_record(&h, &ctx, 2).period_start_day, MON);
    check_i12(&h, &ctx);
}

#[test]
fn a_late_publish_covers_an_outage() {
    // One record covering the gap.
    let (mut h, ctx) = fund_on(TUE);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    at(&mut h, TUE + 2); // nothing published on TUE + 1
    ok(publish(&mut h, &ctx, ONE, RATE, 2));
    let r = read_nav_record(&h, &ctx, 2);
    assert_eq!((r.period_start_day, r.days_covered), (TUE + 1, 2));
    check_i12(&h, &ctx);

    // Or two records in a row on the same day.
    let (mut h, ctx) = fund_on(TUE);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    at(&mut h, TUE + 2);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    ok(publish(&mut h, &ctx, ONE, 125_000_000, 1));
    assert_eq!(read_nav_record(&h, &ctx, 2).period_start_day, TUE + 1);
    assert_eq!(read_nav_record(&h, &ctx, 3).period_start_day, TUE + 2);
    check_i12(&h, &ctx);
}

#[test]
fn a_zero_rate_is_recorded_and_moves_nothing() {
    let (mut h, ctx) = fund_on(TUE);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    let index = fund_state(&h, &ctx).accrual_index;
    at(&mut h, TUE + 1);
    ok(publish(&mut h, &ctx, ONE, 0, 1));
    assert_eq!(fund_state(&h, &ctx).accrual_index, index);
    let r = read_nav_record(&h, &ctx, 2);
    assert_eq!((r.daily_rate_e12, r.index_after), (0, index));
    check_i12(&h, &ctx);
}

#[test]
fn rate_and_days_have_no_cap() {
    // FR-009b: the admin's figures are trusted (Clarification Q2).
    let (mut h, ctx) = fund_on(TUE);
    ok(publish(&mut h, &ctx, ONE, i64::MAX, u16::MAX));
    let r = read_nav_record(&h, &ctx, 1);
    assert_eq!(
        r.index_after,
        i64::MAX as u128 * 1_000_000 * u16::MAX as u128
    );
    assert_eq!(
        fund_state(&h, &ctx).nav_period_end_day,
        TUE + u32::from(u16::MAX) - 1
    );
    check_i12(&h, &ctx);
}

#[test]
fn publishing_touches_no_holder_record() {
    let (mut h, ctx) = fund_on(TUE);
    let a = fixtures::new_investor(&mut h, &ctx);
    let b = fixtures::new_investor(&mut h, &ctx);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 1_000_000);
    let keys = [
        pda::holder(&ctx.m(), &a.pubkey()).0,
        pda::holder(&ctx.m(), &b.pubkey()).0,
    ];
    let before: Vec<Vec<u8>> = keys.iter().map(|k| h.data(k)).collect();
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    at(&mut h, TUE + 1);
    ok(publish(&mut h, &ctx, ONE, RATE, 1));
    let after: Vec<Vec<u8>> = keys.iter().map(|k| h.data(k)).collect();
    assert_eq!(before, after, "FR-010: no HolderState changes on publish");
}
