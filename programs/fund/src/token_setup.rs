//! Builds the fund mint with every extension in one instruction (research R2, R3;
//! data-model.md "Fund mint"). Order matters: extensions are initialized before
//! `InitializeMint2`, and token metadata after it.
//!
//! 1. create the account sized for the fixed-size extensions (incl. PermissionedBurn)
//! 2. TransferHook (program = compliance_hook, authority = governance `["token-admin", mint]`)
//! 3. DefaultAccountState = Frozen
//! 4. PermanentDelegate = governance `["delegate", mint]`
//! 5. MetadataPointer → the mint itself, authority = FundState
//! 6. MintCloseAuthority = FundState
//! 7. PermissionedBurn, authority = FundState (hand-encoded; `anchor-spl` 1.1.2 predates it)
//! 8. InitializeMint2 (6 decimals, mint authority = FundState, freeze authority = Registry PDA)
//! 9. top up lamports for the final size including metadata
//! 10. TokenMetadata initialize (name, symbol, uri = terms_uri; update authority = FundState)
//! 11. UpdateField `fund_id`, then `terms_hash` (lowercase hex)

use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_lang::system_program;
use anchor_spl::token_2022::spl_token_2022::extension::ExtensionType;
use anchor_spl::token_2022::spl_token_2022::state::{AccountState, Mint};
use anchor_spl::token_2022::{self, InitializeMint2};
use anchor_spl::token_2022_extensions::{
    default_account_state_initialize, metadata_pointer_initialize, mint_close_authority_initialize,
    permanent_delegate_initialize, token_metadata_initialize, token_metadata_update_field,
    transfer_hook_initialize, DefaultAccountStateInitialize, MetadataPointerInitialize,
    MintCloseAuthorityInitialize, PermanentDelegateInitialize, TokenMetadataInitialize,
    TokenMetadataUpdateField, TransferHookInitialize,
};
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, GOVERNANCE_PROGRAM_ID};
use mmf_common::{permissioned_burn, seeds};
use spl_token_metadata_interface::state::{Field, TokenMetadata};

/// Fund unit decimals (docs/shared-model.md §1).
pub const UNIT_DECIMALS: u8 = 6;

/// Metadata field keys.
pub const FIELD_FUND_ID: &str = "fund_id";
pub const FIELD_TERMS_HASH: &str = "terms_hash";

pub struct MintSetup<'a, 'info> {
    pub payer: &'a AccountInfo<'info>,
    pub mint: &'a AccountInfo<'info>,
    pub fund_state: &'a AccountInfo<'info>,
    pub fund_state_bump: u8,
    pub token_program: &'a AccountInfo<'info>,
    pub system_program: &'a AccountInfo<'info>,
}

/// What goes into the token metadata.
pub struct MintMetadata {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub fund_id_hex: String,
    pub terms_hash_hex: String,
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

pub fn create_fund_mint(a: MintSetup, md: MintMetadata) -> Result<()> {
    let mint_key = a.mint.key();
    let fund_key = a.fund_state.key();
    let token_admin = Pubkey::find_program_address(
        &[seeds::TOKEN_ADMIN, mint_key.as_ref()],
        &GOVERNANCE_PROGRAM_ID,
    )
    .0;
    let delegate = Pubkey::find_program_address(
        &[seeds::DELEGATE, mint_key.as_ref()],
        &GOVERNANCE_PROGRAM_ID,
    )
    .0;
    let registry = Pubkey::find_program_address(
        &[seeds::REGISTRY, mint_key.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
    .0;
    let token_program_id = a.token_program.key();

    // 1. Account sized exactly for the fixed-size extensions: InitializeMint2 requires the length to
    // match, so metadata space is added later (step 9).
    let fixed_len = ExtensionType::try_calculate_account_len::<Mint>(&[
        ExtensionType::TransferHook,
        ExtensionType::DefaultAccountState,
        ExtensionType::PermanentDelegate,
        ExtensionType::MetadataPointer,
        ExtensionType::MintCloseAuthority,
    ])?
    .checked_add(permissioned_burn::PERMISSIONED_BURN_TLV_LEN)
    .ok_or(ProgramError::ArithmeticOverflow)?;
    let rent = Rent::get()?;
    system_program::create_account(
        CpiContext::new(
            a.system_program.key(),
            system_program::CreateAccount {
                from: a.payer.clone(),
                to: a.mint.clone(),
            },
        ),
        rent.minimum_balance(fixed_len),
        fixed_len as u64,
        &token_program_id,
    )?;

    // 2–6. Fixed-size extensions.
    transfer_hook_initialize(
        CpiContext::new(
            token_program_id,
            TransferHookInitialize {
                token_program_id: a.token_program.clone(),
                mint: a.mint.clone(),
            },
        ),
        Some(token_admin),
        Some(COMPLIANCE_HOOK_PROGRAM_ID),
    )?;
    default_account_state_initialize(
        CpiContext::new(
            token_program_id,
            DefaultAccountStateInitialize {
                token_program_id: a.token_program.clone(),
                mint: a.mint.clone(),
            },
        ),
        &AccountState::Frozen,
    )?;
    permanent_delegate_initialize(
        CpiContext::new(
            token_program_id,
            PermanentDelegateInitialize {
                token_program_id: a.token_program.clone(),
                mint: a.mint.clone(),
            },
        ),
        &delegate,
    )?;
    metadata_pointer_initialize(
        CpiContext::new(
            token_program_id,
            MetadataPointerInitialize {
                token_program_id: a.token_program.clone(),
                mint: a.mint.clone(),
            },
        ),
        Some(fund_key),
        Some(mint_key),
    )?;
    mint_close_authority_initialize(
        CpiContext::new(
            token_program_id,
            MintCloseAuthorityInitialize {
                token_program_id: a.token_program.clone(),
                mint: a.mint.clone(),
            },
        ),
        Some(&fund_key),
    )?;

    // 7. Permissioned burn: only the fund PDA can co-sign burns (research R3).
    invoke(
        &permissioned_burn::initialize(&mint_key, &fund_key),
        std::slice::from_ref(a.mint),
    )?;

    // 8. The mint itself.
    token_2022::initialize_mint2(
        CpiContext::new(
            token_program_id,
            InitializeMint2 {
                mint: a.mint.clone(),
            },
        ),
        UNIT_DECIMALS,
        &fund_key,
        Some(&registry),
    )?;

    // 9. Fund the final size (metadata grows the account in steps 10–11).
    let metadata = TokenMetadata {
        update_authority: Some(fund_key).try_into()?,
        mint: mint_key,
        name: md.name.clone(),
        symbol: md.symbol.clone(),
        uri: md.uri.clone(),
        additional_metadata: vec![
            (FIELD_FUND_ID.to_string(), md.fund_id_hex.clone()),
            (FIELD_TERMS_HASH.to_string(), md.terms_hash_hex.clone()),
        ],
    };
    let final_len = fixed_len
        .checked_add(metadata.tlv_size_of()?)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    let top_up = rent
        .minimum_balance(final_len)
        .saturating_sub(a.mint.lamports());
    if top_up > 0 {
        system_program::transfer(
            CpiContext::new(
                a.system_program.key(),
                system_program::Transfer {
                    from: a.payer.clone(),
                    to: a.mint.clone(),
                },
            ),
            top_up,
        )?;
    }

    // 10–11. Metadata, signed by FundState (mint authority and update authority).
    let bump = [a.fund_state_bump];
    let signer: &[&[&[u8]]] = &[&[seeds::FUND, mint_key.as_ref(), &bump]];
    token_metadata_initialize(
        CpiContext::new(
            token_program_id,
            TokenMetadataInitialize {
                program_id: a.token_program.clone(),
                metadata: a.mint.clone(),
                update_authority: a.fund_state.clone(),
                mint_authority: a.fund_state.clone(),
                mint: a.mint.clone(),
            },
        )
        .with_signer(signer),
        md.name,
        md.symbol,
        md.uri,
    )?;
    for (key, value) in [
        (FIELD_FUND_ID, md.fund_id_hex),
        (FIELD_TERMS_HASH, md.terms_hash_hex),
    ] {
        token_metadata_update_field(
            CpiContext::new(
                token_program_id,
                TokenMetadataUpdateField {
                    program_id: a.token_program.clone(),
                    metadata: a.mint.clone(),
                    update_authority: a.fund_state.clone(),
                },
            )
            .with_signer(signer),
            Field::Key(key.to_string()),
            value,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::hex;

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex(b"MMF"), "4d4d46");
    }
}
