//! An auditor's rebuilds from the records alone:
//! - every holder's earned interest (F002 FR-021, SC-007): the permanent NavRecords and the
//!   transfers' `TransferValidated` records; it doesn't read any HolderState and doesn't use
//!   `mmf_common::accrual`;
//! - every settlement instruction's history and the escrow's movements (F003 FR-031, SC-008):
//!   from `dvp_settlement`'s and `fund`'s settlement events; it doesn't read any
//!   SettlementInstruction.

use std::collections::HashMap;

use anchor_lang::prelude::Pubkey;

/// One record, in ledger order.
#[derive(Clone, Debug)]
pub enum Record {
    /// A NavRecord: the counter after that publish.
    Nav { index_after: u128 },
    /// A transfer's record. `accrued_after` is what the event reports for each side, kept so the
    /// rebuild can cross-check it.
    Transfer {
        source: Pubkey,
        destination: Pubkey,
        amount: u64,
        source_accrued_after: u128,
        destination_accrued_after: u128,
    },
}

#[derive(Clone, Copy, Debug, Default)]
struct Book {
    balance: u64,
    earned: u128,
    index: u128,
}

/// Rebuilds earned interest from `start` balances (held before the first publish) and `records`.
/// Returns each owner's interest earned up to the latest record. Panics if a transfer record's
/// reported figures disagree with the rebuild.
pub fn rebuild(start: &HashMap<Pubkey, u64>, records: &[Record]) -> HashMap<Pubkey, u128> {
    let mut books: HashMap<Pubkey, Book> = start
        .iter()
        .map(|(k, b)| {
            (
                *k,
                Book {
                    balance: *b,
                    ..Book::default()
                },
            )
        })
        .collect();
    let mut index: u128 = 0;
    let settle = |b: &mut Book, index: u128| {
        b.earned += u128::from(b.balance) * (index - b.index);
        b.index = index;
    };
    for r in records {
        match r {
            Record::Nav { index_after } => index = *index_after,
            Record::Transfer {
                source,
                destination,
                amount,
                source_accrued_after,
                destination_accrued_after,
            } => {
                let mut s = books.get(source).copied().unwrap_or_default();
                settle(&mut s, index);
                assert_eq!(
                    s.earned, *source_accrued_after,
                    "record of {source}'s settlement"
                );
                s.balance -= amount;
                books.insert(*source, s);
                let mut d = books.get(destination).copied().unwrap_or_default();
                settle(&mut d, index);
                assert_eq!(
                    d.earned, *destination_accrued_after,
                    "record of {destination}'s settlement"
                );
                d.balance += amount;
                books.insert(*destination, d);
            }
        }
    }
    books
        .into_iter()
        .map(|(k, mut b)| {
            settle(&mut b, index);
            (k, b.earned)
        })
        .collect()
}

// ---- F003: settlements ----------------------------------------------------------------------

use std::collections::BTreeMap;

use anchor_lang::event::EVENT_IX_TAG_LE;
use anchor_lang::{AnchorDeserialize, Discriminator};
use dvp_settlement::events as dvp;
use fund::events as fev;

use crate::harness::TxMeta;

/// One settlement record, in ledger order.
pub enum SettlementRecord {
    Created(dvp::SettlementCreated),
    LegFunded(dvp::LegFunded),
    Settled(dvp::Settled),
    CancelRequested(dvp::CancelRequested),
    CancelRequestWithdrawn(dvp::CancelRequestWithdrawn),
    Cancelled(dvp::SettlementCancelled),
    Expired(dvp::SettlementExpired),
    Rejected(dvp::SettlementRejected),
    Returned(dvp::LegReturned),
    Minted(fev::SubscriptionMinted),
    Burned(fev::RedemptionBurned),
}

fn try_decode<T: AnchorDeserialize + Discriminator>(payload: &[u8]) -> Option<T> {
    let d = T::DISCRIMINATOR;
    (payload.len() >= d.len() && &payload[..d.len()] == d)
        .then(|| T::deserialize(&mut &payload[d.len()..]).ok())
        .flatten()
}

/// Every settlement record a transaction emitted, in the order its instructions emitted them.
pub fn settlement_records(meta: &TxMeta) -> Vec<SettlementRecord> {
    use SettlementRecord as R;
    meta.inner_instructions
        .iter()
        .flatten()
        .filter_map(|inner| inner.instruction.data.strip_prefix(EVENT_IX_TAG_LE))
        .filter_map(|p| {
            try_decode(p)
                .map(R::Created)
                .or_else(|| try_decode(p).map(R::LegFunded))
                .or_else(|| try_decode(p).map(R::Settled))
                .or_else(|| try_decode(p).map(R::CancelRequested))
                .or_else(|| try_decode(p).map(R::CancelRequestWithdrawn))
                .or_else(|| try_decode(p).map(R::Cancelled))
                .or_else(|| try_decode(p).map(R::Expired))
                .or_else(|| try_decode(p).map(R::Rejected))
                .or_else(|| try_decode(p).map(R::Returned))
                .or_else(|| try_decode(p).map(R::Minted))
                .or_else(|| try_decode(p).map(R::Burned))
        })
        .collect()
}

/// What the records say about one instruction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebuiltInstruction {
    pub kind: u8,
    pub units: u64,
    pub cash: u64,
    pub status: u8,
    pub seller_funded: bool,
    pub buyer_funded: bool,
    pub cancel_requests: u8,
    pub units_to_return: u64,
    pub cash_to_return: u64,
}

/// What the records say about every instruction and the escrow.
#[derive(Clone, Debug, Default)]
pub struct RebuiltSettlements {
    pub instructions: BTreeMap<u64, RebuiltInstruction>,
    pub escrow_units: u64,
    pub escrow_cash: u64,
    /// Units created minus units destroyed.
    pub net_units: i128,
}

/// Replays `records`. Panics if a record's own figures (earmarks, status after funding)
/// disagree with the replay.
pub fn rebuild_settlements(records: &[SettlementRecord]) -> RebuiltSettlements {
    use SettlementRecord as R;
    let mut out = RebuiltSettlements::default();
    let held = |i: &RebuiltInstruction| -> (u64, u64) {
        let units = if i.seller_funded && i.kind != 0 {
            i.units
        } else {
            0
        };
        let cash = if i.buyer_funded { i.cash } else { 0 };
        (units, cash)
    };
    for r in records {
        match r {
            R::Created(e) => {
                out.instructions.insert(
                    e.seq,
                    RebuiltInstruction {
                        kind: e.kind,
                        units: e.units,
                        cash: e.cash_amount,
                        ..Default::default()
                    },
                );
            }
            R::LegFunded(e) => {
                let i = out.instructions.get_mut(&e.seq).expect("created first");
                if e.side == 0 {
                    i.seller_funded = true;
                    out.escrow_units += e.amount;
                } else {
                    i.buyer_funded = true;
                    out.escrow_cash += e.amount;
                }
                i.status = match (i.seller_funded, i.buyer_funded) {
                    (true, true) => 3,
                    (true, false) => 1,
                    _ => 2,
                };
                assert_eq!(i.status, e.status_after, "LegFunded {} status", e.seq);
            }
            R::Settled(e) => {
                let i = out.instructions.get_mut(&e.seq).expect("created first");
                // A subscription's units were minted into escrow and delivered in the same step.
                if i.kind != 0 {
                    out.escrow_units -= i.units;
                }
                out.escrow_cash -= i.cash;
                i.status = 4;
            }
            R::CancelRequested(e) => {
                out.instructions.get_mut(&e.seq).unwrap().cancel_requests |= 1 << e.side;
            }
            R::CancelRequestWithdrawn(e) => {
                out.instructions.get_mut(&e.seq).unwrap().cancel_requests &= !(1 << e.side);
            }
            R::Cancelled(e) => end(
                &mut out,
                e.seq,
                5,
                e.units_to_return,
                e.cash_to_return,
                held,
            ),
            R::Expired(e) => end(
                &mut out,
                e.seq,
                6,
                e.units_to_return,
                e.cash_to_return,
                held,
            ),
            R::Rejected(e) => end(
                &mut out,
                e.seq,
                7,
                e.units_to_return,
                e.cash_to_return,
                held,
            ),
            R::Returned(e) => {
                let i = out.instructions.get_mut(&e.seq).unwrap();
                if e.leg == 0 {
                    i.units_to_return -= e.amount;
                    out.escrow_units -= e.amount;
                } else {
                    i.cash_to_return -= e.amount;
                    out.escrow_cash -= e.amount;
                }
            }
            R::Minted(e) => out.net_units += i128::from(e.units),
            R::Burned(e) => out.net_units -= i128::from(e.units),
        }
    }
    out
}

fn end(
    out: &mut RebuiltSettlements,
    seq: u64,
    status: u8,
    units_to_return: u64,
    cash_to_return: u64,
    held: impl Fn(&RebuiltInstruction) -> (u64, u64),
) {
    let i = out.instructions.get_mut(&seq).expect("created first");
    assert_eq!(
        held(i),
        (units_to_return, cash_to_return),
        "earmarks of {seq}"
    );
    i.status = status;
    i.units_to_return = units_to_return;
    i.cash_to_return = cash_to_return;
}
