use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod extra_metas;
pub mod instructions;
pub mod state;

pub use errors::HookError;
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
}
