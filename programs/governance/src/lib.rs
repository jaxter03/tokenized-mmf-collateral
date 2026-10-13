use anchor_lang::prelude::*;
use mmf_common::enums::StatusAction;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;

pub use errors::GovernanceError;
pub use instructions::*;
pub use state::RoleRegistry;

declare_id!("8Z7an6h8k8dbq7Bxzxyh4HseyaNgTHpVzSwvcq5H9pTV");

#[program]
pub mod governance {
    use super::*;

    pub fn init_roles(ctx: Context<InitRoles>) -> Result<()> {
        instructions::init_roles::handle_init_roles(ctx)
    }

    pub fn suspend_holder(
        ctx: Context<HolderAction>,
        reason: u8,
        case_ref_hash: [u8; 32],
    ) -> Result<()> {
        instructions::compliance_actions::handle_holder_action(
            ctx,
            StatusAction::Suspend.as_u8(),
            reason,
            case_ref_hash,
        )
    }

    pub fn reinstate_holder(
        ctx: Context<HolderAction>,
        reason: u8,
        case_ref_hash: [u8; 32],
    ) -> Result<()> {
        instructions::compliance_actions::handle_holder_action(
            ctx,
            StatusAction::Reinstate.as_u8(),
            reason,
            case_ref_hash,
        )
    }

    pub fn revoke_holder(
        ctx: Context<HolderAction>,
        reason: u8,
        case_ref_hash: [u8; 32],
    ) -> Result<()> {
        instructions::compliance_actions::handle_holder_action(
            ctx,
            StatusAction::Revoke.as_u8(),
            reason,
            case_ref_hash,
        )
    }

    pub fn pause_transfers(
        ctx: Context<PauseAction>,
        reason: u8,
        case_ref_hash: [u8; 32],
    ) -> Result<()> {
        instructions::compliance_actions::handle_pause_action(ctx, true, reason, case_ref_hash)
    }

    pub fn unpause_transfers(
        ctx: Context<PauseAction>,
        reason: u8,
        case_ref_hash: [u8; 32],
    ) -> Result<()> {
        instructions::compliance_actions::handle_pause_action(ctx, false, reason, case_ref_hash)
    }

    pub fn approve_nav_override(
        ctx: Context<NavOverrideAction>,
        nav: u64,
        reason: u8,
        case_ref_hash: [u8; 32],
    ) -> Result<()> {
        instructions::nav_override::handle_approve_nav_override(ctx, nav, reason, case_ref_hash)
    }

    pub fn withdraw_nav_override(
        ctx: Context<NavOverrideAction>,
        reason: u8,
        case_ref_hash: [u8; 32],
    ) -> Result<()> {
        instructions::nav_override::handle_withdraw_nav_override(ctx, reason, case_ref_hash)
    }

    pub fn set_roles(
        ctx: Context<SetRoles>,
        fund_admin: Pubkey,
        transfer_agent: Pubkey,
        compliance: Pubkey,
        keeper: Pubkey,
        auditor: Pubkey,
    ) -> Result<()> {
        instructions::set_roles::handle_set_roles(
            ctx,
            fund_admin,
            transfer_agent,
            compliance,
            keeper,
            auditor,
        )
    }
}
