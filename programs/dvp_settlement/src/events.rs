//! Records (FR-031). Emitted with `emit_cpi!`; every payload carries `fund`, `slot`, `unix_ts`,
//! and the instruction's `seq` (and `external_ref` where the data model lists it).

use anchor_lang::prelude::*;

/// Settlement was set up for a fund.
#[event]
pub struct DvpInitialized {
    pub fund: Pubkey,
    pub default_expiry_secs: u32,
    pub escrow_authority: Pubkey,
    pub unit_escrow: Pubkey,
    pub cash_escrow: Pubkey,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct SettlementCreated {
    pub fund: Pubkey,
    pub seq: u64,
    /// The SettlementInstruction account.
    pub address: Pubkey,
    pub kind: u8,
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub units: u64,
    pub cash_amount: u64,
    pub nav_seq: u64,
    pub not_before_ts: i64,
    pub expires_at_ts: i64,
    pub external_ref: [u8; 32],
    pub created_by: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

/// One side funded its leg (or, for a subscription's Fund side, affirmed).
#[event]
pub struct LegFunded {
    pub fund: Pubkey,
    pub seq: u64,
    /// `Side`.
    pub side: u8,
    /// Units or cash moved into escrow; 0 for the Fund side's affirmation of a subscription.
    pub amount: u64,
    pub status_after: u8,
    pub signer: Pubkey,
    pub external_ref: [u8; 32],
    pub slot: u64,
    pub unix_ts: i64,
}

/// Both legs moved.
#[event]
pub struct Settled {
    pub fund: Pubkey,
    pub seq: u64,
    pub kind: u8,
    pub units: u64,
    pub cash_amount: u64,
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub nav_seq: u64,
    pub triggered_by: Pubkey,
    pub external_ref: [u8; 32],
    pub slot: u64,
    pub unix_ts: i64,
}

/// One side of an Affirmed instruction asked to cancel.
#[event]
pub struct CancelRequested {
    pub fund: Pubkey,
    pub seq: u64,
    pub side: u8,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct CancelRequestWithdrawn {
    pub fund: Pubkey,
    pub seq: u64,
    pub side: u8,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct SettlementCancelled {
    pub fund: Pubkey,
    pub seq: u64,
    pub units_to_return: u64,
    pub cash_to_return: u64,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct SettlementExpired {
    pub fund: Pubkey,
    pub seq: u64,
    pub units_to_return: u64,
    pub cash_to_return: u64,
    pub triggered_by: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

/// Compliance rejected the instruction, through governance.
#[event]
pub struct SettlementRejected {
    pub fund: Pubkey,
    pub seq: u64,
    pub reason: u8,
    pub case_ref_hash: [u8; 32],
    /// The compliance key that signed the governance instruction.
    pub actor: Pubkey,
    pub units_to_return: u64,
    pub cash_to_return: u64,
    pub slot: u64,
    pub unix_ts: i64,
}

/// An earmarked leg was delivered back to its funder.
#[event]
pub struct LegReturned {
    pub fund: Pubkey,
    pub seq: u64,
    /// `Leg`.
    pub leg: u8,
    pub amount: u64,
    pub recipient: Pubkey,
    pub triggered_by: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}
