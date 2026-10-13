//! `dvp_settlement`: escrowed both-or-neither settlement of subscriptions, redemptions and
//! secondary transfers (F003, docs/shared-model.md §4.3).

use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;

pub use errors::DvpError;
pub use instructions::*;
pub use state::{DvpConfig, SettlementInstruction};

declare_id!("418DJ7wBLXAWBGA81vGLxEJTMxZMNMqrmj6t3sErck6w");

#[program]
pub mod dvp_settlement {
    use super::*;

    pub fn init_dvp(ctx: Context<InitDvp>, default_expiry_secs: Option<u32>) -> Result<()> {
        instructions::init_dvp::handle_init_dvp(ctx, default_expiry_secs)
    }
}
