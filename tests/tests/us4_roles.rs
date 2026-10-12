//! US4 — Roles (FR-010, FR-011, FR-012). Independent test: give every role to a different party,
//! attempt each role-restricted action from every party, and show only the right one succeeds.

use anchor_lang::prelude::Pubkey;
use governance::{GovernanceError, RoleRegistry};
use mmf_sdk::ix_governance::{self, Roles};
use mmf_sdk::pda;
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::fixtures;
use tests::{Harness, TxResult};

struct Setup {
    h: Harness,
    issuer: Keypair,
    mint: Pubkey,
}

/// A fund in Setup (created, not launched).
fn setup() -> Setup {
    let mut h = Harness::new();
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    Setup {
        h,
        issuer,
        mint: mint.pubkey(),
    }
}

fn distinct() -> Roles {
    Roles {
        fund_admin: Pubkey::new_unique(),
        transfer_agent: Pubkey::new_unique(),
        compliance: Pubkey::new_unique(),
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    }
}

impl Setup {
    fn set(&mut self, roles: &Roles) -> TxResult {
        let ix = ix_governance::set_roles(&self.issuer.pubkey(), &self.mint, roles);
        let issuer = self.issuer.insecure_clone();
        self.h.send(&[ix], &[&issuer])
    }

    fn stored(&self) -> RoleRegistry {
        fixtures::read(
            &self.h,
            &pda::role_registry(&pda::fund_state(&self.mint).0).0,
        )
    }
}

// ---------------------------------------------------------------------------------------------
// T071: separation of duties (FR-010, Clarification Q4, US4-5, US4-6)
// ---------------------------------------------------------------------------------------------

#[test]
fn keeper_must_differ_from_every_other_role() {
    let mut s = setup();
    let issuer = s.issuer.pubkey();
    let base = distinct();
    for keeper in [
        issuer,
        base.fund_admin,
        base.transfer_agent,
        base.compliance,
        base.auditor,
    ] {
        let r = Roles { keeper, ..base };
        assert_anchor_error(&s.set(&r), GovernanceError::KeeperNotSeparate);
    }
}

#[test]
fn compliance_must_differ_from_the_issuer() {
    let mut s = setup();
    let r = Roles {
        compliance: s.issuer.pubkey(),
        ..distinct()
    };
    assert_anchor_error(&s.set(&r), GovernanceError::ComplianceIsIssuer);
}

#[test]
fn other_overlaps_are_allowed() {
    // US4-6: fund admin and transfer agent are often the same firm.
    let mut s = setup();
    let base = distinct();
    let r = Roles {
        transfer_agent: base.fund_admin,
        ..base
    };
    s.set(&r).expect("fund admin = transfer agent");
    let r = Roles {
        auditor: base.fund_admin,
        ..base
    };
    s.set(&r).expect("auditor = fund admin");
    let issuer = s.issuer.pubkey();
    let r = Roles {
        fund_admin: issuer,
        transfer_agent: issuer,
        auditor: issuer,
        ..base
    };
    s.set(&r)
        .expect("the issuer may hold roles other than compliance and keeper");
}

#[test]
fn a_rejected_assignment_leaves_roles_unchanged() {
    // US4-5.
    let mut s = setup();
    let good = distinct();
    s.set(&good).unwrap();
    let before = s.stored();
    assert_anchor_error(
        &s.set(&Roles {
            keeper: good.compliance,
            ..good
        }),
        GovernanceError::KeeperNotSeparate,
    );
    let after = s.stored();
    assert_eq!(
        (
            after.fund_admin,
            after.transfer_agent,
            after.compliance,
            after.keeper,
            after.auditor
        ),
        (
            before.fund_admin,
            before.transfer_agent,
            before.compliance,
            before.keeper,
            before.auditor
        )
    );
}

// ---------------------------------------------------------------------------------------------
// T072: permission matrix (FR-011, US4-1, US4-2)
// ---------------------------------------------------------------------------------------------

use compliance_hook::HookError;
use fund::FundError;
use mmf_common::enums::ReasonCode;
use mmf_sdk::{ix_fund, ix_hook};
use tests::clock;
use tests::Ix;

/// Every party: index 0 is the issuer, 1–5 the five roles, 6 an unrelated key.
struct Parties {
    all: Vec<Keypair>,
}

const ISSUER: usize = 0;
const COMPLIANCE: usize = 3;

impl Parties {
    fn roles(&self) -> Roles {
        Roles {
            fund_admin: self.all[1].pubkey(),
            transfer_agent: self.all[2].pubkey(),
            compliance: self.all[3].pubkey(),
            keeper: self.all[4].pubkey(),
            auditor: self.all[5].pubkey(),
        }
    }
}

/// Builds the instruction for `signer`, asserts every party except `allowed` gets `Unauthorized`
/// (via `check`), then runs it for `allowed`. `extra` instructions go in front (e.g. Ed25519).
fn matrix(
    h: &mut Harness,
    p: &Parties,
    allowed: usize,
    extra: &[Ix],
    build: impl Fn(&Pubkey) -> Ix,
    check: impl Fn(&TxResult),
) {
    for (i, party) in p.all.iter().enumerate() {
        if i == allowed {
            continue;
        }
        let mut ixs = extra.to_vec();
        ixs.push(build(&party.pubkey()));
        check(&h.send(&ixs, &[party]));
    }
    let mut ixs = extra.to_vec();
    ixs.push(build(&p.all[allowed].pubkey()));
    h.send(&ixs, &[&p.all[allowed]])
        .expect("the role holder succeeds");
}

#[test]
fn each_action_is_accepted_only_from_its_role_holder() {
    let mut h = Harness::new();
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    let m = mint.pubkey();
    let mut all = vec![issuer];
    all.extend((0..6).map(|_| h.funded_keypair()));
    let p = Parties { all };
    let provider = Keypair::new();
    let providers = [
        provider.pubkey(),
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    let us = fixtures::jurisdictions(&[*b"US"]);
    let hook_unauthorized = |r: &TxResult| assert_anchor_error(r, HookError::Unauthorized);
    let gov_unauthorized = |r: &TxResult| assert_anchor_error(r, GovernanceError::Unauthorized);

    // Issuer-only setup actions.
    matrix(
        &mut h,
        &p,
        ISSUER,
        &[],
        |s| ix_hook::init_registry(s, &m),
        hook_unauthorized,
    );
    matrix(
        &mut h,
        &p,
        ISSUER,
        &[],
        |s| ix_hook::set_rules(s, &m, providers, us, 0),
        hook_unauthorized,
    );
    let roles = p.roles();
    matrix(
        &mut h,
        &p,
        ISSUER,
        &[],
        |s| ix_governance::set_roles(s, &m, &roles),
        gov_unauthorized,
    );
    matrix(
        &mut h,
        &p,
        ISSUER,
        &[],
        |s| ix_fund::launch(s, &m),
        |r| assert_anchor_error(r, FundError::Unauthorized),
    );

    // Compliance-only onboarding actions. Nobody but compliance (or the owner itself) may
    // register someone else.
    let inv = Pubkey::new_unique();
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        &[],
        |s| ix_hook::register_holder(s, s, &m, &inv, 0),
        hook_unauthorized,
    );
    let stmt = mmf_sdk::kyc::sign(
        &provider,
        &mmf_sdk::kyc::statement(m, inv, *b"US", 1, clock::now(&h.svm) + fixtures::YEAR),
    );
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        std::slice::from_ref(&stmt.ix),
        |s| ix_hook::approve_investor(s, &m, &inv, stmt.attestation_hash),
        hook_unauthorized,
    );
    let renewal = mmf_sdk::kyc::sign(
        &provider,
        &mmf_sdk::kyc::statement(m, inv, *b"US", 1, clock::now(&h.svm) + 2 * fixtures::YEAR),
    );
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        std::slice::from_ref(&renewal.ix),
        |s| ix_hook::renew_investor(s, &m, &inv, renewal.attestation_hash),
        hook_unauthorized,
    );
    let treasury = pda::treasury(&pda::fund_state(&m).0).0;
    let c = p.all[COMPLIANCE].insecure_clone();
    h.send(
        &[ix_hook::register_holder(
            &c.pubkey(),
            &c.pubkey(),
            &m,
            &treasury,
            1,
        )],
        &[&c],
    )
    .unwrap();
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        &[],
        |s| ix_hook::approve_fund_account(s, &m, &treasury),
        hook_unauthorized,
    );

    // Compliance-only governance actions.
    let r = ReasonCode::OperationalError as u8;
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        &[],
        |s| ix_governance::suspend_holder(s, &m, &inv, r, [0; 32]),
        gov_unauthorized,
    );
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        &[],
        |s| ix_governance::reinstate_holder(s, &m, &inv, r, [0; 32]),
        gov_unauthorized,
    );
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        &[],
        |s| ix_governance::pause_transfers(s, &m, r, [0; 32]),
        gov_unauthorized,
    );
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        &[],
        |s| ix_governance::unpause_transfers(s, &m, r, [0; 32]),
        gov_unauthorized,
    );
    matrix(
        &mut h,
        &p,
        COMPLIANCE,
        &[],
        |s| ix_governance::revoke_holder(s, &m, &inv, r, [0; 32]),
        gov_unauthorized,
    );
}

// ---------------------------------------------------------------------------------------------
// T073: compliance as a Squads 2-of-3 vault (FR-012, US4-3, research R8)
// ---------------------------------------------------------------------------------------------

use compliance_hook::HolderState;
use mmf_common::enums::HolderStatus;
use tests::squads::{self, Multisig, INVALID_PROPOSAL_STATUS};

/// A launched fund whose compliance role is a 2-of-3 Squads vault, plus a registered investor.
fn fund_with_squads_compliance() -> (Harness, Multisig, Keypair, Pubkey, Keypair) {
    let mut h = Harness::new();
    let ms = squads::create(&mut h, 2, 3);
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    let m = mint.pubkey();
    let provider = Keypair::new();
    let roles = Roles {
        compliance: ms.vault,
        ..distinct()
    };
    let i = issuer.pubkey();
    let providers = [
        provider.pubkey(),
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
    let inv = h.funded_keypair();
    let o = inv.pubkey();
    h.send(&[ix_hook::register_holder(&o, &o, &m, &o, 0)], &[&inv])
        .unwrap();
    (h, ms, provider, m, inv)
}

fn holder_status(h: &Harness, m: &Pubkey, owner: &Pubkey) -> u8 {
    fixtures::read::<HolderState>(h, &pda::holder(m, owner).0).status
}

#[test]
fn a_multisig_role_acts_only_at_its_threshold() {
    let (mut h, mut ms, provider, m, inv) = fund_with_squads_compliance();
    let o = inv.pubkey();

    // Approve the investor through the vault (2 approvals) so there is someone to suspend.
    let stmt = mmf_sdk::kyc::sign(
        &provider,
        &mmf_sdk::kyc::statement(m, o, *b"US", 1, clock::now(&h.svm) + fixtures::YEAR),
    );
    let approve = ix_hook::approve_investor(&ms.vault, &m, &o, stmt.attestation_hash);
    squads::squads_execute(
        &mut h,
        &mut ms,
        &[approve],
        2,
        std::slice::from_ref(&stmt.ix),
    )
    .expect("approve via vault");
    assert_eq!(holder_status(&h, &m, &o), HolderStatus::Approved.as_u8());

    // One approval of a 2-of-3: execute fails and the holder is unchanged (US4-3).
    let suspend =
        ix_governance::suspend_holder(&ms.vault, &m, &o, ReasonCode::SanctionsHit as u8, [0; 32]);
    let (index, remaining) = ms.propose(&mut h, &[suspend]);
    ms.approve(&mut h, index, 0).unwrap();
    let res = ms.execute(&mut h, index, &remaining, &[]);
    tests::assert::assert_squads_error(&res, INVALID_PROPOSAL_STATUS);
    assert_eq!(holder_status(&h, &m, &o), HolderStatus::Approved.as_u8());

    // A second approval reaches the threshold: the same proposal now executes.
    ms.approve(&mut h, index, 1).unwrap();
    ms.execute(&mut h, index, &remaining, &[])
        .expect("executes at threshold");
    assert_eq!(holder_status(&h, &m, &o), HolderStatus::Suspended.as_u8());
}

#[test]
fn the_vault_approves_one_exact_statement() {
    // The Ed25519 instruction must be top-level, outside the approved vault transaction; the
    // attestation_hash argument binds the approval to one statement (research R8).
    let (mut h, mut ms, provider, m, inv) = fund_with_squads_compliance();
    let o = inv.pubkey();
    let now = clock::now(&h.svm);
    let approved = mmf_sdk::kyc::sign(
        &provider,
        &mmf_sdk::kyc::statement(m, o, *b"US", 1, now + fixtures::YEAR),
    );
    let swapped = mmf_sdk::kyc::sign(
        &provider,
        &mmf_sdk::kyc::statement(m, o, *b"US", 7, now + 2 * fixtures::YEAR),
    );
    let ix = ix_hook::approve_investor(&ms.vault, &m, &o, approved.attestation_hash);

    // The executor attaches a different, validly signed statement for the same owner.
    let (index, remaining) = ms.propose(&mut h, std::slice::from_ref(&ix));
    ms.approve(&mut h, index, 0).unwrap();
    ms.approve(&mut h, index, 1).unwrap();
    let res = ms.execute(&mut h, index, &remaining, std::slice::from_ref(&swapped.ix));
    assert_anchor_error(&res, HookError::StatementHashMismatch);
    assert_eq!(holder_status(&h, &m, &o), HolderStatus::Pending.as_u8());

    // With the statement the members approved, it goes through.
    ms.execute(
        &mut h,
        index,
        &remaining,
        std::slice::from_ref(&approved.ix),
    )
    .expect("approved statement");
    let rec = fixtures::read::<HolderState>(&h, &pda::holder(&m, &o).0);
    assert_eq!(
        (rec.status, rec.attestation_hash),
        (HolderStatus::Approved.as_u8(), approved.attestation_hash)
    );
}
