//! `set_cash_treasury` (contracts/fund.md, F003 FR-001, research R6): the issuer designates the
//! fund's treasury cash account, once. Subscription cash is paid to it; the transfer agent funds
//! redemption cash from it.

use anchor_lang::prelude::*;
use mmf_common::cash;
use mmf_common::enums::FundStatus;
use mmf_common::views::RoleRegistryView;

use crate::errors::FundError;
use crate::events::CashTreasurySet;
use crate::state::FundState;

#[event_cpi]
#[derive(Accounts)]
pub struct SetCashTreasury<'info> {
    pub issuer: Signer<'info>,
    #[account(mut)]
    pub fund_state: Account<'info, FundState>,
    /// CHECK: decoded with `RoleRegistryView` at `fund_state.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: a token account of `fund_state.terms.cash_mint`; checked in the handler.
    pub cash_treasury: UncheckedAccount<'info>,
    /// CHECK: must be `fund_state.terms.cash_mint`; checked in the handler.
    pub cash_mint: UncheckedAccount<'info>,
}

pub fn handle_set_cash_treasury(ctx: Context<SetCashTreasury>) -> Result<()> {
    let fs = &ctx.accounts.fund_state;

    // 1. The role registry is governance's registry for this fund.
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &fs.role_registry)
        .map_err(|_| error!(FundError::RoleRegistryMismatch))?;

    // 2. Only the issuer.
    require_keys_eq!(
        ctx.accounts.issuer.key(),
        roles.issuer,
        FundError::Unauthorized
    );

    // 3. Setup or Open (funds launched before F003 are already Open).
    require!(
        fs.status == FundStatus::Setup.as_u8() || fs.status == FundStatus::Open.as_u8(),
        FundError::FundNotOpen
    );

    // 4. Once (FR-001).
    require_keys_eq!(
        fs.cash_treasury,
        Pubkey::default(),
        FundError::CashTreasuryAlreadySet
    );

    // 5. A token account of the fund's cash mint, under the cash mint's own token program.
    let cash_mint = &ctx.accounts.cash_mint;
    let treasury = &ctx.accounts.cash_treasury;
    require_keys_eq!(
        cash_mint.key(),
        fs.terms.cash_mint,
        FundError::InvalidCashTreasury
    );
    let account = {
        let data = treasury.try_borrow_data()?;
        cash::token_account(treasury.owner, &data)
    };
    require!(
        treasury.owner == cash_mint.owner && account.is_some_and(|a| a.mint == fs.terms.cash_mint),
        FundError::InvalidCashTreasury
    );

    // 6. No transfer fee or hook on the cash mint (research R14).
    let supported = {
        let data = cash_mint.try_borrow_data()?;
        cash::mint_supported(cash_mint.owner, &data)
    };
    require!(supported, FundError::UnsupportedCashMint);

    let fund = fs.key();
    let cash_treasury = treasury.key();
    ctx.accounts.fund_state.cash_treasury = cash_treasury;
    let clock = Clock::get()?;
    emit_cpi!(CashTreasurySet {
        fund,
        cash_treasury,
        signer: ctx.accounts.issuer.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
