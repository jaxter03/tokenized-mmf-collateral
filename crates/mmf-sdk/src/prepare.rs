//! Prepared steps for slow signers (research R13, US5). A custodian's approval can take hours,
//! while an ordinary transaction expires with its recent blockhash in about a minute. A message
//! built on the signer's own durable nonce stays valid until the nonce is advanced, which this
//! message does as its first instruction, so a signed step can be submitted at most once. The
//! programs judge the step only on account state and the clock, so it succeeds as long as the
//! settlement instruction is still open and not expired.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use solana_hash::Hash;
use solana_message::Message;

/// An unsigned message: `AdvanceNonceAccount(nonce_account, nonce_authority)` first, then `ixs`,
/// with `nonce_value` (the nonce account's current value) as its blockhash.
pub fn with_durable_nonce(
    ixs: &[Instruction],
    nonce_account: &Pubkey,
    nonce_authority: &Pubkey,
    payer: &Pubkey,
    nonce_value: Hash,
) -> Message {
    let mut message =
        Message::new_with_nonce(ixs.to_vec(), Some(payer), nonce_account, nonce_authority);
    message.recent_blockhash = nonce_value;
    message
}

/// Layout of a system nonce account (bincode `nonce::state::Versions`): a `u32` version tag
/// (0 Legacy, 1 Current), a `u32` state tag (0 Uninitialized, 1 Initialized), then the data:
/// authority (32 bytes), durable nonce (32 bytes), fee calculator (8 bytes).
const STATE_TAG: core::ops::Range<usize> = 4..8;
const DURABLE_NONCE: core::ops::Range<usize> = 40..72;

/// The current value of a nonce account, from its data; `None` if it isn't an initialized nonce
/// account.
pub fn read_nonce(account_data: &[u8]) -> Option<Hash> {
    let version = u32::from_le_bytes(account_data.get(0..4)?.try_into().ok()?);
    let state = u32::from_le_bytes(account_data.get(STATE_TAG)?.try_into().ok()?);
    if version > 1 || state != 1 {
        return None;
    }
    let bytes: [u8; 32] = account_data.get(DURABLE_NONCE)?.try_into().ok()?;
    Some(Hash::new_from_array(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_nonce_comes_first_and_the_nonce_is_the_blockhash() {
        let (nonce, authority, payer) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let ix = Instruction::new_with_bytes(Pubkey::new_unique(), &[7], vec![]);
        let value = Hash::new_from_array([9; 32]);
        let m = with_durable_nonce(&[ix], &nonce, &authority, &payer, value);
        assert_eq!(m.recent_blockhash, value);
        assert_eq!(m.instructions.len(), 2);
        let first = &m.instructions[0];
        assert_eq!(
            m.account_keys[first.program_id_index as usize],
            anchor_lang::solana_program::system_program::ID,
            "AdvanceNonceAccount first"
        );
        assert_eq!(m.account_keys[0], payer, "payer pays");
    }

    #[test]
    fn reads_only_initialized_nonce_accounts() {
        let mut data = vec![0u8; 80];
        data[0..4].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(read_nonce(&data), None, "uninitialized");
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        data[40..72].copy_from_slice(&[5; 32]);
        assert_eq!(read_nonce(&data), Some(Hash::new_from_array([5; 32])));
        assert_eq!(read_nonce(&data[..50]), None, "short");
    }
}
