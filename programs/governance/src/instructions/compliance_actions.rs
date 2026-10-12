//! Compliance actions (contracts/governance.md): suspend, reinstate and revoke a holder; pause and
//! unpause all transfers. Each is role-gated, reason-coded and takes effect immediately
//! (Constitution VIII: protective and reversible). Governance CPIs into compliance_hook with its
//! `["hook-admin"]` PDA; the record in F001 is compliance_hook's event (shared-model I8).

use anchor_lang::prelude::*;
use compliance_hook::program::ComplianceHook;
use mmf_common::seeds;

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

pub fn handle_holder_action(
    _ctx: Context<HolderAction>,
    _action: u8,
    _reason: u8,
    _case_ref_hash: [u8; 32],
) -> Result<()> {
    // Implemented in T069.
    Ok(())
}

pub fn handle_pause_action(
    _ctx: Context<PauseAction>,
    _paused: bool,
    _reason: u8,
    _case_ref_hash: [u8; 32],
) -> Result<()> {
    // Implemented in T069.
    Ok(())
}
