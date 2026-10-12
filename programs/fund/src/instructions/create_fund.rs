//! `create_fund` (contracts/fund.md): creates the Fund and its Token-2022 mint together (FR-001 to
//! FR-004). One business transition; creating the mint is part of it because FR-004 requires the
//! token to exist exactly when the Fund does.

use anchor_lang::prelude::*;
use anchor_spl::token_2022::Token2022;
use mmf_common::seeds;

use crate::state::{FundState, FundTerms};

#[event_cpi]
#[derive(Accounts)]
pub struct CreateFund<'info> {
    /// Must equal `role_registry.issuer`; pays rent.
    #[account(mut)]
    pub issuer: Signer<'info>,
    /// New account; becomes the fund mint (created in the handler with all extensions).
    #[account(mut)]
    pub mint: Signer<'info>,
    #[account(
        init,
        payer = issuer,
        space = 8 + FundState::INIT_SPACE,
        seeds = [seeds::FUND, mint.key().as_ref()],
        bump
    )]
    pub fund_state: Account<'info, FundState>,
    /// CHECK: decoded with `RoleRegistryView` (owner governance, address `["roles", fund_state]`).
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: validated in the handler: an initialized SPL Token or Token-2022 mint, 6 decimals.
    pub cash_mint: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

pub fn handle_create_fund(
    _ctx: Context<CreateFund>,
    _terms: FundTerms,
    _name: String,
    _symbol: String,
) -> Result<()> {
    // Implemented in T035.
    Ok(())
}
