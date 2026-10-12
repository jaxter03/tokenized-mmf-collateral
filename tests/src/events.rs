//! Event decoding (T021).
//!
//! - `emit_cpi!` events are inner instructions whose data is Anchor's event tag
//!   (`EVENT_IX_TAG_LE`, `0x1d9acb512ea545e4` little-endian), the 8-byte event discriminator, then the
//!   Borsh payload. They survive log truncation, so FR-033 records are read from here.
//! - `emit!` events are `Program data: <base64>` log lines (used by the hook's `TransferValidated`).

use anchor_lang::event::EVENT_IX_TAG_LE;
use anchor_lang::{AnchorDeserialize, Discriminator};
use base64::Engine;

use crate::harness::TxMeta;

fn decode<T: AnchorDeserialize + Discriminator>(payload: &[u8]) -> Option<T> {
    let disc = T::DISCRIMINATOR;
    if payload.len() < disc.len() || &payload[..disc.len()] != disc {
        return None;
    }
    T::deserialize(&mut &payload[disc.len()..]).ok()
}

/// All `emit_cpi!` events of type `T` in a transaction, in order.
pub fn cpi_events<T: AnchorDeserialize + Discriminator>(meta: &TxMeta) -> Vec<T> {
    meta.inner_instructions
        .iter()
        .flatten()
        .filter_map(|inner| {
            let data = &inner.instruction.data;
            data.strip_prefix(EVENT_IX_TAG_LE).and_then(decode::<T>)
        })
        .collect()
}

/// All `emit!` events of type `T` in a transaction's logs, in order.
pub fn log_events<T: AnchorDeserialize + Discriminator>(meta: &TxMeta) -> Vec<T> {
    meta.logs
        .iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .filter_map(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
        .filter_map(|bytes| decode::<T>(&bytes))
        .collect()
}

/// Exactly one `emit_cpi!` event of type `T`; panics otherwise.
#[track_caller]
pub fn one_cpi_event<T: AnchorDeserialize + Discriminator>(meta: &TxMeta) -> T {
    let mut all = cpi_events::<T>(meta);
    assert_eq!(
        all.len(),
        1,
        "expected exactly one {} event",
        core::any::type_name::<T>()
    );
    all.remove(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::AnchorSerialize;
    use compliance_hook::events::TransferValidated;
    use solana_message::compiled_instruction::CompiledInstruction;
    use solana_message::inner_instruction::InnerInstruction;

    fn sample() -> TransferValidated {
        TransferValidated {
            fund: Default::default(),
            source_owner: Default::default(),
            destination_owner: Default::default(),
            amount: 42,
            holder_count: 3,
            slot: 7,
            unix_ts: 9,
        }
    }

    fn payload() -> Vec<u8> {
        let mut p = TransferValidated::DISCRIMINATOR.to_vec();
        sample().serialize(&mut p).unwrap();
        p
    }

    #[test]
    fn decodes_log_events() {
        let meta = TxMeta {
            logs: vec![
                "Program log: hello".into(),
                format!(
                    "Program data: {}",
                    base64::engine::general_purpose::STANDARD.encode(payload())
                ),
                "Program data: AAAA".into(),
            ],
            ..Default::default()
        };
        let evs = log_events::<TransferValidated>(&meta);
        assert_eq!(evs.len(), 1);
        assert_eq!((evs[0].amount, evs[0].holder_count), (42, 3));
    }

    #[test]
    fn decodes_cpi_events() {
        let mut data = EVENT_IX_TAG_LE.to_vec();
        data.extend(payload());
        let inner = |data: Vec<u8>| InnerInstruction {
            instruction: CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data,
            },
            stack_height: 2,
        };
        let meta = TxMeta {
            inner_instructions: vec![vec![inner(vec![1, 2, 3]), inner(data)]],
            ..Default::default()
        };
        let ev = one_cpi_event::<TransferValidated>(&meta);
        assert_eq!(ev.slot, 7);
    }
}
