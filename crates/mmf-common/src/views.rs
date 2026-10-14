//! Read-only views of accounts owned by other F001 programs (research R12).
//!
//! `compliance_hook` reads `FundState` (fund; from F002 also its `accrual_index` in the transfer
//! hook) and `RoleRegistry` (governance); `fund` reads
//! `Registry` (compliance_hook) and `RoleRegistry`; `governance` reads `FundState`. From F003,
//! `dvp_settlement` reads `FundState`, `RoleRegistry`, `Registry` and `HolderState`, and `fund`
//! reads `SettlementInstruction` (dvp_settlement) before minting or burning. Depending on
//! each other's program crates would create a cycle, so each view decodes only the fields other
//! programs need, at fixed offsets in the Anchor/Borsh layout.
//!
//! Every `load` checks, in order: owning program, expected address, length, then the 8-byte
//! Anchor discriminator (`sha256("account:<Name>")[..8]`). Layout drift between a program and its
//! view is caught by `tests/tests/layouts.rs` (T019).

use anchor_lang::prelude::{AccountInfo, Pubkey};

use crate::enums::{HolderClass, HolderStatus};
use crate::ids::{
    COMPLIANCE_HOOK_PROGRAM_ID, DVP_SETTLEMENT_PROGRAM_ID, FUND_PROGRAM_ID, GOVERNANCE_PROGRAM_ID,
};
use crate::jurisdiction::{JurisdictionSet, SET_WORDS};
use crate::seeds;

/// Why an account isn't the expected one. Callers map this to their own error code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewError {
    WrongOwner,
    WrongAddress,
    TooShort,
    WrongDiscriminator,
}

pub const FUND_STATE_DISCRIMINATOR: [u8; 8] = [3, 254, 145, 43, 146, 96, 162, 104];
pub const ROLE_REGISTRY_DISCRIMINATOR: [u8; 8] = [173, 129, 34, 250, 223, 92, 0, 232];
pub const REGISTRY_DISCRIMINATOR: [u8; 8] = [47, 174, 110, 246, 184, 182, 252, 218];
pub const HOLDER_STATE_DISCRIMINATOR: [u8; 8] = [222, 82, 176, 75, 3, 75, 155, 184];
pub const SETTLEMENT_INSTRUCTION_DISCRIMINATOR: [u8; 8] = [12, 204, 192, 184, 130, 29, 218, 88];

/// Size of `FundTerms` in Borsh: 12 + 32 + 8 + 2 + 2 + 1 + 2 + 32 + 128.
const FUND_TERMS_LEN: usize = 219;

mod offsets {
    use super::FUND_TERMS_LEN;
    // All offsets include the 8-byte discriminator and the 1-byte `version`.
    pub const BASE: usize = 8 + 1;

    pub const FUND_MINT: usize = BASE;
    pub const FUND_ROLE_REGISTRY: usize = FUND_MINT + 32;
    /// `terms.cash_mint`, after `terms.fund_id` (12 bytes).
    pub const FUND_CASH_MINT: usize = FUND_ROLE_REGISTRY + 32 + 12;
    pub const FUND_STATUS: usize = FUND_ROLE_REGISTRY + 32 + FUND_TERMS_LEN;
    pub const FUND_NAV: usize = FUND_STATUS + 1;
    pub const FUND_NAV_SEQ: usize = FUND_NAV + 8 + 8; // after nav_ts
    /// After `status`, `nav u64`, `nav_ts i64` and `nav_seq u64` (F002).
    pub const FUND_ACCRUAL_INDEX: usize = FUND_STATUS + 1 + 8 + 8 + 8;
    /// After `accrual_index u128` and `daily_rate_e12 u64` (F003).
    pub const FUND_UNITS_OUTSTANDING: usize = FUND_ACCRUAL_INDEX + 16 + 8;
    /// After `units_outstanding`, `redeemed_today u64`, `day_start_ts i64`.
    pub const FUND_CASH_TREASURY: usize = FUND_UNITS_OUTSTANDING + 8 + 8 + 8;
    /// After `cash_treasury`, `current_cycle u32` and `bump u8` (F002, after `bump`).
    pub const FUND_NAV_PERIOD_END_DAY: usize = FUND_CASH_TREASURY + 32 + 4 + 1;
    /// After `nav_period_end_day u32` and `pending_override_nav u64` (F003).
    pub const FUND_UNITS_AT_DAY_START: usize = FUND_NAV_PERIOD_END_DAY + 4 + 8;

    pub const ROLES_FUND_STATE: usize = BASE;
    pub const ROLES_ISSUER: usize = ROLES_FUND_STATE + 32;
    pub const ROLES_FUND_ADMIN: usize = ROLES_ISSUER + 32;
    pub const ROLES_TRANSFER_AGENT: usize = ROLES_FUND_ADMIN + 32;
    pub const ROLES_COMPLIANCE: usize = ROLES_TRANSFER_AGENT + 32;
    pub const ROLES_KEEPER: usize = ROLES_COMPLIANCE + 32;
    pub const ROLES_AUDITOR: usize = ROLES_KEEPER + 32;

    pub const REG_MINT: usize = BASE;
    pub const REG_FUND_STATE: usize = REG_MINT + 32 * 3; // after governance, role_registry
    pub const REG_PROVIDERS: usize = REG_FUND_STATE + 32 * 2; // after collateral_program
    pub const REG_JURISDICTIONS: usize = REG_PROVIDERS + 32 * 4;
    /// After `allowed_jurisdictions [u64; 11]`, `max_holders u32`, `holder_count u32`.
    pub const REG_TRANSFERS_PAUSED: usize = REG_JURISDICTIONS + 8 * 11 + 4 + 4;

    pub const HOLDER_MINT: usize = BASE;
    pub const HOLDER_OWNER: usize = HOLDER_MINT + 32;
    pub const HOLDER_STATUS: usize = HOLDER_OWNER + 32;
    pub const HOLDER_CLASS: usize = HOLDER_STATUS + 1;
    /// After `class u8`, `jurisdiction [u8; 2]`, `investor_type u8`.
    pub const HOLDER_KYC_EXPIRY: usize = HOLDER_CLASS + 1 + 2 + 1;
    /// After `kyc_expiry_ts`, `attestation_hash`, `attested_by`, `pledged_amount u64`,
    /// `active_liens u16`, `index_at_last_touch u128`, `accrued_e18 u128`, `last_balance u64`,
    /// `last_slot u64`, `last_paid_cycle u32`.
    pub const HOLDER_BUMP: usize = HOLDER_KYC_EXPIRY + 8 + 32 + 32 + 8 + 2 + 16 + 16 + 8 + 8 + 4;

    pub const SI_FUND_STATE: usize = BASE;
    pub const SI_KIND: usize = SI_FUND_STATE + 32 + 8; // after seq
    pub const SI_UNITS: usize = SI_KIND + 1 + 32 + 32; // after seller, buyer
    pub const SI_CASH_AMOUNT: usize = SI_UNITS + 8;
    /// After `cash_amount`, `nav_seq`, `not_before_ts`, `expires_at_ts`.
    pub const SI_STATUS: usize = SI_CASH_AMOUNT + 8 + 8 + 8 + 8;
    /// After `status`, `seller_funded_at`, `buyer_funded_at`.
    pub const SI_EXTERNAL_REF: usize = SI_STATUS + 1 + 8 + 8;
    pub const SI_CREATED_BY: usize = SI_EXTERNAL_REF + 32;
    /// After `created_by`, `created_at`, `closed_at`, `cancel_requests u8`, `units_to_return`,
    /// `cash_to_return`.
    pub const SI_BUMP: usize = SI_CREATED_BY + 32 + 8 + 8 + 1 + 8 + 8;
}

fn checked<'a>(
    info: &'a AccountInfo,
    owner: &Pubkey,
    address: &Pubkey,
    discriminator: &[u8; 8],
    min_len: usize,
) -> Result<core::cell::Ref<'a, &'a mut [u8]>, ViewError> {
    if info.owner != owner {
        return Err(ViewError::WrongOwner);
    }
    if info.key != address {
        return Err(ViewError::WrongAddress);
    }
    let data = info.try_borrow_data().map_err(|_| ViewError::TooShort)?;
    if data.len() < min_len {
        return Err(ViewError::TooShort);
    }
    if &data[..8] != discriminator {
        return Err(ViewError::WrongDiscriminator);
    }
    Ok(data)
}

fn key_at(data: &[u8], at: usize) -> Pubkey {
    Pubkey::new_from_array(data[at..at + 32].try_into().expect("32 bytes"))
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"))
}

fn i64_at(data: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"))
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("4 bytes"))
}

/// Fields of `FundState` (fund program) that other programs read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundStateView {
    pub mint: Pubkey,
    pub role_registry: Pubkey,
    /// `crate::enums::FundStatus` as stored.
    pub status: u8,
    /// Interest per base unit × 10^18 since the first publish (F002).
    pub accrual_index: u128,
    /// `terms.cash_mint` (F001).
    pub cash_mint: Pubkey,
    /// Latest published price, 8 dp; 0 before the first publish (F002).
    pub nav: u64,
    /// Seq of the latest price record; 0 = none (F002).
    pub nav_seq: u64,
    /// Last UTC day covered by any publish; 0 = none (F002).
    pub nav_period_end_day: u32,
    /// Equals the mint's supply (F003, I4).
    pub units_outstanding: u64,
    /// Treasury cash account; default until `set_cash_treasury` (F003).
    pub cash_treasury: Pubkey,
}

impl FundStateView {
    pub fn load(info: &AccountInfo, expected: &Pubkey) -> Result<Self, ViewError> {
        let d = checked(
            info,
            &FUND_PROGRAM_ID,
            expected,
            &FUND_STATE_DISCRIMINATOR,
            offsets::FUND_UNITS_AT_DAY_START + 8,
        )?;
        Ok(Self::decode(&d))
    }

    /// Decodes already-validated account data.
    pub fn decode(d: &[u8]) -> Self {
        Self {
            mint: key_at(d, offsets::FUND_MINT),
            role_registry: key_at(d, offsets::FUND_ROLE_REGISTRY),
            status: d[offsets::FUND_STATUS],
            accrual_index: u128::from_le_bytes(
                d[offsets::FUND_ACCRUAL_INDEX..offsets::FUND_ACCRUAL_INDEX + 16]
                    .try_into()
                    .expect("16 bytes"),
            ),
            cash_mint: key_at(d, offsets::FUND_CASH_MINT),
            nav: u64_at(d, offsets::FUND_NAV),
            nav_seq: u64_at(d, offsets::FUND_NAV_SEQ),
            nav_period_end_day: u32_at(d, offsets::FUND_NAV_PERIOD_END_DAY),
            units_outstanding: u64_at(d, offsets::FUND_UNITS_OUTSTANDING),
            cash_treasury: key_at(d, offsets::FUND_CASH_TREASURY),
        }
    }
}

/// Fields of `RoleRegistry` (governance) that other programs read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleRegistryView {
    pub fund_state: Pubkey,
    pub issuer: Pubkey,
    pub fund_admin: Pubkey,
    pub transfer_agent: Pubkey,
    pub compliance: Pubkey,
    pub keeper: Pubkey,
    pub auditor: Pubkey,
}

impl RoleRegistryView {
    pub fn load(info: &AccountInfo, expected: &Pubkey) -> Result<Self, ViewError> {
        let d = checked(
            info,
            &GOVERNANCE_PROGRAM_ID,
            expected,
            &ROLE_REGISTRY_DISCRIMINATOR,
            offsets::ROLES_AUDITOR + 32,
        )?;
        Ok(Self::decode(&d))
    }

    pub fn decode(d: &[u8]) -> Self {
        Self {
            fund_state: key_at(d, offsets::ROLES_FUND_STATE),
            issuer: key_at(d, offsets::ROLES_ISSUER),
            fund_admin: key_at(d, offsets::ROLES_FUND_ADMIN),
            transfer_agent: key_at(d, offsets::ROLES_TRANSFER_AGENT),
            compliance: key_at(d, offsets::ROLES_COMPLIANCE),
            keeper: key_at(d, offsets::ROLES_KEEPER),
            auditor: key_at(d, offsets::ROLES_AUDITOR),
        }
    }

    /// True when all five operating roles are set to non-default keys.
    pub fn all_assigned(&self) -> bool {
        [
            self.fund_admin,
            self.transfer_agent,
            self.compliance,
            self.keeper,
            self.auditor,
        ]
        .iter()
        .all(|k| *k != Pubkey::default())
    }
}

/// Fields of `Registry` (compliance_hook) that other programs read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryView {
    pub mint: Pubkey,
    pub fund_state: Pubkey,
    pub kyc_providers: [Pubkey; 4],
    pub allowed_jurisdictions: JurisdictionSet,
    /// F001 pause flag; read by settlement, which burns without running the hook (F003).
    pub transfers_paused: bool,
}

impl RegistryView {
    pub fn load(info: &AccountInfo, expected: &Pubkey) -> Result<Self, ViewError> {
        let d = checked(
            info,
            &COMPLIANCE_HOOK_PROGRAM_ID,
            expected,
            &REGISTRY_DISCRIMINATOR,
            offsets::REG_TRANSFERS_PAUSED + 1,
        )?;
        Ok(Self::decode(&d))
    }

    pub fn decode(d: &[u8]) -> Self {
        let mut providers = [Pubkey::default(); 4];
        for (i, p) in providers.iter_mut().enumerate() {
            *p = key_at(d, offsets::REG_PROVIDERS + 32 * i);
        }
        let mut set = [0u64; SET_WORDS];
        for (i, w) in set.iter_mut().enumerate() {
            let at = offsets::REG_JURISDICTIONS + 8 * i;
            *w = u64::from_le_bytes(d[at..at + 8].try_into().expect("8 bytes"));
        }
        Self {
            mint: key_at(d, offsets::REG_MINT),
            fund_state: key_at(d, offsets::REG_FUND_STATE),
            kyc_providers: providers,
            allowed_jurisdictions: set,
            transfers_paused: d[offsets::REG_TRANSFERS_PAUSED] != 0,
        }
    }

    pub fn has_provider(&self) -> bool {
        self.kyc_providers.iter().any(|k| *k != Pubkey::default())
    }
}

/// Fields of `HolderState` (compliance_hook) that settlement reads (F003).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HolderStateView {
    pub mint: Pubkey,
    pub owner: Pubkey,
    /// `crate::enums::HolderStatus` as stored.
    pub status: u8,
    /// `crate::enums::HolderClass` as stored.
    pub class: u8,
    pub kyc_expiry_ts: i64,
    pub bump: u8,
}

impl HolderStateView {
    /// Loads the HolderState of `owner` for `mint`: owned by compliance_hook, the right
    /// discriminator, storing that mint and owner, and at PDA `["holder", mint, owner]`
    /// (re-derived with its stored bump).
    pub fn load(info: &AccountInfo, mint: &Pubkey, owner: &Pubkey) -> Result<Self, ViewError> {
        if info.owner != &COMPLIANCE_HOOK_PROGRAM_ID {
            return Err(ViewError::WrongOwner);
        }
        let d = info.try_borrow_data().map_err(|_| ViewError::TooShort)?;
        if d.len() < offsets::HOLDER_BUMP + 1 {
            return Err(ViewError::TooShort);
        }
        if d[..8] != HOLDER_STATE_DISCRIMINATOR {
            return Err(ViewError::WrongDiscriminator);
        }
        let v = Self::decode(&d);
        let expected = Pubkey::create_program_address(
            &[seeds::HOLDER, mint.as_ref(), owner.as_ref(), &[v.bump]],
            &COMPLIANCE_HOOK_PROGRAM_ID,
        )
        .map_err(|_| ViewError::WrongAddress)?;
        if info.key != &expected || &v.mint != mint || &v.owner != owner {
            return Err(ViewError::WrongAddress);
        }
        Ok(v)
    }

    pub fn decode(d: &[u8]) -> Self {
        Self {
            mint: key_at(d, offsets::HOLDER_MINT),
            owner: key_at(d, offsets::HOLDER_OWNER),
            status: d[offsets::HOLDER_STATUS],
            class: d[offsets::HOLDER_CLASS],
            kyc_expiry_ts: i64_at(d, offsets::HOLDER_KYC_EXPIRY),
            bump: d[offsets::HOLDER_BUMP],
        }
    }

    /// May be a party to a settlement (research R12): an Approved Investor whose KYC is valid at
    /// `now` (it expires at `kyc_expiry_ts`, F001 FR-025).
    pub fn eligible(&self, now: i64) -> bool {
        self.class == HolderClass::Investor.as_u8()
            && self.status == HolderStatus::Approved.as_u8()
            && self.kyc_expiry_ts > now
    }

    /// The fund's escrow holder is ready (FR-003): class Escrow and Approved.
    pub fn is_approved_escrow(&self) -> bool {
        self.class == HolderClass::Escrow.as_u8() && self.status == HolderStatus::Approved.as_u8()
    }
}

/// Fields of `SettlementInstruction` (dvp_settlement) that `fund` checks before minting or
/// burning (F003, research R7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementView {
    pub fund_state: Pubkey,
    /// `crate::enums::SettlementKind` as stored.
    pub kind: u8,
    pub units: u64,
    pub cash_amount: u64,
    /// `crate::enums::SettlementStatus` as stored.
    pub status: u8,
    pub external_ref: [u8; 32],
    pub created_by: Pubkey,
    pub bump: u8,
}

impl SettlementView {
    /// Loads a SettlementInstruction: owned by dvp_settlement, the right discriminator, and at
    /// the PDA of its own `fund_state`, `created_by` and `external_ref` (re-derived with its
    /// stored bump). The caller checks `fund_state`, kind, status and amounts.
    pub fn load(info: &AccountInfo) -> Result<Self, ViewError> {
        if info.owner != &DVP_SETTLEMENT_PROGRAM_ID {
            return Err(ViewError::WrongOwner);
        }
        let d = info.try_borrow_data().map_err(|_| ViewError::TooShort)?;
        if d.len() < offsets::SI_BUMP + 1 {
            return Err(ViewError::TooShort);
        }
        if d[..8] != SETTLEMENT_INSTRUCTION_DISCRIMINATOR {
            return Err(ViewError::WrongDiscriminator);
        }
        let v = Self::decode(&d);
        let expected = Pubkey::create_program_address(
            &[
                seeds::SETTLEMENT,
                v.fund_state.as_ref(),
                v.created_by.as_ref(),
                &v.external_ref,
                &[v.bump],
            ],
            &DVP_SETTLEMENT_PROGRAM_ID,
        )
        .map_err(|_| ViewError::WrongAddress)?;
        if info.key != &expected {
            return Err(ViewError::WrongAddress);
        }
        Ok(v)
    }

    pub fn decode(d: &[u8]) -> Self {
        Self {
            fund_state: key_at(d, offsets::SI_FUND_STATE),
            kind: d[offsets::SI_KIND],
            units: u64_at(d, offsets::SI_UNITS),
            cash_amount: u64_at(d, offsets::SI_CASH_AMOUNT),
            status: d[offsets::SI_STATUS],
            external_ref: d[offsets::SI_EXTERNAL_REF..offsets::SI_EXTERNAL_REF + 32]
                .try_into()
                .expect("32 bytes"),
            created_by: key_at(d, offsets::SI_CREATED_BY),
            bump: d[offsets::SI_BUMP],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>(
        key: &'a Pubkey,
        owner: &'a Pubkey,
        lamports: &'a mut u64,
        data: &'a mut [u8],
    ) -> AccountInfo<'a> {
        AccountInfo::new(key, false, false, lamports, data, owner, false)
    }

    #[test]
    fn rejects_in_order() {
        let key = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let (mut l, mut d) = (0u64, vec![0u8; 512]);
        d[..8].copy_from_slice(&FUND_STATE_DISCRIMINATOR);

        let wrong_owner = Pubkey::new_unique();
        assert_eq!(
            FundStateView::load(&info(&key, &wrong_owner, &mut l, &mut d), &key),
            Err(ViewError::WrongOwner)
        );
        assert_eq!(
            FundStateView::load(&info(&key, &FUND_PROGRAM_ID, &mut l, &mut d), &other),
            Err(ViewError::WrongAddress)
        );
        let mut short = vec![0u8; 10];
        assert_eq!(
            FundStateView::load(&info(&key, &FUND_PROGRAM_ID, &mut l, &mut short), &key),
            Err(ViewError::TooShort)
        );
        let mut wrong_disc = d.clone();
        wrong_disc[0] ^= 1;
        assert_eq!(
            FundStateView::load(&info(&key, &FUND_PROGRAM_ID, &mut l, &mut wrong_disc), &key),
            Err(ViewError::WrongDiscriminator)
        );
        // A RoleRegistry presented as a FundState is caught by the owner check first.
        assert_eq!(
            RoleRegistryView::load(&info(&key, &FUND_PROGRAM_ID, &mut l, &mut d), &key),
            Err(ViewError::WrongOwner)
        );
        assert!(FundStateView::load(&info(&key, &FUND_PROGRAM_ID, &mut l, &mut d), &key).is_ok());
    }

    fn holder_bytes(mint: &Pubkey, owner: &Pubkey, bump: u8, class: HolderClass) -> Vec<u8> {
        let mut d = vec![0u8; offsets::HOLDER_BUMP + 1 + 64];
        d[..8].copy_from_slice(&HOLDER_STATE_DISCRIMINATOR);
        d[offsets::HOLDER_MINT..offsets::HOLDER_MINT + 32].copy_from_slice(mint.as_ref());
        d[offsets::HOLDER_OWNER..offsets::HOLDER_OWNER + 32].copy_from_slice(owner.as_ref());
        d[offsets::HOLDER_STATUS] = HolderStatus::Approved.as_u8();
        d[offsets::HOLDER_CLASS] = class.as_u8();
        d[offsets::HOLDER_KYC_EXPIRY..offsets::HOLDER_KYC_EXPIRY + 8]
            .copy_from_slice(&1_000i64.to_le_bytes());
        d[offsets::HOLDER_BUMP] = bump;
        d
    }

    #[test]
    fn holder_view_checks_owner_discriminator_and_address() {
        let (mint, owner) = (Pubkey::new_unique(), Pubkey::new_unique());
        let (key, bump) = Pubkey::find_program_address(
            &[seeds::HOLDER, mint.as_ref(), owner.as_ref()],
            &COMPLIANCE_HOOK_PROGRAM_ID,
        );
        let hook = COMPLIANCE_HOOK_PROGRAM_ID;
        let mut l = 0u64;
        let mut d = holder_bytes(&mint, &owner, bump, HolderClass::Investor);
        let v = HolderStateView::load(&info(&key, &hook, &mut l, &mut d), &mint, &owner).unwrap();
        assert!(v.eligible(999));
        assert!(!v.eligible(1_000), "KYC expires at kyc_expiry_ts");
        assert!(!v.is_approved_escrow());

        let other = Pubkey::new_unique();
        assert_eq!(
            HolderStateView::load(&info(&key, &other, &mut l, &mut d), &mint, &owner),
            Err(ViewError::WrongOwner)
        );
        assert_eq!(
            HolderStateView::load(&info(&key, &hook, &mut l, &mut d), &mint, &other),
            Err(ViewError::WrongAddress),
            "another owner's record"
        );
        let mut short = vec![0u8; 20];
        assert_eq!(
            HolderStateView::load(&info(&key, &hook, &mut l, &mut short), &mint, &owner),
            Err(ViewError::TooShort)
        );
        let mut bad = d.clone();
        bad[0] ^= 1;
        assert_eq!(
            HolderStateView::load(&info(&key, &hook, &mut l, &mut bad), &mint, &owner),
            Err(ViewError::WrongDiscriminator)
        );

        let mut suspended = holder_bytes(&mint, &owner, bump, HolderClass::Investor);
        suspended[offsets::HOLDER_STATUS] = HolderStatus::Suspended.as_u8();
        assert!(!HolderStateView::decode(&suspended).eligible(0));
        let escrow = holder_bytes(&mint, &owner, bump, HolderClass::Escrow);
        let e = HolderStateView::decode(&escrow);
        assert!(
            e.is_approved_escrow() && !e.eligible(0),
            "fund-operated: never a party"
        );
    }

    #[test]
    fn settlement_view_checks_its_own_address() {
        let (fund, creator, ext) = (Pubkey::new_unique(), Pubkey::new_unique(), [7u8; 32]);
        let (key, bump) = Pubkey::find_program_address(
            &[seeds::SETTLEMENT, fund.as_ref(), creator.as_ref(), &ext],
            &DVP_SETTLEMENT_PROGRAM_ID,
        );
        let mut d = vec![0u8; offsets::SI_BUMP + 1 + 64];
        d[..8].copy_from_slice(&SETTLEMENT_INSTRUCTION_DISCRIMINATOR);
        d[offsets::SI_FUND_STATE..offsets::SI_FUND_STATE + 32].copy_from_slice(fund.as_ref());
        d[offsets::SI_KIND] = 1;
        d[offsets::SI_UNITS..offsets::SI_UNITS + 8].copy_from_slice(&5u64.to_le_bytes());
        d[offsets::SI_CASH_AMOUNT..offsets::SI_CASH_AMOUNT + 8]
            .copy_from_slice(&6u64.to_le_bytes());
        d[offsets::SI_STATUS] = 3;
        d[offsets::SI_EXTERNAL_REF..offsets::SI_EXTERNAL_REF + 32].copy_from_slice(&ext);
        d[offsets::SI_CREATED_BY..offsets::SI_CREATED_BY + 32].copy_from_slice(creator.as_ref());
        d[offsets::SI_BUMP] = bump;
        let dvp = DVP_SETTLEMENT_PROGRAM_ID;
        let mut l = 0u64;

        let v = SettlementView::load(&info(&key, &dvp, &mut l, &mut d)).unwrap();
        assert_eq!(
            (v.fund_state, v.kind, v.units, v.cash_amount, v.status),
            (fund, 1, 5, 6, 3)
        );

        let elsewhere = Pubkey::new_unique();
        assert_eq!(
            SettlementView::load(&info(&elsewhere, &dvp, &mut l, &mut d)),
            Err(ViewError::WrongAddress),
            "a copy at another address"
        );
        let forged_owner = Pubkey::new_unique();
        assert_eq!(
            SettlementView::load(&info(&key, &forged_owner, &mut l, &mut d)),
            Err(ViewError::WrongOwner)
        );
        let mut other_fund = d.clone();
        other_fund[offsets::SI_FUND_STATE] ^= 1;
        assert_eq!(
            SettlementView::load(&info(&key, &dvp, &mut l, &mut other_fund)),
            Err(ViewError::WrongAddress),
            "fund_state rewritten: no longer this account's seeds"
        );
    }

    #[test]
    fn role_assignment_check() {
        let mut v = RoleRegistryView {
            fund_state: Pubkey::new_unique(),
            issuer: Pubkey::new_unique(),
            fund_admin: Pubkey::new_unique(),
            transfer_agent: Pubkey::new_unique(),
            compliance: Pubkey::new_unique(),
            keeper: Pubkey::new_unique(),
            auditor: Pubkey::new_unique(),
        };
        assert!(v.all_assigned());
        v.keeper = Pubkey::default();
        assert!(!v.all_assigned());
    }
}
