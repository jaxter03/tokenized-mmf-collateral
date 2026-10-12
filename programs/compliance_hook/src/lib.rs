use anchor_lang::prelude::*;
use spl_discriminator::SplDiscriminate;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

pub mod errors;
pub mod events;
pub mod extra_metas;
pub mod hook;
pub mod instructions;
pub mod state;
pub mod statement;

pub use errors::HookError;
pub use hook::*;
pub use instructions::*;
pub use state::{HolderState, Registry};

declare_id!("A2R9A9xmCpicyUAYixJZr63oBPmKSR95CxxLoSPQfqtM");

#[program]
pub mod compliance_hook {
    use super::*;

    pub fn init_registry(ctx: Context<InitRegistry>) -> Result<()> {
        instructions::init_registry::handle_init_registry(ctx)
    }

    pub fn set_rules(
        ctx: Context<SetRules>,
        kyc_providers: [Pubkey; 4],
        allowed_jurisdictions: [u64; 11],
        max_holders: u32,
    ) -> Result<()> {
        instructions::set_rules::handle_set_rules(
            ctx,
            kyc_providers,
            allowed_jurisdictions,
            max_holders,
        )
    }

    pub fn register_holder(ctx: Context<RegisterHolder>, owner: Pubkey, class: u8) -> Result<()> {
        instructions::register_holder::handle_register_holder(ctx, owner, class)
    }

    pub fn approve_investor(
        ctx: Context<ApproveInvestor>,
        attestation_hash: [u8; 32],
    ) -> Result<()> {
        instructions::approve_investor::handle_approve_investor(ctx, attestation_hash)
    }

    pub fn approve_fund_account(ctx: Context<ApproveFundAccount>) -> Result<()> {
        instructions::approve_fund_account::handle_approve_fund_account(ctx)
    }

    pub fn thaw_holding_account(ctx: Context<ThawHoldingAccount>) -> Result<()> {
        instructions::thaw_holding_account::handle_thaw_holding_account(ctx)
    }

    /// Token-2022 transfer hook entry point.
    #[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)]
    pub fn execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
        hook::execute::handle_execute(ctx, amount)
    }
}
