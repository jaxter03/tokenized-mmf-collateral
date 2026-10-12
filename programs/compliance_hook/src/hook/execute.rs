//! The transfer hook `execute` (contracts/compliance_hook.md "Transfer hook", shared-model §6).
//! Token-2022 calls it during `transfer_checked`, after moving the balances, with the five base
//! accounts read-only and then the extra accounts from the `ExtraAccountMetaList`.
//!
//! Steps (F001 builds 0, 1, 3, 4, 6, 7, 9, 10; step 8 arrives with F002, step 5 with F004 and
//! step 2 with F005):
//!  0. both token accounts carry the `transferring` flag      → NotTransferring
//!  1. accounts belong to this fund's mint                      → WrongMint
//!  3. transfers paused                                         → TransfersPaused
//!  4. sender approved with valid KYC                           → SenderNotApproved
//!  6. receiver approved, class allowed, jurisdiction allowed   → ReceiverNotApproved
//!  7. new investor while the cap is reached                    → HolderCapReached
//!  9. holder count and last_balance / last_slot (once if same owner)
//! 10. emit! TransferValidated

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::extension::transfer_hook::TransferHookAccount;
use anchor_spl::token_2022::spl_token_2022::extension::{
    BaseStateWithExtensions, StateWithExtensions,
};
use anchor_spl::token_2022::spl_token_2022::state::Account as TokenAccount;
use mmf_common::seeds;

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
    /// CHECK: extra 2: FundState (checked against `registry.fund_state`).
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
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
