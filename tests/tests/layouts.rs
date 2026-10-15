//! T019 (F001), T008 (F002), T011 (F003), T010 (F004): account layouts are fixed, and the read-only views in
//! `mmf_common::views` decode program-serialized accounts to the same values (guards against
//! layout drift, F001 research R12).

use anchor_lang::prelude::Pubkey;
use anchor_lang::{AccountSerialize, Discriminator, Space};
use collateral::{CollateralCall, Pledge, PledgeAgreement};
use compliance_hook::{HolderState, Registry};
use dvp_settlement::{DvpConfig, SettlementInstruction};
use fund::{FundState, FundTerms, NavRecord};
use governance::RoleRegistry;
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, DVP_SETTLEMENT_PROGRAM_ID};
use mmf_common::jurisdiction::{self, SET_WORDS};
use mmf_common::seeds;
use mmf_common::views::{
    FundStateView, HolderStateView, RegistryView, RoleRegistryView, SettlementView,
    FUND_STATE_DISCRIMINATOR, HOLDER_STATE_DISCRIMINATOR, REGISTRY_DISCRIMINATOR,
    ROLE_REGISTRY_DISCRIMINATOR, SETTLEMENT_INSTRUCTION_DISCRIMINATOR,
};

const RESERVED: [u8; 64] = [0xAB; 64];
/// FundState's reserved bytes after F002 took 12 of them (F002 research R10) and F003 took 8
/// more (F003 research R9).
const FUND_RESERVED: [u8; 44] = [0xAB; 44];
/// `8 + FundState::INIT_SPACE` as shipped in F001; F002 and F003 must not change it.
const F001_FUND_STATE_LEN: usize =
    8 + 1 + 32 + 32 + 219 + 1 + 8 + 8 + 8 + 16 + 8 + 8 + 8 + 8 + 32 + 4 + 1 + 64;

fn bytes<T: AccountSerialize>(account: &T) -> Vec<u8> {
    let mut out = Vec::new();
    account.try_serialize(&mut out).unwrap();
    out
}

/// Every account: total size = 8 + INIT_SPACE, `version` is the first byte after the
/// discriminator, `_reserved` is the last 64 bytes.
fn assert_frame<T: Space>(data: &[u8], discriminator: &[u8], version: u8) {
    assert_eq!(data.len(), 8 + T::INIT_SPACE, "serialized size");
    assert_eq!(&data[..8], discriminator, "discriminator");
    assert_eq!(data[8], version, "version first");
    assert_eq!(&data[data.len() - 64..], &RESERVED, "_reserved last");
}

#[test]
fn discriminator_constants_match_anchor() {
    assert_eq!(FundState::DISCRIMINATOR, FUND_STATE_DISCRIMINATOR);
    assert_eq!(RoleRegistry::DISCRIMINATOR, ROLE_REGISTRY_DISCRIMINATOR);
    assert_eq!(Registry::DISCRIMINATOR, REGISTRY_DISCRIMINATOR);
    assert_eq!(HolderState::DISCRIMINATOR, HOLDER_STATE_DISCRIMINATOR);
    assert_eq!(
        SettlementInstruction::DISCRIMINATOR,
        SETTLEMENT_INSTRUCTION_DISCRIMINATOR
    );
}

#[test]
fn fund_state_layout_and_view() {
    let terms = FundTerms {
        fund_id: [1; 12],
        cash_mint: Pubkey::new_unique(),
        target_nav: 100_000_000,
        max_nav_move_bps: 50,
        redemption_gate_bps: 1_000,
        distribution_mode: 1,
        distribution_cadence_days: 30,
        terms_hash: [2; 32],
        terms_uri: [3; 128],
    };
    let fs = FundState {
        version: FundState::VERSION,
        mint: Pubkey::new_unique(),
        role_registry: Pubkey::new_unique(),
        terms,
        status: 1,
        nav: 100_000_000,
        nav_ts: 1_791_849_600,
        nav_seq: 3,
        accrual_index: 0x0102_0304_0506_0708_090A_0B0C_0D0E_0F10,
        daily_rate_e12: 123_287_671,
        units_outstanding: 2_000_000_000_000,
        redeemed_today: 500_000_000_000,
        day_start_ts: 20_739 * 86_400,
        cash_treasury: Pubkey::new_unique(),
        current_cycle: 0,
        bump: 254,
        nav_period_end_day: 20_741,
        pending_override_nav: 99_000_000,
        units_at_day_start: 1_500_000_000_000,
        _reserved: FUND_RESERVED,
    };
    let d = bytes(&fs);

    // F002 and F003 took reserved bytes without resizing or moving any F001 field.
    assert_eq!(
        d.len(),
        F001_FUND_STATE_LEN,
        "FundState size unchanged since F001"
    );
    assert_eq!(d.len(), 8 + FundState::INIT_SPACE);
    assert_eq!(&d[..8], FundState::DISCRIMINATOR);
    assert_eq!(d[8], 1, "version first");
    // In F001 the last 64 bytes were `_reserved`, right after `bump`.
    let tail = d.len() - 64;
    assert_eq!(d[tail - 1], 254, "bump stays where F001 put it");
    assert_eq!(
        &d[tail..tail + 4],
        &20_741u32.to_le_bytes(),
        "nav_period_end_day after bump"
    );
    assert_eq!(
        &d[tail + 4..tail + 12],
        &99_000_000u64.to_le_bytes(),
        "then pending_override_nav"
    );
    assert_eq!(
        &d[tail + 12..tail + 20],
        &1_500_000_000_000u64.to_le_bytes(),
        "then units_at_day_start (F003)"
    );
    assert_eq!(&d[tail + 20..], &FUND_RESERVED, "then 44 reserved bytes");

    let v = FundStateView::decode(&d);
    assert_eq!(
        v,
        FundStateView {
            mint: fs.mint,
            role_registry: fs.role_registry,
            status: 1,
            accrual_index: fs.accrual_index,
            cash_mint: fs.terms.cash_mint,
            nav: 100_000_000,
            // F004: collateral reads the publish time for its price age.
            nav_ts: fs.nav_ts,
            nav_seq: 3,
            nav_period_end_day: 20_741,
            units_outstanding: 2_000_000_000_000,
            cash_treasury: fs.cash_treasury,
        }
    );
}

#[test]
fn nav_record_layout() {
    let r = NavRecord {
        version: NavRecord::VERSION,
        seq: 7,
        nav: 100_000_000,
        daily_rate_e12: 123_287_671,
        days_covered: 3,
        period_start_day: 20_742,
        index_after: 9,
        as_of_ts: 1_792_108_800,
        published_by: Pubkey::new_unique(),
        override_used: true,
        bump: 250,
    };
    let d = bytes(&r);
    // 8 + 1 + 8 + 8 + 8 + 2 + 4 + 16 + 8 + 32 + 1 + 1 (data-model.md). No `_reserved`: immutable.
    assert_eq!(d.len(), 97);
    assert_eq!(d.len(), 8 + NavRecord::INIT_SPACE);
    assert_eq!(&d[..8], NavRecord::DISCRIMINATOR);
    assert_eq!(d[8], 1, "version first");
    assert_eq!(&d[9..17], &7u64.to_le_bytes(), "seq after version");
}

#[test]
fn role_registry_layout_and_view() {
    let r = RoleRegistry {
        version: RoleRegistry::VERSION,
        fund_state: Pubkey::new_unique(),
        issuer: Pubkey::new_unique(),
        fund_admin: Pubkey::new_unique(),
        transfer_agent: Pubkey::new_unique(),
        compliance: Pubkey::new_unique(),
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
        timelock_secs: 0,
        action_count: 0,
        bump: 253,
        _reserved: RESERVED,
    };
    let d = bytes(&r);
    assert_frame::<RoleRegistry>(&d, RoleRegistry::DISCRIMINATOR, 1);
    let v = RoleRegistryView::decode(&d);
    assert_eq!(
        v,
        RoleRegistryView {
            fund_state: r.fund_state,
            issuer: r.issuer,
            fund_admin: r.fund_admin,
            transfer_agent: r.transfer_agent,
            compliance: r.compliance,
            keeper: r.keeper,
            auditor: r.auditor,
        }
    );
}

#[test]
fn registry_layout_and_view() {
    let mut allowed = [0u64; SET_WORDS];
    jurisdiction::set_insert(&mut allowed, *b"US");
    jurisdiction::set_insert(&mut allowed, *b"ZW"); // lands in the last word
    let r = Registry {
        version: Registry::VERSION,
        mint: Pubkey::new_unique(),
        governance: Pubkey::new_unique(),
        role_registry: Pubkey::new_unique(),
        fund_state: Pubkey::new_unique(),
        collateral_program: Pubkey::new_unique(),
        kyc_providers: [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::default(),
            Pubkey::new_unique(),
        ],
        allowed_jurisdictions: allowed,
        max_holders: 7,
        holder_count: 3,
        transfers_paused: true,
        bump: 252,
        _reserved: RESERVED,
    };
    let d = bytes(&r);
    assert_frame::<Registry>(&d, Registry::DISCRIMINATOR, 1);
    let v = RegistryView::decode(&d);
    assert_eq!(
        v,
        RegistryView {
            mint: r.mint,
            fund_state: r.fund_state,
            kyc_providers: r.kyc_providers,
            allowed_jurisdictions: allowed,
            transfers_paused: true,
        }
    );
}

#[test]
fn holder_state_layout() {
    let mint = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let (key, bump) = Pubkey::find_program_address(
        &[seeds::HOLDER, mint.as_ref(), owner.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    );
    let h = HolderState {
        version: HolderState::VERSION,
        mint,
        owner,
        status: 1,
        class: 0,
        jurisdiction: *b"US",
        investor_type: 2,
        kyc_expiry_ts: 1_800_000_000,
        attestation_hash: [9; 32],
        attested_by: Pubkey::new_unique(),
        pledged_amount: 0,
        active_liens: 0,
        index_at_last_touch: 0,
        accrued_e18: 0,
        last_balance: 0,
        last_slot: 0,
        last_paid_cycle: 7,
        bump,
        _reserved: RESERVED,
    };
    let mut d = bytes(&h);
    assert_frame::<HolderState>(&d, HolderState::DISCRIMINATOR, 1);
    // owner sits right after version and mint: the hook's ExtraAccountMeta seeds don't read it,
    // but clients and indexers do.
    assert_eq!(&d[8 + 1 + 32..8 + 1 + 64], h.owner.as_ref());

    // F003: settlement reads it through HolderStateView.
    let mut lamports = 0u64;
    let info = anchor_lang::prelude::AccountInfo::new(
        &key,
        false,
        false,
        &mut lamports,
        &mut d,
        &COMPLIANCE_HOOK_PROGRAM_ID,
        false,
    );
    let v = HolderStateView::load(&info, &mint, &owner).expect("view loads");
    assert_eq!(
        v,
        HolderStateView {
            mint,
            owner,
            status: 1,
            class: 0,
            kyc_expiry_ts: 1_800_000_000,
            bump,
        }
    );
}

#[test]
fn dvp_config_layout() {
    let c = DvpConfig {
        version: DvpConfig::VERSION,
        fund_state: Pubkey::new_unique(),
        next_seq: 1,
        default_expiry_secs: 86_400,
        bump: 255,
        escrow_bump: 254,
        _reserved: RESERVED,
    };
    let d = bytes(&c);
    // 8 + 1 + 32 + 8 + 4 + 1 + 1 + 64 (data-model.md).
    assert_eq!(d.len(), 119);
    assert_frame::<DvpConfig>(&d, DvpConfig::DISCRIMINATOR, 1);
}

#[test]
fn settlement_instruction_layout_and_view() {
    let fund_state = Pubkey::new_unique();
    let created_by = Pubkey::new_unique();
    let external_ref = [0x42; 32];
    let (key, bump) = Pubkey::find_program_address(
        &[
            seeds::SETTLEMENT,
            fund_state.as_ref(),
            created_by.as_ref(),
            &external_ref,
        ],
        &DVP_SETTLEMENT_PROGRAM_ID,
    );
    let si = SettlementInstruction {
        version: SettlementInstruction::VERSION,
        fund_state,
        seq: 9,
        kind: 1,
        seller: Pubkey::new_unique(),
        buyer: fund_state,
        units: 500_000_000_000,
        cash_amount: 500_000_000_001,
        nav_seq: 4,
        not_before_ts: 10,
        expires_at_ts: 20,
        status: 3,
        seller_funded_at: 11,
        buyer_funded_at: 12,
        external_ref,
        created_by,
        created_at: 9,
        closed_at: 0,
        cancel_requests: 2,
        units_to_return: 0,
        cash_to_return: 0,
        bump,
        _reserved: RESERVED,
    };
    let mut d = bytes(&si);
    // 333 bytes with the discriminator (data-model.md).
    assert_eq!(d.len(), 333);
    assert_frame::<SettlementInstruction>(&d, SettlementInstruction::DISCRIMINATOR, 1);

    let mut lamports = 0u64;
    let info = anchor_lang::prelude::AccountInfo::new(
        &key,
        false,
        false,
        &mut lamports,
        &mut d,
        &DVP_SETTLEMENT_PROGRAM_ID,
        false,
    );
    let v = SettlementView::load(&info).expect("view loads");
    assert_eq!(
        v,
        SettlementView {
            fund_state,
            kind: 1,
            units: 500_000_000_000,
            cash_amount: 500_000_000_001,
            status: 3,
            external_ref,
            created_by,
            bump,
        }
    );
}

/// F004 (data-model.md): the three collateral accounts have their final sizes, start with the
/// discriminator and `version`, and end with `bump` then 64 reserved bytes.
#[test]
fn collateral_account_layouts() {
    let agreement = PledgeAgreement {
        version: PledgeAgreement::VERSION,
        fund_state: Pubkey::new_unique(),
        giver: Pubkey::new_unique(),
        taker: Pubkey::new_unique(),
        nonce: 1,
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
        status: 1,
        giver_signed: true,
        taker_signed: true,
        pledged_units: 1_020_408_163_266,
        required_value: 1_000_000_000_000,
        pledge_count: 2,
        call_count: 2,
        created_ts: 1,
        activated_ts: 2,
        pending_raise_value: 0,
        pending_raise_ref: [0; 32],
        pending_raise_ts: 0,
        exposure_ref: [9; 32],
        exposure_updated_ts: 3,
        called_outstanding: 0,
        open_delivery_calls: 0,
        met_below_seq: 2,
        open_return_call: Pubkey::default(),
        bump: 253,
        _reserved: RESERVED,
    };
    let d = bytes(&agreement);
    assert_eq!(d.len(), 432);
    assert_frame::<PledgeAgreement>(&d, PledgeAgreement::DISCRIMINATOR, 1);
    assert_eq!(d[d.len() - 65], 253, "bump before the reserved bytes");
    assert_eq!(&d[d.len() - 64..], &RESERVED);

    let pledge = Pledge {
        version: Pledge::VERSION,
        agreement: Pubkey::new_unique(),
        seq: 1,
        pledge_ref: [1; 32],
        units: 1_020_408_163_266,
        mode: 0,
        posted_at: 4,
        nav_seq_at_post: 1,
        released_units: 0,
        enforced_units: 0,
        status: 0,
        closed_at: 0,
        bump: 252,
        _reserved: RESERVED,
    };
    let d = bytes(&pledge);
    assert_eq!(d.len(), 196);
    assert_frame::<Pledge>(&d, Pledge::DISCRIMINATOR, 1);
    assert_eq!(d[d.len() - 65], 252);
    assert_eq!(pledge.remaining(), 1_020_408_163_266);

    let call = CollateralCall {
        version: CollateralCall::VERSION,
        agreement: Pubkey::new_unique(),
        seq: 1,
        kind: 0,
        value_delta: 1_000_000_000_000,
        units_required: 1_020_408_163_266,
        nav_seq: 1,
        issued_at: 5,
        cure_deadline_ts: 14_405,
        enforce_after_ts: 21_605,
        status: 0,
        enforced_units: 0,
        enforced_value: 0,
        external_ref: [2; 32],
        units_released: 0,
        dispute_ref: [0; 32],
        disputed_at: 0,
        closed_at: 0,
        bump: 251,
        _reserved: RESERVED,
    };
    let d = bytes(&call);
    assert_eq!(d.len(), 268);
    assert_frame::<CollateralCall>(&d, CollateralCall::DISCRIMINATOR, 1);
    assert_eq!(d[d.len() - 65], 251);
}
