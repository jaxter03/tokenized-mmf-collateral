//! `mmf_demo`: DEMO ONLY. A tokenized money market fund whose shares an investor pledges to a
//! bank to meet a margin call raised elsewhere (e.g. on a loan backed by Tesla stock), and which
//! the bank releases once that call is cured. Interest accrues on every share the investor owns,
//! pledged or not.
//!
//! This is a deliberately small stand-in for the real programs in `programs/` (no transfer hook,
//! no governance, plain SPL Token, a fixed $1.00 NAV). Do not deploy it anywhere that matters.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Token, TokenAccount, Transfer, CloseAccount};

declare_id!("6eNynvrVyhyWN6d4dTDiRWps42YPhdHpBM56iXefBkEj");

/// Shares and cash both use 6 decimals; NAV is in micro-dollars per share.
pub const DECIMALS: u8 = 6;
pub const ONE: u64 = 1_000_000;
pub const SECONDS_PER_YEAR: u128 = 365 * 24 * 60 * 60;
pub const MAX_NAME_LEN: usize = 32;
pub const MAX_REF_LEN: usize = 48;
/// Demo only: how far back a first subscription may date its interest accrual.
pub const MAX_BACKDATE_SECS: i64 = 90 * 24 * 60 * 60;

#[program]
pub mod mmf_demo {
    use super::*;

    /// The issuer launches the fund: its share mint, cash treasury and annual rate.
    pub fn launch_fund(ctx: Context<LaunchFund>, name: String, rate_bps: u16) -> Result<()> {
        require!(name.len() <= MAX_NAME_LEN, DemoError::NameTooLong);
        require!(rate_bps > 0 && rate_bps <= 2_000, DemoError::BadRate);
        let fund = &mut ctx.accounts.fund;
        fund.issuer = ctx.accounts.issuer.key();
        fund.share_mint = ctx.accounts.share_mint.key();
        fund.cash_mint = ctx.accounts.cash_mint.key();
        fund.treasury = ctx.accounts.treasury.key();
        fund.rate_bps = rate_bps;
        fund.nav = ONE;
        fund.name = name;
        fund.bump = ctx.bumps.fund;
        msg!("Fund launched: {} at {} bps", fund.name, rate_bps);
        Ok(())
    }

    /// The issuer approves an investor (KYC) and opens their position.
    pub fn onboard_investor(ctx: Context<OnboardInvestor>) -> Result<()> {
        let position = &mut ctx.accounts.position;
        position.fund = ctx.accounts.fund.key();
        position.investor = ctx.accounts.investor.key();
        position.kyc_approved = true;
        position.last_accrual_ts = Clock::get()?.unix_timestamp;
        position.bump = ctx.bumps.position;
        msg!("Investor approved: {}", position.investor);
        Ok(())
    }

    /// The investor pays cash into the treasury and receives shares at NAV.
    /// `backdate_secs` (demo only) starts a first position's accrual in the past so the demo
    /// opens with interest already earned.
    pub fn subscribe(ctx: Context<Subscribe>, cash_amount: u64, backdate_secs: i64) -> Result<()> {
        require!(cash_amount > 0, DemoError::ZeroAmount);
        require!(
            (0..=MAX_BACKDATE_SECS).contains(&backdate_secs),
            DemoError::BadBackdate
        );
        let fund = &ctx.accounts.fund;
        let position = &mut ctx.accounts.position;
        require!(position.kyc_approved, DemoError::NotApproved);

        let now = Clock::get()?.unix_timestamp;
        if position.shares == 0 {
            position.last_accrual_ts = now - backdate_secs;
        } else {
            position.accrue(fund.rate_bps, now);
        }

        let shares = (cash_amount as u128 * ONE as u128 / fund.nav as u128) as u64;
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                Transfer {
                    from: ctx.accounts.investor_cash.to_account_info(),
                    to: ctx.accounts.treasury.to_account_info(),
                    authority: ctx.accounts.investor.to_account_info(),
                },
            ),
            cash_amount,
        )?;
        let issuer = fund.issuer;
        let signer: &[&[&[u8]]] = &[&[b"fund", issuer.as_ref(), &[fund.bump]]];
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                MintTo {
                    mint: ctx.accounts.share_mint.to_account_info(),
                    to: ctx.accounts.investor_shares.to_account_info(),
                    authority: ctx.accounts.fund.to_account_info(),
                },
                signer,
            ),
            shares,
        )?;
        position.shares = position.shares.checked_add(shares).ok_or(DemoError::Overflow)?;
        msg!("Subscribed: {} cash for {} shares", cash_amount, shares);
        Ok(())
    }

    /// A bank records a margin call it raised off-platform against `borrower`, so the pledge
    /// that meets it has an on-chain reference.
    pub fn record_margin_call(
        ctx: Context<RecordMarginCall>,
        call_id: u64,
        shortfall: u64,
        reference: String,
    ) -> Result<()> {
        require!(shortfall > 0, DemoError::ZeroAmount);
        require!(reference.len() <= MAX_REF_LEN, DemoError::ReferenceTooLong);
        let call = &mut ctx.accounts.call;
        call.fund = ctx.accounts.fund.key();
        call.bank = ctx.accounts.bank.key();
        call.borrower = ctx.accounts.borrower.key();
        call.call_id = call_id;
        call.shortfall = shortfall;
        call.reference = reference;
        call.status = CallStatus::Open;
        call.issued_ts = Clock::get()?.unix_timestamp;
        call.bump = ctx.bumps.call;
        msg!("Margin call {} recorded: shortfall {}", call.reference, shortfall);
        Ok(())
    }

    /// The borrower pledges shares into a vault controlled by the call. The shares stay in their
    /// position, so they keep earning interest.
    pub fn pledge(ctx: Context<MakePledge>, shares: u64) -> Result<()> {
        let call = &mut ctx.accounts.call;
        require!(call.status == CallStatus::Open, DemoError::CallNotOpen);
        let value = (shares as u128 * ctx.accounts.fund.nav as u128 / ONE as u128) as u64;
        require!(value >= call.shortfall, DemoError::PledgeTooSmall);

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                Transfer {
                    from: ctx.accounts.borrower_shares.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.borrower.to_account_info(),
                },
            ),
            shares,
        )?;
        call.status = CallStatus::Pledged;
        call.pledged_shares = shares;
        call.pledged_ts = Clock::get()?.unix_timestamp;
        msg!("Pledged {} shares against {}", shares, call.reference);
        Ok(())
    }

    /// The bank releases the pledge once the call is cured; shares go back to the borrower.
    pub fn release(ctx: Context<Release>) -> Result<()> {
        let call = &ctx.accounts.call;
        require!(call.status == CallStatus::Pledged, DemoError::NotPledged);

        let fund_key = call.fund;
        let borrower_key = call.borrower;
        let id_bytes = call.call_id.to_le_bytes();
        let signer: &[&[&[u8]]] = &[&[
            b"call",
            fund_key.as_ref(),
            borrower_key.as_ref(),
            &id_bytes,
            &[call.bump],
        ]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.borrower_shares.to_account_info(),
                    authority: ctx.accounts.call.to_account_info(),
                },
                signer,
            ),
            ctx.accounts.vault.amount,
        )?;
        token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            CloseAccount {
                account: ctx.accounts.vault.to_account_info(),
                destination: ctx.accounts.borrower.to_account_info(),
                authority: ctx.accounts.call.to_account_info(),
            },
            signer,
        ))?;
        let call = &mut ctx.accounts.call;
        call.status = CallStatus::Released;
        call.released_ts = Clock::get()?.unix_timestamp;
        msg!("Released pledge for {}", call.reference);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// State

#[account]
#[derive(InitSpace)]
pub struct Fund {
    pub issuer: Pubkey,
    pub share_mint: Pubkey,
    pub cash_mint: Pubkey,
    pub treasury: Pubkey,
    pub rate_bps: u16,
    /// Micro-dollars per share (fixed at $1.00 in the demo).
    pub nav: u64,
    #[max_len(MAX_NAME_LEN)]
    pub name: String,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Position {
    pub fund: Pubkey,
    pub investor: Pubkey,
    pub kyc_approved: bool,
    /// Every share the investor owns, including any pledged.
    pub shares: u64,
    /// Interest settled up to `last_accrual_ts`, in micro-dollars.
    pub accrued: u64,
    pub last_accrual_ts: i64,
    pub bump: u8,
}

impl Position {
    fn accrue(&mut self, rate_bps: u16, now: i64) {
        let elapsed = now.saturating_sub(self.last_accrual_ts);
        if elapsed > 0 {
            let earned = self.shares as u128 * rate_bps as u128 * elapsed as u128
                / (10_000 * SECONDS_PER_YEAR);
            self.accrued = self.accrued.saturating_add(earned as u64);
            self.last_accrual_ts = now;
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum CallStatus {
    Open,
    Pledged,
    Released,
}

#[account]
#[derive(InitSpace)]
pub struct MarginCall {
    pub fund: Pubkey,
    pub bank: Pubkey,
    pub borrower: Pubkey,
    pub call_id: u64,
    /// Micro-dollars the bank asked for.
    pub shortfall: u64,
    #[max_len(MAX_REF_LEN)]
    pub reference: String,
    pub status: CallStatus,
    pub pledged_shares: u64,
    pub issued_ts: i64,
    pub pledged_ts: i64,
    pub released_ts: i64,
    pub bump: u8,
}

// ---------------------------------------------------------------------------------------------
// Accounts

#[derive(Accounts)]
pub struct LaunchFund<'info> {
    #[account(mut)]
    pub issuer: Signer<'info>,
    #[account(
        init,
        payer = issuer,
        space = 8 + Fund::INIT_SPACE,
        seeds = [b"fund", issuer.key().as_ref()],
        bump
    )]
    pub fund: Account<'info, Fund>,
    #[account(
        init,
        payer = issuer,
        seeds = [b"shares", fund.key().as_ref()],
        bump,
        mint::decimals = DECIMALS,
        mint::authority = fund,
    )]
    pub share_mint: Account<'info, Mint>,
    #[account(mint::decimals = DECIMALS)]
    pub cash_mint: Account<'info, Mint>,
    #[account(
        init,
        payer = issuer,
        seeds = [b"treasury", fund.key().as_ref()],
        bump,
        token::mint = cash_mint,
        token::authority = fund,
    )]
    pub treasury: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OnboardInvestor<'info> {
    #[account(mut)]
    pub issuer: Signer<'info>,
    #[account(has_one = issuer @ DemoError::Unauthorized)]
    pub fund: Account<'info, Fund>,
    /// CHECK: any wallet; this instruction is what approves it.
    pub investor: UncheckedAccount<'info>,
    #[account(
        init,
        payer = issuer,
        space = 8 + Position::INIT_SPACE,
        seeds = [b"position", fund.key().as_ref(), investor.key().as_ref()],
        bump
    )]
    pub position: Account<'info, Position>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Subscribe<'info> {
    pub investor: Signer<'info>,
    #[account(has_one = share_mint, has_one = treasury)]
    pub fund: Account<'info, Fund>,
    #[account(
        mut,
        seeds = [b"position", fund.key().as_ref(), investor.key().as_ref()],
        bump = position.bump
    )]
    pub position: Account<'info, Position>,
    #[account(mut, token::mint = fund.cash_mint, token::authority = investor)]
    pub investor_cash: Account<'info, TokenAccount>,
    #[account(mut)]
    pub treasury: Account<'info, TokenAccount>,
    #[account(mut)]
    pub share_mint: Account<'info, Mint>,
    #[account(mut, token::mint = share_mint, token::authority = investor)]
    pub investor_shares: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(call_id: u64)]
pub struct RecordMarginCall<'info> {
    #[account(mut)]
    pub bank: Signer<'info>,
    pub fund: Account<'info, Fund>,
    /// CHECK: the borrower the call is against.
    pub borrower: UncheckedAccount<'info>,
    #[account(
        init,
        payer = bank,
        space = 8 + MarginCall::INIT_SPACE,
        seeds = [b"call", fund.key().as_ref(), borrower.key().as_ref(), &call_id.to_le_bytes()],
        bump
    )]
    pub call: Account<'info, MarginCall>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MakePledge<'info> {
    #[account(mut)]
    pub borrower: Signer<'info>,
    pub fund: Account<'info, Fund>,
    #[account(
        mut,
        has_one = fund,
        has_one = borrower @ DemoError::Unauthorized,
    )]
    pub call: Account<'info, MarginCall>,
    #[account(address = fund.share_mint)]
    pub share_mint: Account<'info, Mint>,
    #[account(mut, token::mint = share_mint, token::authority = borrower)]
    pub borrower_shares: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = borrower,
        seeds = [b"vault", call.key().as_ref()],
        bump,
        token::mint = share_mint,
        token::authority = call,
    )]
    pub vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Release<'info> {
    pub bank: Signer<'info>,
    #[account(
        mut,
        has_one = bank @ DemoError::Unauthorized,
        has_one = borrower,
    )]
    pub call: Account<'info, MarginCall>,
    /// CHECK: receives the vault's rent; must be the call's borrower.
    #[account(mut)]
    pub borrower: UncheckedAccount<'info>,
    #[account(mut, token::authority = borrower)]
    pub borrower_shares: Account<'info, TokenAccount>,
    #[account(mut, seeds = [b"vault", call.key().as_ref()], bump)]
    pub vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[error_code]
pub enum DemoError {
    #[msg("Fund name is too long")]
    NameTooLong,
    #[msg("Rate must be between 1 and 2000 bps")]
    BadRate,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Backdate must be between 0 and 90 days")]
    BadBackdate,
    #[msg("Investor is not KYC approved")]
    NotApproved,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Reference is too long")]
    ReferenceTooLong,
    #[msg("Margin call is not open")]
    CallNotOpen,
    #[msg("Pledge is worth less than the shortfall")]
    PledgeTooSmall,
    #[msg("Nothing is pledged against this call")]
    NotPledged,
    #[msg("Signer is not allowed to do this")]
    Unauthorized,
}
