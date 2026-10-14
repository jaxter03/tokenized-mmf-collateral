//! `PledgeAgreement`, `Pledge` and `CollateralCall` (docs/shared-model.md §4.4, data-model.md),
//! created with their final layout.

use anchor_lang::prelude::*;
use mmf_common::enums::{AgreementStatus, CallKind, CallStatus, PledgeStatus};

/// `propose_agreement` arguments (contracts/collateral.md), in the contract's order.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProposeArgs {
    pub giver: Pubkey,
    pub taker: Pubkey,
    pub nonce: u64,
    pub mode: u8,
    pub income_to: u8,
    pub haircut_bps: u16,
    pub max_nav_age_secs: u32,
    pub call_cure_secs: u32,
    pub enforcement_grace_secs: u32,
    pub min_transfer_value: u64,
    pub concentration_cap_units: u64,
    pub taker_may_call_unilaterally: bool,
    pub agreement_hash: [u8; 32],
}

/// PDA `["agreement", fund_state, giver, taker, nonce.to_le_bytes()]`. One per agreement, never
/// closed. Parties and terms never change after `propose_agreement` (FR-005).
#[account]
#[derive(Debug, InitSpace)]
pub struct PledgeAgreement {
    pub version: u8,
    pub fund_state: Pubkey,
    pub giver: Pubkey,
    pub taker: Pubkey,
    /// Chosen by the proposer.
    pub nonce: u64,
    /// `PledgeMode`: 0 Lien (only value accepted).
    pub mode: u8,
    /// `IncomeTo`: 0 Giver (only value accepted).
    pub income_to: u8,
    /// < 10_000.
    pub haircut_bps: u16,
    /// > 0; the price is fresh iff `now − FundState.nav_ts ≤ max_nav_age_secs`.
    pub max_nav_age_secs: u32,
    /// > 0.
    pub call_cure_secs: u32,
    pub enforcement_grace_secs: u32,
    /// Cash base units; a shortfall or excess must be more than this.
    pub min_transfer_value: u64,
    /// Pledge limit; > 0.
    pub concentration_cap_units: u64,
    /// Otherwise a raise waits for the giver's confirmation.
    pub taker_may_call_unilaterally: bool,
    /// Non-zero fingerprint of the legal agreement.
    pub agreement_hash: [u8; 32],
    /// `AgreementStatus`.
    pub status: u8,
    pub giver_signed: bool,
    pub taker_signed: bool,
    /// Σ remaining units over this agreement's Active pledges (I15).
    pub pledged_units: u64,
    /// Current exposure, cash base units.
    pub required_value: u64,
    /// Next Pledge seq (starts 1).
    pub pledge_count: u64,
    /// Next CollateralCall seq (starts 1).
    pub call_count: u64,
    pub created_ts: i64,
    /// 0 while Proposed.
    pub activated_ts: i64,
    /// Raise awaiting the giver's confirmation; 0 = none.
    pub pending_raise_value: u64,
    pub pending_raise_ref: [u8; 32],
    pub pending_raise_ts: i64,
    /// Reference of the exposure in force.
    pub exposure_ref: [u8; 32],
    pub exposure_updated_ts: i64,
    /// Demanded part of the shortfall (research R5, I16).
    pub called_outstanding: u64,
    /// Unresolved delivery calls with `seq ≥ met_below_seq` (I16).
    pub open_delivery_calls: u16,
    /// Delivery calls with a lower seq are Met (derived status).
    pub met_below_seq: u64,
    /// Default = none; at most one open return call.
    pub open_return_call: Pubkey,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl PledgeAgreement {
    pub const VERSION: u8 = 1;

    pub fn is_active(&self) -> bool {
        self.status == AgreementStatus::Active.as_u8()
    }
}

/// PDA `["pledge", agreement, pledge_ref]` (research R6). Never closed.
#[account]
#[derive(Debug, InitSpace)]
pub struct Pledge {
    pub version: u8,
    pub agreement: Pubkey,
    /// `agreement.pledge_count` at creation.
    pub seq: u64,
    /// Giver's reference; non-zero (in the seeds: unique per agreement).
    pub pledge_ref: [u8; 32],
    /// > 0.
    pub units: u64,
    /// `PledgeMode`: 0 Lien.
    pub mode: u8,
    pub posted_at: i64,
    /// `FundState.nav_seq` at posting (0 if no price yet).
    pub nav_seq_at_post: u64,
    pub released_units: u64,
    /// F005.
    pub enforced_units: u64,
    /// `PledgeStatus`.
    pub status: u8,
    /// 0 while Active.
    pub closed_at: i64,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl Pledge {
    pub const VERSION: u8 = 1;

    /// `units − released − enforced`.
    pub fn remaining(&self) -> u64 {
        self.units
            .saturating_sub(self.released_units)
            .saturating_sub(self.enforced_units)
    }

    pub fn is_active(&self) -> bool {
        self.status == PledgeStatus::Active.as_u8()
    }
}

/// PDA `["call", agreement, external_ref]` (research R6). Never closed.
#[account]
#[derive(Debug, InitSpace)]
pub struct CollateralCall {
    pub version: u8,
    pub agreement: Pubkey,
    /// `agreement.call_count` at creation.
    pub seq: u64,
    /// `CallKind`.
    pub kind: u8,
    /// Delivery: the uncalled shortfall; Return: the excess (cash base units).
    pub value_delta: u64,
    /// Delivery: rounded up; Return: rounded down (shared-model §8).
    pub units_required: u64,
    /// Price record used.
    pub nav_seq: u64,
    pub issued_at: i64,
    /// `issued_at + call_cure_secs`.
    pub cure_deadline_ts: i64,
    /// Delivery: `cure_deadline_ts + enforcement_grace_secs`; Return: 0.
    pub enforce_after_ts: i64,
    /// `CallStatus` as stored; see the derived-Met rule (`mmf_common::collateral::delivery_met`).
    pub status: u8,
    /// F005: running total for partial enforcement.
    pub enforced_units: u64,
    /// F005.
    pub enforced_value: u64,
    /// Non-zero; unique per agreement (in the seeds).
    pub external_ref: [u8; 32],
    /// Return calls: units released against it.
    pub units_released: u64,
    /// Giver's dispute reference; zero if never disputed.
    pub dispute_ref: [u8; 32],
    pub disputed_at: i64,
    /// Met (stored) or Cancelled time; 0 while unresolved.
    pub closed_at: i64,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl CollateralCall {
    pub const VERSION: u8 = 1;

    pub fn is_delivery(&self) -> bool {
        self.kind == CallKind::Delivery.as_u8()
    }

    pub fn status_is(&self, status: CallStatus) -> bool {
        self.status == status.as_u8()
    }
}
