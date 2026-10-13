use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;
pub mod token_setup;

pub use errors::FundError;
pub use instructions::*;
pub use state::{FundState, FundTerms, NavRecord};

declare_id!("HV75Ur4qgfHHTNwbJdpiTMkq2sA7YoynBXFtzpsZzYT5");

#[program]
pub mod fund {
    use super::*;

    pub fn create_fund(
        ctx: Context<CreateFund>,
        terms: FundTerms,
        name: String,
        symbol: String,
    ) -> Result<()> {
        instructions::create_fund::handle_create_fund(ctx, terms, name, symbol)
    }

    pub fn launch(ctx: Context<Launch>) -> Result<()> {
        instructions::launch::handle_launch(ctx)
    }

    pub fn publish_daily(
        ctx: Context<PublishDaily>,
        nav: u64,
        daily_rate_e12: i64,
        days_covered: u16,
    ) -> Result<()> {
        instructions::publish_daily::handle_publish_daily(ctx, nav, daily_rate_e12, days_covered)
    }

    pub fn set_nav_override(
        ctx: Context<SetNavOverride>,
        nav: u64,
        reason: u8,
        case_ref_hash: [u8; 32],
        actor: Pubkey,
    ) -> Result<()> {
        instructions::nav_override::handle_set_nav_override(ctx, nav, reason, case_ref_hash, actor)
    }

    pub fn clear_nav_override(
        ctx: Context<SetNavOverride>,
        reason: u8,
        case_ref_hash: [u8; 32],
        actor: Pubkey,
    ) -> Result<()> {
        instructions::nav_override::handle_clear_nav_override(ctx, reason, case_ref_hash, actor)
    }

    pub fn set_cash_treasury(ctx: Context<SetCashTreasury>) -> Result<()> {
        instructions::set_cash_treasury::handle_set_cash_treasury(ctx)
    }

    /// dvp_settlement's escrow PDA only (CPI from `settle`).
    pub fn mint_for_subscription(ctx: Context<MintForSubscription>, units: u64) -> Result<()> {
        instructions::mint_burn::handle_mint_for_subscription(ctx, units)
    }
}
