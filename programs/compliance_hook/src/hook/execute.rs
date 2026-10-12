//! The transfer hook `execute` (contracts/compliance_hook.md "Transfer hook", shared-model §6).
//! Token-2022 calls it during `transfer_checked` with the five base accounts read-only, then the
//! extra accounts from the `ExtraAccountMetaList`.

use anchor_lang::prelude::*;

use crate::state::Registry;

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

pub fn handle_execute(_ctx: Context<Execute>, _amount: u64) -> Result<()> {
    // Implemented in T056.
    Ok(())
}
