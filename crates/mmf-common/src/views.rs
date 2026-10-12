//! Read-only views of accounts owned by other F001 programs (research R12).
//!
//! `compliance_hook` reads `FundState` (fund) and `RoleRegistry` (governance); `fund` reads
//! `Registry` (compliance_hook) and `RoleRegistry`; `governance` reads `FundState`. Depending on
//! each other's program crates would create a cycle, so each view decodes only the fields other
//! programs need, at fixed offsets in the Anchor/Borsh layout.
//!
//! Every `load` checks, in order: owning program, expected address, length, then the 8-byte
//! Anchor discriminator (`sha256("account:<Name>")[..8]`). Layout drift between a program and its
//! view is caught by `tests/tests/layouts.rs` (T019).

use anchor_lang::prelude::{AccountInfo, Pubkey};

use crate::ids::{COMPLIANCE_HOOK_PROGRAM_ID, FUND_PROGRAM_ID, GOVERNANCE_PROGRAM_ID};
use crate::jurisdiction::{JurisdictionSet, SET_WORDS};

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

/// Size of `FundTerms` in Borsh: 12 + 32 + 8 + 2 + 2 + 1 + 2 + 32 + 128.
const FUND_TERMS_LEN: usize = 219;

mod offsets {
    use super::FUND_TERMS_LEN;
    // All offsets include the 8-byte discriminator and the 1-byte `version`.
    pub const BASE: usize = 8 + 1;

    pub const FUND_MINT: usize = BASE;
    pub const FUND_ROLE_REGISTRY: usize = FUND_MINT + 32;
    pub const FUND_STATUS: usize = FUND_ROLE_REGISTRY + 32 + FUND_TERMS_LEN;

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

/// Fields of `FundState` (fund program) that other programs read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundStateView {
    pub mint: Pubkey,
    pub role_registry: Pubkey,
    /// `crate::enums::FundStatus` as stored.
    pub status: u8,
}

impl FundStateView {
    pub fn load(info: &AccountInfo, expected: &Pubkey) -> Result<Self, ViewError> {
        let d = checked(
            info,
            &FUND_PROGRAM_ID,
            expected,
            &FUND_STATE_DISCRIMINATOR,
            offsets::FUND_STATUS + 1,
        )?;
        Ok(Self::decode(&d))
    }

    /// Decodes already-validated account data.
    pub fn decode(d: &[u8]) -> Self {
        Self {
            mint: key_at(d, offsets::FUND_MINT),
            role_registry: key_at(d, offsets::FUND_ROLE_REGISTRY),
            status: d[offsets::FUND_STATUS],
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
}

impl RegistryView {
    pub fn load(info: &AccountInfo, expected: &Pubkey) -> Result<Self, ViewError> {
        let d = checked(
            info,
            &COMPLIANCE_HOOK_PROGRAM_ID,
            expected,
            &REGISTRY_DISCRIMINATOR,
            offsets::REG_JURISDICTIONS + 8 * SET_WORDS,
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
        }
    }

    pub fn has_provider(&self) -> bool {
        self.kyc_providers.iter().any(|k| *k != Pubkey::default())
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
        let (mut l, mut d) = (0u64, vec![0u8; 400]);
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
