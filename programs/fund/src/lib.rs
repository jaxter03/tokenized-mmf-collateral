use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod state;

pub use errors::FundError;
pub use state::{FundState, FundTerms};

declare_id!("HV75Ur4qgfHHTNwbJdpiTMkq2sA7YoynBXFtzpsZzYT5");

#[program]
pub mod fund {}
