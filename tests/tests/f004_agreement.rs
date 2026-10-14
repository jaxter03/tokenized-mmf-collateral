//! F004 US1, agree the rules (FR-001–FR-006; contracts/collateral.md `propose_agreement`,
//! `sign_agreement`). T022: written before the code, every rule with its exact error.

use anchor_lang::prelude::Pubkey;
use collateral::events::{AgreementActive, AgreementProposed};
use collateral::{CollateralError, PledgeAgreement, ProposeArgs};
use mmf_common::enums::AgreementStatus;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use tests::assert::{assert_anchor_error, assert_instruction_error};
use tests::collateral::{agr, propose, revoke, sign, suspend};
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, worked_terms, CollatBase};
use tests::{clock, Harness};

fn base() -> (Harness, CollatBase) {
    let mut h = Harness::new();
    let c = fixtures::collateral_base_fund(&mut h);
    (h, c)
}

fn terms(c: &CollatBase, nonce: u64) -> ProposeArgs {
    worked_terms(&c.giver.pubkey(), &c.taker.pubkey(), nonce)
}

#[test]
fn taker_proposes_giver_signs() {
    let (mut h, c) = base();
    let args = terms(&c, 1);
    let a = agr(&c, 1);
    let now = clock::now(&h.svm);

    let meta = propose(&mut h, &c.m(), &c.taker, args).expect("propose");
    let ev: AgreementProposed = one_cpi_event(&meta);
    assert_eq!(
        (ev.fund, ev.agreement, ev.proposer, ev.args),
        (c.fund_state(), a.agreement(), c.taker.pubkey(), args)
    );
    let ag: PledgeAgreement = read(&h, &a.agreement());
    assert_eq!(ag.version, PledgeAgreement::VERSION);
    assert_eq!(
        (ag.fund_state, ag.giver, ag.taker, ag.nonce),
        (c.fund_state(), c.giver.pubkey(), c.taker.pubkey(), 1)
    );
    assert_eq!((ag.mode, ag.income_to, ag.haircut_bps), (0, 0, 200));
    assert_eq!(
        (
            ag.max_nav_age_secs,
            ag.call_cure_secs,
            ag.enforcement_grace_secs
        ),
        (93_600, 14_400, 7_200)
    );
    assert_eq!(
        (ag.min_transfer_value, ag.concentration_cap_units),
        (10_000_000_000, 5_000_000_000_000)
    );
    assert!(ag.taker_may_call_unilaterally);
    assert_eq!(ag.agreement_hash, [7; 32]);
    assert_eq!(ag.status, AgreementStatus::Proposed.as_u8());
    assert_eq!((ag.giver_signed, ag.taker_signed), (false, true));
    assert_eq!((ag.pledge_count, ag.call_count), (1, 1));
    assert_eq!((ag.created_ts, ag.activated_ts), (now, 0));
    assert_eq!(
        (ag.pledged_units, ag.required_value, ag.called_outstanding),
        (0, 0, 0)
    );
    assert_eq!(ag.open_return_call, Pubkey::default());

    clock::advance(&mut h.svm, 60);
    let meta = sign(&mut h, &c.giver, &a).expect("sign");
    let ev: AgreementActive = one_cpi_event(&meta);
    assert_eq!(
        (ev.fund, ev.agreement, ev.signer),
        (c.fund_state(), a.agreement(), c.giver.pubkey())
    );
    let ag: PledgeAgreement = read(&h, &a.agreement());
    assert_eq!(ag.status, AgreementStatus::Active.as_u8());
    assert_eq!((ag.giver_signed, ag.taker_signed), (true, true));
    assert_eq!(ag.activated_ts, now + 60);
}

#[test]
fn giver_may_propose_and_taker_sign() {
    let (mut h, c) = base();
    propose(&mut h, &c.m(), &c.giver, terms(&c, 2)).expect("giver proposes");
    let a = agr(&c, 2);
    let ag: PledgeAgreement = read(&h, &a.agreement());
    assert_eq!((ag.giver_signed, ag.taker_signed), (true, false));
    sign(&mut h, &c.taker, &a).expect("taker signs");
    let ag: PledgeAgreement = read(&h, &a.agreement());
    assert_eq!(ag.status, AgreementStatus::Active.as_u8());
}

#[test]
fn fund_must_be_open() {
    let mut h = Harness::new();
    let (_issuer, mint, _cash) = fixtures::create_standard_fund(&mut h);
    let (g, t) = (h.funded_keypair(), h.funded_keypair());
    let args = worked_terms(&g.pubkey(), &t.pubkey(), 1);
    let res = propose(&mut h, &mint.pubkey(), &t, args);
    assert_anchor_error(&res, CollateralError::FundNotOpen);
}

#[test]
fn proposer_must_be_a_party_and_parties_must_differ() {
    let (mut h, c) = base();
    let outsider = fixtures::new_investor(&mut h, &c);
    let res = propose(&mut h, &c.m(), &outsider, terms(&c, 1));
    assert_anchor_error(&res, CollateralError::Unauthorized);

    let g = c.giver.pubkey();
    let res = propose(&mut h, &c.m(), &c.giver, worked_terms(&g, &g, 1));
    assert_anchor_error(&res, CollateralError::SameParty);
}

#[test]
fn every_term_is_checked() {
    let (mut h, c) = base();
    let cases: Vec<(&str, ProposeArgs, CollateralError)> = vec![
        (
            "mode 1",
            ProposeArgs {
                mode: 1,
                ..terms(&c, 10)
            },
            CollateralError::UnsupportedPledgeMode,
        ),
        (
            "income 1",
            ProposeArgs {
                income_to: 1,
                ..terms(&c, 11)
            },
            CollateralError::UnsupportedIncomeRouting,
        ),
        (
            "income 2",
            ProposeArgs {
                income_to: 2,
                ..terms(&c, 12)
            },
            CollateralError::UnsupportedIncomeRouting,
        ),
        (
            "haircut 10000",
            ProposeArgs {
                haircut_bps: 10_000,
                ..terms(&c, 13)
            },
            CollateralError::InvalidHaircut,
        ),
        (
            "haircut 10001",
            ProposeArgs {
                haircut_bps: 10_001,
                ..terms(&c, 14)
            },
            CollateralError::InvalidHaircut,
        ),
        (
            "cure 0",
            ProposeArgs {
                call_cure_secs: 0,
                ..terms(&c, 15)
            },
            CollateralError::InvalidCureTime,
        ),
        (
            "cap 0",
            ProposeArgs {
                concentration_cap_units: 0,
                ..terms(&c, 16)
            },
            CollateralError::InvalidPledgeLimit,
        ),
        (
            "age 0",
            ProposeArgs {
                max_nav_age_secs: 0,
                ..terms(&c, 17)
            },
            CollateralError::InvalidPriceAge,
        ),
        (
            "hash 0",
            ProposeArgs {
                agreement_hash: [0; 32],
                ..terms(&c, 18)
            },
            CollateralError::EmptyAgreementHash,
        ),
    ];
    for (name, args, err) in cases {
        let res = propose(&mut h, &c.m(), &c.taker, args);
        assert!(res.is_err(), "{name} must be rejected");
        assert_anchor_error(&res, err);
    }
    // Boundaries that pass: haircut 99.99 %, zero grace, zero minimum transfer amount.
    let ok = ProposeArgs {
        haircut_bps: 9_999,
        enforcement_grace_secs: 0,
        min_transfer_value: 0,
        ..terms(&c, 19)
    };
    propose(&mut h, &c.m(), &c.taker, ok).expect("boundary terms pass");
}

#[test]
fn suspended_party_cannot_propose_or_be_named() {
    for suspended_giver in [true, false] {
        let (mut h, c) = base();
        let who = if suspended_giver {
            c.giver.pubkey()
        } else {
            c.taker.pubkey()
        };
        suspend(&mut h, &c, &who);
        let res = propose(&mut h, &c.m(), &c.taker, terms(&c, 1));
        assert_anchor_error(&res, CollateralError::PartyNotEligible);
    }
}

#[test]
fn pending_revoked_unregistered_and_fund_operated_parties() {
    // Pending: registered, never approved.
    let (mut h, c) = base();
    let pending = h.funded_keypair();
    let p = pending.pubkey();
    let ix = mmf_sdk::ix_hook::register_holder(&p, &p, &c.m(), &p, 0);
    h.send(&[ix], &[&pending]).expect("register");
    let res = propose(
        &mut h,
        &c.m(),
        &c.giver,
        worked_terms(&c.giver.pubkey(), &p, 1),
    );
    assert_anchor_error(&res, CollateralError::PartyNotEligible);

    // Revoked.
    revoke(&mut h, &c, &c.taker.pubkey());
    let res = propose(&mut h, &c.m(), &c.giver, terms(&c, 2));
    assert_anchor_error(&res, CollateralError::PartyNotEligible);

    // Unregistered: no holder record at all.
    let stranger = Keypair::new().pubkey();
    let res = propose(
        &mut h,
        &c.m(),
        &c.giver,
        worked_terms(&c.giver.pubkey(), &stranger, 3),
    );
    assert_anchor_error(&res, CollateralError::WrongHolder);

    // Fund-operated: the settlement escrow is an Approved holder of class Escrow.
    let res = propose(
        &mut h,
        &c.m(),
        &c.giver,
        worked_terms(&c.giver.pubkey(), &c.escrow, 4),
    );
    assert_anchor_error(&res, CollateralError::PartyNotEligible);
}

#[test]
fn kyc_expires_at_the_exact_second() {
    let (mut h, c) = base();
    let holder: compliance_hook::HolderState =
        read(&h, &mmf_sdk::pda::holder(&c.m(), &c.taker.pubkey()).0);
    clock::set_unix_timestamp(&mut h.svm, holder.kyc_expiry_ts);
    let res = propose(&mut h, &c.m(), &c.giver, terms(&c, 1));
    assert_anchor_error(&res, CollateralError::PartyNotEligible);
    clock::set_unix_timestamp(&mut h.svm, holder.kyc_expiry_ts - 1);
    propose(&mut h, &c.m(), &c.giver, terms(&c, 2)).expect("one second before expiry");
}

#[test]
fn signing_rules() {
    let (mut h, c) = base();
    let a = agr(&c, 1);
    propose(&mut h, &c.m(), &c.taker, terms(&c, 1)).expect("propose");

    let outsider = fixtures::new_investor(&mut h, &c);
    assert_anchor_error(&sign(&mut h, &outsider, &a), CollateralError::Unauthorized);
    assert_anchor_error(&sign(&mut h, &c.taker, &a), CollateralError::AlreadySigned);

    // Suspended between the proposal and the signature.
    suspend(&mut h, &c, &c.giver.pubkey());
    assert_anchor_error(
        &sign(&mut h, &c.giver, &a),
        CollateralError::PartyNotEligible,
    );
    tests::collateral::reinstate(&mut h, &c, &c.giver.pubkey());
    sign(&mut h, &c.giver, &a).expect("sign after reinstatement");

    // An Active agreement can't be signed again.
    assert_anchor_error(
        &sign(&mut h, &c.giver, &a),
        CollateralError::AgreementNotProposed,
    );
}

#[test]
fn identity_is_fund_giver_taker_and_nonce() {
    let (mut h, c) = base();
    propose(&mut h, &c.m(), &c.taker, terms(&c, 1)).expect("first");
    // Same address: Anchor's `init` fails (system program AccountAlreadyInUse).
    let res = propose(&mut h, &c.m(), &c.giver, terms(&c, 1));
    assert_instruction_error(&res, InstructionError::Custom(0));
    // A new nonce is a second agreement between the same pair.
    propose(&mut h, &c.m(), &c.taker, terms(&c, 2)).expect("second agreement");
    let ag: PledgeAgreement = read(&h, &agr(&c, 2).agreement());
    assert_eq!(ag.nonce, 2);
}

/// FR-005: no instruction other than `propose_agreement` takes the agreement's terms, so terms
/// can't change after the proposal.
#[test]
fn only_the_proposal_takes_terms() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/idl/collateral.json");
    let idl: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("run `anchor build` first"))
            .unwrap();
    let term_names = [
        "haircut_bps",
        "max_nav_age_secs",
        "call_cure_secs",
        "enforcement_grace_secs",
        "min_transfer_value",
        "concentration_cap_units",
        "taker_may_call_unilaterally",
        "agreement_hash",
        "mode",
        "income_to",
        "args",
    ];
    for ix in idl["instructions"].as_array().unwrap() {
        let name = ix["name"].as_str().unwrap();
        if name == "propose_agreement" {
            continue;
        }
        for arg in ix["args"].as_array().unwrap() {
            let arg = arg["name"].as_str().unwrap();
            assert!(!term_names.contains(&arg), "{name} takes a term ({arg})");
        }
    }
}

/// Research R14: each collateral instruction stays within 80 000 compute units.
#[test]
fn proposal_and_signature_fit_the_compute_budget() {
    let (mut h, c) = base();
    let meta = propose(&mut h, &c.m(), &c.taker, terms(&c, 1)).expect("propose");
    assert!(
        meta.compute_units_consumed <= 80_000,
        "propose used {}",
        meta.compute_units_consumed
    );
    let meta = sign(&mut h, &c.giver, &agr(&c, 1)).expect("sign");
    assert!(
        meta.compute_units_consumed <= 80_000,
        "sign used {}",
        meta.compute_units_consumed
    );
}
