//! Mock KYC provider (spec Assumptions: real providers are mocked). It signs the same 95-byte
//! statement a real provider would (contracts/kyc-statement.md) and builds the Ed25519 verify
//! instruction that must sit immediately before `approve_investor` / `renew_investor`.
//!
//! `tamper` builds the malformed variants the program must reject (research R6).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use mmf_common::ids::ED25519_PROGRAM_ID;
use mmf_common::statement::{self, Statement};
use solana_keypair::Keypair;
use solana_signer::Signer;

/// A signed statement, ready to submit.
#[derive(Clone, Debug)]
pub struct SignedStatement {
    pub message: Vec<u8>,
    /// Ed25519 verify instruction over `message` with all offsets inside itself.
    pub ix: Instruction,
    /// `SHA-256(message)`: the `attestation_hash` argument of approve / renew.
    pub attestation_hash: [u8; 32],
}

/// The statement fields for `owner` in fund `mint`.
pub fn statement(
    mint: Pubkey,
    owner: Pubkey,
    jurisdiction: [u8; 2],
    investor_type: u8,
    expiry_ts: i64,
) -> Statement {
    Statement {
        mint,
        owner,
        jurisdiction,
        investor_type,
        expiry_ts,
    }
}

/// Signs `stmt` with the provider key.
pub fn sign(provider: &Keypair, stmt: &Statement) -> SignedStatement {
    sign_bytes(provider, &stmt.encode())
}

/// Signs arbitrary bytes (for wrong-length or wrong-domain tests).
pub fn sign_bytes(provider: &Keypair, message: &[u8]) -> SignedStatement {
    let signature: [u8; 64] = provider.sign_message(message).into();
    let pubkey: [u8; 32] = provider.pubkey().to_bytes();
    let ix = solana_ed25519_program::new_ed25519_instruction_with_signature(
        message, &signature, &pubkey,
    );
    SignedStatement {
        message: message.to_vec(),
        ix,
        attestation_hash: statement::fingerprint(message),
    }
}

/// Malformed Ed25519 instructions (each must be rejected as `StatementMalformed`).
pub mod tamper {
    use super::*;

    /// Header: `[num_signatures, padding]`, then one 14-byte offsets block per signature.
    const HEADER: usize = 2;
    const OFFSETS_LEN: usize = 14;

    /// The same signature listed twice (`num_signatures = 2`). Still a valid precompile call.
    pub fn two_signatures(signed: &SignedStatement) -> Instruction {
        let d = &signed.ix.data;
        let offsets = &d[HEADER..HEADER + OFFSETS_LEN];
        let shift = OFFSETS_LEN as u16;
        // Rebuild: header(2) + offsets×2 + payload, with every data offset moved by one block.
        let payload = &d[HEADER + OFFSETS_LEN..];
        let mut block = offsets.to_vec();
        for field in [0usize, 4, 8] {
            let v = u16::from_le_bytes([block[field], block[field + 1]]) + shift;
            block[field..field + 2].copy_from_slice(&v.to_le_bytes());
        }
        let mut data = vec![2u8, 0];
        data.extend_from_slice(&block);
        data.extend_from_slice(&block);
        data.extend_from_slice(payload);
        Instruction {
            program_id: ED25519_PROGRAM_ID,
            accounts: vec![],
            data,
        }
    }

    /// `num_signatures = 0` with no payload: the precompile accepts it trivially.
    pub fn zero_signatures() -> Instruction {
        Instruction {
            program_id: ED25519_PROGRAM_ID,
            accounts: vec![],
            data: vec![0, 0],
        }
    }

    /// An Ed25519 instruction whose signature, pubkey and message all live in the instruction at
    /// `carrier_index` (a valid, self-contained verify instruction). The precompile accepts it;
    /// the program must not, because the verified bytes aren't in the instruction it inspects.
    pub fn referencing(carrier_index: u16, carrier: &SignedStatement) -> Instruction {
        let mut data = carrier.ix.data[..HEADER + OFFSETS_LEN].to_vec();
        for field in [2usize, 6, 12] {
            // signature_instruction_index, public_key_instruction_index, message_instruction_index
            data[HEADER + field..HEADER + field + 2].copy_from_slice(&carrier_index.to_le_bytes());
        }
        Instruction {
            program_id: ED25519_PROGRAM_ID,
            accounts: vec![],
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_statement_layout() {
        let provider = Keypair::new();
        let s = statement(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            *b"US",
            1,
            1_900_000_000,
        );
        let signed = sign(&provider, &s);
        assert_eq!(signed.message.len(), 95);
        assert_eq!(Statement::parse(&signed.message).unwrap(), s);
        assert_eq!(signed.ix.program_id, ED25519_PROGRAM_ID);
        assert_eq!(signed.ix.data[0], 1, "one signature");
        assert_eq!(
            signed.attestation_hash,
            statement::fingerprint(&signed.message)
        );
        // All three instruction indices are u16::MAX (data inside the same instruction).
        for field in [4usize, 8, 14] {
            assert_eq!(&signed.ix.data[field..field + 2], &u16::MAX.to_le_bytes());
        }
    }

    #[test]
    fn tamper_variants() {
        let provider = Keypair::new();
        let s = statement(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            *b"US",
            1,
            1_900_000_000,
        );
        let signed = sign(&provider, &s);
        assert_eq!(tamper::two_signatures(&signed).data[0], 2);
        assert_eq!(tamper::zero_signatures().data, vec![0, 0]);
        let r = tamper::referencing(0, &signed);
        assert_eq!(r.data.len(), 16);
        assert_eq!(&r.data[4..6], &0u16.to_le_bytes());
    }
}
