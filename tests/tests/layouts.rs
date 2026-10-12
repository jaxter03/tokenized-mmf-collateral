//! T019: account layouts are fixed, and the read-only views in `mmf_common::views` decode
//! program-serialized accounts to the same values (guards against layout drift, research R12).

use anchor_lang::prelude::Pubkey;
use anchor_lang::{AccountSerialize, Discriminator, Space};
use compliance_hook::{HolderState, Registry};
use fund::{FundState, FundTerms};
use governance::RoleRegistry;
use mmf_common::jurisdiction::{self, SET_WORDS};
use mmf_common::views::{
    FundStateView, RegistryView, RoleRegistryView, FUND_STATE_DISCRIMINATOR,
    REGISTRY_DISCRIMINATOR, ROLE_REGISTRY_DISCRIMINATOR,
};

const RESERVED: [u8; 64] = [0xAB; 64];

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
        nav: 0,
        nav_ts: 0,
        nav_seq: 0,
        accrual_index: 0,
        daily_rate_e12: 0,
        units_outstanding: 0,
        redeemed_today: 0,
        day_start_ts: 0,
        cash_treasury: Pubkey::default(),
        current_cycle: 0,
        bump: 254,
        _reserved: RESERVED,
    };
    let d = bytes(&fs);
    assert_frame::<FundState>(&d, FundState::DISCRIMINATOR, 1);
    let v = FundStateView::decode(&d);
    assert_eq!(
        v,
        FundStateView {
            mint: fs.mint,
            role_registry: fs.role_registry,
            status: 1
        }
    );
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
