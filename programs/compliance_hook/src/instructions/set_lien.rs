//! `set_lien(change)` (contracts/compliance_hook.md, F004 research R2): the only way a holder's
//! locked units change. Reached only by CPI from `collateral`, signed by its `["hook-caller"]`
//! PDA. A growing lock must stay covered by the holding account's balance (invariant I2).

use anchor_lang::prelude::*;
use mmf_common::seeds;

use crate::errors::HookError;
use crate::state::{LienChange, Registry};

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

pub fn handle_set_lien(ctx: Context<SetLien>, _change: LienChange) -> Result<()> {
    check_caller(&ctx.accounts.registry, &ctx.accounts.hook_caller)?;
    // Checks 2–5 and the effect: US3 (T044), after their tests.
    Ok(())
}
