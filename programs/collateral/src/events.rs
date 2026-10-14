//! Records (FR-037; data-model.md "Events"). Emitted with `emit_cpi!`, one per instruction; every
//! payload carries `fund`, `slot` and `unix_ts`. Steps that value the agreement also carry a
//! `Snapshot`, so the history can be rebuilt from events alone (SC-009).

use anchor_lang::prelude::*;

/// The agreement's valuation and call bookkeeping after a step.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// False when the step ran without a fresh price (no valuation, no observation).
    pub valued: bool,
    /// Collateral value − required value at `nav_seq` (0 when not valued).
    pub coverage: i128,
    pub nav_seq: u64,
    pub pledged_units: u64,
    pub required_value: u64,
    pub called_outstanding: u64,
    pub open_delivery_calls: u16,
    pub met_below_seq: u64,
}

/// A call opened by this step.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallOpened {
    pub call: Pubkey,
    pub seq: u64,
    pub kind: u8,
    pub value_delta: u64,
    pub units_required: u64,
    pub nav_seq: u64,
    pub cure_deadline_ts: i64,
    pub enforce_after_ts: i64,
    pub external_ref: [u8; 32],
}

#[event]
pub struct AgreementProposed {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub proposer: Pubkey,
    pub args: crate::state::ProposeArgs,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct AgreementActive {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

/// `update_exposure` or `confirm_exposure`. `outcome` is `ExposureOutcome`.
#[event]
pub struct ExposureUpdated {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub required_value: u64,
    pub external_ref: [u8; 32],
    pub outcome: u8,
    pub signer: Pubkey,
    pub opened_call: Option<CallOpened>,
    pub snapshot: Snapshot,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct CallIssued {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub call: CallOpened,
    pub signer: Pubkey,
    pub snapshot: Snapshot,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct CoverageChecked {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub triggered_by: Pubkey,
    pub snapshot: Snapshot,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct CallCancelled {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub call: Pubkey,
    pub seq: u64,
    pub kind: u8,
    pub signer: Pubkey,
    pub called_outstanding: u64,
    pub open_delivery_calls: u16,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct CallDisputed {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub call: Pubkey,
    pub seq: u64,
    pub dispute_ref: [u8; 32],
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct DisputeWithdrawn {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub call: Pubkey,
    pub seq: u64,
    pub slot: u64,
    pub unix_ts: i64,
}

/// Compliance's resolution (record until F007's ActionRecord kind 15). `outcome` is the
/// resulting `CallStatus` (Open or Cancelled).
#[event]
pub struct DisputeResolved {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub call: Pubkey,
    pub seq: u64,
    pub outcome: u8,
    pub reason: u8,
    pub case_ref_hash: [u8; 32],
    pub actor: Pubkey,
    pub called_outstanding: u64,
    pub open_delivery_calls: u16,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct Pledged {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub pledge: Pubkey,
    pub seq: u64,
    pub pledge_ref: [u8; 32],
    pub units: u64,
    pub giver: Pubkey,
    /// The giver's total locked units after this pledge, across every agreement.
    pub giver_locked_after: u64,
    pub snapshot: Snapshot,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct Released {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub pledge: Pubkey,
    pub units: u64,
    /// Units the pledge still holds after this release.
    pub pledge_remaining: u64,
    pub signer: Pubkey,
    pub return_call: Option<Pubkey>,
    pub return_call_met: bool,
    pub snapshot: Snapshot,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct Substituted {
    pub fund: Pubkey,
    pub agreement: Pubkey,
    pub old_pledge: Pubkey,
    pub new_pledge: Pubkey,
    pub new_seq: u64,
    pub new_pledge_ref: [u8; 32],
    pub old_remaining: u64,
    pub new_units: u64,
    pub snapshot: Snapshot,
    pub slot: u64,
    pub unix_ts: i64,
}
