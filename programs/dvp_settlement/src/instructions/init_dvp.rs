//! `init_dvp` (contracts/dvp_settlement.md, FR-001, FR-002): the issuer sets up settlement for a
//! fund: the config with its default expiry, and the escrow's unit and cash accounts.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::{self, AssociatedToken, Create};
use mmf_common::cash;
use mmf_common::enums::FundStatus;
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_common::seeds;
use mmf_common::settlement::DEFAULT_EXPIRY_SECS;
use mmf_common::views::{FundStateView, RoleRegistryView};

use crate::errors::DvpError;
use crate::events::DvpInitialized;
use crate::state::DvpConfig;

#[event_cpi]
#[derive(Accounts)]
pub struct InitDvp<'info> {
    pub issuer: Signer<'info>,
    /// Pays the config and escrow-account rent; any key (the issuer may be a Squads vault).
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView` (owner and discriminator checked).
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at the fund's `role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: must be the fund's mint; checked in the handler.
    pub mint: UncheckedAccount<'info>,
    /// CHECK: must be the fund's `terms.cash_mint`; checked in the handler.
    pub cash_mint: UncheckedAccount<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + DvpConfig::INIT_SPACE,
        seeds = [seeds::DVP, fund_state.key().as_ref()],
        bump,
    )]
    pub dvp_config: Account<'info, DvpConfig>,
    /// CHECK: PDA `["escrow", fund_state]`; owns both escrow accounts, holds no data.
    #[account(seeds = [seeds::ESCROW, fund_state.key().as_ref()], bump)]
    pub escrow_authority: UncheckedAccount<'info>,
    /// CHECK: the escrow authority's Token-2022 ATA for `mint`; created by the ATA program.
    #[account(mut)]
    pub unit_escrow: UncheckedAccount<'info>,
    /// CHECK: the escrow authority's ATA for `cash_mint`; created by the ATA program.
    #[account(mut)]
    pub cash_escrow: UncheckedAccount<'info>,
    /// CHECK: must be the Token-2022 program.
    pub token_2022_program: UncheckedAccount<'info>,
    /// CHECK: must be `cash_mint`'s owner (SPL Token or Token-2022).
    pub cash_token_program: UncheckedAccount<'info>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_init_dvp(ctx: Context<InitDvp>, default_expiry_secs: Option<u32>) -> Result<()> {
    let a = &ctx.accounts;
    let fund_key = a.fund_state.key();

    // 1. FundState, its RoleRegistry, mint and cash mint all belong to one fund.
    let fund = FundStateView::load(&a.fund_state, &fund_key)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    let roles = RoleRegistryView::load(&a.role_registry, &fund.role_registry)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    require_keys_eq!(roles.fund_state, fund_key, DvpError::FundMismatch);
    require_keys_eq!(a.mint.key(), fund.mint, DvpError::FundMismatch);
    require_keys_eq!(a.cash_mint.key(), fund.cash_mint, DvpError::FundMismatch);
    require_keys_eq!(
        a.token_2022_program.key(),
        TOKEN_2022_PROGRAM_ID,
        DvpError::WrongAccount
    );
    require_keys_eq!(
        a.cash_token_program.key(),
        *a.cash_mint.owner,
        DvpError::WrongAccount
    );

    // 2. Only the issuer.
    require_keys_eq!(a.issuer.key(), roles.issuer, DvpError::Unauthorized);

    // 3. Setup or Open.
    require!(
        fund.status == FundStatus::Setup.as_u8() || fund.status == FundStatus::Open.as_u8(),
        DvpError::FundNotOpen
    );

    // 4. The treasury cash account is designated first (`fund::set_cash_treasury`).
    require!(
        fund.cash_treasury != Pubkey::default(),
        DvpError::CashTreasuryNotSet
    );

    // 5. No transfer fee or hook on the cash mint (research R14).
    let supported = {
        let data = a.cash_mint.try_borrow_data()?;
        cash::mint_supported(a.cash_mint.owner, &data)
    };
    require!(supported, DvpError::UnsupportedCashMint);

    // 6. Default expiry: 24 h unless given; never zero (research R20).
    let default_expiry_secs = match default_expiry_secs {
        None => DEFAULT_EXPIRY_SECS,
        Some(0) => return err!(DvpError::InvalidExpiry),
        Some(secs) => secs,
    };

    // Both escrow accounts. Idempotent: anyone may create an ATA for any owner, so an account
    // created in advance must not block setup. The ATA program checks each address.
    for (escrow_account, mint, token_program) in [
        (&a.unit_escrow, &a.mint, &a.token_2022_program),
        (&a.cash_escrow, &a.cash_mint, &a.cash_token_program),
    ] {
        associated_token::create_idempotent(CpiContext::new(
            a.associated_token_program.key(),
            Create {
                payer: a.payer.to_account_info(),
                associated_token: escrow_account.to_account_info(),
                authority: a.escrow_authority.to_account_info(),
                mint: mint.to_account_info(),
                system_program: a.system_program.to_account_info(),
                token_program: token_program.to_account_info(),
            },
        ))?;
    }

    let escrow_authority = a.escrow_authority.key();
    let unit_escrow = a.unit_escrow.key();
    let cash_escrow = a.cash_escrow.key();
    let signer = a.issuer.key();
    let cfg = &mut ctx.accounts.dvp_config;
    cfg.version = DvpConfig::VERSION;
    cfg.fund_state = fund_key;
    cfg.next_seq = 1;
    cfg.default_expiry_secs = default_expiry_secs;
    cfg.bump = ctx.bumps.dvp_config;
    cfg.escrow_bump = ctx.bumps.escrow_authority;

    let clock = Clock::get()?;
    emit_cpi!(DvpInitialized {
        fund: fund_key,
        default_expiry_secs,
        escrow_authority,
        unit_escrow,
        cash_escrow,
        signer,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
