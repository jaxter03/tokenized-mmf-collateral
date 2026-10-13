//! F002 FR-008, US1-4: a price beyond the daily move limit needs a compliance override approved
//! through governance (T021; contracts/governance.md, contracts/fund.md).
//!
//! Every rejection asserts the exact error and that FundState is unchanged.

use anchor_lang::prelude::Pubkey;
use fund::events::{NavOverrideApproved, NavOverrideWithdrawn, NavPublished};
use fund::{FundError, FundState};
use governance::GovernanceError;
use mmf_common::enums::ReasonCode;
use mmf_sdk::{ix_fund, ix_governance, ix_hook, pda};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_squads_error};
use tests::events::cpi_events;
use tests::fixtures::{self, publish, read, read_nav_record, FundCtx};
use tests::squads::{self, INVALID_PROPOSAL_STATUS};
use tests::{clock, Harness, Ix, TxMeta, TxResult};

/// Tuesday 2026-10-13.
const TUE: u32 = 20_739;
const RATE: i64 = 123_287_671;
/// 1 % below $1.00: beyond the standard 50 bps limit.
const BREAK: u64 = 99_000_000;
const OTHER: u8 = ReasonCode::Other as u8;
const CASE: [u8; 32] = [7; 32];

fn fund_on(day: u32) -> (Harness, FundCtx) {
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, day, 15 * 3_600);
    let ctx = fixtures::launched_fund(&mut h, 0);
    (h, ctx)
}

fn at(h: &mut Harness, day: u32) {
    clock::set_utc_day(&mut h.svm, day, 15 * 3_600);
}

fn pending(h: &Harness, ctx: &FundCtx) -> u64 {
    read::<FundState>(h, &ctx.fund_state()).pending_override_nav
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

fn approve(h: &mut Harness, ctx: &FundCtx, nav: u64) -> TxResult {
    let ix =
        ix_governance::approve_nav_override(&ctx.compliance.pubkey(), &ctx.m(), nav, OTHER, CASE);
    h.send(&[ix], &[&ctx.compliance])
}

fn withdraw(h: &mut Harness, ctx: &FundCtx) -> TxResult {
    let ix = ix_governance::withdraw_nav_override(&ctx.compliance.pubkey(), &ctx.m(), OTHER, CASE);
    h.send(&[ix], &[&ctx.compliance])
}

/// Sends and asserts `err` with FundState byte-identical afterwards.
#[track_caller]
fn reject<E: Into<u32> + core::fmt::Debug + Copy>(
    h: &mut Harness,
    ctx: &FundCtx,
    ixs: &[Ix],
    signers: &[&Keypair],
    err: E,
) {
    let before = h.data(&ctx.fund_state());
    let res = h.send(ixs, signers);
    assert_anchor_error(&res, err);
    assert_eq!(h.data(&ctx.fund_state()), before, "FundState unchanged");
}

fn published(meta: &TxMeta) -> NavPublished {
    let mut ev = cpi_events::<NavPublished>(meta);
    assert_eq!(ev.len(), 1);
    ev.remove(0)
}

// ---------------------------------------------------------------------------------------------
// Happy paths
// ---------------------------------------------------------------------------------------------

#[test]
fn an_override_lets_one_exact_price_through() {
    let (mut h, ctx) = fund_on(TUE);
    let meta = ok(approve(&mut h, &ctx, BREAK));
    let ev = cpi_events::<NavOverrideApproved>(&meta);
    assert_eq!(ev.len(), 1);
    let e = &ev[0];
    assert_eq!(e.fund, ctx.fund_state());
    assert_eq!((e.nav, e.reason, e.case_ref_hash), (BREAK, OTHER, CASE));
    assert_eq!(e.actor, ctx.compliance.pubkey());
    assert_eq!(pending(&h, &ctx), BREAK);

    let meta = ok(publish(&mut h, &ctx, BREAK, RATE, 1));
    let e = published(&meta);
    assert!(e.override_used);
    assert_eq!(e.override_cleared_nav, BREAK);
    let r = read_nav_record(&h, &ctx, 1);
    assert_eq!(r.nav, BREAK);
    assert!(r.override_used);
    assert_eq!(pending(&h, &ctx), 0, "used up");
}

#[test]
fn the_next_publish_uses_up_the_override_whatever_its_price() {
    let (mut h, ctx) = fund_on(TUE);
    ok(approve(&mut h, &ctx, BREAK));

    // A different out-of-limit price isn't helped, and the failed publish changes nothing.
    let before = h.data(&ctx.fund_state());
    let res = publish(&mut h, &ctx, 98_000_000, RATE, 1);
    assert_anchor_error(&res, FundError::PriceMoveTooLarge);
    assert_eq!(h.data(&ctx.fund_state()), before);
    assert_eq!(
        pending(&h, &ctx),
        BREAK,
        "still pending after a failed publish"
    );

    // A within-limit publish clears it without using it.
    let e = published(&ok(publish(&mut h, &ctx, 100_200_000, RATE, 1)));
    assert!(!e.override_used);
    assert_eq!(e.override_cleared_nav, BREAK);
    assert!(!read_nav_record(&h, &ctx, 1).override_used);
    assert_eq!(pending(&h, &ctx), 0);

    // Gone: the next day the same price is checked against the normal limit again.
    at(&mut h, TUE + 1);
    let res = publish(&mut h, &ctx, BREAK, RATE, 1);
    assert_anchor_error(&res, FundError::PriceMoveTooLarge);
}

#[test]
fn an_override_equal_to_a_within_limit_price_is_not_counted_as_used() {
    let (mut h, ctx) = fund_on(TUE);
    ok(approve(&mut h, &ctx, 100_100_000));
    let e = published(&ok(publish(&mut h, &ctx, 100_100_000, RATE, 1)));
    assert!(!e.override_used, "the price passed on its own");
    assert_eq!(e.override_cleared_nav, 100_100_000);
    assert_eq!(pending(&h, &ctx), 0);
}

#[test]
fn compliance_can_withdraw_an_override_before_use() {
    let (mut h, ctx) = fund_on(TUE);
    ok(approve(&mut h, &ctx, BREAK));
    let meta = ok(withdraw(&mut h, &ctx));
    let ev = cpi_events::<NavOverrideWithdrawn>(&meta);
    assert_eq!(ev.len(), 1);
    let e = &ev[0];
    assert_eq!(e.fund, ctx.fund_state());
    assert_eq!((e.nav, e.reason, e.case_ref_hash), (BREAK, OTHER, CASE));
    assert_eq!(e.actor, ctx.compliance.pubkey());
    assert_eq!(pending(&h, &ctx), 0);
    let res = publish(&mut h, &ctx, BREAK, RATE, 1);
    assert_anchor_error(&res, FundError::PriceMoveTooLarge);
}

// ---------------------------------------------------------------------------------------------
// Rejections
// ---------------------------------------------------------------------------------------------

#[test]
fn one_override_at_a_time_and_only_one_to_withdraw() {
    let (mut h, ctx) = fund_on(TUE);
    let c = ctx.compliance.insecure_clone();
    let w = ix_governance::withdraw_nav_override(&c.pubkey(), &ctx.m(), OTHER, CASE);
    reject(&mut h, &ctx, &[w], &[&c], FundError::NoOverridePending);

    ok(approve(&mut h, &ctx, BREAK));
    let a = ix_governance::approve_nav_override(&c.pubkey(), &ctx.m(), 98_000_000, OTHER, CASE);
    reject(&mut h, &ctx, &[a], &[&c], FundError::OverrideAlreadyPending);
    assert_eq!(pending(&h, &ctx), BREAK);
}

#[test]
fn an_override_must_name_a_positive_price() {
    let (mut h, ctx) = fund_on(TUE);
    let c = ctx.compliance.insecure_clone();
    let a = ix_governance::approve_nav_override(&c.pubkey(), &ctx.m(), 0, OTHER, CASE);
    reject(&mut h, &ctx, &[a], &[&c], FundError::ZeroPrice);
}

#[test]
fn only_operational_error_or_other_are_valid_reasons() {
    let (mut h, ctx) = fund_on(TUE);
    let c = ctx.compliance.insecure_clone();
    for reason in [0u8, 1, 2, 4, 5, 7] {
        let a = ix_governance::approve_nav_override(&c.pubkey(), &ctx.m(), BREAK, reason, CASE);
        reject(&mut h, &ctx, &[a], &[&c], GovernanceError::InvalidReason);
    }
    let op = ReasonCode::OperationalError as u8;
    let a = ix_governance::approve_nav_override(&c.pubkey(), &ctx.m(), BREAK, op, CASE);
    ok(h.send(&[a], &[&c]));
    let w = ix_governance::withdraw_nav_override(&c.pubkey(), &ctx.m(), 1, CASE);
    reject(&mut h, &ctx, &[w], &[&c], GovernanceError::InvalidReason);
}

#[test]
fn only_compliance_approves_or_withdraws() {
    let (mut h, ctx) = fund_on(TUE);
    for who in [&ctx.fund_admin, &ctx.issuer, &ctx.keeper] {
        let k = who.insecure_clone();
        let a = ix_governance::approve_nav_override(&k.pubkey(), &ctx.m(), BREAK, OTHER, CASE);
        reject(&mut h, &ctx, &[a], &[&k], GovernanceError::Unauthorized);
    }
    ok(approve(&mut h, &ctx, BREAK));
    let k = ctx.fund_admin.insecure_clone();
    let w = ix_governance::withdraw_nav_override(&k.pubkey(), &ctx.m(), OTHER, CASE);
    reject(&mut h, &ctx, &[w], &[&k], GovernanceError::Unauthorized);
}

#[test]
fn no_override_on_a_fund_in_setup() {
    let mut h = Harness::new();
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    let compliance = h.funded_keypair();
    let roles = ix_governance::Roles {
        fund_admin: Pubkey::new_unique(),
        transfer_agent: Pubkey::new_unique(),
        compliance: compliance.pubkey(),
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
    let fs = pda::fund_state(&mint.pubkey()).0;
    let before = h.data(&fs);
    let a = ix_governance::approve_nav_override(
        &compliance.pubkey(),
        &mint.pubkey(),
        BREAK,
        OTHER,
        CASE,
    );
    let res = h.send(&[a], &[&compliance]);
    assert_anchor_error(&res, GovernanceError::FundNotOpen);
    assert_eq!(h.data(&fs), before);
}

#[test]
fn fund_override_instructions_accept_only_governance() {
    // Submitted directly, with no governance in the path (Constitution II).
    let (mut h, ctx) = fund_on(TUE);
    let stranger = h.funded_keypair();
    for who in [ctx.compliance.insecure_clone(), stranger] {
        let set = ix_fund::set_nav_override_raw(
            &who.pubkey(),
            &ctx.m(),
            BREAK,
            OTHER,
            CASE,
            &who.pubkey(),
        );
        reject(&mut h, &ctx, &[set], &[&who], FundError::UnauthorizedCaller);
    }
    ok(approve(&mut h, &ctx, BREAK));
    let c = ctx.compliance.insecure_clone();
    let clear = ix_fund::clear_nav_override_raw(&c.pubkey(), &ctx.m(), OTHER, CASE, &c.pubkey());
    reject(&mut h, &ctx, &[clear], &[&c], FundError::UnauthorizedCaller);
    assert_eq!(pending(&h, &ctx), BREAK);
}

#[test]
fn one_funds_compliance_cannot_override_another_fund() {
    let (mut h, a) = fund_on(TUE);
    let b = fixtures::launched_fund(&mut h, 0);
    let bc = b.compliance.insecure_clone();

    // B's role registry with A's fund_state: the registry isn't bound to A.
    let mut ix = ix_governance::approve_nav_override(&bc.pubkey(), &b.m(), BREAK, OTHER, CASE);
    ix.accounts[2].pubkey = a.fund_state();
    reject(
        &mut h,
        &a,
        &[ix],
        &[&bc],
        anchor_lang::error::ErrorCode::ConstraintAddress,
    );

    // A's own role registry and fund_state, signed by B's compliance.
    let ix = ix_governance::approve_nav_override(&bc.pubkey(), &a.m(), BREAK, OTHER, CASE);
    reject(&mut h, &a, &[ix], &[&bc], GovernanceError::Unauthorized);
    assert_eq!(pending(&h, &a), 0);
}

// ---------------------------------------------------------------------------------------------
// Compliance as a Squads 2-of-3 vault (call depth 4: Squads → governance → fund → emit_cpi)
// ---------------------------------------------------------------------------------------------

#[test]
fn a_multisig_compliance_approves_only_at_its_threshold() {
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, TUE, 15 * 3_600);
    let mut ms = squads::create(&mut h, 2, 3);
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    let m = mint.pubkey();
    let admin = h.funded_keypair();
    let roles = ix_governance::Roles {
        fund_admin: admin.pubkey(),
        transfer_agent: Pubkey::new_unique(),
        compliance: ms.vault,
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    };
    let i = issuer.pubkey();
    let providers = [
        Pubkey::new_unique(),
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    h.send(
        &[
            ix_hook::init_registry(&i, &m),
            ix_hook::set_rules(&i, &m, providers, fixtures::jurisdictions(&[*b"US"]), 0),
            ix_governance::set_roles(&i, &m, &roles),
            ix_fund::launch(&i, &m),
        ],
        &[&issuer],
    )
    .unwrap();
    let fs = pda::fund_state(&m).0;

    let approve = ix_governance::approve_nav_override(&ms.vault, &m, BREAK, OTHER, CASE);
    let (index, remaining) = ms.propose(&mut h, &[approve]);
    ms.approve(&mut h, index, 0).unwrap();
    let res = ms.execute(&mut h, index, &remaining, &[]);
    assert_squads_error(&res, INVALID_PROPOSAL_STATUS);
    assert_eq!(read::<FundState>(&h, &fs).pending_override_nav, 0);

    ms.approve(&mut h, index, 1).unwrap();
    let meta = ok(ms.execute(&mut h, index, &remaining, &[]));
    assert_eq!(cpi_events::<NavOverrideApproved>(&meta)[0].actor, ms.vault);
    assert_eq!(read::<FundState>(&h, &fs).pending_override_nav, BREAK);

    let ix = ix_fund::publish_daily(&admin.pubkey(), &h.payer.pubkey(), &m, 1, BREAK, RATE, 1);
    ok(h.send(&[ix], &[&admin]));
    assert!(read::<fund::NavRecord>(&h, &pda::nav_record(&fs, 1).0).override_used);
}
