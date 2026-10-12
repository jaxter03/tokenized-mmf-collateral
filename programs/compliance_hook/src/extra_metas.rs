//! The fund mint's `ExtraAccountMetaList` (data-model.md "ExtraAccountMetaList",
//! docs/shared-model.md §4.1). Token-2022 resolves these accounts, in this order, for every
//! transfer and passes them to the hook after the five base accounts
//! (0 source, 1 mint, 2 destination, 3 authority, 4 this list):
//!
//! | # | Account | Resolution | Writable |
//! |---|---|---|---|
//! | 5 | Registry | literal PDA | yes — the hook maintains `holder_count` (research R5) |
//! | 6 | FundState | literal PDA | no |
//! | 7 | HolderState(source owner) | seeds `["holder", mint, source.owner]` | yes |
//! | 8 | HolderState(destination owner) | seeds `["holder", mint, destination.owner]` | yes |
//! | 9 | governance delegate `["delegate", mint]` | literal pubkey | no |

use anchor_lang::prelude::*;
use mmf_common::seeds;
use spl_tlv_account_resolution::account::ExtraAccountMeta;
use spl_tlv_account_resolution::seeds::Seed;
use spl_tlv_account_resolution::state::ExtraAccountMetaList;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

/// Registry, FundState, HolderState(source owner), HolderState(destination owner), delegate PDA.
pub const EXTRA_META_COUNT: usize = 5;

/// Base-account indices in the Execute instruction.
const SOURCE_INDEX: u8 = 0;
const MINT_INDEX: u8 = 1;
const DESTINATION_INDEX: u8 = 2;
/// The owner field of a token account: bytes 32..64.
const OWNER_OFFSET: u8 = 32;

/// Account size for the list.
pub fn extra_metas_space() -> usize {
    ExtraAccountMetaList::size_of(EXTRA_META_COUNT).expect("fixed, small entry count")
}

fn holder_meta(token_account_index: u8) -> Result<ExtraAccountMeta> {
    Ok(ExtraAccountMeta::new_with_seeds(
        &[
            Seed::Literal {
                bytes: seeds::HOLDER.to_vec(),
            },
            Seed::AccountKey { index: MINT_INDEX },
            Seed::AccountData {
                account_index: token_account_index,
                data_index: OWNER_OFFSET,
                length: 32,
            },
        ],
        false,
        true,
    )?)
}

/// The five entries, in order.
pub fn extra_metas(
    registry: &Pubkey,
    fund_state: &Pubkey,
    delegate: &Pubkey,
) -> Result<[ExtraAccountMeta; 5]> {
    Ok([
        ExtraAccountMeta::new_with_pubkey(registry, false, true)?,
        ExtraAccountMeta::new_with_pubkey(fund_state, false, false)?,
        holder_meta(SOURCE_INDEX)?,
        holder_meta(DESTINATION_INDEX)?,
        ExtraAccountMeta::new_with_pubkey(delegate, false, false)?,
    ])
}

/// Writes the list into the (already allocated) account.
pub fn write_extra_metas(account: &AccountInfo, metas: &[ExtraAccountMeta]) -> Result<()> {
    let mut data = account.try_borrow_mut_data()?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, metas)?;
    Ok(())
}
