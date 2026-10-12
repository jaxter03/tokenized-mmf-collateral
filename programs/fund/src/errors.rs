use anchor_lang::prelude::*;

/// `fund` errors, in the order the contract checks them (contracts/fund.md).
#[error_code]
pub enum FundError {
    #[msg("Role registry is not governance's registry for this fund")]
    RoleRegistryMismatch,
    #[msg("Signer is not the issuer")]
    Unauthorized,
    #[msg("Fund identifier must not be all zero")]
    InvalidFundId,
    #[msg("Cash asset must be an initialized SPL Token or Token-2022 mint with 6 decimals")]
    InvalidCashMint,
    #[msg("Target price must be greater than zero")]
    ZeroTargetNav,
    #[msg("Largest daily price move must be greater than zero")]
    ZeroMaxNavMove,
    #[msg("Daily redemption limit must be between 1 and 10000 basis points")]
    InvalidRedemptionGate,
    #[msg("Distribution mode must be 0 (reinvest) or 1 (cash)")]
    InvalidDistributionMode,
    #[msg("Distribution frequency must be greater than zero")]
    ZeroDistributionCadence,
    #[msg("Legal documents fingerprint is missing")]
    MissingTermsHash,
    #[msg("Legal documents link is missing or not valid UTF-8")]
    MissingTermsUri,
    #[msg("Name must be 1 to 32 bytes")]
    InvalidName,
    #[msg("Symbol must be 1 to 10 bytes")]
    InvalidSymbol,
    #[msg("Fund is not in Setup")]
    FundNotInSetup,
    #[msg("Every operating role must be assigned before launch")]
    RolesIncomplete,
    #[msg("Register and its extra-account list must exist before launch")]
    RegisterNotInitialized,
    #[msg("Register must name at least one KYC provider")]
    NoKycProvider,
    #[msg("Register must allow at least one jurisdiction")]
    NoAllowedJurisdiction,
}
