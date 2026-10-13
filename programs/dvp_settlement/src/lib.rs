//! `dvp_settlement`: escrowed both-or-neither settlement of subscriptions, redemptions and
//! secondary transfers (F003, docs/shared-model.md §4.3).

use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod state;

pub use errors::DvpError;
pub use state::{DvpConfig, SettlementInstruction};

declare_id!("418DJ7wBLXAWBGA81vGLxEJTMxZMNMqrmj6t3sErck6w");

#[program]
pub mod dvp_settlement {}
