//! `launch` (contracts/fund.md): the issuer moves the Fund from Setup to Open (FR-005, FR-006).

use anchor_lang::prelude::*;

use crate::state::FundState;

#[event_cpi]
#[derive(Accounts)]
pub struct Launch<'info> {
    pub issuer: Signer<'info>,
    #[account(mut)]
    pub fund_state: Account<'info, FundState>,
    /// CHECK: decoded with `RoleRegistryView` at `fund_state.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: decoded with `RegistryView` at PDA `["registry", mint]` of compliance_hook.
    pub registry: UncheckedAccount<'info>,
    /// CHECK: must be owned by compliance_hook at PDA `["extra-account-metas", mint]` and hold data.
    pub extra_account_meta_list: UncheckedAccount<'info>,
}

pub fn handle_launch(_ctx: Context<Launch>) -> Result<()> {
    // Implemented in T036.
    Ok(())
}
