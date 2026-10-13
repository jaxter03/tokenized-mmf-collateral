//! `dvp_settlement`: escrowed both-or-neither settlement of subscriptions, redemptions and
//! secondary transfers (F003, docs/shared-model.md §4.3).

use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;
pub mod token;

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

    pub fn create_instruction(ctx: Context<CreateInstruction>, args: CreateArgs) -> Result<()> {
        instructions::create_instruction::handle_create_instruction(ctx, args)
    }

    pub fn affirm_and_fund(ctx: Context<AffirmAndFund>, side: u8) -> Result<()> {
        instructions::affirm_and_fund::handle_affirm_and_fund(ctx, side)
    }

    pub fn settle(ctx: Context<Settle>) -> Result<()> {
        instructions::settle::handle_settle(ctx)
    }
}
