//! F004 US6, dispute (FR-020 for Disputed calls, FR-031–FR-033; contracts/collateral.md
//! `dispute_call`, `withdraw_dispute`, `resolve_dispute`; contracts/governance.md
//! `resolve_call_dispute`; research R10). T062: written before the code.

use anchor_lang::prelude::Pubkey;
use collateral::events::{CallDisputed, DisputeResolved};
use collateral::CollateralError;
use governance::GovernanceError;
use mmf_common::enums::{CallStatus, ReasonCode};
use mmf_sdk::{ix_collateral, ix_fund, ix_governance, ix_hook, pda};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_squads_error};
use tests::collateral::{
    agreement_of, call_of, call_status, cancel, exposure, issue, pledge, r, Tracker,
};
use tests::events::{cpi_events, one_cpi_event};
use tests::fixtures::{self, CollatCtx, NAV_ONE, SETTLE_DAY};
use tests::squads::{self, INVALID_PROPOSAL_STATUS};
use tests::{clock, units, Harness, TxResult};

const MILLION: u64 = 1_000_000_000_000;
const CASE: [u8; 32] = [0xC5; 32];
const OPERATIONAL: u8 = ReasonCode::OperationalError as u8;
const OPEN: u8 = 0;
const CANCELLED: u8 = 5;

fn with_call() -> (Harness, CollatCtx, Tracker, Pubkey) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    exposure(&mut h, &c, &c.taker, MILLION, r(1)).expect("$1M call");
    let call = t.call(c.agr.call(&r(1)));
    (h, c, t, call)
}

fn dispute(h: &mut Harness, c: &CollatCtx, signer: &Keypair, call: &Pubkey, n: u8) -> TxResult {
    let ix = ix_collateral::dispute_call(&signer.pubkey(), &c.agr, call, r(n));
    h.send(&[ix], &[signer])
}

fn withdraw(h: &mut Harness, c: &CollatCtx, signer: &Keypair, call: &Pubkey) -> TxResult {
    let ix = ix_collateral::withdraw_dispute(&signer.pubkey(), &c.agr, call);
    h.send(&[ix], &[signer])
}

fn resolve_as(
    h: &mut Harness,
    c: &CollatCtx,
    who: &Keypair,
    call: &Pubkey,
    outcome: u8,
    reason: u8,
) -> TxResult {
    let ix = ix_governance::resolve_call_dispute(&who.pubkey(), &c.m(), &c.agreement(), call, outcome, reason, CASE);
    h.send(&[ix], &[who])
}

fn resolve(h: &mut Harness, c: &CollatCtx, call: &Pubkey, outcome: u8, reason: u8) -> TxResult {
    let compliance = c.compliance.insecure_clone();
    resolve_as(h, c, &compliance, call, outcome, reason)
}

#[test]
fn us6_dispute_withdraw_dispute_and_compliance_cancels() {
    let (mut h, c, mut t, call) = with_call();
    let before = call_of(&h, &c, r(1));
    let now = clock::now(&h.svm);
    let meta = dispute(&mut h, &c, &c.giver, &call, 7).expect("dispute");
    let ev: CallDisputed = one_cpi_event(&meta);
    assert_eq!((ev.call, ev.seq, ev.dispute_ref), (call, 1, r(7)));
    let after = call_of(&h, &c, r(1));
    assert_eq!(after.status, CallStatus::Disputed.as_u8());
    assert_eq!((after.dispute_ref, after.disputed_at), (r(7), now));
    assert_eq!(
        (after.cure_deadline_ts, after.enforce_after_ts),
        (before.cure_deadline_ts, before.enforce_after_ts),
        "a dispute moves no deadline"
    );
    t.check(&h);

    withdraw(&mut h, &c, &c.giver, &call).expect("withdraw");
    assert_eq!(call_status(&h, &call), CallStatus::Open);
    dispute(&mut h, &c, &c.giver, &call, 8).expect("dispute again");

    let meta = resolve(&mut h, &c, &call, CANCELLED, OPERATIONAL).expect("compliance cancels");
    let ev: DisputeResolved = one_cpi_event(&meta);
    assert_eq!((ev.call, ev.outcome, ev.reason, ev.case_ref_hash), (call, CANCELLED, OPERATIONAL, CASE));
    assert_eq!(ev.actor, c.compliance.pubkey());
    assert_eq!(call_status(&h, &call), CallStatus::Cancelled);
    let a = agreement_of(&h, &c);
    assert_eq!((a.open_delivery_calls, a.called_outstanding), (0, 0));
    t.check(&h);

    // The shortfall is uncalled again: the taker calls it afresh.
    issue(&mut h, &c, &c.taker, 0, r(2)).expect("new call");
    t.call(c.agr.call(&r(2)));
    assert_eq!(call_of(&h, &c, r(2)).value_delta, MILLION);
    t.check(&h);
}

#[test]
fn dispute_rejections() {
    let (mut h, c, mut t, call) = with_call();
    assert_anchor_error(&dispute(&mut h, &c, &c.taker, &call, 7), CollateralError::Unauthorized);
    assert_anchor_error(&dispute(&mut h, &c, &c.giver, &call, 0), CollateralError::ZeroExternalRef);
    assert_anchor_error(&withdraw(&mut h, &c, &c.giver, &call), CollateralError::NotDisputed);
    dispute(&mut h, &c, &c.giver, &call, 7).expect("dispute");
    assert_anchor_error(&dispute(&mut h, &c, &c.giver, &call, 8), CollateralError::CallNotOpen);
    assert_anchor_error(&withdraw(&mut h, &c, &c.taker, &call), CollateralError::Unauthorized);

    // The taker can cancel a Disputed call.
    cancel(&mut h, &c, &c.taker, r(1)).expect("taker cancels the disputed call");
    assert_eq!(call_status(&h, &call), CallStatus::Cancelled);
    assert_eq!(agreement_of(&h, &c).open_delivery_calls, 0);
    assert_anchor_error(&dispute(&mut h, &c, &c.giver, &call, 9), CollateralError::CallNotOpen);
    t.check(&h);

    // Return calls can't be disputed.
    pledge(&mut h, &c, &c.giver, MILLION, r(10)).expect("pledge");
    t.pledge(c.agr.pledge(&r(10)));
    exposure(&mut h, &c, &c.taker, 0, r(3)).expect("no exposure");
    issue(&mut h, &c, &c.giver, 1, r(4)).expect("return call");
    let ret = t.call(c.agr.call(&r(4)));
    assert_anchor_error(&dispute(&mut h, &c, &c.giver, &ret, 9), CollateralError::WrongCallKind);
    t.check(&h);
}

#[test]
fn a_met_call_cannot_be_disputed() {
    let (mut h, c, mut t, call) = with_call();
    pledge(&mut h, &c, &c.giver, 1_020_408_163_266, r(10)).expect("cover");
    t.pledge(c.agr.pledge(&r(10)));
    assert_anchor_error(&dispute(&mut h, &c, &c.giver, &call, 7), CollateralError::CallNotOpen);
    assert_anchor_error(&resolve(&mut h, &c, &call, OPEN, OPERATIONAL), CollateralError::NotDisputed);
}

#[test]
fn compliance_reopens_with_the_original_deadlines() {
    let (mut h, c, t, call) = with_call();
    let before = call_of(&h, &c, r(1));
    dispute(&mut h, &c, &c.giver, &call, 7).expect("dispute");
    assert_eq!(call_status(&h, &call), CallStatus::Disputed);
    resolve(&mut h, &c, &call, OPEN, OPERATIONAL).expect("reopen");
    let after = call_of(&h, &c, r(1));
    assert_eq!(after.status, CallStatus::Open.as_u8());
    assert_eq!(after.cure_deadline_ts, before.cure_deadline_ts);
    assert_eq!(agreement_of(&h, &c).open_delivery_calls, 1);
    t.check(&h);
}

#[test]
fn rejected_resolutions() {
    let (mut h, c, _, call) = with_call();
    // Not disputed yet.
    assert_anchor_error(&resolve(&mut h, &c, &call, OPEN, OPERATIONAL), CollateralError::NotDisputed);
    dispute(&mut h, &c, &c.giver, &call, 7).expect("dispute");
    for outcome in [1, 3] {
        assert_anchor_error(&resolve(&mut h, &c, &call, outcome, OPERATIONAL), CollateralError::InvalidOutcome);
    }
    for reason in [4, 5, 7] {
        assert_anchor_error(&resolve(&mut h, &c, &call, CANCELLED, reason), GovernanceError::InvalidReason);
    }
    let others = [
        c.issuer.insecure_clone(),
        c.fund_admin.insecure_clone(),
        c.transfer_agent.insecure_clone(),
        c.keeper.insecure_clone(),
        c.auditor.insecure_clone(),
        c.giver.insecure_clone(),
        c.taker.insecure_clone(),
    ];
    for who in &others {
        assert_anchor_error(&resolve_as(&mut h, &c, who, &call, CANCELLED, OPERATIONAL), GovernanceError::Unauthorized);
    }
    // Compliance calling collateral directly, not through governance.
    let compliance = c.compliance.insecure_clone();
    let ix = ix_collateral::resolve_dispute_direct(&compliance.pubkey(), &c.agr, &call, CANCELLED, OPERATIONAL, CASE);
    assert_anchor_error(&h.send(&[ix], &[&compliance]), CollateralError::UnauthorizedCaller);
    assert_eq!(call_status(&h, &call), CallStatus::Disputed, "still disputed");
}

#[test]
fn a_call_of_another_funds_agreement_is_refused() {
    let (mut h, c, _, _) = with_call();
    let other = fixtures::collateral_ready_fund(&mut h);
    exposure(&mut h, &other, &other.taker, MILLION, r(1)).expect("call on the other fund");
    let foreign = other.agr.call(&r(1));
    dispute(&mut h, &other, &other.giver, &foreign, 7).expect("dispute there");
    // This fund's compliance, with the other fund's agreement and call.
    let ix = ix_governance::resolve_call_dispute(
        &c.compliance.pubkey(),
        &c.m(),
        &other.agreement(),
        &foreign,
        CANCELLED,
        OPERATIONAL,
        CASE,
    );
    assert_anchor_error(&h.send(&[ix], &[&c.compliance]), CollateralError::FundMismatch);
}

#[test]
fn a_disputed_call_still_counts_and_is_met_when_covered() {
    let (mut h, c, mut t, call) = with_call();
    dispute(&mut h, &c, &c.giver, &call, 7).expect("dispute");
    assert_eq!(call_status(&h, &call), CallStatus::Disputed);
    assert_anchor_error(&issue(&mut h, &c, &c.taker, 0, r(2)), CollateralError::NothingToCall);
    assert_eq!(agreement_of(&h, &c).called_outstanding, MILLION);
    pledge(&mut h, &c, &c.giver, 1_020_408_163_266, r(10)).expect("cover");
    t.pledge(c.agr.pledge(&r(10)));
    assert_eq!(call_status(&h, &call), CallStatus::Met);
    t.check(&h);
}

// ---- compliance as a Squads 2-of-3 vault (Squads → governance → collateral → emit_cpi) ------

#[test]
fn a_multisig_compliance_resolves_only_at_its_threshold() {
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, SETTLE_DAY, 3_600);
    let mut ms = squads::create(&mut h, 2, 3);
    h.airdrop(&ms.vault, 7_000_000_000);
    let cash_mint = fixtures::create_cash_mint(&mut h, fixtures::CASH_DECIMALS);
    let terms = fund::FundTerms {
        max_nav_move_bps: fixtures::COLLATERAL_MAX_MOVE_BPS,
        ..fixtures::standard_terms(cash_mint)
    };
    let (issuer, mint, _) = fixtures::create_fund_with_terms(&mut h, terms);
    let (m, i, v) = (mint.pubkey(), issuer.pubkey(), ms.vault);
    let admin = h.funded_keypair();
    let provider = Keypair::new();
    let roles = ix_governance::Roles {
        fund_admin: admin.pubkey(),
        transfer_agent: Pubkey::new_unique(),
        compliance: v,
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    };
    let providers = [provider.pubkey(), Pubkey::default(), Pubkey::default(), Pubkey::default()];
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
    h.send(&[ix_fund::publish_daily(&admin.pubkey(), &h.payer.pubkey(), &m, 1, NAV_ONE, 0, 1)], &[&admin])
        .unwrap();

    // The vault approves the giver and the taker.
    let p = h.payer.pubkey();
    let parties = [h.funded_keypair(), h.funded_keypair()];
    for party in &parties {
        let o = party.pubkey();
        h.send(&[ix_hook::register_holder(&o, &o, &m, &o, 0)], &[party]).unwrap();
        let stmt = mmf_sdk::kyc::sign(
            &provider,
            &mmf_sdk::kyc::statement(m, o, *b"US", 1, clock::now(&h.svm) + fixtures::YEAR),
        );
        squads::squads_execute(
            &mut h,
            &mut ms,
            &[ix_hook::approve_investor(&v, &m, &o, stmt.attestation_hash)],
            2,
            std::slice::from_ref(&stmt.ix),
        )
        .expect("approve via vault");
        h.send(
            &[
                ix_hook::create_holding_account(&p, &m, &o),
                ix_hook::thaw_holding_account(&p, &m, &o, &pda::holding_account(&o, &m)),
            ],
            &[],
        )
        .unwrap();
    }
    let [giver, taker] = parties;
    units::seed_units(&mut h, &m, &giver.pubkey(), fixtures::GIVER_UNITS);

    // The worked agreement, a $1M call, and the giver's dispute.
    let a = ix_collateral::Agr { mint: m, giver: giver.pubkey(), taker: taker.pubkey(), nonce: 1 };
    tests::collateral::propose(&mut h, &m, &taker, fixtures::worked_terms(&a.giver, &a.taker, 1)).unwrap();
    tests::collateral::sign(&mut h, &giver, &a).unwrap();
    let ix = ix_collateral::update_exposure(&taker.pubkey(), &p, &a, MILLION, r(1));
    h.send(&[ix], &[&taker]).unwrap();
    let call = a.call(&r(1));
    h.send(&[ix_collateral::dispute_call(&giver.pubkey(), &a, &call, r(7))], &[&giver]).unwrap();

    // One approval: nothing happens. Two: cancelled, recorded with the vault as actor.
    let resolve = ix_governance::resolve_call_dispute(&v, &m, &a.agreement(), &call, CANCELLED, OPERATIONAL, CASE);
    let (index, remaining) = ms.propose(&mut h, &[resolve]);
    ms.approve(&mut h, index, 0).unwrap();
    assert_squads_error(&ms.execute(&mut h, index, &remaining, &[]), INVALID_PROPOSAL_STATUS);
    let stored: collateral::CollateralCall = fixtures::read(&h, &call);
    assert_eq!(stored.status, CallStatus::Disputed.as_u8());
    ms.approve(&mut h, index, 1).unwrap();
    let meta = ms.execute(&mut h, index, &remaining, &[]).expect("executes at threshold");
    let stored: collateral::CollateralCall = fixtures::read(&h, &call);
    assert_eq!(stored.status, CallStatus::Cancelled.as_u8());
    assert_eq!(cpi_events::<DisputeResolved>(&meta)[0].actor, v);
    // Squads → governance → collateral → emit_cpi: four levels, the runtime's limit.
    assert!(meta.logs.iter().any(|l| l.contains("invoke [4]")));
    assert!(!meta.logs.iter().any(|l| l.contains("invoke [5]")));
}
