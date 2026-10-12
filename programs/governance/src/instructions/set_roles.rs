//! `set_roles` (contracts/governance.md): the issuer assigns the five operating roles while the
//! Fund is in Setup (FR-010). Separation rules arrive in T074.

use anchor_lang::prelude::*;
use mmf_common::enums::FundStatus;
use mmf_common::views::FundStateView;

use crate::errors::GovernanceError;
use crate::events::RolesAssigned;
use crate::state::RoleRegistry;

#[event_cpi]
#[derive(Accounts)]
pub struct SetRoles<'info> {
    pub issuer: Signer<'info>,
    #[account(mut)]
    pub role_registry: Account<'info, RoleRegistry>,
    /// CHECK: must be the fund bound to this role registry; decoded with `FundStateView`.
    #[account(address = role_registry.fund_state)]
    pub fund_state: UncheckedAccount<'info>,
}

pub fn handle_set_roles(
    ctx: Context<SetRoles>,
    fund_admin: Pubkey,
    transfer_agent: Pubkey,
    compliance: Pubkey,
    keeper: Pubkey,
    auditor: Pubkey,
) -> Result<()> {
    let roles = &mut ctx.accounts.role_registry;
    require_keys_eq!(
        ctx.accounts.issuer.key(),
        roles.issuer,
        GovernanceError::Unauthorized
    );

    // A fund that doesn't exist yet, or isn't in Setup, can't have its roles changed (FR-010; after
    // launch role changes are a governance action — US4-4).
    let in_setup = FundStateView::load(&ctx.accounts.fund_state, &roles.fund_state)
        .map(|f| f.status == FundStatus::Setup.as_u8())
        .unwrap_or(false);
    require!(in_setup, GovernanceError::FundNotInSetup);

    let assigned = [fund_admin, transfer_agent, compliance, keeper, auditor];
    require!(
        assigned.iter().all(|k| *k != Pubkey::default()),
        GovernanceError::RoleUnassigned
    );

    roles.fund_admin = fund_admin;
    roles.transfer_agent = transfer_agent;
    roles.compliance = compliance;
    roles.keeper = keeper;
    roles.auditor = auditor;

    let clock = Clock::get()?;
    emit_cpi!(RolesAssigned {
        fund: roles.fund_state,
        issuer: roles.issuer,
        fund_admin,
        transfer_agent,
        compliance,
        keeper,
        auditor,
        signer: ctx.accounts.issuer.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
