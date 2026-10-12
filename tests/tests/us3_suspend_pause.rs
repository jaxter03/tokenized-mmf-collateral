//! US3 — Suspend, revoke and pause (FR-018, FR-021, FR-027 to FR-034). Independent test: suspend a
//! holder and show it can't send or receive; reinstate it and show it can; pause and show no
//! transfer succeeds; unpause and show transfers work again.

use anchor_lang::prelude::Pubkey;
use compliance_hook::{HolderState, HookError};
use governance::GovernanceError;
use mmf_common::enums::{HolderStatus, ReasonCode};
use mmf_sdk::{ix_governance, ix_hook, pda};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::fixtures::{self, FundCtx};
use tests::{clock, units, Harness, TxResult};

const SANCTIONS: u8 = ReasonCode::SanctionsHit as u8;
const CASE: [u8; 32] = [0xC5; 32];

fn suspend(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey) -> TxResult {
    let ix =
        ix_governance::suspend_holder(&ctx.compliance.pubkey(), &ctx.m(), owner, SANCTIONS, CASE);
    h.send(&[ix], &[&ctx.compliance])
}

fn reinstate(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey) -> TxResult {
    let ix =
        ix_governance::reinstate_holder(&ctx.compliance.pubkey(), &ctx.m(), owner, SANCTIONS, CASE);
    h.send(&[ix], &[&ctx.compliance])
}

fn revoke(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey) -> TxResult {
    let ix =
        ix_governance::revoke_holder(&ctx.compliance.pubkey(), &ctx.m(), owner, SANCTIONS, CASE);
    h.send(&[ix], &[&ctx.compliance])
}

fn status(h: &Harness, ctx: &FundCtx, owner: &Pubkey) -> u8 {
    fixtures::read::<HolderState>(h, &pda::holder(&ctx.m(), owner).0).status
}

/// Alice holds 1 000 units; Bob is an approved investor with none.
fn two_investors() -> (Harness, FundCtx, Keypair, Keypair) {
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    (h, ctx, alice, bob)
}

// ---------------------------------------------------------------------------------------------
// T060: suspend, reinstate, revoke (FR-027 to FR-029, US3-1 to US3-3)
// ---------------------------------------------------------------------------------------------

#[test]
fn a_suspended_holder_can_neither_send_nor_receive() {
    // US3-1, FR-027.
    let (mut h, ctx, alice, bob) = two_investors();
    suspend(&mut h, &ctx, &bob.pubkey()).expect("suspend");
    assert_eq!(
        status(&h, &ctx, &bob.pubkey()),
        HolderStatus::Suspended.as_u8()
    );
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::ReceiverNotApproved,
    );

    suspend(&mut h, &ctx, &alice.pubkey()).expect("suspend sender");
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::SenderNotApproved,
    );
    assert_eq!(units::balance(&h, &ctx.m(), &alice.pubkey()), 1_000);
}

#[test]
fn reinstated_holder_transacts_again() {
    // US3-2, FR-028.
    let (mut h, ctx, alice, bob) = two_investors();
    suspend(&mut h, &ctx, &bob.pubkey()).unwrap();
    assert_eq!(
        status(&h, &ctx, &bob.pubkey()),
        HolderStatus::Suspended.as_u8()
    );
    reinstate(&mut h, &ctx, &bob.pubkey()).expect("reinstate");
    assert_eq!(
        status(&h, &ctx, &bob.pubkey()),
        HolderStatus::Approved.as_u8()
    );
    fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 10).expect("receive again");
    fixtures::transfer(&mut h, &ctx, &bob, &alice.pubkey(), 5).expect("send again");
}

#[test]
fn kyc_expiry_still_applies_after_reinstatement() {
    // FR-028.
    let (mut h, ctx, alice, bob) = two_investors();
    suspend(&mut h, &ctx, &bob.pubkey()).unwrap();
    assert_eq!(
        status(&h, &ctx, &bob.pubkey()),
        HolderStatus::Suspended.as_u8()
    );
    clock::advance(&mut h.svm, fixtures::YEAR + 1); // both statements expire
    reinstate(&mut h, &ctx, &bob.pubkey()).expect("reinstate is allowed");
    assert_eq!(
        status(&h, &ctx, &bob.pubkey()),
        HolderStatus::Approved.as_u8()
    );
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::SenderNotApproved,
    );
}

#[test]
fn revocation_is_final() {
    // US3-3, FR-029: from Pending, Approved and Suspended.
    let (mut h, ctx, alice, bob) = two_investors();
    let m = ctx.m();
    let pending = h.funded_keypair();
    let p = pending.pubkey();
    h.send(&[ix_hook::register_holder(&p, &p, &m, &p, 0)], &[&pending])
        .unwrap();
    let carol = fixtures::new_investor(&mut h, &ctx);
    suspend(&mut h, &ctx, &carol.pubkey()).unwrap();
    for owner in [p, bob.pubkey(), carol.pubkey()] {
        revoke(&mut h, &ctx, &owner).expect("revoke");
        assert_eq!(status(&h, &ctx, &owner), HolderStatus::Revoked.as_u8());
    }

    let b = bob.pubkey();
    let signed = fixtures::signed_statement(&ctx, &b, *b"US", clock::now(&h.svm) + fixtures::YEAR);
    let c = ctx.compliance.pubkey();
    let approve = h.send(
        &[
            signed.ix.clone(),
            ix_hook::approve_investor(&c, &m, &b, signed.attestation_hash),
        ],
        &[&ctx.compliance],
    );
    assert_anchor_error(&approve, HookError::HolderRevoked);
    let renew = h.send(
        &[
            signed.ix.clone(),
            ix_hook::renew_investor(&c, &m, &b, signed.attestation_hash),
        ],
        &[&ctx.compliance],
    );
    assert_anchor_error(&renew, HookError::HolderRevoked);
    assert_anchor_error(&reinstate(&mut h, &ctx, &b), HookError::HolderRevoked);
    assert_anchor_error(&revoke(&mut h, &ctx, &b), HookError::HolderRevoked);
    let again = h.send(&[ix_hook::register_holder(&b, &b, &m, &b, 0)], &[&bob]);
    assert_anchor_error(&again, HookError::HolderAlreadyRegistered);

    // A revoked holder's units stay where they are and can't move.
    revoke(&mut h, &ctx, &alice.pubkey()).unwrap();
    let dave = fixtures::new_investor(&mut h, &ctx);
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &dave.pubkey(), 1),
        HookError::SenderNotApproved,
    );
    assert_eq!(units::balance(&h, &m, &alice.pubkey()), 1_000);
}

#[test]
fn status_changes_follow_the_transition_table() {
    let (mut h, ctx, _alice, bob) = two_investors();
    let pending = h.funded_keypair();
    let p = pending.pubkey();
    h.send(
        &[ix_hook::register_holder(&p, &p, &ctx.m(), &p, 0)],
        &[&pending],
    )
    .unwrap();

    assert_anchor_error(&suspend(&mut h, &ctx, &p), HookError::InvalidHolderStatus); // Pending
    assert_anchor_error(
        &reinstate(&mut h, &ctx, &bob.pubkey()),
        HookError::InvalidHolderStatus,
    ); // Approved
    suspend(&mut h, &ctx, &bob.pubkey()).unwrap();
    assert_anchor_error(
        &suspend(&mut h, &ctx, &bob.pubkey()),
        HookError::InvalidHolderStatus,
    ); // Suspended
}

#[test]
fn only_compliance_acts_and_only_on_an_open_fund() {
    let (mut h, ctx, _alice, bob) = two_investors();
    let ix = ix_governance::suspend_holder(
        &ctx.issuer.pubkey(),
        &ctx.m(),
        &bob.pubkey(),
        SANCTIONS,
        CASE,
    );
    assert_anchor_error(
        &h.send(&[ix], &[&ctx.issuer]),
        GovernanceError::Unauthorized,
    );

    // A fund still in Setup (roles assigned, not launched).
    let mut h = Harness::new();
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    let compliance = h.funded_keypair();
    let m = mint.pubkey();
    let roles = ix_governance::Roles {
        fund_admin: Pubkey::new_unique(),
        transfer_agent: Pubkey::new_unique(),
        compliance: compliance.pubkey(),
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    };
    h.send(
        &[
            ix_hook::init_registry(&issuer.pubkey(), &m),
            ix_governance::set_roles(&issuer.pubkey(), &m, &roles),
        ],
        &[&issuer],
    )
    .unwrap();
    let ix = ix_governance::pause_transfers(&compliance.pubkey(), &m, SANCTIONS, CASE);
    assert_anchor_error(&h.send(&[ix], &[&compliance]), GovernanceError::FundNotOpen);
}

// ---------------------------------------------------------------------------------------------
// T061: reason codes, pause / unpause, admin while paused, governance-only entry points
// (FR-021, FR-030, FR-031)
// ---------------------------------------------------------------------------------------------

fn pause(h: &mut Harness, ctx: &FundCtx) -> TxResult {
    let ix = ix_governance::pause_transfers(&ctx.compliance.pubkey(), &ctx.m(), SANCTIONS, CASE);
    h.send(&[ix], &[&ctx.compliance])
}

fn unpause(h: &mut Harness, ctx: &FundCtx) -> TxResult {
    let ix = ix_governance::unpause_transfers(
        &ctx.compliance.pubkey(),
        &ctx.m(),
        ReasonCode::OperationalError as u8,
        CASE,
    );
    h.send(&[ix], &[&ctx.compliance])
}

#[test]
fn every_action_needs_a_reason_from_the_f001_list() {
    // FR-031: CourtOrder, SanctionsHit, LostKey, OperationalError, Other — not CollateralDefault
    // (4), IssuerWindDown (5) or unknown codes.
    let (mut h, ctx, _alice, bob) = two_investors();
    let (c, m, b) = (ctx.compliance.pubkey(), ctx.m(), bob.pubkey());
    for reason in [4u8, 5, 7, 255] {
        for ix in [
            ix_governance::suspend_holder(&c, &m, &b, reason, CASE),
            ix_governance::reinstate_holder(&c, &m, &b, reason, CASE),
            ix_governance::revoke_holder(&c, &m, &b, reason, CASE),
            ix_governance::pause_transfers(&c, &m, reason, CASE),
            ix_governance::unpause_transfers(&c, &m, reason, CASE),
        ] {
            assert_anchor_error(
                &h.send(&[ix], &[&ctx.compliance]),
                GovernanceError::InvalidReason,
            );
        }
    }
    for reason in [0u8, 1, 2, 3, 6] {
        let ix = ix_governance::suspend_holder(&c, &m, &b, reason, CASE);
        h.send(&[ix], &[&ctx.compliance]).expect("allowed reason");
        let ix = ix_governance::reinstate_holder(&c, &m, &b, reason, CASE);
        h.send(&[ix], &[&ctx.compliance]).expect("allowed reason");
    }
}

#[test]
fn pause_stops_every_transfer_until_unpaused() {
    // US3-4, US3-5, FR-021.
    let (mut h, ctx, alice, bob) = two_investors();
    let treasury = fixtures::onboard_treasury(&mut h, &ctx);
    pause(&mut h, &ctx).expect("pause");
    for (to, amount) in [(bob.pubkey(), 10), (bob.pubkey(), 0), (treasury, 10)] {
        assert_anchor_error(
            &fixtures::transfer(&mut h, &ctx, &alice, &to, amount),
            HookError::TransfersPaused,
        );
    }
    assert_anchor_error(&pause(&mut h, &ctx), HookError::AlreadyPaused);

    unpause(&mut h, &ctx).expect("unpause");
    fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 10).expect("transfers work again");
    assert_anchor_error(&unpause(&mut h, &ctx), HookError::NotPaused);
}

#[test]
fn compliance_can_still_act_while_paused() {
    // US3-4, FR-030: register, approve, renew, suspend, reinstate and revoke all work while paused.
    let (mut h, ctx, alice, bob) = two_investors();
    pause(&mut h, &ctx).unwrap();
    assert!(fixtures::read::<compliance_hook::Registry>(&h, &ctx.registry()).transfers_paused);
    let carol = h.funded_keypair();
    fixtures::onboard_investor(&mut h, &ctx, &carol, *b"GB"); // register + approve + thaw
    let b = bob.pubkey();
    let signed =
        fixtures::signed_statement(&ctx, &b, *b"US", clock::now(&h.svm) + 2 * fixtures::YEAR);
    let renew = ix_hook::renew_investor(
        &ctx.compliance.pubkey(),
        &ctx.m(),
        &b,
        signed.attestation_hash,
    );
    h.send(&[signed.ix.clone(), renew], &[&ctx.compliance])
        .expect("renew while paused");
    let rec = fixtures::read::<HolderState>(&h, &pda::holder(&ctx.m(), &b).0);
    assert_eq!(
        rec.attestation_hash, signed.attestation_hash,
        "renewal recorded"
    );
    suspend(&mut h, &ctx, &b).expect("suspend while paused");
    assert_eq!(status(&h, &ctx, &b), HolderStatus::Suspended.as_u8());
    reinstate(&mut h, &ctx, &b).expect("reinstate while paused");
    assert_eq!(status(&h, &ctx, &b), HolderStatus::Approved.as_u8());
    revoke(&mut h, &ctx, &alice.pubkey()).expect("revoke while paused");
    assert_eq!(
        status(&h, &ctx, &alice.pubkey()),
        HolderStatus::Revoked.as_u8()
    );
    assert_eq!(
        status(&h, &ctx, &carol.pubkey()),
        HolderStatus::Approved.as_u8()
    );
}

#[test]
fn only_governance_can_call_set_status_and_set_paused() {
    let (mut h, ctx, _alice, bob) = two_investors();
    let m = ctx.m();
    for signer in [&ctx.compliance, &ctx.issuer] {
        let s = signer.pubkey();
        let ix = ix_hook::set_status_direct(&s, &m, &bob.pubkey(), 0, SANCTIONS, CASE, s);
        assert_anchor_error(&h.send(&[ix], &[signer]), HookError::UnauthorizedCaller);
        let ix = ix_hook::set_paused_direct(&s, &m, true, SANCTIONS, CASE, s);
        assert_anchor_error(&h.send(&[ix], &[signer]), HookError::UnauthorizedCaller);
    }
}
