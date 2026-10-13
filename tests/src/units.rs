//! Placing units in tests (research R8). F001 has no way to create units (FR-035), and the fund
//! treasury's owner is a program PDA that no F001 instruction signs for, so units can't be moved
//! out of it. Tests therefore seed units directly into an approved investor's (or fund account's)
//! thawed holding account, and record exactly the bookkeeping a hook-checked transfer from a
//! fund-operated account would have done:
//! - F002: settle the holder's interest at the fund's current counter on its old balance first
//!   (hook step 8, research R12), so seeded units never earn for publishes made before they
//!   arrived;
//! - the holder's `last_balance`, and `holder_count` when an investor goes from zero to a balance;
//! - F003: `FundState.units_outstanding`, so it keeps equalling the mint's supply (invariant I4,
//!   research R18). Seeded units stand for units issued before the test began.
//!
//! Every later movement is a real `transfer_checked`.

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022 as t22;
use compliance_hook::{HolderState, Registry};
use fund::FundState;
use t22::extension::{StateWithExtensions, StateWithExtensionsMut};
use t22::state::{Account as TokenAccount, AccountState, Mint};

use crate::fixtures::{patch, read};
use crate::harness::Harness;

/// Adds `amount` units to `owner`'s holding account (must exist and be thawed), to supply and
/// `units_outstanding`, and to the holder's recorded balance, after settling its interest and
/// updating the holder count as the hook would.
pub fn seed_units(h: &mut Harness, mint: &Pubkey, owner: &Pubkey, amount: u64) {
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
    let fund_key = mmf_sdk::pda::fund_state(mint).0;
    patch::<FundState>(h, &fund_key, |fs| {
        fs.units_outstanding = fs.units_outstanding.checked_add(amount).unwrap();
    });

    let index = read::<FundState>(h, &fund_key).accrual_index;
    let holder_key = mmf_sdk::pda::holder(mint, owner).0;
    let mut newly_counted = false;
    patch::<HolderState>(h, &holder_key, |hs| {
        // Step 8 bookkeeping, written out here rather than via `mmf_common::accrual` so the test
        // helper doesn't share code with what it helps test.
        let delta = index.checked_sub(hs.index_at_last_touch).unwrap();
        hs.accrued_e18 = hs
            .accrued_e18
            .checked_add(u128::from(hs.last_balance) * delta)
            .unwrap();
        hs.index_at_last_touch = index;
        newly_counted = hs.is_investor() && hs.last_balance == 0 && amount > 0;
        hs.last_balance = hs.last_balance.checked_add(amount).unwrap();
    });
    if newly_counted {
        patch::<Registry>(h, &mmf_sdk::pda::registry(mint).0, |r| r.holder_count += 1);
    }
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
