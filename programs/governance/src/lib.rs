use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod state;

pub use errors::GovernanceError;
pub use state::RoleRegistry;

declare_id!("8Z7an6h8k8dbq7Bxzxyh4HseyaNgTHpVzSwvcq5H9pTV");

#[program]
pub mod governance {}
