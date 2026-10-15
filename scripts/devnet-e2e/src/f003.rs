//! F003 on a real cluster (quickstart.md "Devnet run", research R13). Compliance is a plain key
//! here, as in the F002 steps; the Squads path for rejection is proven in LiteSVM
//! (`f003_reject.rs`).
//!
//! - `devnet-e2e --feature f003`: a new fund set up for settlement; a subscription whose funding
//!   is prepared on a durable nonce and submitted after the blockhash it could have used has
//!   expired; settlement by the keeper; a one-sided secondary transfer left to expire and its
//!   units returned; and a redemption refused today by the daily limit (no units were
//!   outstanding at the start of the day, research R9).
//! - `devnet-e2e --feature f003 --continue <mint>`: on a later UTC day, publish the day's price and
//!   settle that redemption.
//!
//! Keys the continue run needs are kept in `STATE_DIR` as `*-keypair.json` (ignored by git).

use std::process::ExitCode;
use std::time::Duration;

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022 as t22;
use dvp_settlement::{CreateArgs, SettlementInstruction};
use fund::{FundError, FundState};
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_sdk::ix_dvp::{self, Deal};
use mmf_sdk::ix_governance::{self, Roles};
use mmf_sdk::{ix_fund, ix_hook, kyc, pda, prepare};
use solana_keypair::{read_keypair_file, write_keypair_file, Keypair};
use solana_message::Message;
use solana_program_pack::Pack;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::{days_for, finish, read, state_dir, today, Expect, Run, ONE, YEAR};

/// One whole unit or one dollar (6 dp).
const UNIT: u64 = 1_000_000;
const SUBSCRIBE: u64 = 1_000 * UNIT;
const SECONDARY: u64 = 100 * UNIT;
const REDEEM: u64 = 400 * UNIT;
const REDEEM_REF: [u8; 32] = [3; 32];

fn key_path(mint: &Pubkey, name: &str) -> std::path::PathBuf {
    state_dir().join(format!("{mint}-f003-{name}-keypair.json"))
}

fn save(mint: &Pubkey, name: &str, k: &Keypair) -> Result<(), String> {
    std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
    write_keypair_file(k, key_path(mint, name)).map_err(|e| e.to_string())?;
    Ok(())
}

fn load(mint: &Pubkey, name: &str) -> Result<Keypair, String> {
    read_keypair_file(key_path(mint, name)).map_err(|e| format!("{name}: {e}"))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Run {
    /// Signs a message prepared earlier with `signers` and submits it unchanged.
    pub(crate) fn send_prepared(
        &mut self,
        label: &str,
        message: Message,
        signers: &[&Keypair],
        expect: Expect,
    ) {
        self.step += 1;
        let blockhash = message.recent_blockhash;
        let tx = Transaction::new(signers, message, blockhash);
        let result = self.rpc.send_and_confirm_transaction(&tx);
        let ok = matches!((&result, &expect), (Ok(_), Expect::Ok));
        match &result {
            Ok(sig) => println!("[{:>2}] {label}: ok   {sig}", self.step),
            Err(e) => println!("[{:>2}] {label}: UNEXPECTED FAILURE: {e}", self.step),
        }
        if !ok {
            self.failures += 1;
        }
    }
}

fn deal(mint: &Pubkey, cash_mint: &Pubkey, creator: &Pubkey, args: &CreateArgs) -> Deal {
    Deal {
        mint: *mint,
        cash_mint: *cash_mint,
        cash_program: TOKEN_2022_PROGRAM_ID,
        kind: args.kind,
        seller: args.seller,
        buyer: args.buyer,
        created_by: *creator,
        external_ref: args.external_ref,
    }
}

/// The mock stablecoin is a Token-2022 mint, so cash accounts are Token-2022 ATAs; they are
/// created with `ix_hook::create_holding_account`, which makes the Token-2022 ATA of any mint.
fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    pda::ata(owner, mint, &TOKEN_2022_PROGRAM_ID)
}

/// `--feature f003`.
pub fn run(mut run: Run) -> ExitCode {
    println!("F003 end-to-end run as {}", run.payer.pubkey());
    let issuer = run.payer.insecure_clone();
    let i = issuer.pubkey();
    let (mint, cash) = (Keypair::new(), Keypair::new());
    let (m, cm) = (mint.pubkey(), cash.pubkey());
    let compliance = Keypair::new();
    let provider = Keypair::new();
    let (admin, ta, keeper) = (Keypair::new(), Keypair::new(), Keypair::new());
    let (alice, bob) = (Keypair::new(), Keypair::new());
    for k in [&compliance, &admin, &ta, &keeper, &alice, &bob] {
        run.fund(&k.pubkey(), 100_000_000);
    }
    for (name, k) in [
        ("admin", &admin),
        ("ta", &ta),
        ("keeper", &keeper),
        ("alice", &alice),
    ] {
        if let Err(e) = save(&m, name, k) {
            eprintln!("cannot save the {name} key: {e}");
            return ExitCode::FAILURE;
        }
    }

    // A mock 6-decimal stablecoin (Token-2022, no extensions).
    let lamports = run
        .rpc
        .get_minimum_balance_for_rent_exemption(t22::state::Mint::LEN)
        .unwrap_or(0);
    run.send(
        "F003-0. create the mock stablecoin",
        &[
            solana_system_interface::instruction::create_account(
                &i,
                &cm,
                lamports,
                t22::state::Mint::LEN as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            t22::instruction::initialize_mint2(&TOKEN_2022_PROGRAM_ID, &cm, &i, None, 6).unwrap(),
        ],
        &[&cash],
        Expect::Ok,
    );

    // A launched fund, priced at $1.00 today.
    let mut terms_uri = [0u8; 128];
    terms_uri[..30].copy_from_slice(b"https://example.org/terms.pdf\0");
    let terms = fund::FundTerms {
        fund_id: *b"MMF-DEVNET-3",
        cash_mint: cm,
        target_nav: ONE,
        max_nav_move_bps: 50,
        redemption_gate_bps: 1_000,
        distribution_mode: 0,
        distribution_cadence_days: 30,
        terms_hash: [0x5a; 32],
        terms_uri,
    };
    let mut allowed = [0u64; 11];
    mmf_common::jurisdiction::set_insert(&mut allowed, *b"US");
    let providers = [
        provider.pubkey(),
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    let roles = Roles {
        fund_admin: admin.pubkey(),
        transfer_agent: ta.pubkey(),
        compliance: compliance.pubkey(),
        keeper: keeper.pubkey(),
        auditor: Pubkey::new_unique(),
    };
    run.send(
        "F003-1. init_roles + create_fund",
        &[
            ix_governance::init_roles(&i, &m),
            ix_fund::create_fund(&i, &m, &cm, terms, "Devnet Settlement MMF", "DSMMF"),
        ],
        &[&mint],
        Expect::Ok,
    );
    run.send(
        "F003-1. register rules, roles, launch",
        &[
            ix_hook::init_registry(&i, &m),
            ix_hook::set_rules(&i, &m, providers, allowed, 0),
            ix_governance::set_roles(&i, &m, &roles),
            ix_fund::launch(&i, &m),
        ],
        &[],
        Expect::Ok,
    );
    run.send(
        "F003-1. publish $1.00",
        &[ix_fund::publish_daily(
            &admin.pubkey(),
            &i,
            &m,
            1,
            ONE,
            0,
            days_for(today()),
        )],
        &[&admin],
        Expect::Ok,
    );

    // Settlement setup: treasury cash account, init_dvp, escrow approved and thawed.
    let fs = pda::fund_state(&m).0;
    let treasury = ata(&ta.pubkey(), &cm);
    let escrow = pda::escrow_authority(&fs).0;
    let c = compliance.pubkey();
    run.send(
        "F003-2. treasury cash account, set_cash_treasury, init_dvp",
        &[
            ix_hook::create_holding_account(&i, &cm, &ta.pubkey()),
            ix_fund::set_cash_treasury(&i, &m, &treasury, &cm),
            ix_dvp::init_dvp(&i, &i, &m, &cm, &TOKEN_2022_PROGRAM_ID, None),
        ],
        &[],
        Expect::Ok,
    );
    run.send(
        "F003-2. compliance registers and approves the escrow; thaw",
        &[
            ix_hook::register_holder(&c, &c, &m, &escrow, 2),
            ix_hook::approve_fund_account(&c, &m, &escrow),
            ix_hook::thaw_holding_account(&i, &m, &escrow, &pda::unit_escrow(&fs, &m)),
        ],
        &[&compliance],
        Expect::Ok,
    );

    // Investors with cash; cash in the treasury for the redemption.
    for (name, k) in [("A", &alice), ("B", &bob)] {
        let o = k.pubkey();
        run.send(
            &format!("F003-3. {name} registers itself"),
            &[ix_hook::register_holder(&o, &o, &m, &o, 0)],
            &[k],
            Expect::Ok,
        );
        let signed = kyc::sign(&provider, &kyc::statement(m, o, *b"US", 1, now() + YEAR));
        let ixs = ix_hook::onboarding_tx(&i, &c, &m, &o, &signed);
        run.send(
            &format!("F003-3. compliance approves {name}"),
            &ixs,
            &[&compliance],
            Expect::Ok,
        );
        run.send(
            &format!("F003-3. {name} gets $2 000"),
            &[
                ix_hook::create_holding_account(&i, &cm, &o),
                t22::instruction::mint_to(
                    &TOKEN_2022_PROGRAM_ID,
                    &cm,
                    &ata(&o, &cm),
                    &i,
                    &[],
                    2_000 * UNIT,
                )
                .unwrap(),
            ],
            &[],
            Expect::Ok,
        );
    }
    run.send(
        "F003-3. treasury gets $1 000",
        &[t22::instruction::mint_to(
            &TOKEN_2022_PROGRAM_ID,
            &cm,
            &treasury,
            &i,
            &[],
            1_000 * UNIT,
        )
        .unwrap()],
        &[],
        Expect::Ok,
    );

    // Subscription, with the funding prepared on A's durable nonce (US5).
    let a = alice.pubkey();
    let sub = CreateArgs {
        kind: 0,
        seller: fs,
        buyer: a,
        units: SUBSCRIBE,
        cash_amount: 0,
        not_before_ts: 0,
        expires_at_ts: 0,
        external_ref: [1; 32],
    };
    let d = deal(&m, &cm, &a, &sub);
    run.send(
        "F003-4. A creates a subscription for 1 000 units",
        &[ix_dvp::create_instruction(&a, &i, &m, sub)],
        &[&alice],
        Expect::Ok,
    );
    run.send(
        "F003-4. transfer agent affirms",
        &[ix_dvp::affirm_fund_side(&ta.pubkey(), &d)],
        &[&ta],
        Expect::Ok,
    );
    let nonce = Keypair::new();
    let rent = run
        .rpc
        .get_minimum_balance_for_rent_exemption(80)
        .unwrap_or(0);
    run.send(
        "F003-4. A's durable nonce account",
        &solana_system_interface::instruction::create_nonce_account(&i, &nonce.pubkey(), &a, rent),
        &[&nonce],
        Expect::Ok,
    );
    let value = run
        .rpc
        .get_account(&nonce.pubkey())
        .ok()
        .and_then(|acc| prepare::read_nonce(&acc.data));
    let Some(value) = value else {
        println!("F003-4. cannot read the nonce value");
        run.failures += 1;
        return finish(&run);
    };
    let message = prepare::with_durable_nonce(
        &[ix_dvp::fund_cash(&a, &d, &ata(&a, &cm))],
        &nonce.pubkey(),
        &a,
        &a,
        value,
    );
    println!("F003-4. funding prepared; waiting 130 s (past any recent blockhash) before signing…");
    std::thread::sleep(Duration::from_secs(130));
    run.send_prepared(
        "F003-4. A signs and submits the prepared funding",
        message,
        &[&alice],
        Expect::Ok,
    );
    run.send(
        "F003-4. keeper settles",
        &[ix_dvp::settle_subscription(&keeper.pubkey(), &d, &treasury)],
        &[&keeper],
        Expect::Ok,
    );

    // A one-sided secondary transfer, left to expire; its units go back to A.
    let b = bob.pubkey();
    let sec = CreateArgs {
        kind: 2,
        seller: a,
        buyer: b,
        units: SECONDARY,
        cash_amount: 99 * UNIT,
        not_before_ts: 0,
        expires_at_ts: now() + 60,
        external_ref: [2; 32],
    };
    let ds = deal(&m, &cm, &b, &sec);
    run.send(
        "F003-5. B creates a secondary (expires in 60 s)",
        &[ix_dvp::create_instruction(&b, &i, &m, sec)],
        &[&bob],
        Expect::Ok,
    );
    run.send(
        "F003-5. A funds its units",
        &[ix_dvp::fund_units(&a, &ds)],
        &[&alice],
        Expect::Ok,
    );
    std::thread::sleep(Duration::from_secs(75));
    let k = keeper.pubkey();
    run.send(
        "F003-5. keeper expires it and returns A's units",
        &[ix_dvp::expire(&k, &ds), ix_dvp::return_units(&k, &ds)],
        &[&keeper],
        Expect::Ok,
    );

    // A redemption, funded today, refused by today's limit.
    let red = CreateArgs {
        kind: 1,
        seller: a,
        buyer: fs,
        units: REDEEM,
        cash_amount: 0,
        not_before_ts: 0,
        expires_at_ts: now() + 4 * 86_400,
        external_ref: REDEEM_REF,
    };
    let dr = deal(&m, &cm, &a, &red);
    run.send(
        "F003-6. A creates a redemption for 400 units",
        &[ix_dvp::create_instruction(&a, &i, &m, red)],
        &[&alice],
        Expect::Ok,
    );
    run.send(
        "F003-6. A funds its units",
        &[ix_dvp::fund_units(&a, &dr)],
        &[&alice],
        Expect::Ok,
    );
    run.send(
        "F003-6. transfer agent funds the cash from the treasury",
        &[ix_dvp::fund_redemption_cash(&ta.pubkey(), &dr, &treasury)],
        &[&ta],
        Expect::Ok,
    );
    run.send(
        "F003-6. settle today (no units at the day's start)",
        &[ix_dvp::settle_redemption(&k, &dr)],
        &[&keeper],
        Expect::Err(
            FundError::RedemptionLimitExceeded.into(),
            "RedemptionLimitExceeded",
        ),
    );

    print_balances(
        &run,
        &m,
        &cm,
        &[("A", a), ("B", b), ("treasury", ta.pubkey())],
    );
    println!("\nFund mint: {m}\nCash mint: {cm}");
    println!("Next UTC day: devnet-e2e --feature f003 --continue {m}");
    finish(&run)
}

/// `--feature f003 --continue <mint>`: on a later UTC day, the day's price and the redemption.
pub fn continue_run(mut run: Run, m: Pubkey) -> ExitCode {
    let keys = ["admin", "ta", "keeper", "alice"].map(|n| load(&m, n));
    let [Ok(admin), Ok(ta), Ok(keeper), Ok(alice)] = keys else {
        eprintln!(
            "cannot read the saved keys for {m} from {}",
            state_dir().display()
        );
        return ExitCode::FAILURE;
    };
    let fs_key = pda::fund_state(&m).0;
    let Some(fs) = read::<FundState>(&run.rpc, &fs_key) else {
        eprintln!("no fund at {fs_key}");
        return ExitCode::FAILURE;
    };
    let day = today();
    if fs.nav_period_end_day + 1 > day {
        println!("The fund's price covers today already; run this on a later UTC day.");
        return ExitCode::FAILURE;
    }
    let days = (day - fs.nav_period_end_day) as u16;
    let i = run.payer.pubkey();
    run.send(
        &format!("F003-7. publish $1.00 for {days} day(s)"),
        &[ix_fund::publish_daily(
            &admin.pubkey(),
            &i,
            &m,
            fs.nav_seq + 1,
            ONE,
            0,
            days,
        )],
        &[&admin],
        Expect::Ok,
    );
    let cm = fs.terms.cash_mint;
    let a = alice.pubkey();
    let red = CreateArgs {
        kind: 1,
        seller: a,
        buyer: fs_key,
        units: REDEEM,
        cash_amount: 0,
        not_before_ts: 0,
        expires_at_ts: 0,
        external_ref: REDEEM_REF,
    };
    let dr = deal(&m, &cm, &a, &red);
    run.send(
        "F003-7. keeper settles yesterday's redemption",
        &[ix_dvp::settle_redemption(&keeper.pubkey(), &dr)],
        &[&keeper],
        Expect::Ok,
    );
    match read::<SettlementInstruction>(&run.rpc, &dr.settlement()) {
        Some(si) if si.status == 4 => println!("F003-7. redemption #{} Settled", si.seq),
        other => {
            println!(
                "F003-7. redemption NOT settled: {:?}",
                other.map(|s| s.status)
            );
            run.failures += 1;
        }
    }
    print_balances(&run, &m, &cm, &[("A", a), ("treasury", ta.pubkey())]);
    finish(&run)
}

fn print_balances(run: &Run, m: &Pubkey, cm: &Pubkey, who: &[(&str, Pubkey)]) {
    let bal = |acct: &Pubkey| {
        run.rpc
            .get_account(acct)
            .ok()
            .and_then(|a| {
                t22::extension::StateWithExtensions::<t22::state::Account>::unpack(&a.data)
                    .ok()
                    .map(|s| s.base.amount)
            })
            .unwrap_or(0)
    };
    println!("\nBalances (units, cash):");
    for (name, o) in who {
        println!(
            "  {name}: {} units, ${}",
            bal(&pda::holding_account(o, m)) / UNIT,
            bal(&ata(o, cm)) / UNIT
        );
    }
    if let Some(fs) = read::<FundState>(&run.rpc, &pda::fund_state(m).0) {
        println!("  units outstanding: {}", fs.units_outstanding / UNIT);
    }
}
