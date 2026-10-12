//! `create_fund` (contracts/fund.md): creates the Fund and its Token-2022 mint together (FR-001 to
//! FR-004). One business transition; creating the mint is part of it because FR-004 requires the
//! token to exist exactly when the Fund does.

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::extension::StateWithExtensions;
use anchor_spl::token_2022::spl_token_2022::state::Mint;
use anchor_spl::token_2022::Token2022;
use mmf_common::enums::FundStatus;
use mmf_common::ids::{GOVERNANCE_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use mmf_common::seeds;
use mmf_common::views::RoleRegistryView;

use crate::errors::FundError;
use crate::events::FundCreated;
use crate::state::{FundState, FundTerms};
use crate::token_setup::{self, MintMetadata, MintSetup};

/// Cash leg decimals (docs/shared-model.md §1).
pub const CASH_DECIMALS: u8 = 6;
pub const MAX_NAME_LEN: usize = 32;
pub const MAX_SYMBOL_LEN: usize = 10;
pub const MAX_REDEMPTION_GATE_BPS: u16 = 10_000;

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
    ctx: Context<CreateFund>,
    terms: FundTerms,
    name: String,
    symbol: String,
) -> Result<()> {
    let fund_key = ctx.accounts.fund_state.key();

    // 1. The role registry is governance's registry for exactly this fund.
    let expected_roles =
        Pubkey::find_program_address(&[seeds::ROLES, fund_key.as_ref()], &GOVERNANCE_PROGRAM_ID).0;
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &expected_roles)
        .map_err(|_| error!(FundError::RoleRegistryMismatch))?;
    require_keys_eq!(roles.fund_state, fund_key, FundError::RoleRegistryMismatch);

    // 2. Only the issuer named in it may create the fund (FR-002).
    require_keys_eq!(
        ctx.accounts.issuer.key(),
        roles.issuer,
        FundError::Unauthorized
    );

    // 3. Terms (FR-003), in contract order.
    let uri = validate_terms(&terms, &ctx.accounts.cash_mint, &name, &symbol)?;

    // Mint with every extension (FR-004).
    token_setup::create_fund_mint(
        MintSetup {
            payer: &ctx.accounts.issuer.to_account_info(),
            mint: &ctx.accounts.mint.to_account_info(),
            fund_state: &ctx.accounts.fund_state.to_account_info(),
            fund_state_bump: ctx.bumps.fund_state,
            token_program: &ctx.accounts.token_2022_program.to_account_info(),
            system_program: &ctx.accounts.system_program.to_account_info(),
        },
        MintMetadata {
            name,
            symbol,
            uri,
            fund_id_hex: token_setup::hex(&terms.fund_id),
            terms_hash_hex: token_setup::hex(&terms.terms_hash),
        },
    )?;

    let fs = &mut ctx.accounts.fund_state;
    fs.version = FundState::VERSION;
    fs.mint = ctx.accounts.mint.key();
    fs.role_registry = expected_roles;
    fs.terms = terms;
    fs.status = FundStatus::Setup.as_u8();
    fs.bump = ctx.bumps.fund_state;

    let clock = Clock::get()?;
    emit_cpi!(FundCreated {
        fund: fund_key,
        mint: ctx.accounts.mint.key(),
        issuer: roles.issuer,
        terms,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}

/// FR-003 checks in contract order. Returns the documents link as a string for the metadata.
fn validate_terms(
    terms: &FundTerms,
    cash_mint: &AccountInfo,
    name: &str,
    symbol: &str,
) -> Result<String> {
    require!(terms.fund_id != [0; 12], FundError::InvalidFundId);
    require!(is_cash_mint(terms, cash_mint), FundError::InvalidCashMint);
    require!(terms.target_nav > 0, FundError::ZeroTargetNav);
    require!(terms.max_nav_move_bps > 0, FundError::ZeroMaxNavMove);
    require!(
        (1..=MAX_REDEMPTION_GATE_BPS).contains(&terms.redemption_gate_bps),
        FundError::InvalidRedemptionGate
    );
    require!(
        terms.distribution_mode <= 1,
        FundError::InvalidDistributionMode
    );
    require!(
        terms.distribution_cadence_days > 0,
        FundError::ZeroDistributionCadence
    );
    require!(terms.terms_hash != [0; 32], FundError::MissingTermsHash);
    let uri = terms_uri(&terms.terms_uri).ok_or(error!(FundError::MissingTermsUri))?;
    require!(
        (1..=MAX_NAME_LEN).contains(&name.len()),
        FundError::InvalidName
    );
    require!(
        (1..=MAX_SYMBOL_LEN).contains(&symbol.len()),
        FundError::InvalidSymbol
    );
    Ok(uri)
}

/// The cash asset is the account named in the terms, an initialized SPL Token or Token-2022 mint
/// with 6 decimals.
fn is_cash_mint(terms: &FundTerms, cash_mint: &AccountInfo) -> bool {
    if cash_mint.key() != terms.cash_mint {
        return false;
    }
    if cash_mint.owner != &SPL_TOKEN_PROGRAM_ID && cash_mint.owner != &TOKEN_2022_PROGRAM_ID {
        return false;
    }
    let Ok(data) = cash_mint.try_borrow_data() else {
        return false;
    };
    // Only the base mint is read, so unknown extensions on a Token-2022 cash mint don't matter.
    match StateWithExtensions::<Mint>::unpack(&data) {
        Ok(state) => state.base.is_initialized && state.base.decimals == CASH_DECIMALS,
        Err(_) => false,
    }
}

/// "at least one non-zero byte, zero-padded, valid UTF-8".
fn terms_uri(raw: &[u8; 128]) -> Option<String> {
    let len = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    if len == 0 || raw[len..].iter().any(|b| *b != 0) {
        return None;
    }
    core::str::from_utf8(&raw[..len]).ok().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::terms_uri;

    fn uri(bytes: &[u8]) -> [u8; 128] {
        let mut a = [0u8; 128];
        a[..bytes.len()].copy_from_slice(bytes);
        a
    }

    #[test]
    fn terms_uri_rules() {
        assert_eq!(terms_uri(&uri(b"https://x")), Some("https://x".into()));
        assert_eq!(terms_uri(&[0; 128]), None, "empty");
        assert_eq!(terms_uri(&uri(&[0xFF, 0xFE])), None, "not UTF-8");
        let mut gap = uri(b"ab");
        gap[5] = b'c';
        assert_eq!(terms_uri(&gap), None, "not zero-padded");
        assert_eq!(
            terms_uri(&[b'a'; 128]).map(|s| s.len()),
            Some(128),
            "full length"
        );
    }
}
