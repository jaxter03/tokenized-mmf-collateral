//! LiteSVM test harness (T020, research R8).
//!
//! `Harness::new()` builds LiteSVM 0.12 with precompiles and loads:
//! - the four programs (F001 three, F003 `dvp_settlement`) from `target/deploy/` (run `anchor build`
//!   first);
//! - the mainnet Token-2022 v11 fixture over the bundled Token-2022 (permissioned burn);
//! - the mainnet Squads v4 program and its ProgramConfig account.

use std::path::PathBuf;

use anchor_lang::prelude::Pubkey;
use base64::Engine;
use litesvm::types::{FailedTransactionMetadata, TransactionMetadata};
use litesvm::LiteSVM;
use mmf_common::ids;
use solana_account::Account;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub type Ix = anchor_lang::solana_program::instruction::Instruction;
pub type TxMeta = TransactionMetadata;
pub type TxFailure = FailedTransactionMetadata;
pub type TxResult = Result<TxMeta, TxFailure>;

/// Squads v4 ProgramConfig PDA (`["multisig", "program_config"]`).
pub const SQUADS_PROGRAM_CONFIG: Pubkey =
    anchor_lang::prelude::pubkey!("BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr");

/// Lamports given to the default payer.
const PAYER_LAMPORTS: u64 = 1_000_000_000_000;

pub struct Harness {
    pub svm: LiteSVM,
    /// Pays fees and rent unless a test chooses another payer.
    pub payer: Keypair,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness {
    pub fn new() -> Self {
        let mut svm = LiteSVM::new();
        let deploy = repo_root().join("target/deploy");
        for (id, name) in [
            (ids::FUND_PROGRAM_ID, "fund.so"),
            (ids::COMPLIANCE_HOOK_PROGRAM_ID, "compliance_hook.so"),
            (ids::GOVERNANCE_PROGRAM_ID, "governance.so"),
            (ids::DVP_SETTLEMENT_PROGRAM_ID, "dvp_settlement.so"),
            (ids::COLLATERAL_PROGRAM_ID, "collateral.so"),
        ] {
            svm.add_program_from_file(id, deploy.join(name))
                .unwrap_or_else(|e| panic!("load {name} (run `anchor build` first): {e:?}"));
        }
        svm.add_program_from_file(ids::TOKEN_2022_PROGRAM_ID, fixture("spl_token_2022.so"))
            .expect("load Token-2022 v11 fixture");
        svm.add_program_from_file(
            ids::SQUADS_PROGRAM_ID,
            fixture("squads_multisig_program.so"),
        )
        .expect("load Squads v4 fixture");
        let (key, config) = load_account_json(&fixture("squads_program_config.json"));
        assert_eq!(key, SQUADS_PROGRAM_CONFIG);
        svm.set_account(key, config)
            .expect("set Squads ProgramConfig");

        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), PAYER_LAMPORTS)
            .expect("airdrop payer");
        Self { svm, payer }
    }

    /// Sends `ixs` with the default payer plus `signers`. Expires the blockhash afterwards so an
    /// identical later transaction isn't rejected as already processed.
    pub fn send(&mut self, ixs: &[Ix], signers: &[&Keypair]) -> TxResult {
        let mut all: Vec<&Keypair> = vec![&self.payer];
        all.extend(
            signers
                .iter()
                .copied()
                .filter(|k| k.pubkey() != self.payer.pubkey()),
        );
        let msg = Message::new(ixs, Some(&self.payer.pubkey()));
        let tx = Transaction::new(&all, msg, self.svm.latest_blockhash());
        let res = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        res
    }

    /// Sends with `payer` as fee payer (for tests where the payer is itself a party).
    pub fn send_as(&mut self, payer: &Keypair, ixs: &[Ix], signers: &[&Keypair]) -> TxResult {
        let mut all: Vec<&Keypair> = vec![payer];
        all.extend(
            signers
                .iter()
                .copied()
                .filter(|k| k.pubkey() != payer.pubkey()),
        );
        let msg = Message::new(ixs, Some(&payer.pubkey()));
        let tx = Transaction::new(&all, msg, self.svm.latest_blockhash());
        let res = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        res
    }

    pub fn airdrop(&mut self, to: &Pubkey, lamports: u64) {
        self.svm.airdrop(to, lamports).expect("airdrop");
    }

    /// A fresh keypair with 10 SOL.
    pub fn funded_keypair(&mut self) -> Keypair {
        let k = Keypair::new();
        self.airdrop(&k.pubkey(), 10_000_000_000);
        k
    }

    pub fn account(&self, key: &Pubkey) -> Option<Account> {
        self.svm.get_account(key)
    }

    /// Account data, panicking if the account doesn't exist.
    pub fn data(&self, key: &Pubkey) -> Vec<u8> {
        self.account(key)
            .unwrap_or_else(|| panic!("account {key} not found"))
            .data
    }

    /// True if the account exists and holds data or lamports.
    pub fn exists(&self, key: &Pubkey) -> bool {
        self.account(key)
            .is_some_and(|a| a.lamports > 0 || !a.data.is_empty())
    }
}

/// Reads a `solana account --output json` file.
pub fn load_account_json(path: &std::path::Path) -> (Pubkey, Account) {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read account json"))
            .expect("parse json");
    let key: Pubkey = v["pubkey"]
        .as_str()
        .expect("pubkey")
        .parse()
        .expect("pubkey");
    let a = &v["account"];
    assert_eq!(a["data"][1].as_str(), Some("base64"));
    let data = base64::engine::general_purpose::STANDARD
        .decode(a["data"][0].as_str().expect("data"))
        .expect("base64");
    let owner: Pubkey = a["owner"].as_str().expect("owner").parse().expect("owner");
    let account = Account {
        lamports: a["lamports"].as_u64().expect("lamports"),
        data,
        owner,
        executable: a["executable"].as_bool().unwrap_or(false),
        rent_epoch: 0,
    };
    (key, account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_loads_programs_and_fixtures() {
        let h = Harness::new();
        for id in [
            ids::FUND_PROGRAM_ID,
            ids::COMPLIANCE_HOOK_PROGRAM_ID,
            ids::GOVERNANCE_PROGRAM_ID,
            ids::TOKEN_2022_PROGRAM_ID,
            ids::SQUADS_PROGRAM_ID,
        ] {
            assert!(h.account(&id).is_some_and(|a| a.executable), "{id} loaded");
        }
        let cfg = h.account(&SQUADS_PROGRAM_CONFIG).expect("ProgramConfig");
        assert_eq!(cfg.owner, ids::SQUADS_PROGRAM_ID);
    }

    #[test]
    fn identical_transactions_can_be_sent_twice() {
        let mut h = Harness::new();
        let to = Pubkey::new_unique();
        let ix = solana_system_interface::instruction::transfer(&h.payer.pubkey(), &to, 1_000_000);
        h.send(std::slice::from_ref(&ix), &[]).expect("first");
        h.send(&[ix], &[])
            .expect("second (blockhash expired in between)");
        assert_eq!(h.account(&to).unwrap().lamports, 2_000_000);
    }
}
