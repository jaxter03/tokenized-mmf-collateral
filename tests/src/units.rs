//! Placing units in tests (research R8). F001 has no way to create units (FR-035), so tests put
//! them in the fund treasury's approved, thawed holding account by patching its balance and the
//! mint's supply. From there units reach investors only through real `transfer_checked` calls, so
//! the hook maintains the holder count itself. No test edits `HolderState` or `Registry` here.

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022 as t22;
use t22::extension::{StateWithExtensions, StateWithExtensionsMut};
use t22::state::{Account as TokenAccount, AccountState, Mint};

use crate::harness::Harness;

/// Adds `amount` units to `owner`'s holding account (must exist and be thawed) and to supply.
pub fn place_units(h: &mut Harness, mint: &Pubkey, owner: &Pubkey, amount: u64) {
    let ata = mmf_sdk::pda::holding_account(owner, mint);
    let mut acc = h.account(&ata).expect("holding account exists");
    {
        let mut st = StateWithExtensionsMut::<TokenAccount>::unpack(&mut acc.data).unwrap();
        assert_eq!(
            st.base.state,
            AccountState::Initialized,
            "holding account must be thawed"
        );
        st.base.amount = st.base.amount.checked_add(amount).unwrap();
        st.pack_base();
    }
    h.svm.set_account(ata, acc).unwrap();

    let mut macc = h.account(mint).expect("mint exists");
    {
        let mut st = StateWithExtensionsMut::<Mint>::unpack(&mut macc.data).unwrap();
        st.base.supply = st.base.supply.checked_add(amount).unwrap();
        st.pack_base();
    }
    h.svm.set_account(*mint, macc).unwrap();
}

/// Balance of `owner`'s holding account (0 if it doesn't exist).
pub fn balance(h: &Harness, mint: &Pubkey, owner: &Pubkey) -> u64 {
    token_balance(h, &mmf_sdk::pda::holding_account(owner, mint))
}

/// Balance of any token account (0 if it doesn't exist).
pub fn token_balance(h: &Harness, token_account: &Pubkey) -> u64 {
    h.account(token_account)
        .map(|a| {
            StateWithExtensions::<TokenAccount>::unpack(&a.data)
                .unwrap()
                .base
                .amount
        })
        .unwrap_or(0)
}

/// Whether a token account is frozen.
pub fn is_frozen(h: &Harness, token_account: &Pubkey) -> bool {
    let a = h.account(token_account).expect("token account exists");
    StateWithExtensions::<TokenAccount>::unpack(&a.data)
        .unwrap()
        .base
        .state
        == AccountState::Frozen
}

/// Total supply of the mint.
pub fn supply(h: &Harness, mint: &Pubkey) -> u64 {
    let a = h.account(mint).expect("mint exists");
    StateWithExtensions::<Mint>::unpack(&a.data)
        .unwrap()
        .base
        .supply
}
