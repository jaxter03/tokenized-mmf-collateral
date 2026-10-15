//! Shared test setup: standard terms, a cash mint, and (from T039) a fully launched fund.

use anchor_lang::prelude::Pubkey;
use anchor_lang::AccountDeserialize;
use anchor_spl::token_2022::spl_token_2022 as t22;
use fund::FundTerms;
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_common::jurisdiction::{self, JurisdictionSet, SET_WORDS};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_signer::Signer;

use crate::harness::Harness;

pub const CASH_DECIMALS: u8 = 6;

/// Terms that pass every FR-003 check.
pub fn standard_terms(cash_mint: Pubkey) -> FundTerms {
    let mut terms_uri = [0u8; 128];
    let uri = b"https://example.org/fund/terms.pdf";
    terms_uri[..uri.len()].copy_from_slice(uri);
    FundTerms {
        fund_id: *b"MMF-USD-0001",
        cash_mint,
        target_nav: 100_000_000,
        max_nav_move_bps: 50,
        redemption_gate_bps: 1_000,
        distribution_mode: 0,
        distribution_cadence_days: 30,
        terms_hash: [0x5a; 32],
        terms_uri,
    }
}

pub const FUND_NAME: &str = "Tokenized Government MMF";
pub const FUND_SYMBOL: &str = "TMMF";

/// A jurisdiction set from codes, e.g. `jurisdictions(&[*b"US", *b"GB"])`.
pub fn jurisdictions(codes: &[[u8; 2]]) -> JurisdictionSet {
    let mut set = [0u64; SET_WORDS];
    for c in codes {
        assert!(
            jurisdiction::set_insert(&mut set, *c),
            "{c:?} must be assigned"
        );
    }
    set
}

/// Creates a Token-2022 cash mint with `decimals` (6 for a valid cash asset).
pub fn create_cash_mint(h: &mut Harness, decimals: u8) -> Pubkey {
    let mint = Keypair::new();
    let len = t22::state::Mint::LEN;
    let lamports = h.svm.minimum_balance_for_rent_exemption(len);
    let ixs = [
        solana_system_interface::instruction::create_account(
            &h.payer.pubkey(),
            &mint.pubkey(),
            lamports,
            len as u64,
            &TOKEN_2022_PROGRAM_ID,
        ),
        t22::instruction::initialize_mint2(
            &TOKEN_2022_PROGRAM_ID,
            &mint.pubkey(),
            &h.payer.pubkey(),
            None,
            decimals,
        )
        .unwrap(),
    ];
    h.send(&ixs, &[&mint]).expect("create cash mint");
    mint.pubkey()
}

/// A 6-decimal Token-2022 cash mint with the TransferFeeConfig extension (1 bp fee), which
/// settlement must refuse (F003 research R14). The harness payer is its mint authority.
pub fn create_fee_cash_mint(h: &mut Harness) -> Pubkey {
    use t22::extension::ExtensionType;
    let mint = Keypair::new();
    let len = ExtensionType::try_calculate_account_len::<t22::state::Mint>(&[
        ExtensionType::TransferFeeConfig,
    ])
    .unwrap();
    let lamports = h.svm.minimum_balance_for_rent_exemption(len);
    let payer = h.payer.pubkey();
    let ixs = [
        solana_system_interface::instruction::create_account(
            &payer,
            &mint.pubkey(),
            lamports,
            len as u64,
            &TOKEN_2022_PROGRAM_ID,
        ),
        t22::extension::transfer_fee::instruction::initialize_transfer_fee_config(
            &TOKEN_2022_PROGRAM_ID,
            &mint.pubkey(),
            Some(&payer),
            Some(&payer),
            1,
            u64::MAX,
        )
        .unwrap(),
        t22::instruction::initialize_mint2(
            &TOKEN_2022_PROGRAM_ID,
            &mint.pubkey(),
            &payer,
            None,
            CASH_DECIMALS,
        )
        .unwrap(),
    ];
    h.send(&ixs, &[&mint]).expect("create fee cash mint");
    mint.pubkey()
}

/// Token program that owns `mint` (SPL Token or Token-2022).
pub fn token_program_of(h: &Harness, mint: &Pubkey) -> Pubkey {
    h.account(mint).expect("mint exists").owner
}

/// `owner`'s associated token account for the cash mint, under the cash mint's own token
/// program (F003, research R14).
pub fn cash_ata(h: &Harness, owner: &Pubkey, cash_mint: &Pubkey) -> Pubkey {
    spl_associated_token_account_interface::address::get_associated_token_address_with_program_id(
        owner,
        cash_mint,
        &token_program_of(h, cash_mint),
    )
}

/// Creates `owner`'s cash ATA if it doesn't exist yet. Returns its address.
pub fn create_cash_ata(h: &mut Harness, owner: &Pubkey, cash_mint: &Pubkey) -> Pubkey {
    let program = token_program_of(h, cash_mint);
    let ix = spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(
        &h.payer.pubkey(),
        owner,
        cash_mint,
        &program,
    );
    h.send(&[ix], &[]).expect("create cash ATA");
    cash_ata(h, owner, cash_mint)
}

/// Mints `amount` cash base units to `owner`'s cash ATA (created if missing). The harness payer
/// is the cash mint's authority (`create_cash_mint`).
pub fn mint_cash(h: &mut Harness, cash_mint: &Pubkey, owner: &Pubkey, amount: u64) {
    let program = token_program_of(h, cash_mint);
    let ata = create_cash_ata(h, owner, cash_mint);
    let ix = t22::instruction::mint_to(&program, cash_mint, &ata, &h.payer.pubkey(), &[], amount)
        .expect("mint_to instruction");
    h.send(&[ix], &[]).expect("mint cash");
}

/// Balance of a cash token account (0 if it doesn't exist).
pub fn cash_balance(h: &Harness, token_account: &Pubkey) -> u64 {
    crate::units::token_balance(h, token_account)
}

/// Deserializes an Anchor account (discriminator checked).
pub fn read<T: AccountDeserialize>(h: &Harness, key: &Pubkey) -> T {
    let data = h.data(key);
    T::try_deserialize(&mut data.as_slice()).expect("deserialize account")
}

/// Rewrites an Anchor account in place. Only for tests of conditions no instruction can reach
/// (documented exception, quickstart.md FR-023); each caller says why in a comment.
pub fn patch<T: AccountDeserialize + anchor_lang::AccountSerialize>(
    h: &mut Harness,
    key: &Pubkey,
    edit: impl FnOnce(&mut T),
) {
    let mut acc = h.account(key).expect("account to patch");
    let mut value = T::try_deserialize(&mut acc.data.as_slice()).expect("deserialize");
    edit(&mut value);
    let mut out = Vec::with_capacity(acc.data.len());
    value.try_serialize(&mut out).expect("serialize");
    assert_eq!(
        out.len(),
        acc.data.len(),
        "patch must not change the account size"
    );
    acc.data = out;
    h.svm.set_account(*key, acc).expect("set_account");
}

/// Raw TLV lookup on a Token-2022 mint (`spl-token-2022-interface` 2.1.0 can't enumerate a mint
/// carrying PermissionedBurn, research R8). Returns the extension's data bytes.
pub fn mint_tlv(data: &[u8], extension_type: u16) -> Option<Vec<u8>> {
    // Base mint padded to the account size (165) plus the 1-byte account type.
    let mut i = 166;
    while i + 4 <= data.len() {
        let ty = u16::from_le_bytes([data[i], data[i + 1]]);
        let len = u16::from_le_bytes([data[i + 2], data[i + 3]]) as usize;
        if ty == 0 && len == 0 {
            return None; // uninitialized tail
        }
        if ty == extension_type {
            return Some(data[i + 4..i + 4 + len].to_vec());
        }
        i += 4 + len;
    }
    None
}

/// Lowercase hex, as used for the `fund_id` and `terms_hash` metadata fields.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Sends init_roles + create_fund with standard terms. Returns `(issuer, mint, cash_mint)`.
pub fn create_standard_fund(h: &mut Harness) -> (Keypair, Keypair, Pubkey) {
    let cash_mint = create_cash_mint(h, CASH_DECIMALS);
    create_standard_fund_with(h, cash_mint)
}

/// As `create_standard_fund`, with a cash mint the caller made (F003: e.g. a fee-bearing mint).
pub fn create_standard_fund_with(h: &mut Harness, cash_mint: Pubkey) -> (Keypair, Keypair, Pubkey) {
    create_fund_with_terms(h, standard_terms(cash_mint))
}

/// As `create_standard_fund`, with the caller's terms (F004: a wider price-move guard).
pub fn create_fund_with_terms(h: &mut Harness, terms: FundTerms) -> (Keypair, Keypair, Pubkey) {
    let cash_mint = terms.cash_mint;
    let issuer = h.funded_keypair();
    let mint = Keypair::new();
    let m = mint.pubkey();
    let ixs = [
        mmf_sdk::ix_governance::init_roles(&issuer.pubkey(), &m),
        mmf_sdk::ix_fund::create_fund(
            &issuer.pubkey(),
            &m,
            &cash_mint,
            terms,
            FUND_NAME,
            FUND_SYMBOL,
        ),
    ];
    h.send(&ixs, &[&issuer, &mint]).expect("create fund");
    (issuer, mint, cash_mint)
}

/// A launched fund with every role held by its own keypair (T039). Used by US2–US4 tests.
pub struct FundCtx {
    pub issuer: Keypair,
    pub mint: Keypair,
    pub cash_mint: Pubkey,
    pub fund_admin: Keypair,
    pub transfer_agent: Keypair,
    pub compliance: Keypair,
    pub keeper: Keypair,
    pub auditor: Keypair,
    /// The one recognised mock KYC provider.
    pub provider: Keypair,
}

impl FundCtx {
    pub fn m(&self) -> Pubkey {
        self.mint.pubkey()
    }

    pub fn fund_state(&self) -> Pubkey {
        mmf_sdk::pda::fund_state(&self.m()).0
    }

    pub fn registry(&self) -> Pubkey {
        mmf_sdk::pda::registry(&self.m()).0
    }

    pub fn roles(&self) -> mmf_sdk::ix_governance::Roles {
        mmf_sdk::ix_governance::Roles {
            fund_admin: self.fund_admin.pubkey(),
            transfer_agent: self.transfer_agent.pubkey(),
            compliance: self.compliance.pubkey(),
            keeper: self.keeper.pubkey(),
            auditor: self.auditor.pubkey(),
        }
    }
}

/// Creates, configures and launches a fund: one provider, `US` and `GB` allowed, holder cap
/// `cap` (0 = no cap), five distinct role keypairs (all funded so they can pay fees).
pub fn launched_fund(h: &mut Harness, cap: u32) -> FundCtx {
    let cash_mint = create_cash_mint(h, CASH_DECIMALS);
    launched_fund_with(h, cap, cash_mint)
}

/// As `launched_fund`, with a cash mint the caller made (F003).
pub fn launched_fund_with(h: &mut Harness, cap: u32, cash_mint: Pubkey) -> FundCtx {
    launched_fund_terms(h, cap, standard_terms(cash_mint))
}

/// As `launched_fund`, with the caller's terms (F004).
pub fn launched_fund_terms(h: &mut Harness, cap: u32, terms: FundTerms) -> FundCtx {
    let (issuer, mint, cash_mint) = create_fund_with_terms(h, terms);
    let ctx = FundCtx {
        issuer,
        mint,
        cash_mint,
        fund_admin: h.funded_keypair(),
        transfer_agent: h.funded_keypair(),
        compliance: h.funded_keypair(),
        keeper: h.funded_keypair(),
        auditor: h.funded_keypair(),
        provider: Keypair::new(),
    };
    let m = ctx.m();
    let i = ctx.issuer.pubkey();
    let providers = [
        ctx.provider.pubkey(),
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    let ixs = [
        mmf_sdk::ix_hook::init_registry(&i, &m),
        mmf_sdk::ix_hook::set_rules(&i, &m, providers, jurisdictions(&[*b"US", *b"GB"]), cap),
        mmf_sdk::ix_governance::set_roles(&i, &m, &ctx.roles()),
        mmf_sdk::ix_fund::launch(&i, &m),
    ];
    h.send(&ixs, &[&ctx.issuer])
        .expect("configure and launch fund");
    ctx
}

/// Seconds in a year; mock statements expire a year after "now" unless a test says otherwise.
pub const YEAR: i64 = 365 * 24 * 60 * 60;

/// A statement for `owner` signed by the fund's mock provider.
pub fn signed_statement(
    ctx: &FundCtx,
    owner: &Pubkey,
    jurisdiction: [u8; 2],
    expiry_ts: i64,
) -> mmf_sdk::kyc::SignedStatement {
    mmf_sdk::kyc::sign(
        &ctx.provider,
        &mmf_sdk::kyc::statement(ctx.m(), *owner, jurisdiction, 1, expiry_ts),
    )
}

/// Onboards an investor the SC-005 way: the institution registers itself, then compliance sends
/// `[create ATA, ed25519, approve_investor, thaw]`. KYC expires a year from now.
pub fn onboard_investor(h: &mut Harness, ctx: &FundCtx, owner: &Keypair, jurisdiction: [u8; 2]) {
    let m = ctx.m();
    let o = owner.pubkey();
    let ix = mmf_sdk::ix_hook::register_holder(&o, &o, &m, &o, 0);
    h.send(&[ix], &[owner]).expect("register investor");
    let expiry = crate::clock::now(&h.svm) + YEAR;
    let signed = signed_statement(ctx, &o, jurisdiction, expiry);
    let ixs = mmf_sdk::ix_hook::onboarding_tx(
        &h.payer.pubkey(),
        &ctx.compliance.pubkey(),
        &m,
        &o,
        &signed,
    );
    h.send(&ixs, &[&ctx.compliance])
        .expect("approve and thaw investor");
}

/// A funded investor keypair, onboarded in `US`.
pub fn new_investor(h: &mut Harness, ctx: &FundCtx) -> Keypair {
    let k = h.funded_keypair();
    onboard_investor(h, ctx, &k, *b"US");
    k
}

/// Registers, approves and thaws the fund treasury (owner = fund PDA `["treasury", fund_state]`).
/// Returns the treasury owner.
pub fn onboard_treasury(h: &mut Harness, ctx: &FundCtx) -> Pubkey {
    let m = ctx.m();
    let c = ctx.compliance.pubkey();
    let treasury = mmf_sdk::pda::treasury(&ctx.fund_state()).0;
    let ixs = [
        mmf_sdk::ix_hook::register_holder(&c, &c, &m, &treasury, 1),
        mmf_sdk::ix_hook::approve_fund_account(&c, &m, &treasury),
        mmf_sdk::ix_hook::create_holding_account(&h.payer.pubkey(), &m, &treasury),
        mmf_sdk::ix_hook::thaw_holding_account(
            &h.payer.pubkey(),
            &m,
            &treasury,
            &mmf_sdk::pda::holding_account(&treasury, &m),
        ),
    ];
    h.send(&ixs, &[&ctx.compliance]).expect("onboard treasury");
    treasury
}

/// A launched fund with one onboarded `US` investor holding `units`. Returns the investor.
pub fn fund_with_investor_units(h: &mut Harness, cap: u32, units: u64) -> (FundCtx, Keypair) {
    let ctx = launched_fund(h, cap);
    let investor = new_investor(h, &ctx);
    crate::units::seed_units(h, &ctx.m(), &investor.pubkey(), units);
    (ctx, investor)
}

/// Transfers `amount` from `from`'s holding account to `to_owner`'s, signed by `from`.
pub fn transfer(
    h: &mut Harness,
    ctx: &FundCtx,
    from: &Keypair,
    to_owner: &Pubkey,
    amount: u64,
) -> crate::harness::TxResult {
    let ix = mmf_sdk::transfer::transfer_between_owners(&ctx.m(), &from.pubkey(), to_owner, amount);
    h.send(&[ix], &[from])
}

/// Publishes as the fund's fund admin (F002), signed and paid by keys the harness controls.
/// The record address uses `FundState.nav_seq + 1`.
pub fn publish(
    h: &mut Harness,
    ctx: &FundCtx,
    nav: u64,
    daily_rate_e12: i64,
    days_covered: u16,
) -> crate::harness::TxResult {
    let next = read::<fund::FundState>(h, &ctx.fund_state())
        .nav_seq
        .wrapping_add(1);
    let ix = mmf_sdk::ix_fund::publish_daily(
        &ctx.fund_admin.pubkey(),
        &h.payer.pubkey(),
        &ctx.m(),
        next,
        nav,
        daily_rate_e12,
        days_covered,
    );
    h.send(&[ix], &[&ctx.fund_admin])
}

/// The NavRecord with sequence number `seq` (F002).
pub fn read_nav_record(h: &Harness, ctx: &FundCtx, seq: u64) -> fund::NavRecord {
    read(h, &mmf_sdk::pda::nav_record(&ctx.fund_state(), seq).0)
}

/// Invariant I12 (shared-model §7), checked from the records alone and written out here rather
/// than through `mmf_common::accrual`: the counter equals the latest record's `index_after`, each
/// record adds `rate × 10^6 × days`, periods are contiguous, and the fund's last covered day is
/// the latest record's.
#[track_caller]
pub fn check_i12(h: &Harness, ctx: &FundCtx) {
    let fs: fund::FundState = read(h, &ctx.fund_state());
    let mut index: u128 = 0;
    let mut next_start: Option<u32> = None;
    for seq in 1..=fs.nav_seq {
        let r = read_nav_record(h, ctx, seq);
        assert_eq!(r.seq, seq, "record {seq} seq");
        index += u128::from(r.daily_rate_e12) * 1_000_000 * u128::from(r.days_covered);
        assert_eq!(r.index_after, index, "record {seq} index_after");
        if let Some(start) = next_start {
            assert_eq!(
                r.period_start_day, start,
                "record {seq} starts after the previous period"
            );
        }
        next_start = Some(r.period_start_day + u32::from(r.days_covered));
    }
    assert_eq!(
        fs.accrual_index, index,
        "accrual_index == latest index_after"
    );
    if fs.nav_seq > 0 {
        assert_eq!(
            fs.nav_period_end_day,
            next_start.unwrap() - 1,
            "nav_period_end_day"
        );
    }
}

// ---- F003: settlement ---------------------------------------------------------------------

/// UTC day the settlement fixtures start on: 2026-10-13 (Tuesday).
pub const SETTLE_DAY: u32 = 20_739;
/// $1.00 (8 dp).
pub const NAV_ONE: u64 = 100_000_000;

/// A fund ready for settlement (T022, quickstart "Setting up a fund for settlement").
pub struct SettleCtx {
    pub fund: FundCtx,
    /// The cash mint's token program (the fixture's cash mint is Token-2022).
    pub cash_program: Pubkey,
    /// The transfer agent's cash ATA, designated as the treasury cash account.
    pub cash_treasury: Pubkey,
    pub dvp_config: Pubkey,
    pub escrow: Pubkey,
    pub unit_escrow: Pubkey,
    pub cash_escrow: Pubkey,
}

impl std::ops::Deref for SettleCtx {
    type Target = FundCtx;
    fn deref(&self) -> &FundCtx {
        &self.fund
    }
}

/// A launched fund (no holder cap) on day 20 739, one hour in, with:
/// - a $1.00 price published for that day (rate 0);
/// - the transfer agent's cash ATA set as the treasury cash account;
/// - `init_dvp(None)` (24 h default expiry);
/// - the escrow registered and approved by compliance, and its unit account thawed.
pub fn settlement_ready_fund(h: &mut Harness) -> SettleCtx {
    settlement_ready_fund_with_cap(h, 0)
}

/// As `settlement_ready_fund`, with holder cap `cap` (0 = none).
pub fn settlement_ready_fund_with_cap(h: &mut Harness, cap: u32) -> SettleCtx {
    settlement_ready_fund_opts(h, cap, true)
}

/// As `settlement_ready_fund`; `priced = false` skips the $1.00 publish (no price yet).
pub fn settlement_ready_fund_opts(h: &mut Harness, cap: u32, priced: bool) -> SettleCtx {
    let cash_mint = create_cash_mint(h, CASH_DECIMALS);
    settlement_ready_fund_terms(h, cap, priced, standard_terms(cash_mint))
}

/// As `settlement_ready_fund_opts`, with the caller's fund terms (F004).
pub fn settlement_ready_fund_terms(
    h: &mut Harness,
    cap: u32,
    priced: bool,
    terms: FundTerms,
) -> SettleCtx {
    // Time first: onboarding sets KYC expiry a year from "now".
    crate::clock::set_utc_day(&mut h.svm, SETTLE_DAY, 3_600);
    let fund = launched_fund_terms(h, cap, terms);
    if priced {
        publish(h, &fund, NAV_ONE, 0, 1).expect("publish $1.00");
    }

    let m = fund.m();
    let fs = fund.fund_state();
    let cash_program = token_program_of(h, &fund.cash_mint);
    let cash_treasury = create_cash_ata(h, &fund.transfer_agent.pubkey(), &fund.cash_mint);
    let i = fund.issuer.pubkey();
    let ixs = [
        mmf_sdk::ix_fund::set_cash_treasury(&i, &m, &cash_treasury, &fund.cash_mint),
        mmf_sdk::ix_dvp::init_dvp(
            &i,
            &h.payer.pubkey(),
            &m,
            &fund.cash_mint,
            &cash_program,
            None,
        ),
    ];
    h.send(&ixs, &[&fund.issuer]).expect("settlement setup");

    let escrow = mmf_sdk::pda::escrow_authority(&fs).0;
    let unit_escrow = mmf_sdk::pda::unit_escrow(&fs, &m);
    let c = fund.compliance.pubkey();
    let ixs = [
        mmf_sdk::ix_hook::register_holder(&c, &c, &m, &escrow, 2),
        mmf_sdk::ix_hook::approve_fund_account(&c, &m, &escrow),
        mmf_sdk::ix_hook::thaw_holding_account(&h.payer.pubkey(), &m, &escrow, &unit_escrow),
    ];
    h.send(&ixs, &[&fund.compliance])
        .expect("approve and thaw escrow");

    SettleCtx {
        cash_program,
        cash_treasury,
        dvp_config: mmf_sdk::pda::dvp_config(&fs).0,
        escrow,
        unit_escrow,
        cash_escrow: mmf_sdk::pda::cash_escrow(&fs, &fund.cash_mint, &cash_program),
        fund,
    }
}

/// Sets a token account's balance directly (`set_account`). Only for planting states no
/// instruction can reach; each test that uses it says why.
pub fn patch_token_amount(h: &mut Harness, token_account: &Pubkey, amount: u64) {
    use t22::extension::StateWithExtensionsMut;
    let mut acc = h.account(token_account).expect("token account exists");
    {
        let mut st = StateWithExtensionsMut::<t22::state::Account>::unpack(&mut acc.data).unwrap();
        st.base.amount = amount;
        st.pack_base();
    }
    h.svm.set_account(*token_account, acc).unwrap();
}

/// An onboarded `US` investor holding `cash` in its cash ATA (and no units).
pub fn investor_with_cash(h: &mut Harness, s: &SettleCtx, cash: u64) -> Keypair {
    let k = new_investor(h, s);
    create_cash_ata(h, &k.pubkey(), &s.cash_mint);
    if cash > 0 {
        mint_cash(h, &s.cash_mint, &k.pubkey(), cash);
    }
    k
}

/// An onboarded `US` investor holding `units` (seeded, so I4 holds) and `cash`.
pub fn investor_with_units(h: &mut Harness, s: &SettleCtx, units: u64, cash: u64) -> Keypair {
    let k = investor_with_cash(h, s, cash);
    if units > 0 {
        crate::units::seed_units(h, &s.m(), &k.pubkey(), units);
    }
    k
}

// ---- F004: collateral ---------------------------------------------------------------------

/// The collateral fixtures' price-move guard: 5 %, so test publishes of $0.985–$1.01 pass F002's
/// check (the standard terms allow 0.5 %).
pub const COLLATERAL_MAX_MOVE_BPS: u16 = 500;
/// Units the giver holds in the collateral fixtures: 2 000 000.
pub const GIVER_UNITS: u64 = 2_000_000_000_000;

/// A settlement-ready fund with a giver G holding `GIVER_UNITS` and a taker T (T017).
pub struct CollatBase {
    pub settle: SettleCtx,
    pub giver: Keypair,
    pub taker: Keypair,
}

impl std::ops::Deref for CollatBase {
    type Target = SettleCtx;
    fn deref(&self) -> &SettleCtx {
        &self.settle
    }
}

/// The worked terms (tasks.md "Fixed values"): haircut 2 %, MTA $10 000, 4 h to meet a call,
/// 2 h grace, pledge limit 5 000 000 units, 26 h price age, taker may raise alone.
pub fn worked_terms(giver: &Pubkey, taker: &Pubkey, nonce: u64) -> collateral::ProposeArgs {
    collateral::ProposeArgs {
        giver: *giver,
        taker: *taker,
        nonce,
        mode: 0,
        income_to: 0,
        haircut_bps: 200,
        max_nav_age_secs: 93_600,
        call_cure_secs: 14_400,
        enforcement_grace_secs: 7_200,
        min_transfer_value: 10_000_000_000,
        concentration_cap_units: 5_000_000_000_000,
        taker_may_call_unilaterally: true,
        agreement_hash: [7; 32],
    }
}

/// A settlement-ready fund (day 20 739, one hour in; no holder cap; 5 % price-move guard) with
/// $1.00 published, giver G holding 2 000 000 units (seeded, so I4 holds) and taker T, both
/// onboarded `US` investors.
pub fn collateral_base_fund(h: &mut Harness) -> CollatBase {
    collateral_base_fund_opts(h, true)
}

/// As `collateral_base_fund`; `priced = false` skips the $1.00 publish (no price yet).
pub fn collateral_base_fund_opts(h: &mut Harness, priced: bool) -> CollatBase {
    let cash_mint = create_cash_mint(h, CASH_DECIMALS);
    let terms = FundTerms {
        max_nav_move_bps: COLLATERAL_MAX_MOVE_BPS,
        ..standard_terms(cash_mint)
    };
    let settle = settlement_ready_fund_terms(h, 0, priced, terms);
    let giver = investor_with_units(h, &settle, GIVER_UNITS, 0);
    let taker = new_investor(h, &settle);
    CollatBase {
        settle,
        giver,
        taker,
    }
}

/// Which agreement `collateral_ready_fund_opts` sets up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ready {
    /// The worked terms, signed by both: Active.
    Standard,
    /// As Standard, but the taker may not raise exposure alone.
    TwoStep,
    /// As Standard, on a fund with no published price.
    Unpriced,
    /// Proposed by the taker, not signed (every FR-007 test).
    ProposedOnly,
}

/// A collateral base fund plus the agreement (nonce 1) between its giver and taker (T025).
pub struct CollatCtx {
    pub base: CollatBase,
    pub agr: mmf_sdk::ix_collateral::Agr,
}

impl std::ops::Deref for CollatCtx {
    type Target = CollatBase;
    fn deref(&self) -> &CollatBase {
        &self.base
    }
}

impl CollatCtx {
    pub fn agreement(&self) -> Pubkey {
        self.agr.agreement()
    }
}

/// The taker proposes the worked terms (nonce 1) and the giver signs: an Active agreement.
pub fn collateral_ready_fund(h: &mut Harness) -> CollatCtx {
    collateral_ready_fund_opts(h, Ready::Standard)
}

pub fn collateral_ready_fund_opts(h: &mut Harness, ready: Ready) -> CollatCtx {
    let base = collateral_base_fund_opts(h, ready != Ready::Unpriced);
    let mut args = worked_terms(&base.giver.pubkey(), &base.taker.pubkey(), 1);
    if ready == Ready::TwoStep {
        args.taker_may_call_unilaterally = false;
    }
    let agr = mmf_sdk::ix_collateral::Agr {
        mint: base.m(),
        giver: base.giver.pubkey(),
        taker: base.taker.pubkey(),
        nonce: 1,
    };
    crate::collateral::propose(h, &base.m(), &base.taker, args).expect("propose");
    if ready != Ready::ProposedOnly {
        crate::collateral::sign(h, &base.giver, &agr).expect("sign");
    }
    CollatCtx { base, agr }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collateral_ready_fund_variants() {
        let mut h = Harness::new();
        for (ready, status, unilateral) in [
            (Ready::Standard, 1, true),
            (Ready::TwoStep, 1, false),
            (Ready::Unpriced, 1, true),
            (Ready::ProposedOnly, 0, true),
        ] {
            let c = collateral_ready_fund_opts(&mut h, ready);
            let a: collateral::PledgeAgreement = read(&h, &c.agreement());
            assert_eq!(a.status, status, "{ready:?}");
            assert_eq!(a.taker_may_call_unilaterally, unilateral, "{ready:?}");
        }
    }

    #[test]
    fn collateral_base_fund_holds_the_worked_figures() {
        let mut h = Harness::new();
        let c = collateral_base_fund(&mut h);
        assert_eq!(
            crate::units::balance(&h, &c.m(), &c.giver.pubkey()),
            GIVER_UNITS
        );
        assert_eq!(crate::units::balance(&h, &c.m(), &c.taker.pubkey()), 0);
        let fs: fund::FundState = read(&h, &c.fund_state());
        assert_eq!((fs.nav, fs.nav_seq), (NAV_ONE, 1));
        assert_eq!(fs.terms.max_nav_move_bps, COLLATERAL_MAX_MOVE_BPS);
        let unpriced = collateral_base_fund_opts(&mut h, false);
        let fs: fund::FundState = read(&h, &unpriced.fund_state());
        assert_eq!(fs.nav_seq, 0);
    }

    #[test]
    fn launched_fund_is_open_with_rules_and_roles() {
        let mut h = Harness::new();
        let c = launched_fund(&mut h, 2);
        let fs: fund::FundState = read(&h, &c.fund_state());
        assert_eq!(fs.status, mmf_common::enums::FundStatus::Open.as_u8());
        let reg: compliance_hook::Registry = read(&h, &c.registry());
        assert_eq!(reg.max_holders, 2);
        assert_eq!(reg.kyc_providers[0], c.provider.pubkey());
        assert!(jurisdiction::set_contains(
            &reg.allowed_jurisdictions,
            *b"US"
        ));
        let roles: governance::RoleRegistry = read(&h, &fs.role_registry);
        assert_eq!(roles.compliance, c.compliance.pubkey());
        assert_eq!(roles.issuer, c.issuer.pubkey());
    }
}
