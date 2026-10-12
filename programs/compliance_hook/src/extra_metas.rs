//! The fund mint's `ExtraAccountMetaList` (data-model.md "ExtraAccountMetaList"). Entries are
//! filled in by `init_registry` (T037).

use anchor_lang::prelude::*;
use spl_tlv_account_resolution::state::ExtraAccountMetaList;

/// Registry, FundState, HolderState(source owner), HolderState(destination owner), delegate PDA.
pub const EXTRA_META_COUNT: usize = 5;

/// Account size for the list.
pub fn extra_metas_space() -> usize {
    ExtraAccountMetaList::size_of(EXTRA_META_COUNT).expect("fixed, small entry count")
}
