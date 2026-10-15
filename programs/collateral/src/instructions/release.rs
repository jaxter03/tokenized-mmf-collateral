//! `release(units)` (contracts/collateral.md, FR-027–FR-029, research R8): the taker, or the
//! keeper against the open return call, unlocks units of a pledge. No units move, and coverage
//! after the release must stay zero or more.

use anchor_lang::prelude::*;
use compliance_hook::program::ComplianceHook;
use compliance_hook::LienChange;
use mmf_common::enums::{CallKind, CallStatus, PledgeStatus};
use mmf_common::seeds;
use mmf_common::views::RoleRegistryView;

use crate::errors::CollateralError;
use crate::events::Released;
use crate::lien::{self, LienAccounts};
use crate::state::{CollateralCall, Pledge, PledgeAgreement};
use crate::valuation::{coverage_with, fresh, load_fund, observe, observe_covered, snapshot};

#[event_cpi]
#[derive(Accounts)]
pub struct Release<'info> {
    /// The taker, or the keeper (RoleRegistry) against the open return call.
    pub signer: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; must be the agreement's FundState.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: the fund's RoleRegistry, via `RoleRegistryView` (the keeper's key).
    pub role_registry: UncheckedAccount<'info>,
    #[account(mut)]
    pub agreement: Account<'info, PledgeAgreement>,
    #[account(mut)]
    pub pledge: Account<'info, Pledge>,
    /// The open return call, when the release is made against it (always for the keeper).
    #[account(mut)]
    pub return_call: Option<Account<'info, CollateralCall>>,
    /// CHECK: the fund's Registry; `set_lien` checks it.
    pub registry: UncheckedAccount<'info>,
    /// CHECK: the giver's HolderState, written by `set_lien`.
    #[account(mut)]
    pub giver_holder: UncheckedAccount<'info>,
    /// CHECK: the giver's holding account; `set_lien` checks it.
    pub giver_holding: UncheckedAccount<'info>,
    /// CHECK: this program's `["hook-caller"]` PDA; signs the lien CPI.
    #[account(seeds = [seeds::HOOK_CALLER], bump)]
    pub hook_caller: UncheckedAccount<'info>,
    pub compliance_hook_program: Program<'info, ComplianceHook>,
}

impl<'info> Release<'info> {
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

pub fn handle_release(ctx: Context<Release>, units: u64) -> Result<()> {
    let a = &ctx.accounts.agreement;
    // 1. The agreement's FundState and the fund's RoleRegistry.
    let fund = load_fund(&ctx.accounts.fund_state, a)?;
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &fund.role_registry)
        .map_err(|_| error!(CollateralError::FundMismatch))?;
    // 2–3. Active; the taker, or the keeper.
    require!(a.is_active(), CollateralError::AgreementNotActive);
    let signer = ctx.accounts.signer.key();
    let by_taker = signer == a.taker;
    require!(
        by_taker || signer == roles.keeper,
        CollateralError::Unauthorized
    );
    // 4–6. An Active pledge of this agreement, holding the units.
    let p = &ctx.accounts.pledge;
    require!(
        p.agreement == a.key() && p.is_active(),
        CollateralError::PledgeNotActive
    );
    require!(units > 0, CollateralError::ZeroUnits);
    require!(
        units <= p.remaining(),
        CollateralError::ExceedsPledgeRemaining
    );
    // 7–9. The keeper only against the open return call, within what it still asks for.
    let ret = ctx.accounts.return_call.as_ref();
    require!(
        by_taker || ret.is_some(),
        CollateralError::KeeperNeedsReturnCall
    );
    if let Some(call) = ret {
        require!(
            call.key() == a.open_return_call
                && call.agreement == a.key()
                && call.kind == CallKind::Return.as_u8()
                && call.status_is(CallStatus::Open),
            CollateralError::WrongReturnCall
        );
        if !by_taker {
            let left = call.units_required.saturating_sub(call.units_released);
            require!(units <= left, CollateralError::ExceedsReturnCall);
        }
    }
    // 10–11. Coverage after the release stays zero or more (no price needed with nothing to
    // protect).
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let pledged_after = a.pledged_units - units;
    let priced = a.required_value > 0;
    if priced {
        fresh(&fund, a, now)?;
        let after = coverage_with(&fund, a, pledged_after, a.required_value)?;
        require!(after >= 0, CollateralError::WouldUndercollateralise);
    }
    // 12. Unlock: no units move.
    let empties = units == p.remaining();
    lien::set_lien(
        &ctx.accounts.lien_accounts(ctx.bumps.hook_caller),
        LienChange {
            lock: 0,
            unlock: units,
            opened: 0,
            closed: u16::from(empties),
        },
    )?;

    let p = &mut ctx.accounts.pledge;
    p.released_units += units;
    if empties {
        p.status = PledgeStatus::Released.as_u8();
        p.closed_at = now;
    }
    let pledge_remaining = p.remaining();
    let a = &mut ctx.accounts.agreement;
    a.pledged_units = pledged_after;
    if priced {
        observe(a, &fund)?;
    } else {
        observe_covered(a);
    }
    let mut return_call_met = false;
    if let Some(call) = ctx.accounts.return_call.as_mut() {
        call.units_released = call
            .units_released
            .checked_add(units)
            .ok_or(error!(CollateralError::Overflow))?;
        if call.units_released >= call.units_required {
            call.status = CallStatus::Met.as_u8();
            call.closed_at = now;
            a.open_return_call = Pubkey::default();
            return_call_met = true;
        }
    }

    let snap = snapshot(a, &fund, priced)?;
    emit_cpi!(Released {
        fund: a.fund_state,
        agreement: a.key(),
        pledge: ctx.accounts.pledge.key(),
        units,
        pledge_remaining,
        signer,
        return_call: ctx.accounts.return_call.as_ref().map(|c| c.key()),
        return_call_met,
        snapshot: snap,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
