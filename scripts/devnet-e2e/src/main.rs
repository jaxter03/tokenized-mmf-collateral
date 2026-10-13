//! End-to-end run against a real cluster: F001 (research R16) and F002 (research R13);
//! quickstart.md "Devnet run" of each feature.
//!
//! Units can't be created before F003, so every transfer here moves zero units; a zero-unit
//! `transfer_checked` still runs every hook check. Each step prints its signature and outcome, and
//! the program exits 0 only if every outcome matches the expectation.
//!
//! Usage:
//! - `devnet-e2e`: a new fund through the F001 steps, then F002's first publish and override steps.
//! - `devnet-e2e --continue <mint>`: on a later UTC day, publish the next record for that fund
//!   (3 days on a Friday, else 1) and print its full price history.
//!
//! Environment:
//! - `RPC_URL` (default `https://api.devnet.solana.com`)
//! - `WALLET`  (default `~/.config/solana/id.json`): becomes the issuer and pays for everything.
//! - `STATE_DIR` (default `target/devnet-e2e`): where the fund admin key is kept between runs.
//!   It is named `*-keypair.json` and lives under `target/`, both ignored by git; never commit it.
//!
//! The three programs must already be deployed at the IDs in `mmf_common::ids`.

use std::process::ExitCode;

use anchor_lang::event::EVENT_IX_TAG_LE;
use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{AccountDeserialize, AnchorDeserialize, Discriminator};
use anchor_spl::token_2022::spl_token_2022 as t22;
use compliance_hook::events::{HolderApproved, HolderRegistered, HolderStatusChanged};
use compliance_hook::HookError;
use fund::{FundError, FundState, NavRecord};
use mmf_common::enums::{HolderStatus, ReasonCode};
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_sdk::ix_governance::{self, Roles};
use mmf_sdk::{ix_fund, ix_hook, kyc, pda, transfer};
use solana_commitment_config::CommitmentConfig;
use solana_keypair::{read_keypair_file, write_keypair_file, Keypair};
use solana_message::Message;
use solana_program_pack::Pack;
use solana_rpc_client::rpc_client::RpcClient;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use solana_transaction::Transaction;
use solana_transaction_status_client_types::option_serializer::OptionSerializer;
use solana_transaction_status_client_types::{UiInstruction, UiTransactionEncoding};

const YEAR: i64 = 365 * 24 * 60 * 60;
const ONE: u64 = 100_000_000;
/// 4.5 % a year, × 10^12 per day.
const RATE: i64 = 123_287_671;
/// 1 % below the $1.00 target: beyond the 50 bps limit.
const BREAK: u64 = 99_000_000;
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

fn fund_err(e: FundError) -> u32 {
    e.into()
}

fn read<T: AccountDeserialize>(rpc: &RpcClient, key: &Pubkey) -> Option<T> {
    let data = rpc.get_account_data(key).ok()?;
    T::try_deserialize(&mut &data[..]).ok()
}

fn today() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    mmf_common::accrual::utc_day(now).unwrap_or(0)
}

/// 3 on a Friday (covering the weekend), else 1. Day 0 (1970-01-01) was a Thursday.
fn days_for(day: u32) -> u16 {
    if (day + 4) % 7 == 5 {
        3
    } else {
        1
    }
}

fn state_dir() -> std::path::PathBuf {
    std::env::var("STATE_DIR")
        .unwrap_or_else(|_| "target/devnet-e2e".into())
        .into()
}

fn admin_key_path(mint: &Pubkey) -> std::path::PathBuf {
    state_dir().join(format!("{mint}-fund-admin-keypair.json"))
}

/// Prints every NavRecord of the fund behind `mint`, oldest first.
fn print_history(rpc: &RpcClient, mint: &Pubkey) {
    let fs_key = pda::fund_state(mint).0;
    let Some(fs) = read::<FundState>(rpc, &fs_key) else {
        println!("  (fund not found)");
        return;
    };
    println!("\nPrice history of {mint} ({} records):", fs.nav_seq);
    for seq in 1..=fs.nav_seq {
        match read::<NavRecord>(rpc, &pda::nav_record(&fs_key, seq).0) {
            Some(r) => println!(
                "  #{:<3} days {}..={} price {} rate_e12 {} index_after {}{}",
                r.seq,
                r.period_start_day,
                r.period_start_day + u32::from(r.days_covered) - 1,
                r.nav,
                r.daily_rate_e12,
                r.index_after,
                if r.override_used { " (override)" } else { "" }
            ),
            None => println!("  #{seq}: MISSING"),
        }
    }
}

/// `--continue <mint>`: the next day's publish for an existing fund (research R13).
fn continue_run(mut run: Run, mint: Pubkey) -> ExitCode {
    let path = admin_key_path(&mint);
    let admin = match read_keypair_file(&path) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("cannot read the fund admin key {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let fs_key = pda::fund_state(&mint).0;
    let Some(fs) = read::<FundState>(&run.rpc, &fs_key) else {
        eprintln!("no fund at {fs_key}");
        return ExitCode::FAILURE;
    };
    let day = today();
    let next_start = fs.nav_period_end_day + 1;
    if fs.nav_seq > 0 && next_start > day {
        print_history(&run.rpc, &mint);
        println!(
            "\nThe next record starts on UTC day {next_start}; today is {day}. Run again then."
        );
        return ExitCode::FAILURE;
    }
    let days = days_for(day);
    let payer = run.payer.pubkey();
    run.send(
        &format!(
            "continue: publish record {} for day {day} ({days} day(s))",
            fs.nav_seq + 1
        ),
        &[ix_fund::publish_daily(
            &admin.pubkey(),
            &payer,
            &mint,
            fs.nav_seq + 1,
            fs.nav,
            RATE,
            days,
        )],
        &[&admin],
        Expect::Ok,
    );
    print_history(&run.rpc, &mint);
    finish(&run)
}

fn finish(run: &Run) -> ExitCode {
    if run.failures == 0 {
        println!("\nAll {} steps matched their expected outcome.", run.step);
        ExitCode::SUCCESS
    } else {
        println!("\n{} of {} steps did not match.", run.failures, run.step);
        ExitCode::FAILURE
    }
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
    let rpc = RpcClient::new_with_commitment(url.clone(), CommitmentConfig::confirmed());
    let mut run = Run {
        rpc,
        payer,
        failures: 0,
        step: 0,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--continue") {
        let Some(mint) = args.get(1).and_then(|m| m.parse::<Pubkey>().ok()) else {
            eprintln!("usage: devnet-e2e --continue <mint>");
            return ExitCode::FAILURE;
        };
        println!("F002 continue run on {url} for fund mint {mint}");
        return continue_run(run, mint);
    }
    println!(
        "F001 + F002 end-to-end run on {url} as {}",
        run.payer.pubkey()
    );
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
    // F002: the fund admin is a real key, kept for `--continue` runs on later days.
    let fund_admin = Keypair::new();
    let key_path = admin_key_path(&m);
    if let Err(e) = std::fs::create_dir_all(state_dir())
        .map_err(|e| e.to_string())
        .and_then(|_| write_keypair_file(&fund_admin, &key_path).map_err(|e| e.to_string()))
    {
        eprintln!(
            "cannot save the fund admin key to {}: {e}",
            key_path.display()
        );
        return ExitCode::FAILURE;
    }
    let roles = Roles {
        fund_admin: fund_admin.pubkey(),
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

    // F002 (quickstart.md "Devnet run", research R13). Compliance is a plain key here; the Squads
    // path for overrides is proven in LiteSVM (f002_override.rs).
    let fa = fund_admin.pubkey();
    let day = today();
    let days = days_for(day);
    let publish = |signer: &Pubkey, seq: u64, nav: u64| {
        ix_fund::publish_daily(signer, &i, &m, seq, nav, RATE, days)
    };
    run.send(
        "F002-1. publish $0.99 (1 % from the $1.00 target, limit 50 bps)",
        &[publish(&fa, 1, BREAK)],
        &[&fund_admin],
        Expect::Err(fund_err(FundError::PriceMoveTooLarge), "PriceMoveTooLarge"),
    );
    run.send(
        "F002-2. publish signed by A (not the fund admin)",
        &[publish(&a, 1, ONE)],
        &[&alice],
        Expect::Err(fund_err(FundError::Unauthorized), "Unauthorized"),
    );
    let other = ReasonCode::Other as u8;
    let case = [0x0c; 32];
    run.send(
        "F002-3. compliance approves an override for $0.99",
        &[ix_governance::approve_nav_override(
            &c, &m, BREAK, other, case,
        )],
        &[&compliance],
        Expect::Ok,
    );
    run.send(
        &format!("F002-4. publish $0.99 covering {days} day(s)"),
        &[publish(&fa, 1, BREAK)],
        &[&fund_admin],
        Expect::Ok,
    );
    run.send(
        "F002-5. publish again the same day",
        &[publish(&fa, 2, BREAK)],
        &[&fund_admin],
        Expect::Err(fund_err(FundError::PeriodNotStarted), "PeriodNotStarted"),
    );
    run.send(
        "F002-6. compliance approves an override for $0.985",
        &[ix_governance::approve_nav_override(
            &c, &m, 98_500_000, other, case,
        )],
        &[&compliance],
        Expect::Ok,
    );
    run.send(
        "F002-6. compliance withdraws it",
        &[ix_governance::withdraw_nav_override(&c, &m, other, case)],
        &[&compliance],
        Expect::Ok,
    );
    let fs_key = pda::fund_state(&m).0;
    match read::<NavRecord>(&run.rpc, &pda::nav_record(&fs_key, 1).0) {
        Some(r) if r.nav == BREAK && r.override_used && r.days_covered == days => {
            println!("F002-7. NavRecord 1 read back: {r:?}")
        }
        other => {
            println!("F002-7. NavRecord 1 NOT as expected: {other:?}");
            run.failures += 1;
        }
    }
    print_history(&run.rpc, &m);

    println!("\nFund mint: {m}\nIssuer: {i}\nCompliance: {c}\nFund admin: {fa}");
    println!(
        "Fund admin key: {} (needed by --continue; never commit it)",
        key_path.display()
    );
    println!("Next days: devnet-e2e --continue {m}");
    finish(&run)
}
