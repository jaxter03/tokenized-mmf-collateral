//! T028: `create_instruction` (FR-004–FR-010; contracts/dvp_settlement.md, checks in order) for
//! all three kinds.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use dvp_settlement::events::SettlementCreated;
use dvp_settlement::{CreateArgs, DvpConfig, DvpError, SettlementInstruction};
use mmf_common::enums::ReasonCode;
use mmf_sdk::{ix_dvp, ix_governance, ix_hook};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use tests::assert::{assert_anchor_error, assert_instruction_error};
use tests::clock::{self, DAY};
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, SettleCtx, NAV_ONE, SETTLE_DAY};
use tests::settle::{self, ext, red_args, sec_args, sub_args, ONE};
use tests::{Harness, TxResult};

const CASE: [u8; 32] = [0xC5; 32];

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

fn create(h: &mut Harness, s: &SettleCtx, creator: &Keypair, args: CreateArgs) -> TxResult {
    settle::create(h, s, creator, args)
}

// ---- happy path ---------------------------------------------------------------------------

#[test]
fn records_every_term_and_numbers_instructions_per_fund() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let b = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    let now = clock::now(&h.svm);

    // 1. Subscription created by the investor.
    let args = sub_args(&s, &a.pubkey(), 2_000_000 * ONE, 1);
    let meta = create(&mut h, &s, &a, args).expect("subscription");
    let d = settle::deal(&s, &a.pubkey(), &args);
    let si = settle::read_si(&h, &d);
    assert_eq!(si.version, SettlementInstruction::VERSION);
    assert_eq!(si.fund_state, s.fund_state());
    assert_eq!(si.seq, 1);
    assert_eq!(si.kind, settle::SUBSCRIPTION);
    assert_eq!((si.seller, si.buyer), (s.fund_state(), a.pubkey()));
    assert_eq!(si.units, 2_000_000 * ONE);
    assert_eq!(si.cash_amount, 2_000_000 * ONE, "$1.00 per unit");
    assert_eq!(si.nav_seq, 1);
    assert_eq!(si.not_before_ts, now, "valid-from defaults to now");
    assert_eq!(si.expires_at_ts, now + 86_400, "24 h default expiry");
    assert_eq!(si.status, 0, "Instructed");
    assert_eq!((si.seller_funded_at, si.buyer_funded_at), (0, 0));
    assert_eq!(si.external_ref, ext(1));
    assert_eq!(si.created_by, a.pubkey());
    assert_eq!((si.created_at, si.closed_at), (now, 0));
    assert_eq!(
        (si.cancel_requests, si.units_to_return, si.cash_to_return),
        (0, 0, 0)
    );
    let ev = one_cpi_event::<SettlementCreated>(&meta);
    assert_eq!(
        (
            ev.fund,
            ev.seq,
            ev.address,
            ev.kind,
            ev.units,
            ev.cash_amount
        ),
        (
            s.fund_state(),
            1,
            d.settlement(),
            0,
            si.units,
            si.cash_amount
        )
    );
    assert_eq!(
        (
            ev.seller,
            ev.buyer,
            ev.nav_seq,
            ev.external_ref,
            ev.created_by
        ),
        (si.seller, si.buyer, 1, ext(1), a.pubkey())
    );
    assert_eq!((ev.not_before_ts, ev.expires_at_ts), (now, now + 86_400));

    // 2. Redemption created by the transfer agent.
    create(
        &mut h,
        &s,
        &s.transfer_agent,
        red_args(&s, &b.pubkey(), 5 * ONE, 2),
    )
    .expect("redemption");
    let si: SettlementInstruction = read(
        &h,
        &mmf_sdk::pda::settlement(&s.fund_state(), &s.transfer_agent.pubkey(), &ext(2)).0,
    );
    assert_eq!(
        (si.seq, si.kind, si.cash_amount),
        (2, settle::REDEMPTION, 5 * ONE)
    );
    assert_eq!((si.seller, si.buyer), (b.pubkey(), s.fund_state()));

    // 3. Secondary transfer created by the buyer, with an explicit window.
    let mut args = sec_args(&b.pubkey(), &a.pubkey(), ONE, 999_500, 3);
    args.not_before_ts = now + 60;
    args.expires_at_ts = now + 3 * DAY;
    create(&mut h, &s, &a, args).expect("secondary");
    let si = settle::read_si(&h, &settle::deal(&s, &a.pubkey(), &args));
    assert_eq!(
        (si.seq, si.kind, si.cash_amount),
        (3, settle::SECONDARY, 999_500)
    );
    assert_eq!(
        (si.not_before_ts, si.expires_at_ts),
        (now + 60, now + 3 * DAY)
    );

    let cfg: DvpConfig = read(&h, &s.dvp_config);
    assert_eq!(cfg.next_seq, 4);
}

#[test]
fn either_party_may_create_and_nobody_else() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let b = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let stranger = fixtures::investor_with_cash(&mut h, &s, 0);

    // Subscription and redemption: the investor or the transfer agent (Clarification Q1).
    create(
        &mut h,
        &s,
        &s.transfer_agent,
        sub_args(&s, &a.pubkey(), ONE, 1),
    )
    .unwrap();
    create(&mut h, &s, &b, red_args(&s, &b.pubkey(), ONE, 2)).unwrap();
    // Secondary: the seller or the buyer.
    create(
        &mut h,
        &s,
        &b,
        sec_args(&b.pubkey(), &a.pubkey(), ONE, 1, 3),
    )
    .unwrap();

    let others: [&Keypair; 5] = [
        &s.keeper,
        &s.fund_admin,
        &s.compliance,
        &s.issuer,
        &stranger,
    ];
    for (i, who) in others.into_iter().enumerate() {
        let n = 10 + i as u8 * 3;
        let res = create(&mut h, &s, who, sub_args(&s, &a.pubkey(), ONE, n));
        assert_anchor_error(&res, DvpError::Unauthorized);
        let res = create(&mut h, &s, who, red_args(&s, &b.pubkey(), ONE, n + 1));
        assert_anchor_error(&res, DvpError::Unauthorized);
        let res = create(
            &mut h,
            &s,
            who,
            sec_args(&b.pubkey(), &a.pubkey(), ONE, 1, n + 2),
        );
        assert_anchor_error(&res, DvpError::Unauthorized);
    }
    // The transfer agent isn't a party to a secondary transfer.
    let res = create(
        &mut h,
        &s,
        &s.transfer_agent,
        sec_args(&b.pubkey(), &a.pubkey(), ONE, 1, 99),
    );
    assert_anchor_error(&res, DvpError::Unauthorized);
}

#[test]
fn same_creator_and_reference_twice_is_one_instruction() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1)).unwrap();
    // The address is the same, so Anchor's `init` fails (system program AccountAlreadyInUse).
    let res = create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), 2 * ONE, 1));
    assert_instruction_error(&res, InstructionError::Custom(0));
    // Another creator may use the same reference.
    create(
        &mut h,
        &s,
        &s.transfer_agent,
        sub_args(&s, &a.pubkey(), ONE, 1),
    )
    .expect("different creator");
    let cfg: DvpConfig = read(&h, &s.dvp_config);
    assert_eq!(cfg.next_seq, 3);
}

// ---- rejections, in contract order ----------------------------------------------------------

#[test]
fn accounts_of_another_fund_are_rejected() {
    let (mut h, s) = ready();
    let other = fixtures::launched_fund(&mut h, 0);
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let mut ix = ix_dvp::create_instruction(
        &a.pubkey(),
        &h.payer.pubkey(),
        &s.m(),
        sub_args(&s, &a.pubkey(), ONE, 1),
    );
    // Index 3 is `role_registry`.
    ix.accounts[3] =
        AccountMeta::new_readonly(mmf_sdk::pda::role_registry(&other.fund_state()).0, false);
    let res = h.send(&[ix], &[&a]);
    assert_anchor_error(&res, DvpError::FundMismatch);
}

#[test]
fn only_while_the_fund_is_open() {
    // A fund in Setup can be set up for settlement (T017) but takes no instructions.
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, SETTLE_DAY, 3_600);
    let (issuer, mint, cash_mint) = fixtures::create_standard_fund(&mut h);
    let m = mint.pubkey();
    let program = fixtures::token_program_of(&h, &cash_mint);
    let t = fixtures::create_cash_ata(&mut h, &issuer.pubkey(), &cash_mint);
    let ixs = [
        mmf_sdk::ix_fund::set_cash_treasury(&issuer.pubkey(), &m, &t, &cash_mint),
        mmf_sdk::ix_dvp::init_dvp(
            &issuer.pubkey(),
            &h.payer.pubkey(),
            &m,
            &cash_mint,
            &program,
            None,
        ),
    ];
    h.send(&ixs, &[&issuer]).expect("setup in Setup");
    let fs = mmf_sdk::pda::fund_state(&m).0;
    let args = CreateArgs {
        kind: settle::SUBSCRIPTION,
        seller: fs,
        buyer: issuer.pubkey(),
        units: ONE,
        cash_amount: 0,
        not_before_ts: 0,
        expires_at_ts: 0,
        external_ref: ext(1),
    };
    let ix = ix_dvp::create_instruction(&issuer.pubkey(), &h.payer.pubkey(), &m, args);
    let res = h.send(&[ix], &[&issuer]);
    assert_anchor_error(&res, DvpError::FundNotOpen);
}

#[test]
fn kind_and_where_the_fund_sits() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let b = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let fs = s.fund_state();
    let mut bad = Vec::new();
    let mut unknown = sub_args(&s, &a.pubkey(), ONE, 1);
    unknown.kind = 3;
    bad.push(unknown);
    // Subscription whose seller isn't the Fund; or whose buyer is the Fund.
    let mut x = sub_args(&s, &a.pubkey(), ONE, 2);
    x.seller = b.pubkey();
    bad.push(x);
    let mut x = sub_args(&s, &a.pubkey(), ONE, 3);
    x.buyer = fs;
    bad.push(x);
    // Redemption whose buyer isn't the Fund.
    let mut x = red_args(&s, &b.pubkey(), ONE, 4);
    x.buyer = a.pubkey();
    bad.push(x);
    // Secondary with the Fund on either side.
    bad.push(sec_args(&fs, &a.pubkey(), ONE, 1, 5));
    bad.push(sec_args(&b.pubkey(), &fs, ONE, 1, 6));
    for args in bad {
        let res = create(&mut h, &s, &a, args);
        assert_anchor_error(&res, DvpError::InvalidKind);
    }
}

#[test]
fn zero_units_zero_reference_and_same_party() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let b = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let res = create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), 0, 1));
    assert_anchor_error(&res, DvpError::ZeroUnits);
    let res = create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 0));
    assert_anchor_error(&res, DvpError::ZeroExternalRef);
    let res = create(
        &mut h,
        &s,
        &b,
        sec_args(&b.pubkey(), &b.pubkey(), ONE, 1, 2),
    );
    assert_anchor_error(&res, DvpError::SameParty);
}

#[test]
fn no_price_yet() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund_opts(&mut h, 0, false);
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let b = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let res = create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    assert_anchor_error(&res, DvpError::NoPrice);
    let res = create(&mut h, &s, &b, red_args(&s, &b.pubkey(), ONE, 2));
    assert_anchor_error(&res, DvpError::NoPrice);
    // A secondary transfer doesn't use the price (FR-008).
    create(
        &mut h,
        &s,
        &b,
        sec_args(&b.pubkey(), &a.pubkey(), ONE, 5, 3),
    )
    .expect("secondary needs no price");
}

#[test]
fn the_price_must_cover_yesterday_or_later() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);

    // Day after the covered day (Wednesday 20 740): fresh.
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1)).expect("ended yesterday");
    // Two days after (Thursday 20 741): stale.
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 2, 60);
    let res = create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 2));
    assert_anchor_error(&res, DvpError::StalePrice);

    // Friday 20 742: publish covering Wednesday through Sunday (catch-up plus the weekend).
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 3, 60);
    fixtures::publish(&mut h, &s, NAV_ONE, 0, 5).expect("publish through Sunday");
    // Monday 20 745 before the day's publish: the record ended yesterday, so it's fresh.
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 6, 60);
    create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 3)).expect("Monday morning");
    // Tuesday with no Monday publish: stale.
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 7, 60);
    let res = create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 4));
    assert_anchor_error(&res, DvpError::StalePrice);
}

#[test]
fn cash_amounts_round_in_the_funds_favour() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let b = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    // Next day, price $0.99999999 (within the 50 bp move limit).
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    fixtures::publish(&mut h, &s, 99_999_999, 0, 1).expect("publish");

    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), 1, 1));
    assert_eq!(
        settle::read_si(&h, &d).cash_amount,
        1,
        "1 base unit costs 1, rounded up"
    );
    assert_eq!(
        settle::read_si(&h, &d).nav_seq,
        2,
        "priced from the latest record"
    );
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), 1_000_001, 2));
    assert_eq!(settle::read_si(&h, &d).cash_amount, 1_000_001);
    let d = settle::created(&mut h, &s, &b, red_args(&s, &b.pubkey(), 1_000_001, 3));
    assert_eq!(
        settle::read_si(&h, &d).cash_amount,
        1_000_000,
        "rounded down"
    );

    let res = create(&mut h, &s, &b, red_args(&s, &b.pubkey(), 1, 4));
    assert_anchor_error(&res, DvpError::ZeroCash);
    let res = create(
        &mut h,
        &s,
        &b,
        sec_args(&b.pubkey(), &a.pubkey(), ONE, 0, 5),
    );
    assert_anchor_error(&res, DvpError::ZeroCash);
    let mut x = sub_args(&s, &a.pubkey(), ONE, 6);
    x.cash_amount = 5;
    let res = create(&mut h, &s, &a, x);
    assert_anchor_error(&res, DvpError::CashAmountNotAllowed);
}

#[test]
fn expiry_must_follow_now_and_valid_from() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    let now = clock::now(&h.svm);
    let mut x = sub_args(&s, &a.pubkey(), ONE, 1);
    x.expires_at_ts = now;
    assert_anchor_error(&create(&mut h, &s, &a, x), DvpError::InvalidExpiry);
    let mut x = sub_args(&s, &a.pubkey(), ONE, 2);
    x.not_before_ts = now + 100;
    x.expires_at_ts = now + 100;
    assert_anchor_error(&create(&mut h, &s, &a, x), DvpError::InvalidExpiry);
    x.expires_at_ts = now + 101;
    create(&mut h, &s, &a, x).expect("one second after valid-from");
}

#[test]
fn every_investor_party_must_be_eligible() {
    let (mut h, s) = ready();
    let creator_ok = fixtures::investor_with_units(&mut h, &s, ONE, 0);
    let m = s.m();
    let c = s.compliance.pubkey();

    // Pending: registered, never approved.
    let pending = h.funded_keypair();
    let p = pending.pubkey();
    h.send(&[ix_hook::register_holder(&p, &p, &m, &p, 0)], &[&pending])
        .unwrap();
    let res = create(&mut h, &s, &pending, sub_args(&s, &p, ONE, 1));
    assert_anchor_error(&res, DvpError::PartyNotEligible);

    // Suspended, and revoked.
    let sus = fixtures::investor_with_cash(&mut h, &s, 0);
    let rev = fixtures::investor_with_cash(&mut h, &s, 0);
    let reason = ReasonCode::SanctionsHit as u8;
    h.send(
        &[
            ix_governance::suspend_holder(&c, &m, &sus.pubkey(), reason, CASE),
            ix_governance::revoke_holder(&c, &m, &rev.pubkey(), reason, CASE),
        ],
        &[&s.compliance],
    )
    .unwrap();
    for who in [&sus, &rev] {
        let res = create(&mut h, &s, who, sub_args(&s, &who.pubkey(), ONE, 2));
        assert_anchor_error(&res, DvpError::PartyNotEligible);
    }
    // The counterparty is checked too: an eligible seller with a suspended buyer.
    let res = create(
        &mut h,
        &s,
        &creator_ok,
        sec_args(&creator_ok.pubkey(), &sus.pubkey(), ONE, 1, 3),
    );
    assert_anchor_error(&res, DvpError::PartyNotEligible);

    // KYC expired: approved with a statement valid for one hour, then two hours pass.
    let short = h.funded_keypair();
    let o = short.pubkey();
    h.send(&[ix_hook::register_holder(&o, &o, &m, &o, 0)], &[&short])
        .unwrap();
    let signed = fixtures::signed_statement(&s, &o, *b"US", clock::now(&h.svm) + 3_600);
    let ixs = ix_hook::onboarding_tx(&h.payer.pubkey(), &c, &m, &o, &signed);
    h.send(&ixs, &[&s.compliance]).unwrap();
    clock::advance(&mut h.svm, 7_200);
    let res = create(&mut h, &s, &short, sub_args(&s, &o, ONE, 4));
    assert_anchor_error(&res, DvpError::PartyNotEligible);

    // A fund-operated holder (the treasury) is never a party.
    let treasury = fixtures::onboard_treasury(&mut h, &s);
    let res = create(
        &mut h,
        &s,
        &s.transfer_agent,
        sub_args(&s, &treasury, ONE, 5),
    );
    assert_anchor_error(&res, DvpError::PartyNotEligible);

    // A party with no holder record at all.
    let nobody = Pubkey::new_unique();
    let res = create(&mut h, &s, &s.transfer_agent, sub_args(&s, &nobody, ONE, 6));
    assert_anchor_error(&res, DvpError::PartyNotEligible);
}

#[test]
fn cash_that_does_not_fit_is_an_overflow() {
    // At $1.0001, u64::MAX units would cost more than u64::MAX cash base units.
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 0);
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    fixtures::publish(&mut h, &s, 100_010_000, 0, 1).unwrap();
    let res = create(&mut h, &s, &a, sub_args(&s, &a.pubkey(), u64::MAX, 1));
    assert_anchor_error(&res, DvpError::Overflow);
}
