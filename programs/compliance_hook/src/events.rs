//! Records (FR-033). Admin and onboarding events are emitted with `emit_cpi!`; the hook's
//! `TransferValidated` uses `emit!` (research R7). Every payload carries `fund`, `slot`, `unix_ts`.

use anchor_lang::prelude::*;

#[event]
pub struct RegistryInitialized {
    pub fund: Pubkey,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct RegisterRulesSet {
    pub fund: Pubkey,
    pub kyc_providers: [Pubkey; 4],
    pub allowed_jurisdictions: [u64; 11],
    pub max_holders: u32,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct HolderRegistered {
    pub fund: Pubkey,
    pub owner: Pubkey,
    pub class: u8,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct HolderApproved {
    pub fund: Pubkey,
    pub owner: Pubkey,
    pub class: u8,
    pub jurisdiction: [u8; 2],
    pub investor_type: u8,
    pub kyc_expiry_ts: i64,
    pub attestation_hash: [u8; 32],
    pub attested_by: Pubkey,
    pub renewal: bool,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct HolderStatusChanged {
    pub fund: Pubkey,
    pub owner: Pubkey,
    pub from: u8,
    pub to: u8,
    pub reason: u8,
    pub case_ref_hash: [u8; 32],
    pub actor: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct TransfersPaused {
    pub fund: Pubkey,
    pub paused: bool,
    pub reason: u8,
    pub case_ref_hash: [u8; 32],
    pub actor: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct HoldingAccountThawed {
    pub fund: Pubkey,
    pub owner: Pubkey,
    pub token_account: Pubkey,
    pub signer: Pubkey,
    pub slot: u64,
    pub unix_ts: i64,
}

#[event]
pub struct TransferValidated {
    pub fund: Pubkey,
    pub source_owner: Pubkey,
    pub destination_owner: Pubkey,
    pub amount: u64,
    pub holder_count: u32,
    pub slot: u64,
    pub unix_ts: i64,
}
