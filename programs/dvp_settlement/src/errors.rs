//! `DvpError` (contracts/dvp_settlement.md "Errors"). Append new variants only, so codes keep
//! their numbers.

use anchor_lang::prelude::*;

#[error_code]
pub enum DvpError {
    #[msg("Accounts do not belong to the same fund")]
    FundMismatch,
    #[msg("Signer does not hold the required role or party")]
    Unauthorized,
    #[msg("Caller is not the program allowed to call this instruction")]
    UnauthorizedCaller,
    #[msg("Fund is not Open")]
    FundNotOpen,
    #[msg("The fund's treasury cash account is not set")]
    CashTreasuryNotSet,
    #[msg("Cash mint has a transfer fee or transfer hook, which settlement does not support")]
    UnsupportedCashMint,
    #[msg("Settlement is not set up, or the escrow holder is not Approved")]
    EscrowNotReady,
    #[msg("Unknown settlement kind, or the Fund is not a party where the kind requires")]
    InvalidKind,
    #[msg("Side must be 0 (seller) or 1 (buyer)")]
    InvalidSide,
    #[msg("Leg must be 0 (units) or 1 (cash)")]
    InvalidLeg,
    #[msg("Units must be greater than zero")]
    ZeroUnits,
    #[msg("Cash amount must be greater than zero")]
    ZeroCash,
    #[msg("Cash amount is computed from the price for subscriptions and redemptions")]
    CashAmountNotAllowed,
    #[msg("External reference must not be empty")]
    ZeroExternalRef,
    #[msg("Seller and buyer must differ")]
    SameParty,
    #[msg("No price has been published")]
    NoPrice,
    #[msg("The latest price covers no day after the day before yesterday")]
    StalePrice,
    #[msg("Expiry must be after now and after valid-from")]
    InvalidExpiry,
    #[msg("Party is not an Approved investor with valid KYC")]
    PartyNotEligible,
    #[msg("Instruction is final")]
    InstructionFinal,
    #[msg("Instruction has expired")]
    InstructionExpired,
    #[msg("This leg is already funded")]
    LegAlreadyFunded,
    #[msg("Redemption cash must come from the fund's treasury cash account")]
    WrongCashSource,
    #[msg("Token account is not the expected one")]
    WrongAccount,
    #[msg("Instruction is not Affirmed")]
    NotAffirmed,
    #[msg("Instruction is not yet valid")]
    NotYetValid,
    #[msg("Transfers are paused")]
    TransfersPaused,
    #[msg("This side has already asked to cancel")]
    CancelAlreadyRequested,
    #[msg("No cancel request of this side to withdraw")]
    NoCancelRequest,
    #[msg("Instruction has not expired")]
    NotExpired,
    #[msg("Only a cancelled, expired or rejected instruction returns legs")]
    NotReturnable,
    #[msg("Nothing is earmarked for return on this leg")]
    NothingToReturn,
    #[msg("Arithmetic overflow")]
    Overflow,
}
