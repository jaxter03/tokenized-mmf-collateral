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

// ---------------------------------------------------------------------------------------------
// T028: create_fund rejections (FR-003, contract order). Each attempt sends init_roles +
// create_fund in one transaction; on failure neither the FundState nor the mint may exist.
// ---------------------------------------------------------------------------------------------

use fund::{FundError, FundTerms};
use tests::assert::assert_anchor_error;
use tests::TxResult;

/// `create_fund` account index of `role_registry` (issuer, mint, fund_state, role_registry, …).
const CREATE_FUND_ROLE_REGISTRY: usize = 3;

fn attempt(c: &mut Ctx, terms: FundTerms, name: &str, symbol: &str) -> TxResult {
    let m = c.mint.pubkey();
    let res = c.h.send(
        &[
            ix_governance::init_roles(&c.issuer.pubkey(), &m),
            ix_fund::create_fund(
                &c.issuer.pubkey(),
                &m,
                &terms.cash_mint,
                terms,
                name,
                symbol,
            ),
        ],
        &[&c.issuer, &c.mint],
    );
    if res.is_err() {
        assert_nothing_created(c);
    }
    res
}

fn assert_nothing_created(c: &Ctx) {
    let m = c.mint.pubkey();
    assert!(
        !c.h.exists(&pda::fund_state(&m).0),
        "no FundState after a failed create_fund"
    );
    assert!(!c.h.exists(&m), "no mint after a failed create_fund");
}

fn reject(edit: impl FnOnce(&mut FundTerms), expected: FundError) {
    let mut c = ctx();
    let mut terms = fixtures::standard_terms(c.cash_mint);
    edit(&mut terms);
    let res = attempt(&mut c, terms, FUND_NAME, FUND_SYMBOL);
    assert_anchor_error(&res, expected);
}

#[test]
fn rejects_role_registry_that_is_not_governance_for_this_fund() {
    // No init_roles at all: the role registry account doesn't exist.
    let mut c = ctx();
    let m = c.mint.pubkey();
    let terms = fixtures::standard_terms(c.cash_mint);
    let res = c.h.send(
        &[ix_fund::create_fund(
            &c.issuer.pubkey(),
            &m,
            &c.cash_mint,
            terms,
            FUND_NAME,
            FUND_SYMBOL,
        )],
        &[&c.issuer, &c.mint],
    );
    assert_anchor_error(&res, FundError::RoleRegistryMismatch);
    assert_nothing_created(&c);

    // A role registry at some other address.
    let mut c = ctx();
    let m = c.mint.pubkey();
    let mut ix = ix_fund::create_fund(
        &c.issuer.pubkey(),
        &m,
        &c.cash_mint,
        terms,
        FUND_NAME,
        FUND_SYMBOL,
    );
    ix.accounts[CREATE_FUND_ROLE_REGISTRY].pubkey = Pubkey::new_unique();
    let res = c.h.send(
        &[ix_governance::init_roles(&c.issuer.pubkey(), &m), ix],
        &[&c.issuer, &c.mint],
    );
    assert_anchor_error(&res, FundError::RoleRegistryMismatch);
    assert_nothing_created(&c);
}

#[test]
fn rejects_creator_who_is_not_the_role_registry_issuer() {
    let mut c = ctx();
    let other = c.h.funded_keypair();
    let m = c.mint.pubkey();
    let terms = fixtures::standard_terms(c.cash_mint);
    let res = c.h.send(
        &[
            ix_governance::init_roles(&c.issuer.pubkey(), &m),
            ix_fund::create_fund(
                &other.pubkey(),
                &m,
                &c.cash_mint,
                terms,
                FUND_NAME,
                FUND_SYMBOL,
            ),
        ],
        &[&c.issuer, &other, &c.mint],
    );
    assert_anchor_error(&res, FundError::Unauthorized);
    assert_nothing_created(&c);
}

#[test]
fn rejects_all_zero_fund_id() {
    reject(|t| t.fund_id = [0; 12], FundError::InvalidFundId);
}

#[test]
fn rejects_cash_asset_that_is_not_a_six_decimal_mint() {
    reject(
        |t| t.cash_mint = Pubkey::new_unique(),
        FundError::InvalidCashMint,
    );

    let mut c = ctx();
    let nine = fixtures::create_cash_mint(&mut c.h, 9);
    let mut terms = fixtures::standard_terms(c.cash_mint);
    terms.cash_mint = nine;
    let res = attempt(&mut c, terms, FUND_NAME, FUND_SYMBOL);
    assert_anchor_error(&res, FundError::InvalidCashMint);
}

#[test]
fn rejects_zero_target_price() {
    reject(|t| t.target_nav = 0, FundError::ZeroTargetNav); // US1-5
}

#[test]
fn rejects_zero_largest_daily_move() {
    reject(|t| t.max_nav_move_bps = 0, FundError::ZeroMaxNavMove);
}

#[test]
fn rejects_redemption_limit_of_zero_or_above_100_percent() {
    reject(
        |t| t.redemption_gate_bps = 0,
        FundError::InvalidRedemptionGate,
    );
    reject(
        |t| t.redemption_gate_bps = 10_001,
        FundError::InvalidRedemptionGate,
    ); // US1-5
}

#[test]
fn accepts_redemption_limit_of_exactly_100_percent() {
    let mut c = ctx();
    let mut terms = fixtures::standard_terms(c.cash_mint);
    terms.redemption_gate_bps = 10_000;
    attempt(&mut c, terms, FUND_NAME, FUND_SYMBOL).expect("10 000 bps is allowed");
}

#[test]
fn rejects_unknown_distribution_mode() {
    reject(
        |t| t.distribution_mode = 2,
        FundError::InvalidDistributionMode,
    );
}

#[test]
fn rejects_zero_distribution_frequency() {
    reject(
        |t| t.distribution_cadence_days = 0,
        FundError::ZeroDistributionCadence,
    );
}

#[test]
fn rejects_missing_documents_fingerprint() {
    reject(|t| t.terms_hash = [0; 32], FundError::MissingTermsHash);
}

#[test]
fn rejects_missing_or_invalid_documents_link() {
    reject(|t| t.terms_uri = [0; 128], FundError::MissingTermsUri);
    reject(
        |t| {
            t.terms_uri = [0; 128];
            t.terms_uri[..2].copy_from_slice(&[0xFF, 0xFE]);
        },
        FundError::MissingTermsUri,
    );
}

#[test]
fn rejects_empty_or_overlong_name() {
    for name in ["", &"N".repeat(33)] {
        let mut c = ctx();
        let terms = fixtures::standard_terms(c.cash_mint);
        assert_anchor_error(
            &attempt(&mut c, terms, name, FUND_SYMBOL),
            FundError::InvalidName,
        );
    }
}

#[test]
fn rejects_empty_or_overlong_symbol() {
    for symbol in ["", "SSSSSSSSSSS"] {
        let mut c = ctx();
        let terms = fixtures::standard_terms(c.cash_mint);
        assert_anchor_error(
            &attempt(&mut c, terms, FUND_NAME, symbol),
            FundError::InvalidSymbol,
        );
    }
}

#[test]
fn checks_terms_in_contract_order() {
    // Several invalid terms at once: the first in the contract order is reported.
    reject(
        |t| {
            t.target_nav = 0;
            t.terms_hash = [0; 32];
            t.distribution_mode = 9;
        },
        FundError::ZeroTargetNav,
    );
}

// ---------------------------------------------------------------------------------------------
// T029: setup and launch rejections (FR-005, FR-006, FR-008, FR-009, FR-010 basic).
// ---------------------------------------------------------------------------------------------

use anchor_lang::error::ErrorCode as AnchorError;
use compliance_hook::HookError;
use governance::GovernanceError;
use mmf_common::jurisdiction::{self as jur, SET_WORDS};
use tests::assert::custom_error;

impl Ctx {
    fn m(&self) -> Pubkey {
        self.mint.pubkey()
    }

    fn fund_key(&self) -> Pubkey {
        pda::fund_state(&self.m()).0
    }

    /// init_roles + create_fund with standard terms.
    fn created(mut self) -> Self {
        let terms = fixtures::standard_terms(self.cash_mint);
        attempt(&mut self, terms, FUND_NAME, FUND_SYMBOL).expect("create fund");
        self
    }

    fn with_registry(mut self) -> Self {
        let ix = ix_hook::init_registry(&self.issuer.pubkey(), &self.m());
        self.h.send(&[ix], &[&self.issuer]).expect("init_registry");
        self
    }

    fn set_rules(&mut self, providers: [Pubkey; 4], allowed: [u64; SET_WORDS]) -> TxResult {
        let ix = ix_hook::set_rules(&self.issuer.pubkey(), &self.m(), providers, allowed, 0);
        self.h.send(&[ix], &[&self.issuer])
    }

    fn with_rules(mut self) -> Self {
        let providers = [
            Pubkey::new_unique(),
            Pubkey::default(),
            Pubkey::default(),
            Pubkey::default(),
        ];
        self.set_rules(providers, fixtures::jurisdictions(&[*b"US"]))
            .expect("set_rules");
        self
    }

    fn with_roles(mut self) -> Self {
        let ix = ix_governance::set_roles(&self.issuer.pubkey(), &self.m(), &distinct_roles());
        self.h.send(&[ix], &[&self.issuer]).expect("set_roles");
        self
    }

    fn launch_by(&mut self, signer: &Keypair) -> TxResult {
        let ix = ix_fund::launch(&signer.pubkey(), &self.m());
        self.h.send(&[ix], &[signer])
    }

    fn launched(mut self) -> Self {
        let issuer = self.issuer.insecure_clone();
        self.launch_by(&issuer).expect("launch");
        self
    }

    fn status(&self) -> u8 {
        read::<FundState>(&self.h, &self.fund_key()).status
    }
}

#[test]
fn init_roles_requires_the_mint_signature() {
    let mut c = ctx();
    let mut ix = ix_governance::init_roles(&c.issuer.pubkey(), &c.m());
    ix.accounts[1].is_signer = false; // mint
    let res = c.h.send(&[ix], &[&c.issuer]);
    assert_anchor_error(&res, AnchorError::AccountNotSigner);
}

#[test]
fn init_roles_cannot_run_twice_for_a_mint() {
    let mut c = ctx().created();
    let res = c.h.send(
        &[ix_governance::init_roles(&c.issuer.pubkey(), &c.m())],
        &[&c.issuer, &c.mint],
    );
    // The role registry already exists, so account creation fails (system program "account already
    // in use" = custom 0) before the handler runs.
    assert_eq!(custom_error(&res).1, 0);
}

#[test]
fn init_roles_rejects_existing_fund_state() {
    // Unreachable through instructions (a fund always has a role registry, so the init above fails
    // first); the FundState is planted directly to prove the defensive check fires.
    let mut c = ctx();
    let key = c.fund_key();
    c.h.svm
        .set_account(
            key,
            solana_account::Account {
                lamports: 1_000_000,
                data: vec![1; 16],
                owner: mmf_common::ids::FUND_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let res = c.h.send(
        &[ix_governance::init_roles(&c.issuer.pubkey(), &c.m())],
        &[&c.issuer, &c.mint],
    );
    assert_anchor_error(&res, GovernanceError::FundAlreadyExists);
}

#[test]
fn init_registry_only_by_issuer_in_setup() {
    let mut c = ctx().created();
    let other = c.h.funded_keypair();
    let res = c.h.send(
        &[ix_hook::init_registry(&other.pubkey(), &c.m())],
        &[&other],
    );
    assert_anchor_error(&res, HookError::Unauthorized);

    // FundNotInSetup is unreachable here (an Open fund already has a registry), so the fund's
    // status is patched to Open to prove the check fires.
    let key = c.fund_key();
    fixtures::patch::<FundState>(&mut c.h, &key, |f| f.status = FundStatus::Open.as_u8());
    let res = c.h.send(
        &[ix_hook::init_registry(&c.issuer.pubkey(), &c.m())],
        &[&c.issuer],
    );
    assert_anchor_error(&res, HookError::FundNotInSetup);
}

#[test]
fn set_rules_rejections() {
    let mut c = ctx().created().with_registry();
    let p = Pubkey::new_unique();
    let us = fixtures::jurisdictions(&[*b"US"]);

    let other = c.h.funded_keypair();
    let ix = ix_hook::set_rules(
        &other.pubkey(),
        &c.m(),
        [p, Pubkey::default(), Pubkey::default(), Pubkey::default()],
        us,
        0,
    );
    assert_anchor_error(&c.h.send(&[ix], &[&other]), HookError::Unauthorized);

    let res = c.set_rules([p, Pubkey::default(), p, Pubkey::default()], us);
    assert_anchor_error(&res, HookError::DuplicateProvider);

    let mut unassigned = us;
    let aa = jur::index(*b"AA").unwrap();
    unassigned[aa / 64] |= 1 << (aa % 64);
    assert_anchor_error(
        &c.set_rules([p].repeat_to4(), unassigned),
        HookError::UnknownJurisdiction,
    );

    let mut beyond = us;
    beyond[10] |= 1 << (676 % 64);
    assert_anchor_error(
        &c.set_rules([p].repeat_to4(), beyond),
        HookError::UnknownJurisdiction,
    );

    // Empty provider slots (default pubkeys) are not duplicates of each other.
    c.set_rules(
        [p, Pubkey::default(), Pubkey::default(), Pubkey::default()],
        us,
    )
    .expect("valid rules");
}

/// Pads one provider with empty slots.
trait RepeatTo4 {
    fn repeat_to4(self) -> [Pubkey; 4];
}

impl RepeatTo4 for [Pubkey; 1] {
    fn repeat_to4(self) -> [Pubkey; 4] {
        [
            self[0],
            Pubkey::default(),
            Pubkey::default(),
            Pubkey::default(),
        ]
    }
}

#[test]
fn rules_and_roles_are_fixed_once_open() {
    let mut c = ctx()
        .created()
        .with_registry()
        .with_rules()
        .with_roles()
        .launched();
    let res = c.set_rules(
        [Pubkey::new_unique()].repeat_to4(),
        fixtures::jurisdictions(&[*b"GB"]),
    );
    assert_anchor_error(&res, HookError::FundNotInSetup); // FR-009
    let ix = ix_governance::set_roles(&c.issuer.pubkey(), &c.m(), &distinct_roles());
    assert_anchor_error(
        &c.h.send(&[ix], &[&c.issuer]),
        GovernanceError::FundNotInSetup,
    ); // US4-4
}

#[test]
fn set_roles_rejections() {
    let mut c = ctx().created();
    let other = c.h.funded_keypair();
    let ix = ix_governance::set_roles(&other.pubkey(), &c.m(), &distinct_roles());
    assert_anchor_error(&c.h.send(&[ix], &[&other]), GovernanceError::Unauthorized);

    let mut r = distinct_roles();
    r.auditor = Pubkey::default();
    let ix = ix_governance::set_roles(&c.issuer.pubkey(), &c.m(), &r);
    assert_anchor_error(
        &c.h.send(&[ix], &[&c.issuer]),
        GovernanceError::RoleUnassigned,
    );
}

#[test]
fn only_the_issuer_can_launch() {
    // US1-3: status unchanged after a rejected launch.
    let mut c = ctx().created().with_registry().with_rules().with_roles();
    let other = c.h.funded_keypair();
    assert_anchor_error(&c.launch_by(&other), FundError::Unauthorized);
    assert_eq!(c.status(), FundStatus::Setup.as_u8());
}

#[test]
fn an_open_fund_cannot_be_launched_again() {
    let mut c = ctx()
        .created()
        .with_registry()
        .with_rules()
        .with_roles()
        .launched(); // US1-6
    let issuer = c.issuer.insecure_clone();
    assert_anchor_error(&c.launch_by(&issuer), FundError::FundNotInSetup);
}

#[test]
fn launch_requires_roles_and_register_rules() {
    // US1-4, FR-006, in contract order.
    let mut c = ctx().created().with_registry().with_rules();
    let issuer = c.issuer.insecure_clone();
    assert_anchor_error(&c.launch_by(&issuer), FundError::RolesIncomplete);

    let mut c = ctx().created().with_roles();
    let issuer = c.issuer.insecure_clone();
    assert_anchor_error(&c.launch_by(&issuer), FundError::RegisterNotInitialized);

    let mut c = ctx().created().with_roles().with_registry();
    let issuer = c.issuer.insecure_clone();
    assert_anchor_error(&c.launch_by(&issuer), FundError::NoKycProvider);

    let mut c = ctx().created().with_roles().with_registry();
    c.set_rules([Pubkey::new_unique()].repeat_to4(), [0; SET_WORDS])
        .expect("providers only");
    let issuer = c.issuer.insecure_clone();
    assert_anchor_error(&c.launch_by(&issuer), FundError::NoAllowedJurisdiction);
    assert_eq!(c.status(), FundStatus::Setup.as_u8());
}
