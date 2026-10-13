//! T019 (F001), T008 (F002): account layouts are fixed, and the read-only views in
//! `mmf_common::views` decode program-serialized accounts to the same values (guards against
//! layout drift, F001 research R12).

use anchor_lang::prelude::Pubkey;
use anchor_lang::{AccountSerialize, Discriminator, Space};
use compliance_hook::{HolderState, Registry};
use fund::{FundState, FundTerms, NavRecord};
use governance::RoleRegistry;
use mmf_common::jurisdiction::{self, SET_WORDS};
use mmf_common::views::{
    FundStateView, RegistryView, RoleRegistryView, FUND_STATE_DISCRIMINATOR,
    REGISTRY_DISCRIMINATOR, ROLE_REGISTRY_DISCRIMINATOR,
};

const RESERVED: [u8; 64] = [0xAB; 64];
/// FundState's reserved bytes after F002 took 12 of them (research R10).
const FUND_RESERVED: [u8; 52] = [0xAB; 52];
/// `8 + FundState::INIT_SPACE` as shipped in F001; F002 must not change it.
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
        units_outstanding: 0,
        redeemed_today: 0,
        day_start_ts: 0,
        cash_treasury: Pubkey::default(),
        current_cycle: 0,
        bump: 254,
        nav_period_end_day: 20_741,
        pending_override_nav: 99_000_000,
        _reserved: FUND_RESERVED,
    };
    let d = bytes(&fs);

    // F002 took reserved bytes without resizing or moving any F001 field (research R10).
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
    assert_eq!(&d[tail + 12..], &FUND_RESERVED, "then 52 reserved bytes");

    let v = FundStateView::decode(&d);
    assert_eq!(
        v,
        FundStateView {
            mint: fs.mint,
            role_registry: fs.role_registry,
            status: 1,
            accrual_index: fs.accrual_index,
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
        }
    );
}

#[test]
fn holder_state_layout() {
    let h = HolderState {
        version: HolderState::VERSION,
        mint: Pubkey::new_unique(),
        owner: Pubkey::new_unique(),
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
        last_paid_cycle: 0,
        bump: 251,
        _reserved: RESERVED,
    };
    let d = bytes(&h);
    assert_frame::<HolderState>(&d, HolderState::DISCRIMINATOR, 1);
    // owner sits right after version and mint: the hook's ExtraAccountMeta seeds don't read it,
    // but clients and indexers do.
    assert_eq!(&d[8 + 1 + 32..8 + 1 + 64], h.owner.as_ref());
}
