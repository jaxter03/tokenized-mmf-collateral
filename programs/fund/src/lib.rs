use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;
pub mod token_setup;

pub use errors::FundError;
pub use instructions::*;
pub use state::{FundState, FundTerms};

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
}
