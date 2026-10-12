use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod state;

pub use errors::HookError;
pub use state::{HolderState, Registry};

declare_id!("A2R9A9xmCpicyUAYixJZr63oBPmKSR95CxxLoSPQfqtM");

#[program]
pub mod compliance_hook {}
