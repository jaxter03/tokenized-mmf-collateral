//! `check_coverage()` (contracts/collateral.md, FR-019): anyone re-values an agreement at a fresh
//! price, which only observes coverage. Permissionless and state-checked (shared-model §5.1).

use anchor_lang::prelude::*;

use crate::errors::CollateralError;
use crate::events::CoverageChecked;
use crate::state::PledgeAgreement;
use crate::valuation::{fresh, load_fund, observe, snapshot};

#[event_cpi]
#[derive(Accounts)]
pub struct CheckCoverage<'info> {
    /// Any key; recorded in the event.
    pub triggered_by: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
}

pub fn handle_check_coverage(ctx: Context<CheckCoverage>) -> Result<()> {
    // 1–3. The agreement's FundState, Active, a fresh price.
    let fund = load_fund(&ctx.accounts.fund_state, &ctx.accounts.agreement)?;
    require!(
        ctx.accounts.agreement.is_active(),
        CollateralError::AgreementNotActive
    );
    let clock = Clock::get()?;
    fresh(&fund, &ctx.accounts.agreement, clock.unix_timestamp)?;
    // Observe only: the outcome depends on state and time, never on who asked.
    let a = &mut ctx.accounts.agreement;
    observe(a, &fund)?;
    let snap = snapshot(a, &fund, true)?;
    emit_cpi!(CoverageChecked {
        fund: a.fund_state,
        agreement: a.key(),
        triggered_by: ctx.accounts.triggered_by.key(),
        snapshot: snap,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
