//! Cash-token checks shared by `fund` and `dvp_settlement` (F003, research R14).
//!
//! The cash asset may live under SPL Token or Token-2022 (F001 FR-001). Both share the base
//! account and mint layout, so one parser reads either. A Token-2022 cash mint that charges a
//! transfer fee or runs its own transfer hook would change the exact cash amounts settlement
//! moves (FR-008), so it is not supported; an unreadable mint counts as unsupported (fail closed).

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022::extension::{
    BaseStateWithExtensions, ExtensionType, StateWithExtensions,
};
use anchor_spl::token_2022::spl_token_2022::state::{Account, AccountState, Mint};

use crate::ids::{SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};

fn is_token_program(program: &Pubkey) -> bool {
    *program == SPL_TOKEN_PROGRAM_ID || *program == TOKEN_2022_PROGRAM_ID
}

/// True for an initialized mint owned by SPL Token, or by Token-2022 without a transfer-fee or
/// transfer-hook extension. `program` is the mint account's owner.
pub fn mint_supported(program: &Pubkey, data: &[u8]) -> bool {
    if !is_token_program(program) {
        return false;
    }
    let Ok(state) = StateWithExtensions::<Mint>::unpack(data) else {
        return false;
    };
    if !state.base.is_initialized {
        return false;
    }
    if *program == SPL_TOKEN_PROGRAM_ID {
        return true;
    }
    match state.get_extension_types() {
        Ok(types) => !types.iter().any(|t| {
            matches!(
                t,
                ExtensionType::TransferFeeConfig | ExtensionType::TransferHook
            )
        }),
        Err(_) => false,
    }
}

/// Decimals of an initialized SPL Token or Token-2022 mint; `None` for anything else.
pub fn mint_decimals(program: &Pubkey, data: &[u8]) -> Option<u8> {
    if !is_token_program(program) {
        return None;
    }
    let state = StateWithExtensions::<Mint>::unpack(data).ok()?;
    state.base.is_initialized.then_some(state.base.decimals)
}

/// The fields of a token account settlement checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenAccountFields {
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub frozen: bool,
}

/// Reads an initialized (or frozen) token account owned by SPL Token or Token-2022. `program` is
/// the account's owner. `None` for anything else.
pub fn token_account(program: &Pubkey, data: &[u8]) -> Option<TokenAccountFields> {
    if !is_token_program(program) {
        return None;
    }
    let state = StateWithExtensions::<Account>::unpack(data).ok()?;
    let base = state.base;
    if base.state == AccountState::Uninitialized {
        return None;
    }
    Some(TokenAccountFields {
        mint: base.mint,
        owner: base.owner,
        amount: base.amount,
        frozen: base.state == AccountState::Frozen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program_option::COption;
    use solana_program_pack::Pack;

    fn classic_mint() -> Vec<u8> {
        let mut d = vec![0u8; Mint::LEN];
        Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        }
        .pack_into_slice(&mut d);
        d
    }

    #[test]
    fn classic_mint_is_supported_under_either_program_only() {
        let d = classic_mint();
        assert!(mint_supported(&SPL_TOKEN_PROGRAM_ID, &d));
        assert!(mint_supported(&TOKEN_2022_PROGRAM_ID, &d), "no extensions");
        assert!(!mint_supported(&Pubkey::new_unique(), &d), "foreign owner");
        assert!(
            !mint_supported(&SPL_TOKEN_PROGRAM_ID, &[0u8; 10]),
            "garbage"
        );
        let mut uninit = d.clone();
        uninit[45] = 0; // is_initialized
        assert!(!mint_supported(&SPL_TOKEN_PROGRAM_ID, &uninit));
    }

    #[test]
    fn token_account_fields() {
        let (mint, owner) = (Pubkey::new_unique(), Pubkey::new_unique());
        let mut d = vec![0u8; Account::LEN];
        Account {
            mint,
            owner,
            amount: 42,
            state: AccountState::Frozen,
            ..Account::default()
        }
        .pack_into_slice(&mut d);
        let f = token_account(&SPL_TOKEN_PROGRAM_ID, &d).unwrap();
        assert_eq!(
            (f.mint, f.owner, f.amount, f.frozen),
            (mint, owner, 42, true)
        );
        assert_eq!(token_account(&Pubkey::new_unique(), &d), None);
        assert_eq!(
            token_account(&SPL_TOKEN_PROGRAM_ID, &[0u8; Account::LEN]),
            None
        );
    }
}
