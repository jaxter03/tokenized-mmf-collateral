//! `set_lien(change)` (contracts/compliance_hook.md, F004 research R2): the only way a holder's
//! locked units change. Reached only by CPI from `collateral`, signed by its `["hook-caller"]`
//! PDA. A growing lock must stay covered by the holding account's balance (invariant I2).

use anchor_lang::prelude::*;
use mmf_common::seeds;

use crate::errors::HookError;
use crate::events::LienSet;
use crate::instructions::accrue::owner_accounts;
use crate::state::{HolderState, LienChange, Registry};

#[derive(Accounts)]
pub struct SetLien<'info> {
    /// CHECK: must be `["hook-caller"]` of `registry.collateral_program` and must sign; checked
    /// by hand so a wrong or unsigned caller reports `NotCollateralCaller`.
    pub hook_caller: UncheckedAccount<'info>,
    pub registry: Account<'info, Registry>,
    /// CHECK: the owner's HolderState; loaded and stored by hand (`WrongHolder`).
    #[account(mut)]
    pub holder_state: UncheckedAccount<'info>,
    /// CHECK: the owner's Token-2022 holding account (its ATA); its balance bounds the lock.
    pub holding_account: UncheckedAccount<'info>,
}

/// Check 1: `registry.collateral_program` is set, and `hook_caller` is its `["hook-caller"]` PDA
/// and signed. A registry created before F004 has no collateral program, so it refuses every
/// caller (research R16).
fn check_caller(registry: &Registry, hook_caller: &AccountInfo) -> Result<()> {
    require!(
        registry.collateral_program != Pubkey::default(),
        HookError::NotCollateralCaller
    );
    let expected =
        Pubkey::find_program_address(&[seeds::HOOK_CALLER], &registry.collateral_program).0;
    require!(
        hook_caller.key() == expected && hook_caller.is_signer,
        HookError::NotCollateralCaller
    );
    Ok(())
}

/// Checks 3 and 5: the new lock and lien count, without underflow or overflow.
pub fn apply(holder: &HolderState, change: &LienChange) -> Result<(u64, u16)> {
    let locked = u128::from(holder.pledged_amount)
        .checked_add(u128::from(change.lock))
        .ok_or(error!(HookError::Overflow))?
        .checked_sub(u128::from(change.unlock))
        .ok_or(error!(HookError::LienUnderflow))?;
    let locked = u64::try_from(locked).map_err(|_| error!(HookError::Overflow))?;
    let liens = u32::from(holder.active_liens)
        .checked_add(u32::from(change.opened))
        .ok_or(error!(HookError::Overflow))?
        .checked_sub(u32::from(change.closed))
        .ok_or(error!(HookError::LienUnderflow))?;
    let liens = u16::try_from(liens).map_err(|_| error!(HookError::Overflow))?;
    Ok((locked, liens))
}

pub fn handle_set_lien(ctx: Context<SetLien>, change: LienChange) -> Result<()> {
    let a = &ctx.accounts;
    // 1. Only collateral's `["hook-caller"]`.
    check_caller(&a.registry, &a.hook_caller)?;
    // 2. The owner's record and holding account, for this fund's mint.
    let (mut holder, balance) = owner_accounts(&a.registry.mint, &a.holder_state, &a.holding_account)?;
    // 3, 5. The new totals.
    let (locked, liens) = apply(&holder, &change)?;
    // 4. A growing lock stays covered by the balance (I2).
    if change.lock > change.unlock {
        require!(balance >= locked, HookError::InsufficientUnlockedUnits);
    }

    holder.pledged_amount = locked;
    holder.active_liens = liens;
    {
        let mut data = a.holder_state.try_borrow_mut_data()?;
        holder.try_serialize(&mut &mut data[..])?;
    }
    let clock = Clock::get()?;
    // emit!, not emit_cpi!, as `accrue` does: reached only by CPI.
    emit!(LienSet {
        fund: a.registry.fund_state,
        owner: holder.owner,
        lock: change.lock,
        unlock: change.unlock,
        opened: change.opened,
        closed: change.closed,
        pledged_amount: locked,
        active_liens: liens,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holder(locked: u64, liens: u16) -> HolderState {
        HolderState {
            version: 1,
            mint: Pubkey::default(),
            owner: Pubkey::default(),
            status: 1,
            class: 0,
            jurisdiction: *b"US",
            investor_type: 1,
            kyc_expiry_ts: i64::MAX,
            attestation_hash: [0; 32],
            attested_by: Pubkey::default(),
            pledged_amount: locked,
            active_liens: liens,
            index_at_last_touch: 0,
            accrued_e18: 0,
            last_balance: 0,
            last_slot: 0,
            last_paid_cycle: 0,
            bump: 0,
            _reserved: [0; 64],
        }
    }

    fn change(lock: u64, unlock: u64, opened: u16, closed: u16) -> LienChange {
        LienChange { lock, unlock, opened, closed }
    }

    #[test]
    fn totals() {
        assert_eq!(apply(&holder(10, 1), &change(5, 0, 1, 0)).unwrap(), (15, 2));
        assert_eq!(apply(&holder(10, 2), &change(0, 10, 0, 1)).unwrap(), (0, 1));
        // A substitution: lock 7, unlock 4, one lien opened and one closed.
        assert_eq!(apply(&holder(4, 1), &change(7, 4, 1, 1)).unwrap(), (7, 1));
    }

    #[test]
    fn underflow_and_overflow() {
        assert_eq!(apply(&holder(3, 1), &change(0, 4, 0, 0)).unwrap_err(), error!(HookError::LienUnderflow));
        assert_eq!(apply(&holder(3, 0), &change(0, 0, 0, 1)).unwrap_err(), error!(HookError::LienUnderflow));
        assert_eq!(apply(&holder(u64::MAX, 0), &change(1, 0, 0, 0)).unwrap_err(), error!(HookError::Overflow));
        assert_eq!(apply(&holder(0, u16::MAX), &change(0, 0, 1, 0)).unwrap_err(), error!(HookError::Overflow));
    }
}
