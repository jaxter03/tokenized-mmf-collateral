use anchor_lang::prelude::*;

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
