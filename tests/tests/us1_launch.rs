//! US1 — Launch a fund (FR-001 to FR-009). Independent test: create a fund, complete setup, launch
//! it, then read back its terms, rules, roles and status.

use anchor_lang::prelude::Pubkey;
use compliance_hook::events::{RegisterRulesSet, RegistryInitialized};
use compliance_hook::Registry;
use fund::events::{FundCreated, FundLaunched};
use fund::FundState;
use governance::events::{RolesAssigned, RolesInitialized};
use governance::RoleRegistry;
use mmf_common::enums::FundStatus;
use mmf_sdk::ix_governance::Roles;
use mmf_sdk::{ix_fund, ix_governance, ix_hook, pda};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, FUND_NAME, FUND_SYMBOL};
use tests::Harness;

struct Ctx {
    h: Harness,
    issuer: Keypair,
    mint: Keypair,
    cash_mint: Pubkey,
}

fn ctx() -> Ctx {
    let mut h = Harness::new();
    let issuer = h.funded_keypair();
    let cash_mint = fixtures::create_cash_mint(&mut h, fixtures::CASH_DECIMALS);
    Ctx {
        h,
        issuer,
        mint: Keypair::new(),
        cash_mint,
    }
}

fn distinct_roles() -> Roles {
    Roles {
        fund_admin: Pubkey::new_unique(),
        transfer_agent: Pubkey::new_unique(),
        compliance: Pubkey::new_unique(),
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    }
}

/// US1-1, US1-2 (FR-001, FR-002, FR-005, FR-007): create in Setup with exactly the given terms,
/// configure, launch to Open, and read everything back.
#[test]
fn issuer_creates_configures_and_launches_fund() {
    let Ctx {
        mut h,
        issuer,
        mint,
        cash_mint,
    } = ctx();
    let m = mint.pubkey();
    let terms = fixtures::standard_terms(cash_mint);

    let meta = h
        .send(
            &[
                ix_governance::init_roles(&issuer.pubkey(), &m),
                ix_fund::create_fund(
                    &issuer.pubkey(),
                    &m,
                    &cash_mint,
                    terms,
                    FUND_NAME,
                    FUND_SYMBOL,
                ),
            ],
            &[&issuer, &mint],
        )
        .expect("init_roles + create_fund");

    let fund_key = pda::fund_state(&m).0;
    let fs: FundState = read(&h, &fund_key);
    assert_eq!(fs.terms, terms, "terms stored exactly");
    assert_eq!(fs.status, FundStatus::Setup.as_u8());
    assert_eq!(fs.mint, m);
    assert_eq!(fs.role_registry, pda::role_registry(&fund_key).0);
    let roles: RoleRegistry = read(&h, &fs.role_registry);
    assert_eq!(
        roles.issuer,
        issuer.pubkey(),
        "creator is the issuer (FR-002)"
    );
    assert_eq!(roles.fund_state, fund_key);

    let ev = one_cpi_event::<RolesInitialized>(&meta);
    assert_eq!((ev.fund, ev.issuer), (fund_key, issuer.pubkey()));
    let ev = one_cpi_event::<FundCreated>(&meta);
    assert_eq!(
        (ev.fund, ev.mint, ev.issuer, ev.terms),
        (fund_key, m, issuer.pubkey(), terms)
    );

    // Register and rules.
    let meta = h
        .send(&[ix_hook::init_registry(&issuer.pubkey(), &m)], &[&issuer])
        .expect("init_registry");
    assert_eq!(one_cpi_event::<RegistryInitialized>(&meta).fund, fund_key);
    let provider = Pubkey::new_unique();
    let providers = [
        provider,
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    let allowed = fixtures::jurisdictions(&[*b"US", *b"GB"]);
    let meta = h
        .send(
            &[ix_hook::set_rules(
                &issuer.pubkey(),
                &m,
                providers,
                allowed,
                0,
            )],
            &[&issuer],
        )
        .expect("set_rules");
    let ev = one_cpi_event::<RegisterRulesSet>(&meta);
    assert_eq!(
        (ev.kyc_providers, ev.allowed_jurisdictions, ev.max_holders),
        (providers, allowed, 0)
    );
    let reg: Registry = read(&h, &pda::registry(&m).0);
    assert_eq!(reg.mint, m);
    assert_eq!(reg.fund_state, fund_key);
    assert_eq!(reg.kyc_providers, providers);
    assert_eq!(reg.allowed_jurisdictions, allowed);
    assert_eq!(
        (reg.max_holders, reg.holder_count, reg.transfers_paused),
        (0, 0, false)
    );

    // Roles.
    let r = distinct_roles();
    let meta = h
        .send(
            &[ix_governance::set_roles(&issuer.pubkey(), &m, &r)],
            &[&issuer],
        )
        .expect("set_roles");
    let ev = one_cpi_event::<RolesAssigned>(&meta);
    assert_eq!((ev.compliance, ev.keeper), (r.compliance, r.keeper));
    let roles: RoleRegistry = read(&h, &pda::role_registry(&fund_key).0);
    assert_eq!(
        (
            roles.fund_admin,
            roles.transfer_agent,
            roles.compliance,
            roles.keeper,
            roles.auditor
        ),
        (
            r.fund_admin,
            r.transfer_agent,
            r.compliance,
            r.keeper,
            r.auditor
        )
    );

    // Launch.
    let meta = h
        .send(&[ix_fund::launch(&issuer.pubkey(), &m)], &[&issuer])
        .expect("launch");
    let ev = one_cpi_event::<FundLaunched>(&meta);
    assert_eq!((ev.fund, ev.signer), (fund_key, issuer.pubkey()));
    let fs: FundState = read(&h, &fund_key);
    assert_eq!(fs.status, FundStatus::Open.as_u8());
}
