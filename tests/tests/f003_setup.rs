//! F003 settlement setup (FR-001–FR-003; contracts/fund.md `set_cash_treasury`,
//! contracts/dvp_settlement.md `init_dvp`). T004 adds the cash-helper check, T013 the I4 check of
//! seeded units, T017 the setup rules, T029 the FR-003 creation checks.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use dvp_settlement::events::DvpInitialized;
use dvp_settlement::{DvpConfig, DvpError};
use fund::events::CashTreasurySet;
use fund::{FundError, FundState};
use mmf_common::ids::{SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use tests::assert::{assert_anchor_error, assert_instruction_error};
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, FundCtx, CASH_DECIMALS};
use tests::{units, Harness, Ix, TxResult};

// ---- helpers ------------------------------------------------------------------------------

/// The transfer agent's cash ATA, used as the treasury cash account (as in quickstart).
fn treasury_ata(h: &mut Harness, ctx: &FundCtx) -> Pubkey {
    fixtures::create_cash_ata(h, &ctx.transfer_agent.pubkey(), &ctx.cash_mint)
}

fn set_treasury_ix(signer: &Pubkey, ctx: &FundCtx, treasury: &Pubkey) -> Ix {
    mmf_sdk::ix_fund::set_cash_treasury(signer, &ctx.m(), treasury, &ctx.cash_mint)
}

fn set_treasury(h: &mut Harness, ctx: &FundCtx) -> Pubkey {
    let t = treasury_ata(h, ctx);
    h.send(
        &[set_treasury_ix(&ctx.issuer.pubkey(), ctx, &t)],
        &[&ctx.issuer],
    )
    .expect("set_cash_treasury");
    t
}

fn init_ix(h: &Harness, signer: &Pubkey, ctx: &FundCtx, expiry: Option<u32>) -> Ix {
    let program = fixtures::token_program_of(h, &ctx.cash_mint);
    mmf_sdk::ix_dvp::init_dvp(
        signer,
        &h.payer.pubkey(),
        &ctx.m(),
        &ctx.cash_mint,
        &program,
        expiry,
    )
}

fn init(h: &mut Harness, ctx: &FundCtx, expiry: Option<u32>) -> TxResult {
    let ix = init_ix(h, &ctx.issuer.pubkey(), ctx, expiry);
    h.send(&[ix], &[&ctx.issuer])
}

fn with_account(mut ix: Ix, index: usize, key: Pubkey) -> Ix {
    let m = &ix.accounts[index];
    ix.accounts[index] = AccountMeta {
        pubkey: key,
        is_signer: m.is_signer,
        is_writable: m.is_writable,
    };
    ix
}

// ---- T004, T013 ---------------------------------------------------------------------------

#[test]
fn cash_helpers_mint_to_an_owners_ata() {
    let mut h = Harness::new();
    let cash_mint = fixtures::create_cash_mint(&mut h, CASH_DECIMALS);
    let owner = Keypair::new().pubkey();
    let ata = fixtures::cash_ata(&h, &owner, &cash_mint);
    assert_eq!(fixtures::cash_balance(&h, &ata), 0, "no account yet");

    fixtures::mint_cash(&mut h, &cash_mint, &owner, 2_000_000_000_000);
    fixtures::mint_cash(&mut h, &cash_mint, &owner, 1);
    assert_eq!(fixtures::cash_balance(&h, &ata), 2_000_000_000_001);
    assert_eq!(
        fixtures::create_cash_ata(&mut h, &owner, &cash_mint),
        ata,
        "idempotent"
    );
}

#[test]
fn seeded_units_keep_units_outstanding_equal_to_supply() {
    let mut h = Harness::new();
    let (ctx, investor) = fixtures::fund_with_investor_units(&mut h, 0, 1_000_000);
    tests::assert::assert_i4(&h, &ctx.m());
    units::seed_units(&mut h, &ctx.m(), &investor.pubkey(), 250);
    tests::assert::assert_i4(&h, &ctx.m());
    let fs: FundState = read(&h, &ctx.fund_state());
    assert_eq!(fs.units_outstanding, 1_000_250);
}

// ---- set_cash_treasury (FR-001) -------------------------------------------------------------

#[test]
fn set_cash_treasury_only_by_the_issuer() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let t = treasury_ata(&mut h, &ctx);
    let investor = fixtures::new_investor(&mut h, &ctx);
    for who in [
        &ctx.fund_admin,
        &ctx.compliance,
        &ctx.transfer_agent,
        &ctx.keeper,
        &ctx.auditor,
        &investor,
    ] {
        let res = h.send(&[set_treasury_ix(&who.pubkey(), &ctx, &t)], &[who]);
        assert_anchor_error(&res, FundError::Unauthorized);
    }
    let fs: FundState = read(&h, &ctx.fund_state());
    assert_eq!(fs.cash_treasury, Pubkey::default(), "nothing stored");
}

#[test]
fn set_cash_treasury_rejects_a_foreign_role_registry() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let t = treasury_ata(&mut h, &ctx);
    // Index 2 is `role_registry`; the Registry is a compliance_hook account, not a RoleRegistry.
    let ix = with_account(
        set_treasury_ix(&ctx.issuer.pubkey(), &ctx, &t),
        2,
        ctx.registry(),
    );
    let res = h.send(&[ix], &[&ctx.issuer]);
    assert_anchor_error(&res, FundError::RoleRegistryMismatch);
}

#[test]
fn set_cash_treasury_is_set_once_in_setup_or_open() {
    // Open fund.
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let t = treasury_ata(&mut h, &ctx);
    let meta = h
        .send(
            &[set_treasury_ix(&ctx.issuer.pubkey(), &ctx, &t)],
            &[&ctx.issuer],
        )
        .expect("first set succeeds");
    let fs: FundState = read(&h, &ctx.fund_state());
    assert_eq!(fs.cash_treasury, t);
    let ev = one_cpi_event::<CashTreasurySet>(&meta);
    assert_eq!(
        (ev.fund, ev.cash_treasury, ev.signer),
        (ctx.fund_state(), t, ctx.issuer.pubkey())
    );

    let other = fixtures::create_cash_ata(&mut h, &ctx.keeper.pubkey(), &ctx.cash_mint);
    let res = h.send(
        &[set_treasury_ix(&ctx.issuer.pubkey(), &ctx, &other)],
        &[&ctx.issuer],
    );
    assert_anchor_error(&res, FundError::CashTreasuryAlreadySet);
    assert_eq!(read::<FundState>(&h, &ctx.fund_state()).cash_treasury, t);

    // Fund still in Setup.
    let (issuer, mint, cash_mint) = fixtures::create_standard_fund(&mut h);
    let t = fixtures::create_cash_ata(&mut h, &issuer.pubkey(), &cash_mint);
    let ix = mmf_sdk::ix_fund::set_cash_treasury(&issuer.pubkey(), &mint.pubkey(), &t, &cash_mint);
    h.send(&[ix], &[&issuer]).expect("allowed in Setup");
    let fs: FundState = read(&h, &mmf_sdk::pda::fund_state(&mint.pubkey()).0);
    assert_eq!(fs.cash_treasury, t);
}

#[test]
fn set_cash_treasury_rejects_accounts_that_are_not_the_cash_mints() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let i = ctx.issuer.pubkey();

    // A token account of another mint.
    let other_mint = fixtures::create_cash_mint(&mut h, CASH_DECIMALS);
    let foreign = fixtures::create_cash_ata(&mut h, &ctx.transfer_agent.pubkey(), &other_mint);
    let res = h.send(&[set_treasury_ix(&i, &ctx, &foreign)], &[&ctx.issuer]);
    assert_anchor_error(&res, FundError::InvalidCashTreasury);

    // Not a token account at all (a plain wallet).
    let wallet = ctx.transfer_agent.pubkey();
    let res = h.send(&[set_treasury_ix(&i, &ctx, &wallet)], &[&ctx.issuer]);
    assert_anchor_error(&res, FundError::InvalidCashTreasury);

    // The right treasury, but the wrong cash mint passed alongside it (index 4).
    let t = treasury_ata(&mut h, &ctx);
    let ix = with_account(set_treasury_ix(&i, &ctx, &t), 4, other_mint);
    let res = h.send(&[ix], &[&ctx.issuer]);
    assert_anchor_error(&res, FundError::InvalidCashTreasury);

    assert_eq!(
        read::<FundState>(&h, &ctx.fund_state()).cash_treasury,
        Pubkey::default()
    );
}

#[test]
fn set_cash_treasury_refuses_a_fee_bearing_cash_mint() {
    let mut h = Harness::new();
    let fee_mint = fixtures::create_fee_cash_mint(&mut h);
    let ctx = fixtures::launched_fund_with(&mut h, 0, fee_mint);
    let t = treasury_ata(&mut h, &ctx);
    let res = h.send(
        &[set_treasury_ix(&ctx.issuer.pubkey(), &ctx, &t)],
        &[&ctx.issuer],
    );
    assert_anchor_error(&res, FundError::UnsupportedCashMint);
}

// ---- init_dvp (FR-001, FR-002) -------------------------------------------------------------

#[test]
fn init_dvp_creates_config_and_both_escrows() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    set_treasury(&mut h, &ctx);
    let meta = init(&mut h, &ctx, None).expect("init_dvp");

    let fs = ctx.fund_state();
    let cfg: DvpConfig = read(&h, &mmf_sdk::pda::dvp_config(&fs).0);
    let (escrow, escrow_bump) = mmf_sdk::pda::escrow_authority(&fs);
    assert_eq!(cfg.version, DvpConfig::VERSION);
    assert_eq!(cfg.fund_state, fs);
    assert_eq!(cfg.next_seq, 1);
    assert_eq!(
        cfg.default_expiry_secs, 86_400,
        "24 h default (Clarification Q2)"
    );
    assert_eq!(cfg.escrow_bump, escrow_bump);

    let unit_escrow = mmf_sdk::pda::unit_escrow(&fs, &ctx.m());
    let program = fixtures::token_program_of(&h, &ctx.cash_mint);
    let cash_escrow = mmf_sdk::pda::cash_escrow(&fs, &ctx.cash_mint, &program);
    assert_eq!(
        h.account(&unit_escrow).unwrap().owner,
        TOKEN_2022_PROGRAM_ID
    );
    assert!(
        units::is_frozen(&h, &unit_escrow),
        "frozen until compliance approves"
    );
    assert_eq!(h.account(&cash_escrow).unwrap().owner, program);
    assert_eq!(fixtures::cash_balance(&h, &cash_escrow), 0);

    let ev = one_cpi_event::<DvpInitialized>(&meta);
    assert_eq!(ev.fund, fs);
    assert_eq!(ev.default_expiry_secs, 86_400);
    assert_eq!(
        (
            ev.escrow_authority,
            ev.unit_escrow,
            ev.cash_escrow,
            ev.signer
        ),
        (escrow, unit_escrow, cash_escrow, ctx.issuer.pubkey())
    );

    // Twice: the config already exists (Anchor `init`, system program AccountAlreadyInUse).
    let res = init(&mut h, &ctx, None);
    assert_instruction_error(&res, InstructionError::Custom(0));
}

#[test]
fn init_dvp_takes_a_custom_default_expiry_and_works_in_setup() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    set_treasury(&mut h, &ctx);
    init(&mut h, &ctx, Some(3_600)).expect("init_dvp");
    let cfg: DvpConfig = read(&h, &mmf_sdk::pda::dvp_config(&ctx.fund_state()).0);
    assert_eq!(cfg.default_expiry_secs, 3_600);

    // A fund still in Setup, with a classic SPL Token cash mint this time.
    let issuer = h.funded_keypair();
    let mint = Keypair::new();
    let cash = Keypair::new();
    let len = <anchor_spl::token::spl_token::state::Mint as solana_program_pack::Pack>::LEN;
    let lamports = h.svm.minimum_balance_for_rent_exemption(len);
    let ixs = [
        solana_system_interface::instruction::create_account(
            &h.payer.pubkey(),
            &cash.pubkey(),
            lamports,
            len as u64,
            &SPL_TOKEN_PROGRAM_ID,
        ),
        anchor_spl::token::spl_token::instruction::initialize_mint2(
            &SPL_TOKEN_PROGRAM_ID,
            &cash.pubkey(),
            &h.payer.pubkey(),
            None,
            CASH_DECIMALS,
        )
        .unwrap(),
    ];
    h.send(&ixs, &[&cash]).expect("classic cash mint");
    let m = mint.pubkey();
    let c = cash.pubkey();
    let ixs = [
        mmf_sdk::ix_governance::init_roles(&issuer.pubkey(), &m),
        mmf_sdk::ix_fund::create_fund(
            &issuer.pubkey(),
            &m,
            &c,
            fixtures::standard_terms(c),
            fixtures::FUND_NAME,
            fixtures::FUND_SYMBOL,
        ),
    ];
    h.send(&ixs, &[&issuer, &mint]).expect("create fund");
    let t = fixtures::create_cash_ata(&mut h, &issuer.pubkey(), &c);
    let ixs = [
        mmf_sdk::ix_fund::set_cash_treasury(&issuer.pubkey(), &m, &t, &c),
        mmf_sdk::ix_dvp::init_dvp(
            &issuer.pubkey(),
            &h.payer.pubkey(),
            &m,
            &c,
            &SPL_TOKEN_PROGRAM_ID,
            None,
        ),
    ];
    h.send(&ixs, &[&issuer]).expect("setup in Setup");
    let fs = mmf_sdk::pda::fund_state(&m).0;
    let cash_escrow = mmf_sdk::pda::cash_escrow(&fs, &c, &SPL_TOKEN_PROGRAM_ID);
    assert_eq!(h.account(&cash_escrow).unwrap().owner, SPL_TOKEN_PROGRAM_ID);
}

#[test]
fn init_dvp_only_by_the_issuer() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    set_treasury(&mut h, &ctx);
    for who in [
        &ctx.fund_admin,
        &ctx.compliance,
        &ctx.transfer_agent,
        &ctx.keeper,
        &ctx.auditor,
    ] {
        let ix = init_ix(&h, &who.pubkey(), &ctx, None);
        let res = h.send(&[ix], &[who]);
        assert_anchor_error(&res, DvpError::Unauthorized);
    }
    assert!(!h.exists(&mmf_sdk::pda::dvp_config(&ctx.fund_state()).0));
}

#[test]
fn init_dvp_rejects_accounts_of_another_fund() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let other = fixtures::launched_fund(&mut h, 0);
    set_treasury(&mut h, &ctx);
    let i = ctx.issuer.pubkey();

    // Index 3 is `role_registry`, index 4 `mint`, index 5 `cash_mint`.
    let other_roles = mmf_sdk::pda::role_registry(&other.fund_state()).0;
    for (index, key) in [(3, other_roles), (4, other.m()), (5, other.cash_mint)] {
        let ix = with_account(init_ix(&h, &i, &ctx, None), index, key);
        let res = h.send(&[ix], &[&ctx.issuer]);
        assert_anchor_error(&res, DvpError::FundMismatch);
    }
}

#[test]
fn init_dvp_needs_the_cash_treasury_first() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let res = init(&mut h, &ctx, None);
    assert_anchor_error(&res, DvpError::CashTreasuryNotSet);
}

#[test]
fn init_dvp_rejects_a_zero_default_expiry() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    set_treasury(&mut h, &ctx);
    let res = init(&mut h, &ctx, Some(0));
    assert_anchor_error(&res, DvpError::InvalidExpiry);
}

#[test]
fn init_dvp_refuses_a_fee_bearing_cash_mint() {
    let mut h = Harness::new();
    let fee_mint = fixtures::create_fee_cash_mint(&mut h);
    let ctx = fixtures::launched_fund_with(&mut h, 0, fee_mint);
    let t = treasury_ata(&mut h, &ctx);
    // `set_cash_treasury` refuses this mint (previous test), so no instruction can set it. Plant
    // the treasury directly to show `init_dvp` refuses the mint on its own (defence in depth).
    fixtures::patch::<FundState>(&mut h, &ctx.fund_state(), |fs| fs.cash_treasury = t);
    let res = init(&mut h, &ctx, None);
    assert_anchor_error(&res, DvpError::UnsupportedCashMint);
}

#[test]
fn init_dvp_is_not_blocked_by_escrow_accounts_created_in_advance() {
    // Anyone may create an ATA for any owner; setup must still succeed (research R4).
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    set_treasury(&mut h, &ctx);
    let escrow = mmf_sdk::pda::escrow_authority(&ctx.fund_state()).0;
    let griefer = h.funded_keypair();
    let ixs = [
        mmf_sdk::ix_hook::create_holding_account(&griefer.pubkey(), &ctx.m(), &escrow),
        spl_associated_token_account_interface::instruction::create_associated_token_account(
            &griefer.pubkey(),
            &escrow,
            &ctx.cash_mint,
            &fixtures::token_program_of(&h, &ctx.cash_mint),
        ),
    ];
    h.send(&ixs, &[&griefer])
        .expect("anyone can create the escrow ATAs");
    init(&mut h, &ctx, None).expect("init_dvp still succeeds");
}

#[test]
fn settlement_ready_fixture() {
    // T022: the fixture every story test starts from.
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let fs: FundState = read(&h, &s.fund_state());
    assert_eq!(
        (fs.nav, fs.nav_seq, fs.nav_period_end_day),
        (fixtures::NAV_ONE, 1, fixtures::SETTLE_DAY)
    );
    assert_eq!(fs.cash_treasury, s.cash_treasury);
    assert!(h.exists(&s.dvp_config));
    assert!(!units::is_frozen(&h, &s.unit_escrow), "escrow thawed");
    let rec: compliance_hook::HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &s.escrow).0);
    assert_eq!(
        rec.status,
        mmf_common::enums::HolderStatus::Approved.as_u8()
    );

    let a = fixtures::investor_with_cash(&mut h, &s, 2_000_000);
    let b = fixtures::investor_with_units(&mut h, &s, 500, 7);
    assert_eq!(
        fixtures::cash_balance(&h, &fixtures::cash_ata(&h, &a.pubkey(), &s.cash_mint)),
        2_000_000
    );
    assert_eq!(units::balance(&h, &s.m(), &b.pubkey()), 500);
    tests::assert::assert_i4(&h, &s.m());
}
