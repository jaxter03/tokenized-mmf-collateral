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
            ..distinct()
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
