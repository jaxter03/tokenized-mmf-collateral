//! `accrue(post_balance)` (contracts/compliance_hook.md, F003 research R8): settles a holder's
//! interest before a mint or burn changes its balance. Mint and burn don't run the hook, so `fund`
//! calls this first, signed by its FundState PDA (F002 FR-013). F003 mints to and burns from the
//! escrow only, so only the escrow's record is accepted; F006 widens it to investors.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use mmf_common::accrual;
use mmf_common::cash;
use mmf_common::enums::HolderClass;
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::HookError;
use crate::events::Accrued;
use crate::state::{HolderState, Registry};

#[derive(Accounts)]
pub struct Accrue<'info> {
    /// The fund's FundState PDA (`registry.fund_state`), signing for `fund`; also the source of
    /// the interest counter.
    pub fund_state: Signer<'info>,
    pub registry: Account<'info, Registry>,
    /// CHECK: the holder's record; loaded and stored by hand (`WrongHolder`, `ClassNotAccrued`).
    #[account(mut)]
    pub holder_state: UncheckedAccount<'info>,
    /// CHECK: the holder's Token-2022 holding account (its ATA); its balance is the pre-balance.
    pub holding_account: UncheckedAccount<'info>,
}

/// Checks `accrue`'s holder accounts (contracts/compliance_hook.md checks 3–4): the holding
/// account is the owner's Token-2022 ATA for `mint`, the record is that owner's HolderState, and
/// its class is Escrow. Public so the tests can show it rejects accounts `fund` never passes.
pub fn check_holder_accounts(
    mint: &Pubkey,
    holder_state: &AccountInfo,
    holding_account: &AccountInfo,
) -> Result<()> {
    holder_accounts(mint, holder_state, holding_account).map(|_| ())
}

/// The holder's record and its holding account's balance (the pre-balance), after the checks of
/// `check_holder_accounts`.
fn holder_accounts(
    mint: &Pubkey,
    holder_state: &AccountInfo,
    holding_account: &AccountInfo,
) -> Result<(HolderState, u64)> {
    // The owner's Token-2022 ATA for this mint.
    let holding = {
        let data = holding_account.try_borrow_data()?;
        (holding_account.owner == &TOKEN_2022_PROGRAM_ID)
            .then(|| cash::token_account(holding_account.owner, &data))
            .flatten()
            .filter(|t| t.mint == *mint)
            .ok_or(error!(HookError::WrongHolder))?
    };
    require_keys_eq!(
        holding_account.key(),
        get_associated_token_address_with_program_id(&holding.owner, mint, &TOKEN_2022_PROGRAM_ID),
        HookError::WrongHolder
    );
    // That owner's record, at its own PDA.
    let record = (holder_state.owner == &crate::ID && !holder_state.data_is_empty())
        .then(|| {
            let data = holder_state.try_borrow_data().ok()?;
            HolderState::try_deserialize(&mut &data[..]).ok()
        })
        .flatten()
        .filter(|h| h.mint == *mint && h.owner == holding.owner)
        .ok_or(error!(HookError::WrongHolder))?;
    let expected = Pubkey::create_program_address(
        &[
            seeds::HOLDER,
            mint.as_ref(),
            record.owner.as_ref(),
            &[record.bump],
        ],
        &crate::ID,
    )
    .map_err(|_| error!(HookError::WrongHolder))?;
    require_keys_eq!(holder_state.key(), expected, HookError::WrongHolder);
    // F003 mints to and burns from the escrow only.
    require!(
        record.class() == Some(HolderClass::Escrow),
        HookError::ClassNotAccrued
    );
    Ok((record, holding.amount))
}

pub fn handle_accrue(ctx: Context<Accrue>, post_balance: u64) -> Result<()> {
    let a = &ctx.accounts;

    // 1. Only the fund's own FundState PDA (signing for `fund`).
    require_keys_eq!(
        a.fund_state.key(),
        a.registry.fund_state,
        HookError::UnauthorizedCaller
    );
    // 2. Its interest counter.
    let index = FundStateView::load(&a.fund_state, &a.registry.fund_state)
        .map_err(|_| error!(HookError::WrongMint))?
        .accrual_index;
    // 3–4. The escrow's record and holding account.
    let (mut holder, pre_balance) =
        holder_accounts(&a.registry.mint, &a.holder_state, &a.holding_account)?;

    // 5. Settle on the balance held until now (F002 FR-013), then record the new balance.
    let (accrued, at) = accrual::settle(
        holder.accrued_e18,
        holder.index_at_last_touch,
        pre_balance,
        index,
    )
    .ok_or(error!(HookError::Overflow))?;
    holder.accrued_e18 = accrued;
    holder.index_at_last_touch = at;
    holder.last_balance = post_balance;
    let clock = Clock::get()?;
    holder.last_slot = clock.slot;
    {
        let mut data = a.holder_state.try_borrow_mut_data()?;
        holder.try_serialize(&mut &mut data[..])?;
    }

    // emit!, not emit_cpi!, to stay within the call-depth limit (research R15).
    emit!(Accrued {
        fund: a.registry.fund_state,
        owner: holder.owner,
        pre_balance,
        post_balance,
        accrual_index: index,
        accrued_e18: accrued,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
