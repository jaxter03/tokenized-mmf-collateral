//! KYC statement verification (contracts/kyc-statement.md, research R6).
//!
//! The Ed25519 precompile has already checked the signature (the transaction fails otherwise).
//! This module makes sure the verified bytes really are a 95-byte statement in the instruction
//! just before ours, signed by a recognised provider, for this fund and this owner, then checks
//! the content. Checks run in the contract's order; the first failure is returned.

use anchor_lang::prelude::*;
use mmf_common::ids::ED25519_PROGRAM_ID;
use mmf_common::jurisdiction;
use mmf_common::statement::{self, Statement};
use solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked};

use crate::errors::HookError;
use crate::state::{HolderState, Registry};

/// Ed25519 instruction layout: `[num_signatures u8, padding u8]`, then 14-byte offset blocks.
const HEADER_LEN: usize = 2;
const OFFSETS_LEN: usize = 14;
const PUBKEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;
/// "data lives in this same instruction".
const SAME_INSTRUCTION: u16 = u16::MAX;

/// What a verified statement establishes.
pub struct Verified {
    pub statement: Statement,
    pub provider: Pubkey,
    pub attestation_hash: [u8; 32],
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn slice(data: &[u8], offset: u16, len: usize) -> Result<&[u8]> {
    let start = offset as usize;
    let end = start
        .checked_add(len)
        .ok_or(error!(HookError::StatementMalformed))?;
    data.get(start..end)
        .ok_or(error!(HookError::StatementMalformed))
}

/// Checks 1–12. `renewal` adds the later-expiry rule (check 11).
pub fn verify(
    instructions: &AccountInfo,
    registry: &Registry,
    holder: &HolderState,
    attestation_hash: &[u8; 32],
    now: i64,
    renewal: bool,
) -> Result<Verified> {
    // 1. The real Instructions sysvar, and an Ed25519 instruction immediately before ours.
    require_keys_eq!(
        instructions.key(),
        solana_instructions_sysvar::ID,
        HookError::StatementMissing
    );
    let current = load_current_index_checked(instructions)
        .map_err(|_| error!(HookError::StatementMissing))?;
    let previous = current
        .checked_sub(1)
        .ok_or(error!(HookError::StatementMissing))?;
    let ix = load_instruction_at_checked(previous as usize, instructions)
        .map_err(|_| error!(HookError::StatementMissing))?;
    require_keys_eq!(
        ix.program_id,
        ED25519_PROGRAM_ID,
        HookError::StatementMissing
    );

    // 2. Exactly one signature, all data inside this instruction, a 95-byte message.
    let data = ix.data.as_slice();
    require!(
        data.len() >= HEADER_LEN + OFFSETS_LEN,
        HookError::StatementMalformed
    );
    require!(data[0] == 1, HookError::StatementMalformed);
    let o = HEADER_LEN;
    let (sig_off, sig_ix) = (u16_at(data, o), u16_at(data, o + 2));
    let (pk_off, pk_ix) = (u16_at(data, o + 4), u16_at(data, o + 6));
    let (msg_off, msg_len, msg_ix) = (
        u16_at(data, o + 8),
        u16_at(data, o + 10),
        u16_at(data, o + 12),
    );
    require!(
        sig_ix == SAME_INSTRUCTION && pk_ix == SAME_INSTRUCTION && msg_ix == SAME_INSTRUCTION,
        HookError::StatementMalformed
    );
    slice(data, sig_off, SIGNATURE_LEN)?;
    let pubkey = Pubkey::new_from_array(
        slice(data, pk_off, PUBKEY_LEN)?
            .try_into()
            .expect("32 bytes"),
    );
    require!(
        msg_len as usize == statement::LEN,
        HookError::StatementMalformed
    );
    let message = slice(data, msg_off, statement::LEN)?;

    // 3. Domain tag (and length, again).
    let s = Statement::parse(message).map_err(|_| error!(HookError::StatementMalformed))?;

    // 4. Signed by a recognised provider.
    require!(
        registry.is_provider(&pubkey),
        HookError::ProviderNotRecognised
    );
    // 5. For this fund.
    require_keys_eq!(s.mint, registry.mint, HookError::StatementWrongFund);
    // 6. For this owner account (Clarification Q2).
    require_keys_eq!(s.owner, holder.owner, HookError::StatementOwnerMismatch);
    // 7. Not expired: expiry must be strictly in the future (FR-025).
    require!(s.expiry_ts > now, HookError::StatementExpired);
    // 8. An assigned ISO 3166-1 code.
    require!(
        jurisdiction::is_assigned(s.jurisdiction),
        HookError::UnknownJurisdiction
    );
    // 9. Allowed by the fund.
    require!(
        registry.jurisdiction_allowed(s.jurisdiction),
        HookError::JurisdictionNotAllowed
    );
    // 10. A real investor type.
    require!(s.investor_type != 0, HookError::InvalidInvestorType);
    // 11. Renewal only: later than what's on record (research R14).
    if renewal {
        require!(
            s.expiry_ts > holder.kyc_expiry_ts,
            HookError::RenewalNotNewer
        );
    }
    // 12. The exact statement compliance approved (research R8).
    let hash = statement::fingerprint(message);
    require!(&hash == attestation_hash, HookError::StatementHashMismatch);

    Ok(Verified {
        statement: s,
        provider: pubkey,
        attestation_hash: hash,
    })
}
