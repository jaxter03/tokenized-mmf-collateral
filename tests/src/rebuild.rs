//! An auditor's rebuilds from the records alone:
//! - every holder's earned interest (F002 FR-021, SC-007): the permanent NavRecords and the
//!   transfers' `TransferValidated` records; it doesn't read any HolderState and doesn't use
//!   `mmf_common::accrual`;
//! - every settlement instruction's history and the escrow's movements (F003 FR-031, SC-008):
//!   from `dvp_settlement`'s and `fund`'s settlement events; it doesn't read any
//!   SettlementInstruction;
//! - every collateral agreement, call and pledge, and every holder's locked units (F004 FR-037,
//!   SC-009): from `collateral`'s events and the hook's `LienSet` records; it doesn't read any
//!   collateral account or HolderState.

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

// ---- F004: collateral ------------------------------------------------------------------------

use collateral::events as col;
use compliance_hook::events::LienSet;

/// One collateral record, in ledger order.
pub enum CollateralRecord {
    Proposed(col::AgreementProposed),
    Active(col::AgreementActive),
    Exposure(col::ExposureUpdated),
    CallIssued(col::CallIssued),
    Checked(col::CoverageChecked),
    CallCancelled(col::CallCancelled),
    Disputed(col::CallDisputed),
    Withdrawn(col::DisputeWithdrawn),
    Resolved(col::DisputeResolved),
    Pledged(col::Pledged),
    Released(col::Released),
    Substituted(col::Substituted),
    Lien(LienSet),
}

/// Every collateral record a transaction emitted: `collateral`'s `emit_cpi!` events in order,
/// then the hook's `LienSet` log records (they only report locks, so their position within the
/// transaction doesn't matter).
pub fn collateral_records(meta: &TxMeta) -> Vec<CollateralRecord> {
    use CollateralRecord as R;
    let mut out: Vec<R> = meta
        .inner_instructions
        .iter()
        .flatten()
        .filter_map(|inner| inner.instruction.data.strip_prefix(EVENT_IX_TAG_LE))
        .filter_map(|p| {
            try_decode(p)
                .map(R::Proposed)
                .or_else(|| try_decode(p).map(R::Active))
                .or_else(|| try_decode(p).map(R::Exposure))
                .or_else(|| try_decode(p).map(R::CallIssued))
                .or_else(|| try_decode(p).map(R::Checked))
                .or_else(|| try_decode(p).map(R::CallCancelled))
                .or_else(|| try_decode(p).map(R::Disputed))
                .or_else(|| try_decode(p).map(R::Withdrawn))
                .or_else(|| try_decode(p).map(R::Resolved))
                .or_else(|| try_decode(p).map(R::Pledged))
                .or_else(|| try_decode(p).map(R::Released))
                .or_else(|| try_decode(p).map(R::Substituted))
        })
        .collect();
    out.extend(
        crate::events::log_events::<LienSet>(meta)
            .into_iter()
            .map(R::Lien),
    );
    out
}

/// What the records say about one agreement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebuiltAgreement {
    pub giver: Pubkey,
    pub status: u8,
    pub pledged_units: u64,
    pub required_value: u64,
    pub called_outstanding: u64,
    pub open_delivery_calls: u16,
    pub met_below_seq: u64,
    pub open_return_call: Pubkey,
}

/// What the records say about one call. `status` is the derived status (Met below the
/// agreement's `met_below_seq` for delivery calls).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebuiltCall {
    pub agreement: Pubkey,
    pub seq: u64,
    pub kind: u8,
    pub value_delta: u64,
    pub status: u8,
}

/// What the records say about one pledge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebuiltPledge {
    pub agreement: Pubkey,
    pub remaining: u64,
    /// `PledgeStatus`: 0 Active, 1 Released, 3 Substituted.
    pub status: u8,
}

#[derive(Clone, Debug, Default)]
pub struct RebuiltCollateral {
    pub agreements: BTreeMap<Pubkey, RebuiltAgreement>,
    pub calls: BTreeMap<Pubkey, RebuiltCall>,
    pub pledges: BTreeMap<Pubkey, RebuiltPledge>,
    /// Each giver's locked units: Σ remaining over its Active pledges.
    pub locks: BTreeMap<Pubkey, u64>,
    /// The latest locked units the hook reported per owner (cross-check of `locks`).
    pub reported_locks: BTreeMap<Pubkey, u64>,
}

/// Replays `records`. Panics on a record about an unknown agreement, call or pledge.
pub fn rebuild_collateral(records: &[CollateralRecord]) -> RebuiltCollateral {
    use CollateralRecord as R;
    const OPEN: u8 = 0;
    const MET: u8 = 1;
    const DISPUTED: u8 = 2;
    const CANCELLED: u8 = 5;
    let mut out = RebuiltCollateral::default();
    fn snap(a: &mut RebuiltAgreement, s: &col::Snapshot) {
        a.pledged_units = s.pledged_units;
        a.required_value = s.required_value;
        a.called_outstanding = s.called_outstanding;
        a.open_delivery_calls = s.open_delivery_calls;
        a.met_below_seq = s.met_below_seq;
    }
    fn open_call(out: &mut RebuiltCollateral, agreement: Pubkey, c: &col::CallOpened) {
        out.calls.insert(
            c.call,
            RebuiltCall {
                agreement,
                seq: c.seq,
                kind: c.kind,
                value_delta: c.value_delta,
                status: OPEN,
            },
        );
        if c.kind == 1 {
            out.agreements
                .get_mut(&agreement)
                .expect("agreement")
                .open_return_call = c.call;
        }
    }
    for r in records {
        match r {
            R::Proposed(e) => {
                out.agreements.insert(
                    e.agreement,
                    RebuiltAgreement {
                        giver: e.args.giver,
                        ..Default::default()
                    },
                );
            }
            R::Active(e) => {
                out.agreements
                    .get_mut(&e.agreement)
                    .expect("agreement")
                    .status = 1
            }
            R::Exposure(e) => {
                snap(
                    out.agreements.get_mut(&e.agreement).expect("agreement"),
                    &e.snapshot,
                );
                if let Some(c) = &e.opened_call {
                    open_call(&mut out, e.agreement, c);
                }
            }
            R::CallIssued(e) => {
                snap(
                    out.agreements.get_mut(&e.agreement).expect("agreement"),
                    &e.snapshot,
                );
                open_call(&mut out, e.agreement, &e.call);
            }
            R::Checked(e) => snap(
                out.agreements.get_mut(&e.agreement).expect("agreement"),
                &e.snapshot,
            ),
            R::CallCancelled(e) => {
                let a = out.agreements.get_mut(&e.agreement).expect("agreement");
                a.called_outstanding = e.called_outstanding;
                a.open_delivery_calls = e.open_delivery_calls;
                if e.kind == 1 {
                    a.open_return_call = Pubkey::default();
                }
                out.calls.get_mut(&e.call).expect("call").status = CANCELLED;
            }
            R::Disputed(e) => out.calls.get_mut(&e.call).expect("call").status = DISPUTED,
            R::Withdrawn(e) => out.calls.get_mut(&e.call).expect("call").status = OPEN,
            R::Resolved(e) => {
                let a = out.agreements.get_mut(&e.agreement).expect("agreement");
                a.called_outstanding = e.called_outstanding;
                a.open_delivery_calls = e.open_delivery_calls;
                out.calls.get_mut(&e.call).expect("call").status = e.outcome;
            }
            R::Pledged(e) => {
                snap(
                    out.agreements.get_mut(&e.agreement).expect("agreement"),
                    &e.snapshot,
                );
                out.pledges.insert(
                    e.pledge,
                    RebuiltPledge {
                        agreement: e.agreement,
                        remaining: e.units,
                        status: 0,
                    },
                );
            }
            R::Released(e) => {
                snap(
                    out.agreements.get_mut(&e.agreement).expect("agreement"),
                    &e.snapshot,
                );
                let p = out.pledges.get_mut(&e.pledge).expect("pledge");
                p.remaining = e.pledge_remaining;
                if p.remaining == 0 {
                    p.status = 1;
                }
                if e.return_call_met {
                    let c = e.return_call.expect("named return call");
                    out.calls.get_mut(&c).expect("call").status = MET;
                    out.agreements
                        .get_mut(&e.agreement)
                        .unwrap()
                        .open_return_call = Pubkey::default();
                }
            }
            R::Substituted(e) => {
                snap(
                    out.agreements.get_mut(&e.agreement).expect("agreement"),
                    &e.snapshot,
                );
                let old = out.pledges.get_mut(&e.old_pledge).expect("pledge");
                old.remaining = 0;
                old.status = 3;
                out.pledges.insert(
                    e.new_pledge,
                    RebuiltPledge {
                        agreement: e.agreement,
                        remaining: e.new_units,
                        status: 0,
                    },
                );
            }
            R::Lien(e) => {
                out.reported_locks.insert(e.owner, e.pledged_amount);
            }
        }
    }
    // Derived status: unresolved delivery calls below their agreement's `met_below_seq` are Met.
    for c in out.calls.values_mut() {
        let a = &out.agreements[&c.agreement];
        if c.kind == 0 && (c.status == OPEN || c.status == DISPUTED) && c.seq < a.met_below_seq {
            c.status = MET;
        }
    }
    // Locks from the pledges alone.
    for p in out.pledges.values().filter(|p| p.status == 0) {
        let giver = out.agreements[&p.agreement].giver;
        *out.locks.entry(giver).or_default() += p.remaining;
    }
    out
}
