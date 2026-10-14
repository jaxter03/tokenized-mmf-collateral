//! The transfer hook `execute` (contracts/compliance_hook.md "Transfer hook", shared-model §6).
//! Token-2022 calls it during `transfer_checked`, after moving the balances, with the five base
//! accounts read-only and then the extra accounts from the `ExtraAccountMetaList`.
//!
//! Steps (F001 builds 0, 1, 3, 4, 6, 7, 9, 10; F002 builds 8; F004 builds 5; step 2, the
//! enforcement bypass, arrives with F005):
//!  0. both token accounts carry the `transferring` flag      → NotTransferring
//!  1. accounts belong to this fund's mint                      → WrongMint
//!  3. transfers paused                                         → TransfersPaused
//!  4. sender approved with valid KYC                           → SenderNotApproved
//!  5. sender's post-balance covers its locked units            → PledgedUnitsLocked
//!  6. receiver approved, class allowed, jurisdiction allowed   → ReceiverNotApproved
//!  7. new investor while the cap is reached                    → HolderCapReached
//!  8. settle both sides' interest on their old balances         → WrongMint, Overflow
//!  9. holder count and last_balance / last_slot (once if same owner)
//! 10. emit! TransferValidated

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::extension::transfer_hook::TransferHookAccount;
use anchor_spl::token_2022::spl_token_2022::extension::{
    BaseStateWithExtensions, StateWithExtensions,
};
use anchor_spl::token_2022::spl_token_2022::state::Account as TokenAccount;
use mmf_common::accrual;
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::HookError;
use crate::events::TransferValidated;
use crate::state::{HolderState, Registry};

#[derive(Accounts)]
pub struct Execute<'info> {
    /// CHECK: source token account (parsed by hand; base accounts arrive read-only).
    pub source_token: UncheckedAccount<'info>,
    /// CHECK: the fund mint.
    pub mint: UncheckedAccount<'info>,
    /// CHECK: destination token account.
    pub destination_token: UncheckedAccount<'info>,
    /// CHECK: transfer authority (owner, delegate or permanent delegate).
    pub authority: UncheckedAccount<'info>,
    /// CHECK: this mint's ExtraAccountMetaList.
    pub extra_account_meta_list: UncheckedAccount<'info>,
    /// Extra 1: writable, the hook maintains `holder_count` (research R5).
    #[account(mut)]
    pub registry: Account<'info, Registry>,
    /// CHECK: extra 2: FundState (checked against `registry.fund_state`; decoded with
    /// `FundStateView` in step 8).
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: extra 3: HolderState of the source owner; loaded by hand (a missing record counts as
    /// not approved; the same record may also be the destination — research R4).
    #[account(mut)]
    pub source_holder: UncheckedAccount<'info>,
    /// CHECK: extra 4: HolderState of the destination owner.
    #[account(mut)]
    pub destination_holder: UncheckedAccount<'info>,
    /// CHECK: extra 5: governance delegate PDA (enforcement bypass lands with F005).
    pub delegate: UncheckedAccount<'info>,
}

/// Owner and post-transfer balance of a token account that is mid-transfer (step 0).
fn transferring_account(info: &AccountInfo) -> Result<(Pubkey, u64)> {
    let data = info.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)
        .map_err(|_| error!(HookError::NotTransferring))?;
    let flag = state
        .get_extension::<TransferHookAccount>()
        .map(|e| bool::from(e.transferring))
        .unwrap_or(false);
    require!(flag, HookError::NotTransferring);
    Ok((state.base.owner, state.base.amount))
}

/// The holder record, or `None` if the account isn't one of ours (missing = not approved).
fn load_holder(info: &AccountInfo) -> Option<HolderState> {
    if info.owner != &crate::ID || info.data_is_empty() {
        return None;
    }
    let data = info.try_borrow_data().ok()?;
    HolderState::try_deserialize(&mut &data[..]).ok()
}

fn store_holder(info: &AccountInfo, holder: &HolderState) -> Result<()> {
    let mut data = info.try_borrow_mut_data()?;
    holder.try_serialize(&mut &mut data[..])
}

/// Step 9 for one owner: holder count follows `last_balance > 0` for investors (invariant I10).
fn record_balance(
    registry: &mut Registry,
    holder: &mut HolderState,
    post_balance: u64,
    slot: u64,
) -> Result<()> {
    if holder.is_investor() {
        let was = holder.last_balance > 0;
        let now = post_balance > 0;
        registry.holder_count = match (was, now) {
            (false, true) => registry
                .holder_count
                .checked_add(1)
                .ok_or(error!(HookError::Overflow))?,
            (true, false) => registry
                .holder_count
                .checked_sub(1)
                .ok_or(error!(HookError::Overflow))?,
            _ => registry.holder_count,
        };
    }
    holder.last_balance = post_balance;
    holder.last_slot = slot;
    Ok(())
}

/// Step 8 for one holder: credit the interest earned on `pre_balance` since its last settlement,
/// then move its checkpoint to `index` (shared-model §6; exact, research R1).
fn settle(holder: &mut HolderState, pre_balance: u64, index: u128) -> Result<()> {
    let (accrued, at) = accrual::settle(
        holder.accrued_e18,
        holder.index_at_last_touch,
        pre_balance,
        index,
    )
    .ok_or(error!(HookError::Overflow))?;
    holder.accrued_e18 = accrued;
    holder.index_at_last_touch = at;
    Ok(())
}

pub fn handle_execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
    let a = &ctx.accounts;

    // 0. Only during a real Token-2022 transfer.
    let (src_owner, post_src) = transferring_account(&a.source_token)?;
    let (dst_owner, post_dst) = transferring_account(&a.destination_token)?;

    // 1. Every account belongs to this fund's mint.
    let mint = a.mint.key();
    require_keys_eq!(a.registry.mint, mint, HookError::WrongMint);
    require_keys_eq!(
        a.fund_state.key(),
        a.registry.fund_state,
        HookError::WrongMint
    );
    let holder_pda = |owner: &Pubkey| {
        Pubkey::find_program_address(&[seeds::HOLDER, mint.as_ref(), owner.as_ref()], &crate::ID).0
    };
    let src_key = holder_pda(&src_owner);
    let dst_key = holder_pda(&dst_owner);
    require_keys_eq!(a.source_holder.key(), src_key, HookError::WrongMint);
    require_keys_eq!(a.destination_holder.key(), dst_key, HookError::WrongMint);
    let same_owner = src_key == dst_key;

    // 3. Paused: every transfer stops (FR-021). Enforcement bypass arrives with F005 (step 2).
    require!(!a.registry.transfers_paused, HookError::TransfersPaused);

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let src = load_holder(&a.source_holder);
    let dst = if same_owner {
        src.clone()
    } else {
        load_holder(&a.destination_holder)
    };

    // 4. Sender approved with valid KYC.
    let mut src = src
        .filter(|h| h.approved(now))
        .ok_or(error!(HookError::SenderNotApproved))?;

    // 5. Locked units stay (F004, research R3): no transfer may leave the sender's balance below
    // its lock, whoever built it. A same-owner transfer is judged on the source account alone
    // (fail closed; an owner has one usable account). The enforcement bypass is F005's step 2.
    require!(
        post_src >= src.pledged_amount,
        HookError::PledgedUnitsLocked
    );

    // 6. Receiver approved, of an allowed class, in an allowed jurisdiction (investors only).
    let registry = &a.registry;
    let mut dst = dst
        .filter(|h| h.approved(now) && h.class_allowed() && h.jurisdiction_ok(registry))
        .ok_or(error!(HookError::ReceiverNotApproved))?;

    // 7. A transfer that would make a new investor holder while the cap is reached.
    let new_holder = !same_owner && dst.is_investor() && dst.last_balance == 0 && amount > 0;
    require!(
        !(new_holder && registry.max_holders > 0 && registry.holder_count >= registry.max_holders),
        HookError::HolderCapReached
    );

    // 8. Settle interest on the balances held until now, before step 9 records the new ones
    // (F002). Only a verified FundState supplies the counter; every check above has passed, so a
    // rejected transfer settles nothing.
    let index = FundStateView::load(&a.fund_state, &a.registry.fund_state)
        .map_err(|_| error!(HookError::WrongMint))?
        .accrual_index;
    if same_owner {
        let total = post_src
            .checked_add(post_dst)
            .ok_or(error!(HookError::Overflow))?;
        settle(&mut src, total, index)?;
    } else {
        let pre_src = post_src
            .checked_add(amount)
            .ok_or(error!(HookError::Overflow))?;
        let pre_dst = post_dst
            .checked_sub(amount)
            .ok_or(error!(HookError::Overflow))?;
        settle(&mut src, pre_src, index)?;
        settle(&mut dst, pre_dst, index)?;
    }

    // 9. Holder count and recorded balances (once when both sides are the same owner).
    let reg = &mut ctx.accounts.registry;
    if same_owner {
        let total = post_src
            .checked_add(post_dst)
            .ok_or(error!(HookError::Overflow))?;
        record_balance(reg, &mut src, total, clock.slot)?;
        store_holder(&ctx.accounts.source_holder, &src)?;
    } else {
        record_balance(reg, &mut src, post_src, clock.slot)?;
        record_balance(reg, &mut dst, post_dst, clock.slot)?;
        store_holder(&ctx.accounts.source_holder, &src)?;
        store_holder(&ctx.accounts.destination_holder, &dst)?;
    }

    // 10. Log (emit!, not emit_cpi!, to save a call level — research R7).
    emit!(TransferValidated {
        fund: reg.fund_state,
        source_owner: src_owner,
        destination_owner: dst_owner,
        amount,
        holder_count: reg.holder_count,
        // The settlement's record (FR-020): both sides' interest after step 8.
        accrual_index: index,
        source_accrued_e18: src.accrued_e18,
        destination_accrued_e18: if same_owner {
            src.accrued_e18
        } else {
            dst.accrued_e18
        },
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
