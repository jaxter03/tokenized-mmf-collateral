//! `pledge(units, pledge_ref)` (contracts/collateral.md, FR-021, FR-022): the giver locks units
//! for the taker. No units move; the hook's `set_lien` locks them against the real balance.

use anchor_lang::prelude::*;
use compliance_hook::program::ComplianceHook;
use compliance_hook::LienChange;
use mmf_common::enums::{PledgeMode, PledgeStatus};
use mmf_common::seeds;

use crate::errors::CollateralError;
use crate::events::Pledged;
use crate::lien::{self, LienAccounts};
use crate::state::{Pledge, PledgeAgreement};
use crate::valuation::{is_fresh, load_fund, load_holder, observe, require_eligible, snapshot};

#[event_cpi]
#[derive(Accounts)]
#[instruction(units: u64, pledge_ref: [u8; 32])]
pub struct MakePledge<'info> {
    /// Must be the agreement's giver.
    pub giver: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    #[account(
        init,
        payer = payer,
        space = 8 + Pledge::INIT_SPACE,
        seeds = [seeds::PLEDGE, agreement.key().as_ref(), &pledge_ref],
        bump
    )]
    pub pledge: Account<'info, Pledge>,
    /// CHECK: the fund's Registry; `set_lien` checks it.
    pub registry: UncheckedAccount<'info>,
    /// CHECK: the giver's HolderState, read here (`HolderStateView`) and written by `set_lien`.
    #[account(mut)]
    pub giver_holder: UncheckedAccount<'info>,
    /// CHECK: the giver's holding account; `set_lien` checks it and its balance.
    pub giver_holding: UncheckedAccount<'info>,
    /// CHECK: this program's `["hook-caller"]` PDA; signs the lien CPI.
    #[account(seeds = [seeds::HOOK_CALLER], bump)]
    pub hook_caller: UncheckedAccount<'info>,
    pub compliance_hook_program: Program<'info, ComplianceHook>,
    pub system_program: Program<'info, System>,
}

impl<'info> MakePledge<'info> {
    pub fn lien_accounts(&self, bump: u8) -> LienAccounts<'_, 'info> {
        LienAccounts {
            hook_caller: self.hook_caller.as_ref(),
            hook_caller_bump: bump,
            registry: self.registry.as_ref(),
            holder_state: self.giver_holder.as_ref(),
            holding_account: self.giver_holding.as_ref(),
            hook_program: self.compliance_hook_program.as_ref(),
        }
    }
}

pub fn handle_pledge(ctx: Context<MakePledge>, units: u64, pledge_ref: [u8; 32]) -> Result<()> {
    let a = &ctx.accounts.agreement;
    // 1–3. The agreement's FundState, Active, signed by its giver.
    let fund = load_fund(&ctx.accounts.fund_state, a)?;
    require!(a.is_active(), CollateralError::AgreementNotActive);
    require_keys_eq!(
        ctx.accounts.giver.key(),
        a.giver,
        CollateralError::Unauthorized
    );
    // 4–6.
    require!(units > 0, CollateralError::ZeroUnits);
    require!(pledge_ref != [0; 32], CollateralError::ZeroPledgeRef);
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let holder = load_holder(&ctx.accounts.giver_holder, &fund.mint, &a.giver)?;
    require_eligible(&holder, now)?;
    // 7. The agreement's pledge limit.
    let pledged = a
        .pledged_units
        .checked_add(units)
        .filter(|p| *p <= a.concentration_cap_units)
        .ok_or(error!(CollateralError::PledgeLimitExceeded))?;
    // 8. Lock the units: no transfer, and the hook checks the balance covers the new lock.
    lien::set_lien(
        &ctx.accounts.lien_accounts(ctx.bumps.hook_caller),
        LienChange {
            lock: units,
            unlock: 0,
            opened: 1,
            closed: 0,
        },
    )?;
    let giver_locked_after = lien::locked_after(&ctx.accounts.giver_holder)?;

    let seq = a.pledge_count;
    ctx.accounts.pledge.set_inner(Pledge {
        version: Pledge::VERSION,
        agreement: ctx.accounts.agreement.key(),
        seq,
        pledge_ref,
        units,
        mode: PledgeMode::Lien.as_u8(),
        posted_at: now,
        nav_seq_at_post: fund.nav_seq,
        released_units: 0,
        enforced_units: 0,
        status: PledgeStatus::Active.as_u8(),
        closed_at: 0,
        bump: ctx.bumps.pledge,
        _reserved: [0; 64],
    });
    let a = &mut ctx.accounts.agreement;
    a.pledge_count = seq
        .checked_add(1)
        .ok_or(error!(CollateralError::Overflow))?;
    a.pledged_units = pledged;
    // No price is needed to pledge; with a fresh one, the call is met in this step.
    let valued = is_fresh(&fund, a, now);
    if valued {
        observe(a, &fund)?;
    }

    let snap = snapshot(a, &fund, valued)?;
    emit_cpi!(Pledged {
        fund: a.fund_state,
        agreement: a.key(),
        pledge: ctx.accounts.pledge.key(),
        seq,
        pledge_ref,
        units,
        giver: a.giver,
        giver_locked_after,
        snapshot: snap,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
