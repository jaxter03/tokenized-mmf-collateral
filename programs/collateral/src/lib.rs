//! `collateral`: collateral agreements, margin calls and lien pledges of fund units (F004,
//! docs/shared-model.md §4.4).

use anchor_lang::prelude::*;

pub mod call_account;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;
pub mod valuation;

pub use errors::CollateralError;
pub use instructions::*;
pub use state::{CollateralCall, Pledge, PledgeAgreement, ProposeArgs};

declare_id!("2RGrj5Jp8PGFVNRFNbJJtZVAph9mBYEubX6QMgqup6Yi");

#[program]
pub mod collateral {
    use super::*;

    pub fn propose_agreement(ctx: Context<ProposeAgreement>, args: ProposeArgs) -> Result<()> {
        instructions::propose_agreement::handle_propose_agreement(ctx, args)
    }

    pub fn sign_agreement(ctx: Context<SignAgreement>) -> Result<()> {
        instructions::sign_agreement::handle_sign_agreement(ctx)
    }

    pub fn update_exposure(
        ctx: Context<UpdateExposure>,
        required_value: u64,
        external_ref: [u8; 32],
    ) -> Result<()> {
        instructions::exposure::handle_update_exposure(ctx, required_value, external_ref)
    }

    pub fn confirm_exposure(
        ctx: Context<ConfirmExposure>,
        required_value: u64,
        external_ref: [u8; 32],
    ) -> Result<()> {
        instructions::exposure::handle_confirm_exposure(ctx, required_value, external_ref)
    }

    pub fn issue_call(ctx: Context<IssueCall>, kind: u8, external_ref: [u8; 32]) -> Result<()> {
        instructions::issue_call::handle_issue_call(ctx, kind, external_ref)
    }

    /// Anyone (permissionless, state-checked): observe coverage at a fresh price.
    pub fn check_coverage(ctx: Context<CheckCoverage>) -> Result<()> {
        instructions::check_coverage::handle_check_coverage(ctx)
    }
}

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
