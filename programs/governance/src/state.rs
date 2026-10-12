//! `RoleRegistry` in its final layout (docs/shared-model.md §4.5).

use anchor_lang::prelude::*;

/// PDA `["roles", fund_state]`. Created by `init_roles` before the Fund exists; the issuer is
/// stored only here (research R10).
#[account]
#[derive(Debug, InitSpace)]
pub struct RoleRegistry {
    pub version: u8,
    /// PDA `["fund", mint]` of the fund program.
    pub fund_state: Pubkey,
    /// Creator of the fund (FR-002); a Squads vault or a key.
    pub issuer: Pubkey,
    pub fund_admin: Pubkey,
    pub transfer_agent: Pubkey,
    /// Typically a Squads vault (FR-012).
    pub compliance: Pubkey,
    pub keeper: Pubkey,
    pub auditor: Pubkey,
    /// F007 (zero in F001).
    pub timelock_secs: u32,
    /// F007: next ActionRecord seq (zero in F001).
    pub action_count: u64,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl RoleRegistry {
    pub const VERSION: u8 = 1;

    /// The five operating roles set by `set_roles`.
    pub fn operating_roles(&self) -> [Pubkey; 5] {
        [
            self.fund_admin,
            self.transfer_agent,
            self.compliance,
            self.keeper,
            self.auditor,
        ]
    }
}
