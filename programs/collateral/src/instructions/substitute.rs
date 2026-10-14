//! `substitute(new_units, new_pledge_ref)` (contracts/collateral.md, FR-030, research R9): the
//! giver replaces an Active pledge with an equal or larger one in one instruction, so the taker's
//! protection never dips. One lien CPI locks the new units and unlocks the old remainder.

use anchor_lang::prelude::*;
use compliance_hook::program::ComplianceHook;
use compliance_hook::LienChange;
use mmf_common::enums::{PledgeMode, PledgeStatus};
use mmf_common::seeds;

use crate::errors::CollateralError;
use crate::events::Substituted;
use crate::lien::{self, LienAccounts};
use crate::state::{Pledge, PledgeAgreement};
use crate::valuation::{is_fresh, load_fund, load_holder, observe, require_eligible, snapshot};

#[event_cpi]
#[derive(Accounts)]
#[instruction(new_units: u64, new_pledge_ref: [u8; 32])]
pub struct Substitute<'info> {
    /// Must be the agreement's giver.
    pub giver: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    #[account(mut)]
    pub old_pledge: Account<'info, Pledge>,
    #[account(
        init,
        payer = payer,
        space = 8 + Pledge::INIT_SPACE,
        seeds = [seeds::PLEDGE, agreement.key().as_ref(), &new_pledge_ref],
        bump
    )]
    pub new_pledge: Account<'info, Pledge>,
    /// CHECK: the fund's Registry; `set_lien` checks it.
    pub registry: UncheckedAccount<'info>,
    /// CHECK: the giver's HolderState, read here and written by `set_lien`.
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

impl<'info> Substitute<'info> {
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

pub fn handle_substitute(
    ctx: Context<Substitute>,
    new_units: u64,
    new_pledge_ref: [u8; 32],
) -> Result<()> {
    let a = &ctx.accounts.agreement;
    // 1–3.
    let fund = load_fund(&ctx.accounts.fund_state, a)?;
    require!(a.is_active(), CollateralError::AgreementNotActive);
    require_keys_eq!(ctx.accounts.giver.key(), a.giver, CollateralError::Unauthorized);
    // 4–6. An Active pledge of this agreement, replaced by at least as many units.
    let old = &ctx.accounts.old_pledge;
    require!(
        old.agreement == a.key() && old.is_active(),
        CollateralError::PledgeNotActive
    );
    require!(new_pledge_ref != [0; 32], CollateralError::ZeroPledgeRef);
    let remaining = old.remaining();
    require!(new_units >= remaining, CollateralError::SubstituteTooSmall);
    // 7–8.
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let holder = load_holder(&ctx.accounts.giver_holder, &fund.mint, &a.giver)?;
    require_eligible(&holder, now)?;
    let pledged = (a.pledged_units - remaining)
        .checked_add(new_units)
        .filter(|p| *p <= a.concentration_cap_units)
        .ok_or(error!(CollateralError::PledgeLimitExceeded))?;
    // 9. One lien change: lock the new units, unlock the old remainder; the hook checks the
    // balance against the net increase.
    lien::set_lien(
        &ctx.accounts.lien_accounts(ctx.bumps.hook_caller),
        LienChange {
            lock: new_units,
            unlock: remaining,
            opened: 1,
            closed: 1,
        },
    )?;

    let old = &mut ctx.accounts.old_pledge;
    old.released_units += remaining;
    old.status = PledgeStatus::Substituted.as_u8();
    old.closed_at = now;
    let seq = ctx.accounts.agreement.pledge_count;
    let agreement = ctx.accounts.agreement.key();
    ctx.accounts.new_pledge.set_inner(Pledge {
        version: Pledge::VERSION,
        agreement,
        seq,
        pledge_ref: new_pledge_ref,
        units: new_units,
        mode: PledgeMode::Lien.as_u8(),
        posted_at: now,
        nav_seq_at_post: fund.nav_seq,
        released_units: 0,
        enforced_units: 0,
        status: PledgeStatus::Active.as_u8(),
        closed_at: 0,
        bump: ctx.bumps.new_pledge,
        _reserved: [0; 64],
    });
    let a = &mut ctx.accounts.agreement;
    a.pledged_units = pledged;
    a.pledge_count = seq.checked_add(1).ok_or(error!(CollateralError::Overflow))?;
    let valued = is_fresh(&fund, a, now);
    if valued {
        observe(a, &fund)?;
    }

    let snap = snapshot(a, &fund, valued)?;
    emit_cpi!(Substituted {
        fund: a.fund_state,
        agreement,
        old_pledge: ctx.accounts.old_pledge.key(),
        new_pledge: ctx.accounts.new_pledge.key(),
        new_seq: seq,
        new_pledge_ref,
        old_remaining: remaining,
        new_units,
        snapshot: snap,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
