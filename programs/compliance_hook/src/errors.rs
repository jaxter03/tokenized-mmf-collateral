use anchor_lang::prelude::*;

/// `compliance_hook` errors (contracts/compliance_hook.md, contracts/kyc-statement.md).
#[error_code]
pub enum HookError {
    // --- authority and fund status ---
    #[msg("Signer does not hold the required role")]
    Unauthorized,
    #[msg("Fund is not in Setup")]
    FundNotInSetup,
    #[msg("Fund is not Open")]
    FundNotOpen,
    #[msg("Caller is not governance's hook-admin PDA")]
    UnauthorizedCaller,
    // --- register rules ---
    #[msg("KYC providers must be distinct")]
    DuplicateProvider,
    #[msg("Jurisdiction is not an assigned ISO 3166-1 alpha-2 code")]
    UnknownJurisdiction,
    // --- registration ---
    #[msg("Holder class is not valid")]
    InvalidClass,
    #[msg("An institution may register itself only as an investor")]
    SelfRegistrationClassNotAllowed,
    #[msg("This holder class is not available yet")]
    ClassNotAvailable,
    #[msg("Fund-operated accounts must be owned by the system's own PDA")]
    NotSystemControlled,
    #[msg("This owner already has a holder record")]
    HolderAlreadyRegistered,
    // --- holder status ---
    #[msg("Holder is revoked")]
    HolderRevoked,
    #[msg("Holder status does not allow this action")]
    InvalidHolderStatus,
    #[msg("Holder is not an investor")]
    NotInvestor,
    #[msg("Holder is not a fund-operated account")]
    NotFundAccount,
    // --- thaw ---
    #[msg("Holder is not approved with valid KYC")]
    HolderNotApproved,
    #[msg("Token account is not the owner's holding account")]
    NotHoldingAccount,
    #[msg("Holding account is already thawed")]
    AlreadyThawed,
    // --- pause ---
    #[msg("Transfers are already paused")]
    AlreadyPaused,
    #[msg("Transfers are not paused")]
    NotPaused,
    // --- KYC statement ---
    #[msg("No Ed25519 statement instruction immediately before this one")]
    StatementMissing,
    #[msg("KYC statement instruction or message is malformed")]
    StatementMalformed,
    #[msg("Statement signer is not a recognised KYC provider")]
    ProviderNotRecognised,
    #[msg("Statement is for a different fund")]
    StatementWrongFund,
    #[msg("Statement names a different owner account")]
    StatementOwnerMismatch,
    #[msg("Statement has expired")]
    StatementExpired,
    #[msg("Jurisdiction is not allowed by the fund")]
    JurisdictionNotAllowed,
    #[msg("Investor type must be non-zero")]
    InvalidInvestorType,
    #[msg("Renewal expiry must be later than the recorded expiry")]
    RenewalNotNewer,
    #[msg("Statement does not match the approved fingerprint")]
    StatementHashMismatch,
    // --- transfer hook ---
    #[msg("Hook called outside a token transfer")]
    NotTransferring,
    #[msg("Accounts do not belong to this fund's mint")]
    WrongMint,
    #[msg("Transfers are paused")]
    TransfersPaused,
    #[msg("Sender is not an approved holder with valid KYC")]
    SenderNotApproved,
    #[msg("Receiver is not an approved holder with valid KYC, allowed class and jurisdiction")]
    ReceiverNotApproved,
    #[msg("Holder cap reached")]
    HolderCapReached,
    #[msg("Arithmetic overflow")]
    Overflow,
    // --- F003 (appended so F001/F002 codes keep their numbers) ---
    #[msg("Holder record or holding account does not match the owner")]
    WrongHolder,
    #[msg("Interest is settled here only for the escrow holder")]
    ClassNotAccrued,
}
