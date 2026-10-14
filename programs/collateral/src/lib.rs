//! `collateral`: collateral agreements, margin calls and lien pledges of fund units (F004,
//! docs/shared-model.md §4.4).

use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod state;
pub mod valuation;

pub use errors::CollateralError;
pub use state::{CollateralCall, Pledge, PledgeAgreement, ProposeArgs};

declare_id!("2RGrj5Jp8PGFVNRFNbJJtZVAph9mBYEubX6QMgqup6Yi");

#[program]
pub mod collateral {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_sizes() {
        // data-model.md: 432, 196 and 268 bytes including the 8-byte discriminator.
        assert_eq!(8 + PledgeAgreement::INIT_SPACE, 432);
        assert_eq!(8 + Pledge::INIT_SPACE, 196);
        assert_eq!(8 + CollateralCall::INIT_SPACE, 268);
    }
}
