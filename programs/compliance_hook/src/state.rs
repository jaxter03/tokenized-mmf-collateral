//! `Registry` and `HolderState` in their final layout (docs/shared-model.md §4.1), plus the
//! approval predicates the hook and `thaw_holding_account` use (data-model.md "Validity predicates").

use anchor_lang::prelude::*;
use mmf_common::enums::{HolderClass, HolderStatus};
use mmf_common::jurisdiction::{self, JurisdictionSet, SET_WORDS};

/// PDA `["registry", mint]`; also the mint's freeze authority.
#[account]
#[derive(Debug, InitSpace)]
pub struct Registry {
    pub version: u8,
    pub mint: Pubkey,
    /// Governance program id (role checks read `RoleRegistry`).
    pub governance: Pubkey,
    pub role_registry: Pubkey,
    pub fund_state: Pubkey,
    /// F004: the only program allowed to change locked units (zero in F001).
    pub collateral_program: Pubkey,
    /// Recognised KYC providers; unused slots are the default pubkey.
    pub kyc_providers: [Pubkey; 4],
    /// 676-bit set of allowed ISO 3166-1 alpha-2 codes (`mmf_common::jurisdiction`).
    pub allowed_jurisdictions: [u64; SET_WORDS],
    /// 0 = no cap.
    pub max_holders: u32,
    /// Investor `HolderState`s with `last_balance > 0` (invariant I10).
    pub holder_count: u32,
    pub transfers_paused: bool,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl Registry {
    pub const VERSION: u8 = 1;

    /// True if `key` is a non-default entry in `kyc_providers`.
    pub fn is_provider(&self, key: &Pubkey) -> bool {
        *key != Pubkey::default() && self.kyc_providers.contains(key)
    }

    pub fn jurisdiction_allowed(&self, code: [u8; 2]) -> bool {
        let set: &JurisdictionSet = &self.allowed_jurisdictions;
        jurisdiction::set_contains(set, code)
    }
}

/// PDA `["holder", mint, owner]`. One per owner account (Clarification Q2).
#[account]
#[derive(Debug, InitSpace)]
pub struct HolderState {
    pub version: u8,
    pub mint: Pubkey,
    /// The owner account: the institution's identity.
    pub owner: Pubkey,
    /// `mmf_common::enums::HolderStatus`.
    pub status: u8,
    /// `mmf_common::enums::HolderClass`.
    pub class: u8,
    /// ISO 3166-1 alpha-2, ASCII; zero for fund-operated accounts.
    pub jurisdiction: [u8; 2],
    /// Provider's code, non-zero for investors; zero for fund-operated accounts.
    pub investor_type: u8,
    /// Investors: from the statement. Fund-operated: `i64::MAX` (never expires).
    pub kyc_expiry_ts: i64,
    /// SHA-256 of the statement message; zero for fund-operated accounts.
    pub attestation_hash: [u8; 32],
    /// Provider key; default for fund-operated accounts.
    pub attested_by: Pubkey,
    // --- F004: liens (zero in F001) ---
    pub pledged_amount: u64,
    pub active_liens: u16,
    // --- F002: accrual (zero in F001) ---
    pub index_at_last_touch: u128,
    pub accrued_e18: u128,
    /// Balance of the owner's holding account after the last hook touch (holder count from F001).
    pub last_balance: u64,
    /// Slot of the last hook touch.
    pub last_slot: u64,
    // --- F006: distributions (zero in F001) ---
    pub last_paid_cycle: u32,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl HolderState {
    pub const VERSION: u8 = 1;

    pub fn status(&self) -> Option<HolderStatus> {
        HolderStatus::try_from(self.status).ok()
    }

    pub fn class(&self) -> Option<HolderClass> {
        HolderClass::try_from(self.class).ok()
    }

    /// `kyc_valid(h, now) := now < h.kyc_expiry_ts` — expired from the exact second the expiry is
    /// reached (FR-025).
    pub fn kyc_valid(&self, now: i64) -> bool {
        now < self.kyc_expiry_ts
    }

    /// `approved(h, now) := h.status == Approved && kyc_valid(h, now)`.
    pub fn approved(&self, now: i64) -> bool {
        self.status() == Some(HolderStatus::Approved) && self.kyc_valid(now)
    }

    /// `class_allowed(h) := h.class ∈ {Investor, FundTreasury, Escrow}`.
    pub fn class_allowed(&self) -> bool {
        self.class().is_some_and(HolderClass::transfer_allowed)
    }

    /// `jurisdiction_ok(h) := h.class != Investor || allowed(registry, h.jurisdiction)`.
    pub fn jurisdiction_ok(&self, registry: &Registry) -> bool {
        self.class() != Some(HolderClass::Investor)
            || registry.jurisdiction_allowed(self.jurisdiction)
    }

    /// Investors count toward the holder cap; fund-operated accounts never do.
    pub fn is_investor(&self) -> bool {
        self.class() == Some(HolderClass::Investor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        let mut allowed = [0u64; SET_WORDS];
        jurisdiction::set_insert(&mut allowed, *b"US");
        Registry {
            version: 1,
            mint: Pubkey::default(),
            governance: Pubkey::default(),
            role_registry: Pubkey::default(),
            fund_state: Pubkey::default(),
            collateral_program: Pubkey::default(),
            kyc_providers: [
                Pubkey::new_from_array([7; 32]),
                Pubkey::default(),
                Pubkey::default(),
                Pubkey::default(),
            ],
            allowed_jurisdictions: allowed,
            max_holders: 0,
            holder_count: 0,
            transfers_paused: false,
            bump: 0,
            _reserved: [0; 64],
        }
    }

    fn holder(
        status: HolderStatus,
        class: HolderClass,
        jurisdiction: [u8; 2],
        expiry: i64,
    ) -> HolderState {
        HolderState {
            version: 1,
            mint: Pubkey::default(),
            owner: Pubkey::default(),
            status: status.as_u8(),
            class: class.as_u8(),
            jurisdiction,
            investor_type: 1,
            kyc_expiry_ts: expiry,
            attestation_hash: [0; 32],
            attested_by: Pubkey::default(),
            pledged_amount: 0,
            active_liens: 0,
            index_at_last_touch: 0,
            accrued_e18: 0,
            last_balance: 0,
            last_slot: 0,
            last_paid_cycle: 0,
            bump: 0,
            _reserved: [0; 64],
        }
    }

    #[test]
    fn kyc_expires_at_exact_second() {
        let h = holder(HolderStatus::Approved, HolderClass::Investor, *b"US", 1_000);
        assert!(h.kyc_valid(999) && h.approved(999));
        assert!(!h.kyc_valid(1_000) && !h.approved(1_000));
    }

    #[test]
    fn only_approved_status_counts() {
        for s in [
            HolderStatus::Pending,
            HolderStatus::Suspended,
            HolderStatus::Revoked,
        ] {
            assert!(!holder(s, HolderClass::Investor, *b"US", i64::MAX).approved(0));
        }
        let mut h = holder(
            HolderStatus::Approved,
            HolderClass::Investor,
            *b"US",
            i64::MAX,
        );
        h.status = 9; // unknown status never counts as approved
        assert!(!h.approved(0));
    }

    #[test]
    fn class_and_jurisdiction() {
        let r = registry();
        let us = holder(
            HolderStatus::Approved,
            HolderClass::Investor,
            *b"US",
            i64::MAX,
        );
        let fr = holder(
            HolderStatus::Approved,
            HolderClass::Investor,
            *b"FR",
            i64::MAX,
        );
        let treasury = holder(
            HolderStatus::Approved,
            HolderClass::FundTreasury,
            [0, 0],
            i64::MAX,
        );
        let vault = holder(
            HolderStatus::Approved,
            HolderClass::CollateralVault,
            [0, 0],
            i64::MAX,
        );
        assert!(us.jurisdiction_ok(&r) && !fr.jurisdiction_ok(&r));
        assert!(
            treasury.jurisdiction_ok(&r),
            "fund-operated accounts have no jurisdiction"
        );
        assert!(us.class_allowed() && treasury.class_allowed() && !vault.class_allowed());
        assert!(us.is_investor() && !treasury.is_investor());
    }

    #[test]
    fn providers() {
        let r = registry();
        assert!(r.is_provider(&Pubkey::new_from_array([7; 32])));
        assert!(
            !r.is_provider(&Pubkey::default()),
            "empty slots never match"
        );
        assert!(!r.is_provider(&Pubkey::new_from_array([8; 32])));
    }
}
