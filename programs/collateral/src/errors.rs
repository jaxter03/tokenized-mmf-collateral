//! `CollateralError` (contracts/collateral.md "Errors"). Append new variants only, so codes keep
//! their numbers.

use anchor_lang::prelude::*;

#[error_code]
pub enum CollateralError {
    #[msg("Accounts do not belong to the same fund or agreement")]
    FundMismatch,
    #[msg("Fund is not Open")]
    FundNotOpen,
    #[msg("Signer is not the party or role this step requires")]
    Unauthorized,
    #[msg("Caller is not the program allowed to call this instruction")]
    UnauthorizedCaller,
    #[msg("Giver and taker must be different holders")]
    SameParty,
    #[msg("Only lien pledges are supported")]
    UnsupportedPledgeMode,
    #[msg("Only income to the giver is supported")]
    UnsupportedIncomeRouting,
    #[msg("Haircut must be below 100 %")]
    InvalidHaircut,
    #[msg("Time to meet a call must be above zero")]
    InvalidCureTime,
    #[msg("Pledge limit must be above zero")]
    InvalidPledgeLimit,
    #[msg("Maximum price age must be above zero")]
    InvalidPriceAge,
    #[msg("Legal document fingerprint must not be empty")]
    EmptyAgreementHash,
    #[msg("Holder record does not match the party")]
    WrongHolder,
    #[msg("Party is not an approved investor with valid KYC")]
    PartyNotEligible,
    #[msg("Agreement is not Proposed")]
    AgreementNotProposed,
    #[msg("This party has already signed")]
    AlreadySigned,
    #[msg("Agreement is not Active")]
    AgreementNotActive,
    #[msg("Reference must not be empty")]
    ZeroExternalRef,
    #[msg("A call with this reference already exists on the agreement")]
    CallRefInUse,
    #[msg("No price has been published yet")]
    NoPrice,
    #[msg("Latest price is older than the agreement's maximum price age")]
    StalePrice,
    #[msg("No exposure raise is pending")]
    NoPendingRaise,
    #[msg("Value or reference differs from the pending raise")]
    PendingRaiseMismatch,
    #[msg("Call kind is not valid")]
    InvalidCallKind,
    #[msg("Uncalled shortfall is not more than the minimum transfer amount")]
    NothingToCall,
    #[msg("A return call is already open on this agreement")]
    ReturnCallAlreadyOpen,
    #[msg("Excess is not more than the minimum transfer amount")]
    NoExcess,
    #[msg("Excess is worth less than one base unit")]
    ReturnTooSmall,
    #[msg("Units must be above zero")]
    ZeroUnits,
    #[msg("Pledge reference must not be empty")]
    ZeroPledgeRef,
    #[msg("Pledge would take the agreement above its pledge limit")]
    PledgeLimitExceeded,
    #[msg("Pledge is not an Active pledge of this agreement")]
    PledgeNotActive,
    #[msg("Release exceeds the units the pledge still holds")]
    ExceedsPledgeRemaining,
    #[msg("The keeper releases only against the open return call")]
    KeeperNeedsReturnCall,
    #[msg("Return call is not this agreement's open return call")]
    WrongReturnCall,
    #[msg("Release exceeds what the return call still asks for")]
    ExceedsReturnCall,
    #[msg("Release would leave the taker under-collateralised")]
    WouldUndercollateralise,
    #[msg("New pledge must hold at least the units the old one still holds")]
    SubstituteTooSmall,
    #[msg("Call is not in a status that allows this step")]
    CallNotOpen,
    #[msg("This step does not apply to this kind of call")]
    WrongCallKind,
    #[msg("Call is not Disputed")]
    NotDisputed,
    #[msg("Resolution outcome must be Open or Cancelled")]
    InvalidOutcome,
    #[msg("Arithmetic overflow")]
    Overflow,
}
