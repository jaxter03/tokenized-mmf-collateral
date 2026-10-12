//! Compliance actions (contracts/governance.md): suspend, reinstate and revoke a holder; pause and
//! unpause all transfers. Each is role-gated, reason-coded and takes effect immediately
//! (Constitution VIII: protective and reversible). Governance CPIs into compliance_hook with its
//! `["hook-admin"]` PDA; the record in F001 is compliance_hook's event (shared-model I8).

use anchor_lang::prelude::*;
use compliance_hook::program::ComplianceHook;
use mmf_common::enums::{FundStatus, ReasonCode};
use mmf_common::ids::COMPLIANCE_HOOK_PROGRAM_ID;
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::GovernanceError;
use crate::state::RoleRegistry;

/// Accounts for suspend / reinstate / revoke.
#[derive(Accounts)]
pub struct HolderAction<'info> {
    pub compliance: Signer<'info>,
    pub role_registry: Account<'info, RoleRegistry>,
    /// CHECK: the fund bound to this role registry; decoded with `FundStateView`.
    #[account(address = role_registry.fund_state)]
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: must be compliance_hook's Registry for this fund's mint (checked in the handler).
    pub registry: UncheckedAccount<'info>,
    /// CHECK: the target holder record; compliance_hook checks it belongs to the registry's mint.
    #[account(mut)]
    pub holder_state: UncheckedAccount<'info>,
    /// CHECK: governance PDA `["hook-admin"]`; signs the CPI.
    #[account(seeds = [seeds::HOOK_ADMIN], bump)]
    pub hook_admin: UncheckedAccount<'info>,
    /// CHECK: compliance_hook's `["__event_authority"]` PDA (for its emit_cpi! event).
    pub hook_event_authority: UncheckedAccount<'info>,
    pub compliance_hook_program: Program<'info, ComplianceHook>,
}

/// Accounts for pause / unpause.
#[derive(Accounts)]
pub struct PauseAction<'info> {
    pub compliance: Signer<'info>,
    pub role_registry: Account<'info, RoleRegistry>,
    /// CHECK: the fund bound to this role registry; decoded with `FundStateView`.
    #[account(address = role_registry.fund_state)]
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: must be compliance_hook's Registry for this fund's mint (checked in the handler).
    #[account(mut)]
    pub registry: UncheckedAccount<'info>,
    /// CHECK: governance PDA `["hook-admin"]`; signs the CPI.
    #[account(seeds = [seeds::HOOK_ADMIN], bump)]
    pub hook_admin: UncheckedAccount<'info>,
    /// CHECK: compliance_hook's `["__event_authority"]` PDA.
    pub hook_event_authority: UncheckedAccount<'info>,
    pub compliance_hook_program: Program<'info, ComplianceHook>,
}

/// Checks shared by all five actions, in contract order: Unauthorized → FundNotOpen →
/// (register belongs to this fund, else Unauthorized) → InvalidReason.
fn check(
    compliance: &Pubkey,
    role_registry: &RoleRegistry,
    fund_state: &AccountInfo,
    registry: &Pubkey,
    reason: u8,
) -> Result<()> {
    // 1. Only the compliance role (a Squads vault signs through Squads' execute).
    require_keys_eq!(
        *compliance,
        role_registry.compliance,
        GovernanceError::Unauthorized
    );

    // 2. Only on an Open fund (a missing fund fails closed).
    let fund = FundStateView::load(fund_state, &role_registry.fund_state)
        .map_err(|_| error!(GovernanceError::FundNotOpen))?;
    require!(
        fund.status == FundStatus::Open.as_u8(),
        GovernanceError::FundNotOpen
    );

    // The hook-admin PDA is shared by every fund, so the register must be this fund's: otherwise
    // one fund's compliance could act on another fund's holders.
    let expected = Pubkey::find_program_address(
        &[seeds::REGISTRY, fund.mint.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
    .0;
    require_keys_eq!(*registry, expected, GovernanceError::Unauthorized);

    // 3. A reason from the F001 list (FR-031).
    let allowed = ReasonCode::try_from(reason)
        .map(|r| r.allowed_in_f001())
        .unwrap_or(false);
    require!(allowed, GovernanceError::InvalidReason);
    Ok(())
}

pub fn handle_holder_action(
    ctx: Context<HolderAction>,
    action: u8,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Result<()> {
    let a = &ctx.accounts;
    let compliance = a.compliance.key();
    check(
        &compliance,
        &a.role_registry,
        &a.fund_state,
        &a.registry.key(),
        reason,
    )?;

    let bump = [ctx.bumps.hook_admin];
    let signer: &[&[&[u8]]] = &[&[seeds::HOOK_ADMIN, &bump]];
    compliance_hook::cpi::set_status(
        CpiContext::new(
            a.compliance_hook_program.key(),
            compliance_hook::cpi::accounts::SetStatus {
                hook_admin: a.hook_admin.to_account_info(),
                registry: a.registry.to_account_info(),
                holder_state: a.holder_state.to_account_info(),
                event_authority: a.hook_event_authority.to_account_info(),
                program: a.compliance_hook_program.to_account_info(),
            },
        )
        .with_signer(signer),
        action,
        reason,
        case_ref_hash,
        compliance,
    )
}

pub fn handle_pause_action(
    ctx: Context<PauseAction>,
    paused: bool,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Result<()> {
    let a = &ctx.accounts;
    let compliance = a.compliance.key();
    check(
        &compliance,
        &a.role_registry,
        &a.fund_state,
        &a.registry.key(),
        reason,
    )?;

    let bump = [ctx.bumps.hook_admin];
    let signer: &[&[&[u8]]] = &[&[seeds::HOOK_ADMIN, &bump]];
    compliance_hook::cpi::set_paused(
        CpiContext::new(
            a.compliance_hook_program.key(),
            compliance_hook::cpi::accounts::SetPaused {
                hook_admin: a.hook_admin.to_account_info(),
                registry: a.registry.to_account_info(),
                event_authority: a.hook_event_authority.to_account_info(),
                program: a.compliance_hook_program.to_account_info(),
            },
        )
        .with_signer(signer),
        paused,
        reason,
        case_ref_hash,
        compliance,
    )
}
