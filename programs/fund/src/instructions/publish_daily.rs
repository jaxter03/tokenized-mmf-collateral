//! `publish_daily` (contracts/fund.md): the fund admin publishes the day's price and rate for the
//! days it covers, creating the next `NavRecord` and moving the interest counter (F002 FR-001 to
//! FR-011). Clearing a pending override is bookkeeping within this one transition.

use anchor_lang::prelude::*;
use mmf_common::accrual;
use mmf_common::enums::FundStatus;
use mmf_common::seeds;
use mmf_common::views::RoleRegistryView;

use crate::errors::FundError;
use crate::events::NavPublished;
use crate::state::{FundState, NavRecord};

#[event_cpi]
#[derive(Accounts)]
pub struct PublishDaily<'info> {
    pub fund_admin: Signer<'info>,
    /// Pays the NavRecord rent; any key (the fund admin may be a Squads vault).
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut)]
    pub fund_state: Account<'info, FundState>,
    /// CHECK: decoded with `RoleRegistryView` at `fund_state.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// The next record. `wrapping_add` keeps the address derivation from panicking at
    /// `u64::MAX`; the handler's checked add reports `Overflow` instead.
    #[account(
        init,
        payer = payer,
        space = 8 + NavRecord::INIT_SPACE,
        seeds = [seeds::NAV, fund_state.key().as_ref(), &fund_state.nav_seq.wrapping_add(1).to_le_bytes()],
        bump,
    )]
    pub nav_record: Account<'info, NavRecord>,
    pub system_program: Program<'info, System>,
}

pub fn handle_publish_daily(
    ctx: Context<PublishDaily>,
    nav: u64,
    daily_rate_e12: i64,
    days_covered: u16,
) -> Result<()> {
    let fs = &ctx.accounts.fund_state;

    // 1. The role registry is governance's registry for this fund.
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &fs.role_registry)
        .map_err(|_| error!(FundError::RoleRegistryMismatch))?;

    // 2. Only the fund admin (FR-001).
    require_keys_eq!(
        ctx.accounts.fund_admin.key(),
        roles.fund_admin,
        FundError::Unauthorized
    );

    // 3. Only while Open.
    require!(
        fs.status == FundStatus::Open.as_u8(),
        FundError::FundNotOpen
    );

    // 4–6. Inputs (FR-006, FR-009a). The rate is signed so a mis-encoded −1 is rejected rather
    // than read as a huge rate (research R6); no upper cap (FR-009b).
    require!(nav > 0, FundError::ZeroPrice);
    require!(daily_rate_e12 >= 0, FundError::NegativeRate);
    let rate = daily_rate_e12 as u64;
    require!(days_covered >= 1, FundError::ZeroDaysCovered);

    // 7. Coverage is contiguous and can't start in the future (FR-009, research R3).
    let clock = Clock::get()?;
    let today = accrual::utc_day(clock.unix_timestamp).ok_or(error!(FundError::Overflow))?;
    let start = if fs.nav_seq == 0 {
        today
    } else {
        fs.nav_period_end_day
            .checked_add(1)
            .ok_or(error!(FundError::Overflow))?
    };
    require!(start <= today, FundError::PeriodNotStarted);

    // 8. Typo guard: measured from the last price, or the target before the first publish
    // (FR-007). Only an override for exactly this price lets a bigger move through (FR-008).
    let prev = if fs.nav_seq == 0 {
        fs.terms.target_nav
    } else {
        fs.nav
    };
    let within_limit = accrual::price_move_ok(prev, nav, fs.terms.max_nav_move_bps);
    let override_used = !within_limit && fs.pending_override_nav == nav;
    require!(within_limit || override_used, FundError::PriceMoveTooLarge);

    // 9. Everything that must fit its type.
    let period_end = start
        .checked_add(u32::from(days_covered) - 1)
        .ok_or(error!(FundError::Overflow))?;
    let seq = fs
        .nav_seq
        .checked_add(1)
        .ok_or(error!(FundError::Overflow))?;
    let index_after = accrual::index_increment(rate, days_covered)
        .and_then(|inc| fs.accrual_index.checked_add(inc))
        .ok_or(error!(FundError::Overflow))?;

    let fund = fs.key();
    let override_cleared_nav = fs.pending_override_nav;
    let publisher = ctx.accounts.fund_admin.key();

    ctx.accounts.nav_record.set_inner(NavRecord {
        version: NavRecord::VERSION,
        seq,
        nav,
        daily_rate_e12: rate,
        days_covered,
        period_start_day: start,
        index_after,
        as_of_ts: clock.unix_timestamp,
        published_by: publisher,
        override_used,
        bump: ctx.bumps.nav_record,
    });

    // No HolderState is read or written: holders settle lazily in the hook (FR-010).
    let fs = &mut ctx.accounts.fund_state;
    fs.nav = nav;
    fs.nav_ts = clock.unix_timestamp;
    fs.nav_seq = seq;
    fs.daily_rate_e12 = rate;
    fs.accrual_index = index_after;
    fs.nav_period_end_day = period_end;
    // The next publish always clears a pending override, used or not (research R5).
    fs.pending_override_nav = 0;

    emit_cpi!(NavPublished {
        fund,
        seq,
        nav,
        daily_rate_e12: rate,
        days_covered,
        period_start_day: start,
        index_after,
        publisher,
        override_used,
        override_cleared_nav,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
