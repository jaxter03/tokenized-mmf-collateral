//! `dvp_settlement`: escrowed both-or-neither settlement of subscriptions, redemptions and
//! secondary transfers (F003, docs/shared-model.md §4.3).

use anchor_lang::prelude::*;

declare_id!("418DJ7wBLXAWBGA81vGLxEJTMxZMNMqrmj6t3sErck6w");

#[program]
pub mod dvp_settlement {}
