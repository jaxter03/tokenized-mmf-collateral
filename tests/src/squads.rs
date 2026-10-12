//! Squads v4 multisig, hand-encoded (T070, research R8). The published `squads-multisig` crate
//! targets Solana 1.17 / Anchor 0.29, so the five instructions the tests need are built here from
//! the Squads v4 source: an Anchor discriminator `sha256("global:<name>")[..8]` followed by Borsh
//! arguments. Runs against the mainnet Squads binary in `tests/fixtures/`.
//!
//! Flow: `multisig_create_v2` → `vault_transaction_create` (compact message) → `proposal_create`
//! → `proposal_approve` × N → `vault_transaction_execute`. Executing below threshold fails with
//! Squads' `InvalidProposalStatus` (6008).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::{system_program, AnchorSerialize};
use mmf_common::ids::SQUADS_PROGRAM_ID;
use solana_keypair::Keypair;
use solana_signer::Signer;

use crate::harness::{Harness, Ix, TxResult, SQUADS_PROGRAM_CONFIG};

/// Squads `InvalidProposalStatus`: execute while the proposal isn't Approved.
pub const INVALID_PROPOSAL_STATUS: u32 = 6008;

const PREFIX: &[u8] = b"multisig";
/// Initiate | Vote | Execute.
const ALL_PERMISSIONS: u8 = 0b111;
/// `ProgramConfig.treasury` offset: discriminator 8 + authority 32 + creation fee 8.
const CONFIG_TREASURY_OFFSET: usize = 48;

fn discriminator(name: &str) -> [u8; 8] {
    let h = mmf_common::statement::fingerprint(format!("global:{name}").as_bytes());
    h[..8].try_into().unwrap()
}

fn ix(name: &str, args: impl AnchorSerialize, accounts: Vec<AccountMeta>) -> Ix {
    let mut data = discriminator(name).to_vec();
    args.serialize(&mut data).unwrap();
    Ix {
        program_id: SQUADS_PROGRAM_ID,
        accounts,
        data,
    }
}

#[derive(AnchorSerialize)]
struct Member {
    key: Pubkey,
    permissions: u8,
}

#[derive(AnchorSerialize)]
struct MultisigCreateArgsV2 {
    config_authority: Option<Pubkey>,
    threshold: u16,
    members: Vec<Member>,
    time_lock: u32,
    rent_collector: Option<Pubkey>,
    memo: Option<String>,
}

#[derive(AnchorSerialize)]
struct VaultTransactionCreateArgs {
    vault_index: u8,
    ephemeral_signers: u8,
    transaction_message: Vec<u8>,
    memo: Option<String>,
}

#[derive(AnchorSerialize)]
struct ProposalCreateArgs {
    transaction_index: u64,
    draft: bool,
}

#[derive(AnchorSerialize)]
struct ProposalVoteArgs {
    memo: Option<String>,
}

/// A Squads multisig whose vault (index 0) holds a role.
pub struct Multisig {
    pub address: Pubkey,
    pub vault: Pubkey,
    pub members: Vec<Keypair>,
    pub threshold: u16,
    next_index: u64,
}

pub fn multisig_pda(create_key: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[PREFIX, b"multisig", create_key.as_ref()],
        &SQUADS_PROGRAM_ID,
    )
    .0
}

pub fn vault_pda(multisig: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[PREFIX, multisig.as_ref(), b"vault", &[0u8]],
        &SQUADS_PROGRAM_ID,
    )
    .0
}

fn transaction_pda(multisig: &Pubkey, index: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[
            PREFIX,
            multisig.as_ref(),
            b"transaction",
            &index.to_le_bytes(),
        ],
        &SQUADS_PROGRAM_ID,
    )
    .0
}

fn proposal_pda(multisig: &Pubkey, index: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[
            PREFIX,
            multisig.as_ref(),
            b"transaction",
            &index.to_le_bytes(),
            b"proposal",
        ],
        &SQUADS_PROGRAM_ID,
    )
    .0
}

/// Creates a `threshold`-of-`n` multisig, no time lock, every member with all permissions.
pub fn create(h: &mut Harness, threshold: u16, n: usize) -> Multisig {
    let members: Vec<Keypair> = (0..n).map(|_| h.funded_keypair()).collect();
    let create_key = Keypair::new();
    let address = multisig_pda(&create_key.pubkey());
    let config = h.data(&SQUADS_PROGRAM_CONFIG);
    let treasury = Pubkey::new_from_array(
        config[CONFIG_TREASURY_OFFSET..CONFIG_TREASURY_OFFSET + 32]
            .try_into()
            .unwrap(),
    );
    let args = MultisigCreateArgsV2 {
        config_authority: None,
        threshold,
        members: members
            .iter()
            .map(|m| Member {
                key: m.pubkey(),
                permissions: ALL_PERMISSIONS,
            })
            .collect(),
        time_lock: 0,
        rent_collector: None,
        memo: None,
    };
    let payer = h.payer.pubkey();
    let accounts = vec![
        AccountMeta::new_readonly(SQUADS_PROGRAM_CONFIG, false),
        AccountMeta::new(treasury, false),
        AccountMeta::new(address, false),
        AccountMeta::new_readonly(create_key.pubkey(), true),
        AccountMeta::new(payer, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    h.send(&[ix("multisig_create_v2", args, accounts)], &[&create_key])
        .expect("multisig_create_v2");
    let vault = vault_pda(&address);
    h.airdrop(&vault, 10_000_000_000); // the vault pays rent for anything its instructions create
    Multisig {
        address,
        vault,
        members,
        threshold,
        next_index: 1,
    }
}

/// Squads' compact transaction message for `ixs` executed by `vault` (account categories in
/// order: writable signers, readonly signers, writable non-signers, readonly non-signers).
fn compile_message(vault: &Pubkey, ixs: &[Ix]) -> (Vec<u8>, Vec<AccountMeta>) {
    // (key, signer, writable)
    let mut keys: Vec<(Pubkey, bool, bool)> = vec![(*vault, true, true)];
    let mut upsert = |k: Pubkey, signer: bool, writable: bool| {
        if let Some(e) = keys.iter_mut().find(|e| e.0 == k) {
            e.1 |= signer;
            e.2 |= writable;
        } else {
            keys.push((k, signer, writable));
        }
    };
    for i in ixs {
        for m in &i.accounts {
            upsert(m.pubkey, m.is_signer, m.is_writable);
        }
    }
    for i in ixs {
        upsert(i.program_id, false, false);
    }
    let rank = |e: &(Pubkey, bool, bool)| match (e.1, e.2) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    };
    let vault_entry = keys.remove(0);
    keys.sort_by_key(rank); // stable: keeps first-seen order inside each category
    keys.insert(0, vault_entry);

    let index_of = |k: &Pubkey| keys.iter().position(|e| e.0 == *k).unwrap() as u8;
    let mut m = vec![
        keys.iter().filter(|e| e.1).count() as u8,
        keys.iter().filter(|e| e.1 && e.2).count() as u8,
        keys.iter().filter(|e| !e.1 && e.2).count() as u8,
        keys.len() as u8,
    ];
    for e in &keys {
        m.extend_from_slice(e.0.as_ref());
    }
    m.push(ixs.len() as u8);
    for i in ixs {
        m.push(index_of(&i.program_id));
        m.push(i.accounts.len() as u8);
        m.extend(i.accounts.iter().map(|a| index_of(&a.pubkey)));
        m.extend_from_slice(&(i.data.len() as u16).to_le_bytes());
        m.extend_from_slice(&i.data);
    }
    m.push(0); // no address table lookups

    // Remaining accounts for execute: the message's keys in order, with its writability; no
    // signatures at transaction level (the vault signs by PDA seeds inside Squads).
    let remaining = keys
        .iter()
        .map(|e| {
            if e.2 {
                AccountMeta::new(e.0, false)
            } else {
                AccountMeta::new_readonly(e.0, false)
            }
        })
        .collect();
    (m, remaining)
}

impl Multisig {
    /// Creates the vault transaction and its proposal (by member 0). Returns its index and the
    /// accounts `execute` will need.
    pub fn propose(&mut self, h: &mut Harness, ixs: &[Ix]) -> (u64, Vec<AccountMeta>) {
        let index = self.next_index;
        self.next_index += 1;
        let (message, remaining) = compile_message(&self.vault, ixs);
        let creator = self.members[0].pubkey();
        let payer = h.payer.pubkey();
        let create = ix(
            "vault_transaction_create",
            VaultTransactionCreateArgs {
                vault_index: 0,
                ephemeral_signers: 0,
                transaction_message: message,
                memo: None,
            },
            vec![
                AccountMeta::new(self.address, false),
                AccountMeta::new(transaction_pda(&self.address, index), false),
                AccountMeta::new_readonly(creator, true),
                AccountMeta::new(payer, true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
        );
        let proposal = ix(
            "proposal_create",
            ProposalCreateArgs {
                transaction_index: index,
                draft: false,
            },
            vec![
                AccountMeta::new_readonly(self.address, false),
                AccountMeta::new(proposal_pda(&self.address, index), false),
                AccountMeta::new_readonly(creator, true),
                AccountMeta::new(payer, true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
        );
        let m0 = self.members[0].insecure_clone();
        h.send(&[create, proposal], &[&m0])
            .expect("vault_transaction_create + proposal_create");
        (index, remaining)
    }

    pub fn approve(&self, h: &mut Harness, index: u64, member: usize) -> TxResult {
        let m = &self.members[member];
        let approve = ix(
            "proposal_approve",
            ProposalVoteArgs { memo: None },
            vec![
                AccountMeta::new_readonly(self.address, false),
                AccountMeta::new(m.pubkey(), true),
                AccountMeta::new(proposal_pda(&self.address, index), false),
            ],
        );
        h.send(&[approve], &[m])
    }

    /// `vault_transaction_execute` by member 0, preceded by `before` top-level instructions
    /// (e.g. an Ed25519 statement, which can't be called through a CPI).
    pub fn execute(
        &self,
        h: &mut Harness,
        index: u64,
        remaining: &[AccountMeta],
        before: &[Ix],
    ) -> TxResult {
        let m = &self.members[0];
        let mut accounts = vec![
            AccountMeta::new_readonly(self.address, false),
            AccountMeta::new(proposal_pda(&self.address, index), false),
            AccountMeta::new_readonly(transaction_pda(&self.address, index), false),
            AccountMeta::new_readonly(m.pubkey(), true),
        ];
        accounts.extend_from_slice(remaining);
        let mut ixs = before.to_vec();
        ixs.push(Ix {
            program_id: SQUADS_PROGRAM_ID,
            accounts,
            data: discriminator("vault_transaction_execute").to_vec(),
        });
        h.send(&ixs, &[m])
    }
}

/// Proposes `ixs`, collects `approvals` member approvals, then executes (with `before` placed in
/// front of the execute instruction).
pub fn squads_execute(
    h: &mut Harness,
    ms: &mut Multisig,
    ixs: &[Ix],
    approvals: usize,
    before: &[Ix],
) -> TxResult {
    let (index, remaining) = ms.propose(h, ixs);
    for i in 0..approvals {
        ms.approve(h, index, i).expect("proposal_approve");
    }
    ms.execute(h, index, &remaining, before)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_executes_a_transfer_at_threshold_only() {
        let mut h = Harness::new();
        let mut ms = create(&mut h, 2, 3);
        let to = Pubkey::new_unique();
        let transfer = solana_system_interface::instruction::transfer(&ms.vault, &to, 1_000_000);

        let res = squads_execute(&mut h, &mut ms, std::slice::from_ref(&transfer), 1, &[]);
        crate::assert::assert_squads_error(&res, INVALID_PROPOSAL_STATUS);
        assert!(h.account(&to).is_none(), "nothing executed below threshold");

        squads_execute(&mut h, &mut ms, &[transfer], 2, &[]).expect("executes at threshold");
        assert_eq!(h.account(&to).unwrap().lamports, 1_000_000);
    }
}
