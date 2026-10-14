//! `DvpConfig` and `SettlementInstruction` (docs/shared-model.md §4.3, data-model.md).

use anchor_lang::prelude::*;
use mmf_common::enums::{SettlementKind, SettlementStatus, Side};

/// PDA `["dvp", fund_state]`: one per fund, created by `init_dvp`, never closed.
#[account]
#[derive(Debug, InitSpace)]
pub struct DvpConfig {
    pub version: u8,
    pub fund_state: Pubkey,
    /// Sequence number of the next instruction; starts at 1.
    pub next_seq: u64,
    /// Expiry used when a creator gives none; 86 400 unless set at `init_dvp` (research R20).
    pub default_expiry_secs: u32,
    pub bump: u8,
    /// Bump of the escrow authority PDA `["escrow", fund_state]`.
    pub escrow_bump: u8,
    pub _reserved: [u8; 64],
}

impl DvpConfig {
    pub const VERSION: u8 = 1;
}

/// PDA `["settlement", fund_state, created_by, external_ref]` (research R2). One per exchange,
/// never closed, so every instruction stays readable (FR-006).
#[account]
#[derive(Debug, InitSpace)]
pub struct SettlementInstruction {
    pub version: u8,
    pub fund_state: Pubkey,
    /// 1, 2, 3, … per fund (`DvpConfig.next_seq` at creation).
    pub seq: u64,
    /// `SettlementKind`.
    pub kind: u8,
    /// Subscription: `fund_state`; otherwise the unit owner.
    pub seller: Pubkey,
    /// Redemption: `fund_state`; otherwise the unit receiver.
    pub buyer: Pubkey,
    /// > 0, base units.
    pub units: u64,
    /// > 0, cash base units, fixed at creation (shared-model §8).
    pub cash_amount: u64,
    /// `FundState.nav_seq` at creation.
    pub nav_seq: u64,
    /// Valid-from: settlement only at or after it.
    pub not_before_ts: i64,
    /// > max(created_at, not_before_ts).
    pub expires_at_ts: i64,
    /// `SettlementStatus`.
    pub status: u8,
    /// 0 = not funded. Subscription: when the transfer agent affirmed for the Fund.
    pub seller_funded_at: i64,
    /// 0 = not funded.
    pub buyer_funded_at: i64,
    /// Not all zero; unique per creator (it is in the seeds).
    pub external_ref: [u8; 32],
    pub created_by: Pubkey,
    pub created_at: i64,
    /// Time of the final status; 0 while open.
    pub closed_at: i64,
    /// Bit 0 seller side, bit 1 buyer side (Affirmed only, research R17).
    pub cancel_requests: u8,
    /// Earmarked units owed back to the seller (research R3).
    pub units_to_return: u64,
    /// Earmarked cash owed back to the buyer side (a redemption's goes to `cash_treasury`).
    pub cash_to_return: u64,
    pub bump: u8,
    pub _reserved: [u8; 64],
}

impl SettlementInstruction {
    pub const VERSION: u8 = 1;

    /// Stored kind. Only `create_instruction` writes it, with a valid value.
    pub fn kind(&self) -> SettlementKind {
        SettlementKind::try_from(self.kind).expect("kind written by create_instruction")
    }

    /// Stored status. Only this program writes it, with a valid value.
    pub fn status(&self) -> SettlementStatus {
        SettlementStatus::try_from(self.status).expect("status written by this program")
    }

    /// Units this instruction holds in the unit escrow (data-model "Escrow held by an
    /// instruction"): the funded units while open, the earmark once final.
    pub fn units_held(&self) -> u64 {
        let status = self.status();
        if status.is_open() {
            if self.seller_funded_at != 0 && self.kind() != SettlementKind::Subscription {
                self.units
            } else {
                0
            }
        } else {
            self.units_to_return
        }
    }

    /// Cash this instruction holds in the cash escrow: the funded cash while open, the earmark
    /// once final.
    pub fn cash_held(&self) -> u64 {
        if self.status().is_open() {
            if self.buyer_funded_at != 0 {
                self.cash_amount
            } else {
                0
            }
        } else {
            self.cash_to_return
        }
    }

    /// The key that signs for `side`: the transfer agent for the Fund side (seller of a
    /// subscription, buyer of a redemption), otherwise the stored party (research R6).
    pub fn side_party(&self, side: Side, transfer_agent: &Pubkey) -> Pubkey {
        match (side, self.kind()) {
            (Side::Seller, SettlementKind::Subscription) => *transfer_agent,
            (Side::Buyer, SettlementKind::Redemption) => *transfer_agent,
            (Side::Seller, _) => self.seller,
            (Side::Buyer, _) => self.buyer,
        }
    }

    /// Bit of `side` in `cancel_requests`.
    pub const fn cancel_bit(side: Side) -> u8 {
        match side {
            Side::Seller => 1,
            Side::Buyer => 2,
        }
    }
}
