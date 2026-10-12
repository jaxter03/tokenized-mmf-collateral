use anchor_lang::prelude::*;

/// `governance` errors (contracts/governance.md).
#[error_code]
pub enum GovernanceError {
    #[msg("Signer does not hold the required role")]
    Unauthorized,
    #[msg("Fund is not in Setup")]
    FundNotInSetup,
    #[msg("Fund is not Open")]
    FundNotOpen,
    #[msg("A fund already exists for this mint")]
    FundAlreadyExists,
    #[msg("Every operating role must be assigned to a non-default key")]
    RoleUnassigned,
    #[msg("The keeper must differ from every other role")]
    KeeperNotSeparate,
    #[msg("Compliance must differ from the issuer")]
    ComplianceIsIssuer,
    #[msg("Reason code is not allowed for this action")]
    InvalidReason,
}
