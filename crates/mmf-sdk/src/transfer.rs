//! `transfer_checked` with the hook's extra accounts (FR-020).
//!
//! - [`transfer_checked_with_hook`] resolves the extra accounts from the on-chain
//!   `ExtraAccountMetaList` (via `spl_transfer_hook_interface::offchain`), as any wallet would.
//! - [`transfer_checked_manual`] builds the same account list by hand from `pda`, with no resolver,
//!   to show the rules apply however a transfer is built.
//!
//! Both append, after Token-2022's four accounts: Registry (writable), FundState,
//! HolderState(source owner) (writable), HolderState(destination owner) (writable), delegate PDA,
//! then the hook program id and the extra-account list (the order the resolver uses).

use std::future::Future;

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_spl::token_2022::spl_token_2022;
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use spl_transfer_hook_interface::offchain::{
    add_extra_account_metas_for_execute, AccountDataResult, AccountFetchError,
};

use crate::pda;

/// The fund unit's decimals.
pub const UNIT_DECIMALS: u8 = 6;

fn base(
    source: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Instruction {
    spl_token_2022::instruction::transfer_checked(
        &TOKEN_2022_PROGRAM_ID,
        source,
        mint,
        destination,
        authority,
        &[],
        amount,
        UNIT_DECIMALS,
    )
    .expect("valid transfer_checked")
}

/// Resolves the extra accounts from chain data. `fetch` returns an account's data (or `None`).
pub async fn transfer_checked_with_hook<F, Fut>(
    source: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
    fetch: F,
) -> Result<Instruction, AccountFetchError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = AccountDataResult>,
{
    let mut ix = base(source, mint, destination, authority, amount);
    add_extra_account_metas_for_execute(
        &mut ix,
        &COMPLIANCE_HOOK_PROGRAM_ID,
        source,
        mint,
        destination,
        authority,
        amount,
        fetch,
    )
    .await?;
    Ok(ix)
}

/// Builds the transfer and its extra accounts by hand (no resolver).
#[allow(clippy::too_many_arguments)]
pub fn transfer_checked_manual(
    source: &Pubkey,
    source_owner: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    destination_owner: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Instruction {
    let mut ix = base(source, mint, destination, authority, amount);
    let fund_state = pda::fund_state(mint).0;
    ix.accounts.extend([
        AccountMeta::new(pda::registry(mint).0, false),
        AccountMeta::new_readonly(fund_state, false),
        AccountMeta::new(pda::holder(mint, source_owner).0, false),
        AccountMeta::new(pda::holder(mint, destination_owner).0, false),
        AccountMeta::new_readonly(pda::delegate(mint).0, false),
        AccountMeta::new_readonly(COMPLIANCE_HOOK_PROGRAM_ID, false),
        AccountMeta::new_readonly(pda::extra_metas(mint).0, false),
    ]);
    ix
}

/// The owner's holding account to the other owner's holding account, signed by `from_owner`.
pub fn transfer_between_owners(
    mint: &Pubkey,
    from_owner: &Pubkey,
    to_owner: &Pubkey,
    amount: u64,
) -> Instruction {
    transfer_checked_manual(
        &pda::holding_account(from_owner, mint),
        from_owner,
        mint,
        &pda::holding_account(to_owner, mint),
        to_owner,
        from_owner,
        amount,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use spl_tlv_account_resolution::state::ExtraAccountMetaList;
    use spl_transfer_hook_interface::instruction::ExecuteInstruction;
    use std::collections::HashMap;

    fn token_account_data(mint: &Pubkey, owner: &Pubkey) -> Vec<u8> {
        let mut d = vec![0u8; 165];
        d[..32].copy_from_slice(mint.as_ref());
        d[32..64].copy_from_slice(owner.as_ref());
        d
    }

    #[test]
    fn manual_matches_resolver() {
        let mint = Pubkey::new_unique();
        let (alice, bob) = (Pubkey::new_unique(), Pubkey::new_unique());
        let (src, dst) = (
            pda::holding_account(&alice, &mint),
            pda::holding_account(&bob, &mint),
        );

        // The extra-account list exactly as init_registry writes it.
        let metas = compliance_hook::extra_metas::extra_metas(
            &pda::registry(&mint).0,
            &pda::fund_state(&mint).0,
            &pda::delegate(&mint).0,
        )
        .unwrap();
        let mut list = vec![0u8; compliance_hook::extra_metas::extra_metas_space()];
        ExtraAccountMetaList::init::<ExecuteInstruction>(&mut list, &metas).unwrap();

        let mut chain: HashMap<Pubkey, Vec<u8>> = HashMap::new();
        chain.insert(pda::extra_metas(&mint).0, list);
        chain.insert(src, token_account_data(&mint, &alice));
        chain.insert(dst, token_account_data(&mint, &bob));
        let fetch = |k: Pubkey| {
            let v = chain.get(&k).cloned();
            async move { Ok(v) }
        };

        let resolved = futures::executor::block_on(transfer_checked_with_hook(
            &src, &mint, &dst, &alice, 5, fetch,
        ))
        .unwrap();
        let manual = transfer_checked_manual(&src, &alice, &mint, &dst, &bob, &alice, 5);
        assert_eq!(resolved.accounts, manual.accounts);
        assert_eq!(resolved.data, manual.data);
        assert_eq!(
            transfer_between_owners(&mint, &alice, &bob, 5).accounts,
            manual.accounts
        );
    }
}
