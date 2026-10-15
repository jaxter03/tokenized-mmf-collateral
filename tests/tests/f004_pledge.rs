//! F004 US3, pledge without moving units (FR-021, FR-022, FR-007, SC-001; contracts/collateral.md
//! `pledge`). T038: written before the code.

use collateral::events::Pledged;
use collateral::{CollateralError, ProposeArgs};
use compliance_hook::HookError;
use mmf_common::enums::{CallStatus, PledgeStatus};
use solana_signer::Signer;
use solana_transaction::InstructionError;
use tests::assert::{assert_anchor_error, assert_instruction_error};
use tests::collateral::{
    age_price, agr, agreement_of, call_status, check, exposure, lock_of, pledge, pledge_of,
    pledge_on, propose, publish_next_day, r, sign, suspend, Tracker,
};
use tests::events::one_cpi_event;
use tests::fixtures::{self, worked_terms, CollatCtx, Ready, GIVER_UNITS};
use tests::{clock, units, Harness};

const MILLION: u64 = 1_000_000_000_000;
const UNITS_FOR_MILLION: u64 = 1_020_408_163_266;

fn ready(ready: Ready) -> (Harness, CollatCtx, Tracker) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund_opts(&mut h, ready);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    (h, c, t)
}

/// Every unit and cash balance the fixture knows of, plus the unit supply.
fn balances(h: &Harness, c: &CollatCtx) -> Vec<u64> {
    let m = c.m();
    vec![
        units::balance(h, &m, &c.giver.pubkey()),
        units::balance(h, &m, &c.taker.pubkey()),
        units::token_balance(h, &c.unit_escrow),
        units::supply(h, &m),
        fixtures::cash_balance(h, &c.cash_treasury),
        fixtures::cash_balance(h, &c.cash_escrow),
    ]
}

#[test]
fn us3_one_pledge_meets_the_call_and_moves_nothing() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M call");
    let call = t.call(c.agr.call(&r(1)));
    let before = balances(&h, &c);

    let now = clock::now(&h.svm);
    let meta = pledge(&mut h, &c, &c.giver, UNITS_FOR_MILLION, r(9)).expect("pledge");
    t.pledge(c.agr.pledge(&r(9)));
    assert!(
        meta.compute_units_consumed <= 80_000,
        "used {}",
        meta.compute_units_consumed
    );
    // Research R14: `set_lien` costs about 13 000 CU plus about 1 500 per bump-search step of
    // the holder's ATA address (address-dependent; 13 141–23 641 over 40 runs). The bound leaves
    // room for 17 extra steps; the deepest call is the third level.
    let hook = mmf_common::ids::COMPLIANCE_HOOK_PROGRAM_ID.to_string();
    let lien_cu: u64 = meta
        .logs
        .iter()
        .find_map(|l| {
            let rest = l.strip_prefix(&format!("Program {hook} consumed "))?;
            rest.split(' ').next()?.parse().ok()
        })
        .expect("set_lien ran");
    assert!(lien_cu <= 40_000, "set_lien used {lien_cu}");
    assert!(
        !meta.logs.iter().any(|l| l.contains("invoke [4]")),
        "call depth ≤ 3"
    );

    assert_eq!(
        balances(&h, &c),
        before,
        "no unit or cash balance changed (SC-001)"
    );
    assert_eq!(units::balance(&h, &c.m(), &c.giver.pubkey()), GIVER_UNITS);
    assert_eq!(
        lock_of(&h, &c.m(), &c.giver.pubkey()),
        (UNITS_FOR_MILLION, 1)
    );
    assert_eq!(
        call_status(&h, &call),
        CallStatus::Met,
        "met in the same transaction"
    );

    let p = pledge_of(&h, &c, r(9));
    assert_eq!(
        (p.agreement, p.seq, p.pledge_ref, p.units),
        (c.agreement(), 1, r(9), UNITS_FOR_MILLION)
    );
    assert_eq!((p.posted_at, p.nav_seq_at_post), (now, 1));
    assert_eq!((p.released_units, p.enforced_units, p.closed_at), (0, 0, 0));
    assert_eq!(p.status, PledgeStatus::Active.as_u8());
    let a = agreement_of(&h, &c);
    assert_eq!((a.pledged_units, a.pledge_count), (UNITS_FOR_MILLION, 2));

    let ev: Pledged = one_cpi_event(&meta);
    assert_eq!(
        (ev.pledge, ev.seq, ev.units, ev.giver),
        (c.agr.pledge(&r(9)), 1, UNITS_FOR_MILLION, c.giver.pubkey())
    );
    assert_eq!(ev.giver_locked_after, UNITS_FOR_MILLION);
    assert!(ev.snapshot.valued);
    assert_eq!(ev.snapshot.coverage, 0);
    assert_eq!(ev.snapshot.met_below_seq, 2);
    t.check(&h);
}

#[test]
fn pledge_rejections() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    assert_anchor_error(
        &pledge(&mut h, &c, &c.giver, 0, r(1)),
        CollateralError::ZeroUnits,
    );
    assert_anchor_error(
        &pledge(&mut h, &c, &c.giver, 1, [0; 32]),
        CollateralError::ZeroPledgeRef,
    );
    assert_anchor_error(
        &pledge(&mut h, &c, &c.taker, 1, r(2)),
        CollateralError::Unauthorized,
    );
    let keeper = c.keeper.insecure_clone();
    assert_anchor_error(
        &pledge(&mut h, &c, &keeper, 1, r(3)),
        CollateralError::Unauthorized,
    );

    // Unlocked units: one base unit more than the balance fails; exactly the balance passes.
    assert_anchor_error(
        &pledge(&mut h, &c, &c.giver, GIVER_UNITS + 1, r(4)),
        HookError::InsufficientUnlockedUnits,
    );
    pledge(&mut h, &c, &c.giver, GIVER_UNITS, r(5)).expect("exactly the unlocked units");
    t.pledge(c.agr.pledge(&r(5)));
    assert_anchor_error(
        &pledge(&mut h, &c, &c.giver, 1, r(6)),
        HookError::InsufficientUnlockedUnits,
    );
    t.check(&h);

    // The same reference twice: Anchor's `init` refuses the existing account.
    assert_instruction_error(
        &pledge(&mut h, &c, &c.giver, 1, r(5)),
        InstructionError::Custom(0),
    );
}

#[test]
fn pledge_limit_is_per_agreement() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    // A second agreement with a 1 500 000-unit limit.
    let args = ProposeArgs {
        concentration_cap_units: 1_500_000_000_000,
        ..worked_terms(&c.giver.pubkey(), &c.taker.pubkey(), 2)
    };
    let small = agr(&c, 2);
    propose(&mut h, &c.m(), &c.taker, args).expect("propose");
    sign(&mut h, &c.giver, &small).expect("sign");
    t.agreement(small.agreement());

    pledge_on(&mut h, &small, &c.giver, 1_000_000_000_000, r(1)).expect("first");
    t.pledge(small.pledge(&r(1)));
    assert_anchor_error(
        &pledge_on(&mut h, &small, &c.giver, 500_000_000_001, r(2)),
        CollateralError::PledgeLimitExceeded,
    );
    pledge_on(&mut h, &small, &c.giver, 500_000_000_000, r(3)).expect("exactly the limit");
    t.pledge(small.pledge(&r(3)));
    t.check(&h);
}

#[test]
fn suspended_giver_cannot_pledge() {
    let (mut h, c, _) = ready(Ready::Standard);
    suspend(&mut h, &c, &c.giver.pubkey());
    assert_anchor_error(
        &pledge(&mut h, &c, &c.giver, 1, r(1)),
        CollateralError::PartyNotEligible,
    );
}

#[test]
fn pledge_on_a_stale_price_locks_and_waits_for_a_check() {
    let (mut h, c, mut t) = ready(Ready::Standard);
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M call");
    let call = t.call(c.agr.call(&r(1)));
    age_price(&mut h, &c, 93_601);
    let meta =
        pledge(&mut h, &c, &c.giver, UNITS_FOR_MILLION, r(9)).expect("pledge on a stale price");
    t.pledge(c.agr.pledge(&r(9)));
    let ev: Pledged = one_cpi_event(&meta);
    assert!(!ev.snapshot.valued);
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()).0, UNITS_FOR_MILLION);
    assert_eq!(
        call_status(&h, &call),
        CallStatus::Open,
        "not observed without a fresh price"
    );
    t.check(&h);

    publish_next_day(&mut h, &c, 100_000_000);
    let anyone = h.funded_keypair();
    check(&mut h, &c, &anyone).expect("check after the publish");
    assert_eq!(call_status(&h, &call), CallStatus::Met);
    t.check(&h);
}

#[test]
fn proposed_agreement_takes_no_pledge() {
    let (mut h, c, _) = ready(Ready::ProposedOnly);
    assert_anchor_error(
        &pledge(&mut h, &c, &c.giver, 1, r(1)),
        CollateralError::AgreementNotActive,
    );
}
