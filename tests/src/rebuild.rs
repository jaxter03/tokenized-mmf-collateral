//! An auditor's rebuild of every holder's earned interest from the records alone (F002 FR-021,
//! SC-007): the permanent NavRecords and the transfers' `TransferValidated` records. It doesn't
//! read any HolderState and doesn't use `mmf_common::accrual`.

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
