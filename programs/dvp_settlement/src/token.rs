//! Token movements into and out of escrow (research R5, R14). Both build Token-2022's
//! `transfer_checked` and invoke it directly, so the caller controls exactly which accounts are
//! passed:
//! - cash: under the cash mint's own program (SPL Token or Token-2022; the instruction encoding is
//!   the same);
//! - units: under Token-2022 with the hook's extra accounts appended in the order of the
//!   `ExtraAccountMetaList` (shared-model §4.1), then the hook program and the list itself, so
//!   the hook runs in full.
//!
//! `signer_seeds` is empty when the authority signed the outer transaction (a party funding its
//! leg), and the escrow PDA's seeds when money leaves escrow.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::token_2022::spl_token_2022;
use mmf_common::cash;
use mmf_common::settlement::UNIT_DECIMALS;

use crate::errors::DvpError;

/// Moves `amount` of the cash mint from `from` to `to`.
pub fn transfer_cash<'info>(
    token_program: &AccountInfo<'info>,
    from: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let decimals = {
        let data = mint.try_borrow_data()?;
        cash::mint_decimals(mint.owner, &data).ok_or(error!(DvpError::WrongAccount))?
    };
    let ix = spl_token_2022::instruction::transfer_checked(
        token_program.key,
        from.key,
        mint.key,
        to.key,
        authority.key,
        &[],
        amount,
        decimals,
    )?;
    invoke_signed(
        &ix,
        &[
            from.clone(),
            mint.clone(),
            to.clone(),
            authority.clone(),
            token_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}

/// The accounts a unit transfer needs: Token-2022's four, then the hook's.
pub struct UnitTransfer<'a, 'info> {
    pub token_2022_program: &'a AccountInfo<'info>,
    pub source: &'a AccountInfo<'info>,
    pub mint: &'a AccountInfo<'info>,
    pub destination: &'a AccountInfo<'info>,
    pub authority: &'a AccountInfo<'info>,
    pub registry: &'a AccountInfo<'info>,
    pub fund_state: &'a AccountInfo<'info>,
    pub source_holder: &'a AccountInfo<'info>,
    pub destination_holder: &'a AccountInfo<'info>,
    pub delegate: &'a AccountInfo<'info>,
    pub hook_program: &'a AccountInfo<'info>,
    pub extra_account_meta_list: &'a AccountInfo<'info>,
}

/// Moves `amount` units with `transfer_checked`; the hook checks and records it.
pub fn transfer_units(t: UnitTransfer, amount: u64, signer_seeds: &[&[&[u8]]]) -> Result<()> {
    let mut ix = spl_token_2022::instruction::transfer_checked(
        t.token_2022_program.key,
        t.source.key,
        t.mint.key,
        t.destination.key,
        t.authority.key,
        &[],
        amount,
        UNIT_DECIMALS,
    )?;
    ix.accounts.extend([
        AccountMeta::new(*t.registry.key, false),
        AccountMeta::new_readonly(*t.fund_state.key, false),
        AccountMeta::new(*t.source_holder.key, false),
        AccountMeta::new(*t.destination_holder.key, false),
        AccountMeta::new_readonly(*t.delegate.key, false),
        AccountMeta::new_readonly(*t.hook_program.key, false),
        AccountMeta::new_readonly(*t.extra_account_meta_list.key, false),
    ]);
    invoke_signed(
        &ix,
        &[
            t.source.clone(),
            t.mint.clone(),
            t.destination.clone(),
            t.authority.clone(),
            t.registry.clone(),
            t.fund_state.clone(),
            t.source_holder.clone(),
            t.destination_holder.clone(),
            t.delegate.clone(),
            t.hook_program.clone(),
            t.extra_account_meta_list.clone(),
            t.token_2022_program.clone(),
        ],
        signer_seeds,
    )?;
    Ok(())
}
