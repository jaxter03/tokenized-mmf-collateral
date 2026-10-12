//! Hand-encoded Token-2022 PermissionedBurn extension (research R3).
//!
//! `anchor-spl` 1.1.2 uses `spl-token-2022-interface` 2.1.0, which predates this extension, so the
//! encoding is written out here. Values are taken from `spl-token-2022-interface` at the
//! `program@v11.0.0` tag, the build deployed on mainnet.
//!
//! - `TokenInstruction::PermissionedBurnExtension` = 46, then a one-byte sub-instruction:
//!   0 = Initialize { authority: Pubkey }, 1 = Burn { amount: u64 }, 2 = BurnChecked { amount: u64,
//!   decimals: u8 }.
//! - `ExtensionType::PermissionedBurn` = 28; its config is one 32-byte (nullable) pubkey.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};

use crate::ids::TOKEN_2022_PROGRAM_ID;

/// `TokenInstruction::PermissionedBurnExtension`.
pub const TOKEN_INSTRUCTION_PERMISSIONED_BURN: u8 = 46;
/// `PermissionedBurnInstruction::Initialize`.
pub const PERMISSIONED_BURN_INITIALIZE: u8 = 0;
/// `PermissionedBurnInstruction::BurnChecked`.
pub const PERMISSIONED_BURN_BURN_CHECKED: u8 = 2;

/// `ExtensionType::PermissionedBurn` (u16, little-endian in the TLV header).
pub const PERMISSIONED_BURN_EXTENSION_TYPE: u16 = 28;
/// Size of `PermissionedBurnConfig` (one nullable pubkey).
pub const PERMISSIONED_BURN_CONFIG_LEN: usize = 32;
/// Bytes the extension adds to a mint: 2-byte type + 2-byte length + config.
pub const PERMISSIONED_BURN_TLV_LEN: usize = 4 + PERMISSIONED_BURN_CONFIG_LEN;

/// Instruction data for Initialize: `[46, 0, authority]`.
pub fn initialize_data(authority: &Pubkey) -> [u8; 34] {
    let mut data = [0u8; 34];
    data[0] = TOKEN_INSTRUCTION_PERMISSIONED_BURN;
    data[1] = PERMISSIONED_BURN_INITIALIZE;
    data[2..].copy_from_slice(authority.as_ref());
    data
}

/// Initialize the extension on an uninitialized mint (must run before `InitializeMint2`).
/// Accounts: `[writable] mint`.
pub fn initialize(mint: &Pubkey, authority: &Pubkey) -> Instruction {
    Instruction {
        program_id: TOKEN_2022_PROGRAM_ID,
        accounts: vec![AccountMeta::new(*mint, false)],
        data: initialize_data(authority).to_vec(),
    }
}

/// Instruction data for BurnChecked: `[46, 2, amount (u64 LE), decimals]`.
pub fn burn_checked_data(amount: u64, decimals: u8) -> [u8; 11] {
    let mut data = [0u8; 11];
    data[0] = TOKEN_INSTRUCTION_PERMISSIONED_BURN;
    data[1] = PERMISSIONED_BURN_BURN_CHECKED;
    data[2..10].copy_from_slice(&amount.to_le_bytes());
    data[10] = decimals;
    data
}

/// Permissioned BurnChecked. Accounts: `[writable] source, [writable] mint,
/// [signer] burn authority, [signer] source owner or delegate`.
pub fn burn_checked(
    source: &Pubkey,
    mint: &Pubkey,
    burn_authority: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    decimals: u8,
) -> Instruction {
    Instruction {
        program_id: TOKEN_2022_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*source, false),
            AccountMeta::new(*mint, false),
            AccountMeta::new_readonly(*burn_authority, true),
            AccountMeta::new_readonly(*owner, true),
        ],
        data: burn_checked_data(amount, decimals).to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_layout() {
        let a = Pubkey::new_unique();
        let d = initialize_data(&a);
        assert_eq!(&d[..2], &[46, 0]);
        assert_eq!(&d[2..], a.as_ref());
    }

    #[test]
    fn burn_checked_layout() {
        let d = burn_checked_data(0x0102_0304_0506_0708, 6);
        assert_eq!(d, [46, 2, 8, 7, 6, 5, 4, 3, 2, 1, 6]);
    }
}
