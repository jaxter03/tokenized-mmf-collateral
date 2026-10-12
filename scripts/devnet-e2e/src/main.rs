//! F001 end-to-end run against a real cluster (research R16, quickstart.md "Devnet run").
//!
//! F001 can't create units (FR-035), so every transfer here moves zero units; a zero-unit
//! `transfer_checked` still runs every hook check. Each step prints its signature and outcome, and
//! the program exits 0 only if every outcome matches the expectation.
//!
//! Environment:
//! - `RPC_URL` (default `https://api.devnet.solana.com`)
//! - `WALLET`  (default `~/.config/solana/id.json`): becomes the issuer and pays for everything.
//!
//! The three programs must already be deployed at the IDs in `mmf_common::ids`.

use std::process::ExitCode;

use anchor_lang::event::EVENT_IX_TAG_LE;
use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{AnchorDeserialize, Discriminator};
use anchor_spl::token_2022::spl_token_2022 as t22;
use compliance_hook::events::{HolderApproved, HolderRegistered, HolderStatusChanged};
use compliance_hook::HookError;
use mmf_common::enums::{HolderStatus, ReasonCode};
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_sdk::ix_governance::{self, Roles};
use mmf_sdk::{ix_fund, ix_hook, kyc, pda, transfer};
use solana_commitment_config::CommitmentConfig;
use solana_keypair::{read_keypair_file, Keypair};
use solana_message::Message;
use solana_program_pack::Pack;
use solana_rpc_client::rpc_client::RpcClient;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use solana_transaction::Transaction;
use solana_transaction_status_client_types::option_serializer::OptionSerializer;
use solana_transaction_status_client_types::{UiInstruction, UiTransactionEncoding};

const YEAR: i64 = 365 * 24 * 60 * 60;
const REASON: u8 = ReasonCode::OperationalError as u8;

/// What a step should do.
enum Expect {
    Ok,
    /// Fail with this custom error code (Anchor or Token-2022).
    Err(u32, &'static str),
}

struct Run {
    rpc: RpcClient,
    payer: Keypair,
    failures: usize,
    step: usize,
}

impl Run {
    fn send(&mut self, label: &str, ixs: &[Instruction], signers: &[&Keypair], expect: Expect) {
        self.step += 1;
        let mut all: Vec<&Keypair> = vec![&self.payer];
        all.extend(
            signers
                .iter()
                .copied()
                .filter(|k| k.pubkey() != self.payer.pubkey()),
        );
        let blockhash = match self.rpc.get_latest_blockhash() {
            Ok(b) => b,
            Err(e) => {
                println!("[{:>2}] {label}: FAILED to get a blockhash: {e}", self.step);
                self.failures += 1;
                return;
            }
        };
        let tx = Transaction::new(
            &all,
            Message::new(ixs, Some(&self.payer.pubkey())),
            blockhash,
        );
        let result = self.rpc.send_and_confirm_transaction(&tx);
        let (ok, detail) = match (&result, &expect) {
            (Ok(sig), Expect::Ok) => (true, format!("ok   {sig}")),
            (Ok(sig), Expect::Err(_, name)) => {
                (false, format!("UNEXPECTED SUCCESS (wanted {name}) {sig}"))
            }
            (Err(e), Expect::Ok) => (false, format!("UNEXPECTED FAILURE: {e}")),
            (Err(e), Expect::Err(code, name)) => {
                let got = custom_code(e);
                if got == Some(*code) {
                    (true, format!("rejected as expected: {name}"))
                } else {
                    (
                        false,
                        format!("WRONG ERROR: wanted {name} ({code}), got {got:?}: {e}"),
                    )
                }
            }
        };
        println!("[{:>2}] {label}: {detail}", self.step);
        if !ok {
            self.failures += 1;
        }
    }

    fn fund(&mut self, to: &Pubkey, lamports: u64) {
        let ix = solana_system_interface::instruction::transfer(&self.payer.pubkey(), to, lamports);
        self.send("fund a test key", &[ix], &[], Expect::Ok);
    }
}

fn custom_code(e: &solana_rpc_client_api::client_error::Error) -> Option<u32> {
    match e.get_transaction_error() {
        Some(solana_transaction_error::TransactionError::InstructionError(
            _,
            InstructionError::Custom(c),
        )) => Some(c),
        _ => None,
    }
}

fn hook(e: HookError) -> u32 {
    e.into()
}

/// Payloads of every `emit_cpi!` event in transactions touching `account`, oldest first and in
/// order within each transaction (each payload starts with its 8-byte event discriminator).
fn event_payloads(rpc: &RpcClient, account: &Pubkey) -> Vec<Vec<u8>> {
    // Default commitment for these lookups is "finalized"; the run's transactions are newer.
    let sig_config = solana_rpc_client::rpc_client::GetConfirmedSignaturesForAddress2Config {
        commitment: Some(CommitmentConfig::confirmed()),
        ..Default::default()
    };
    let mut sigs = rpc
        .get_signatures_for_address_with_config(account, sig_config)
        .unwrap_or_default();
    sigs.reverse(); // RPC returns newest first
    let mut out = Vec::new();
    for s in sigs.iter().filter(|s| s.err.is_none()) {
        let Ok(sig) = s.signature.parse() else {
            continue;
        };
        let config = solana_rpc_client_api::config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };
        let tx = match rpc.get_transaction_with_config(&sig, config) {
            Ok(tx) => tx,
            Err(e) => {
                eprintln!("  (could not fetch {sig}: {e})");
                continue;
            }
        };
        let Some(meta) = tx.transaction.meta else {
            continue;
        };
        let OptionSerializer::Some(inner) = meta.inner_instructions else {
            continue;
        };
        for group in inner {
            for ix in group.instructions {
                let UiInstruction::Compiled(c) = ix else {
                    continue;
                };
                let Ok(data) = bs58::decode(&c.data).into_vec() else {
                    continue;
                };
                if let Some(payload) = data.strip_prefix(EVENT_IX_TAG_LE) {
                    out.push(payload.to_vec());
                }
            }
        }
    }
    out
}

fn decode<T: AnchorDeserialize + Discriminator>(payload: &[u8]) -> Option<T> {
    if payload.len() < 8 || &payload[..8] != T::DISCRIMINATOR {
        return None;
    }
    T::deserialize(&mut &payload[8..]).ok()
}

/// Status history of `owner`, rebuilt in time order from its records alone (FR-034).
fn history(rpc: &RpcClient, mint: &Pubkey, owner: &Pubkey) -> Vec<String> {
    let mut h = Vec::new();
    for p in event_payloads(rpc, &pda::holder(mint, owner).0) {
        if decode::<HolderRegistered>(&p).is_some_and(|e| e.owner == *owner) {
            h.push("Pending (registered)".to_string());
        } else if let Some(e) = decode::<HolderApproved>(&p).filter(|e| e.owner == *owner) {
            h.push(if e.renewal {
                "Approved (renewed)".into()
            } else {
                "Approved".into()
            });
        } else if let Some(e) = decode::<HolderStatusChanged>(&p).filter(|e| e.owner == *owner) {
            let name = HolderStatus::try_from(e.to)
                .map(|s| format!("{s:?}"))
                .unwrap_or_default();
            h.push(format!("{name} (reason {})", e.reason));
        }
    }
    h
}

fn main() -> ExitCode {
    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "https://api.devnet.solana.com".into());
    let wallet = std::env::var("WALLET").unwrap_or_else(|_| {
        format!(
            "{}/.config/solana/id.json",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let payer = match read_keypair_file(&wallet) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("cannot read wallet {wallet}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("F001 end-to-end run on {url} as {}", payer.pubkey());
    let rpc = RpcClient::new_with_commitment(url, CommitmentConfig::confirmed());
    let mut run = Run {
        rpc,
        payer,
        failures: 0,
        step: 0,
    };
    let issuer = run.payer.insecure_clone();
    let i = issuer.pubkey();

    // Keys for this run.
    let mint = Keypair::new();
    let m = mint.pubkey();
    let compliance = Keypair::new();
    let provider = Keypair::new();
    let (alice, bob) = (Keypair::new(), Keypair::new());
    for k in [&compliance, &alice, &bob] {
        run.fund(&k.pubkey(), 100_000_000);
    }

    // A 6-decimal cash mint.
    let cash = Keypair::new();
    let lamports = run
        .rpc
        .get_minimum_balance_for_rent_exemption(t22::state::Mint::LEN)
        .unwrap_or(0);
    run.send(
        "create cash mint",
        &[
            solana_system_interface::instruction::create_account(
                &i,
                &cash.pubkey(),
                lamports,
                t22::state::Mint::LEN as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            t22::instruction::initialize_mint2(&TOKEN_2022_PROGRAM_ID, &cash.pubkey(), &i, None, 6)
                .unwrap(),
        ],
        &[&cash],
        Expect::Ok,
    );

    // Step 3: init_roles + create_fund → init_registry → set_rules → set_roles → launch.
    let mut terms_uri = [0u8; 128];
    terms_uri[..30].copy_from_slice(b"https://example.org/terms.pdf\0");
    let terms = fund::FundTerms {
        fund_id: *b"MMF-DEVNET-1",
        cash_mint: cash.pubkey(),
        target_nav: 100_000_000,
        max_nav_move_bps: 50,
        redemption_gate_bps: 1_000,
        distribution_mode: 0,
        distribution_cadence_days: 30,
        terms_hash: [0x5a; 32],
        terms_uri,
    };
    run.send(
        "3. init_roles + create_fund",
        &[
            ix_governance::init_roles(&i, &m),
            ix_fund::create_fund(&i, &m, &cash.pubkey(), terms, "Devnet Test MMF", "DTMMF"),
        ],
        &[&mint],
        Expect::Ok,
    );
    let mut allowed = [0u64; 11];
    mmf_common::jurisdiction::set_insert(&mut allowed, *b"US");
    mmf_common::jurisdiction::set_insert(&mut allowed, *b"GB");
    let providers = [
        provider.pubkey(),
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    let roles = Roles {
        fund_admin: Pubkey::new_unique(),
        transfer_agent: Pubkey::new_unique(),
        compliance: compliance.pubkey(),
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    };
    run.send(
        "3. init_registry",
        &[ix_hook::init_registry(&i, &m)],
        &[],
        Expect::Ok,
    );
    run.send(
        "3. set_rules (US, GB, cap 2)",
        &[ix_hook::set_rules(&i, &m, providers, allowed, 2)],
        &[],
        Expect::Ok,
    );
    run.send(
        "3. set_roles",
        &[ix_governance::set_roles(&i, &m, &roles)],
        &[],
        Expect::Ok,
    );
    run.send("3. launch", &[ix_fund::launch(&i, &m)], &[], Expect::Ok);

    // Step 4: A and B register themselves; compliance approves and thaws. Treasury too.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    for (name, k) in [("A", &alice), ("B", &bob)] {
        let o = k.pubkey();
        run.send(
            &format!("4. {name} registers itself"),
            &[ix_hook::register_holder(&o, &o, &m, &o, 0)],
            &[k],
            Expect::Ok,
        );
        let signed = kyc::sign(&provider, &kyc::statement(m, o, *b"US", 1, now + YEAR));
        let ixs = ix_hook::onboarding_tx(&i, &compliance.pubkey(), &m, &o, &signed);
        run.send(
            &format!("4. compliance approves and thaws {name}"),
            &ixs,
            &[&compliance],
            Expect::Ok,
        );
    }
    let treasury = pda::treasury(&pda::fund_state(&m).0).0;
    let c = compliance.pubkey();
    run.send(
        "4. treasury registered, approved, thawed",
        &[
            ix_hook::register_holder(&c, &c, &m, &treasury, 1),
            ix_hook::approve_fund_account(&c, &m, &treasury),
            ix_hook::create_holding_account(&i, &m, &treasury),
            ix_hook::thaw_holding_account(&i, &m, &treasury, &pda::holding_account(&treasury, &m)),
        ],
        &[&compliance],
        Expect::Ok,
    );

    let zero =
        |from: &Keypair, to: &Pubkey| transfer::transfer_between_owners(&m, &from.pubkey(), to, 0);
    let (a, b) = (alice.pubkey(), bob.pubkey());

    // Step 5.
    run.send(
        "5. zero-unit A → B",
        &[zero(&alice, &b)],
        &[&alice],
        Expect::Ok,
    );
    // Step 6.
    run.send(
        "6. suspend B",
        &[ix_governance::suspend_holder(&c, &m, &b, REASON, [0; 32])],
        &[&compliance],
        Expect::Ok,
    );
    run.send(
        "6. zero-unit A → B (B suspended)",
        &[zero(&alice, &b)],
        &[&alice],
        Expect::Err(hook(HookError::ReceiverNotApproved), "ReceiverNotApproved"),
    );
    run.send(
        "6. reinstate B",
        &[ix_governance::reinstate_holder(&c, &m, &b, REASON, [0; 32])],
        &[&compliance],
        Expect::Ok,
    );
    run.send(
        "6. zero-unit A → B again",
        &[zero(&alice, &b)],
        &[&alice],
        Expect::Ok,
    );
    // Step 7.
    run.send(
        "7. pause",
        &[ix_governance::pause_transfers(&c, &m, REASON, [0; 32])],
        &[&compliance],
        Expect::Ok,
    );
    run.send(
        "7. zero-unit A → B (paused)",
        &[zero(&alice, &b)],
        &[&alice],
        Expect::Err(hook(HookError::TransfersPaused), "TransfersPaused"),
    );
    run.send(
        "7. unpause",
        &[ix_governance::unpause_transfers(&c, &m, REASON, [0; 32])],
        &[&compliance],
        Expect::Ok,
    );
    run.send(
        "7. zero-unit A → B after unpause",
        &[zero(&alice, &b)],
        &[&alice],
        Expect::Ok,
    );
    // Step 8.
    let stranger = Pubkey::new_unique();
    run.send(
        "8. create holding account for unregistered C",
        &[ix_hook::create_holding_account(&i, &m, &stranger)],
        &[],
        Expect::Ok,
    );
    run.send(
        "8. zero-unit A → C (unregistered)",
        &[zero(&alice, &stranger)],
        &[&alice],
        Expect::Err(
            t22::error::TokenError::AccountFrozen as u32,
            "AccountFrozen",
        ),
    );
    // Step 9.
    run.send(
        "9. revoke B",
        &[ix_governance::revoke_holder(&c, &m, &b, REASON, [0; 32])],
        &[&compliance],
        Expect::Ok,
    );
    let signed = kyc::sign(&provider, &kyc::statement(m, b, *b"US", 1, now + 2 * YEAR));
    run.send(
        "9. approve B after revocation",
        &[
            signed.ix.clone(),
            ix_hook::approve_investor(&c, &m, &b, signed.attestation_hash),
        ],
        &[&compliance],
        Expect::Err(hook(HookError::HolderRevoked), "HolderRevoked"),
    );

    // Step 10: records and rebuilt status history.
    println!("\nStatus history rebuilt from records (FR-034):");
    for (name, o) in [("A", a), ("B", b)] {
        println!("  {name} {o}: {}", history(&run.rpc, &m, &o).join(" → "));
    }
    println!("\nFund mint: {m}\nIssuer: {i}\nCompliance: {c}");

    if run.failures == 0 {
        println!("\nAll {} steps matched their expected outcome.", run.step);
        ExitCode::SUCCESS
    } else {
        println!("\n{} of {} steps did not match.", run.failures, run.step);
        ExitCode::FAILURE
    }
}
