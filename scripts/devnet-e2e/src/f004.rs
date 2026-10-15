//! F004 on a real cluster (quickstart.md "Devnet run", research R16). Every role is a plain key,
//! as in the F003 steps; the Squads path for dispute resolution is proven in LiteSVM
//! (`f004_dispute.rs`).
//!
//! `devnet-e2e --feature f004` builds a new fund, so its Registry names the collateral program
//! (registries created before F004 can't hold liens). It then:
//! 1. launches the fund, publishes $1.00, sets up settlement and onboards giver G and taker T;
//!    G buys 2 000 000 units by an F003 subscription;
//! 2. T proposes the worked agreement and G signs it;
//! 3. T sets exposure $1 000 000 and a call opens. G's pledge of 1 020 408.163266 units is
//!    prepared on G's durable nonce and signed 130 s later. The call is Met in that transaction,
//!    and G's balance is unchanged;
//! 4. G tries to send 979 591.836735 units and gets `PledgedUnitsLocked`; it then sends
//!    979 591.836734 units;
//! 5. T lowers exposure to $400 000, G opens a return call, and the keeper releases
//!    612 244.897959 units against it. The return call is Met and G's lock is 408 163.265307.
//!
//! Each step prints its signature; the run exits 0 only if every outcome matches.

use std::process::ExitCode;
use std::time::Duration;

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022 as t22;
use collateral::{CollateralCall, PledgeAgreement};
use compliance_hook::{HolderState, HookError};
use dvp_settlement::CreateArgs;
use mmf_common::enums::CallStatus;
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_sdk::ix_collateral::{self as ixc, Agr};
use mmf_sdk::ix_dvp::{self, Deal};
use mmf_sdk::ix_governance::{self, Roles};
use mmf_sdk::{ix_fund, ix_hook, kyc, pda, prepare, transfer};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_signer::Signer;

use crate::{days_for, finish, read, today, Expect, Run, ONE, YEAR};

/// One whole unit or one dollar (6 dp).
const UNIT: u64 = 1_000_000;
const GIVER_UNITS: u64 = 2_000_000 * UNIT;
const MILLION: u64 = 1_000_000 * UNIT;
const PLEDGE: u64 = 1_020_408_163_266;
const UNLOCKED: u64 = GIVER_UNITS - PLEDGE;
const RETURN_UNITS: u64 = 612_244_897_959;
const LOCK_AFTER: u64 = 408_163_265_307;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    pda::ata(owner, mint, &TOKEN_2022_PROGRAM_ID)
}

fn r(n: u8) -> [u8; 32] {
    [n; 32]
}

impl Run {
    /// Records a check on chain state read after the steps.
    fn check(&mut self, label: &str, ok: bool, detail: String) {
        self.step += 1;
        if ok {
            println!("[{:>2}] {label}: ok   {detail}", self.step);
        } else {
            println!("[{:>2}] {label}: MISMATCH {detail}", self.step);
            self.failures += 1;
        }
    }

    fn balance(&self, mint: &Pubkey, owner: &Pubkey) -> u64 {
        self.rpc
            .get_account_data(&pda::holding_account(owner, mint))
            .ok()
            .and_then(|d| {
                t22::extension::StateWithExtensions::<t22::state::Account>::unpack(&d)
                    .ok()
                    .map(|s| s.base.amount)
            })
            .unwrap_or(0)
    }

    fn locked(&self, mint: &Pubkey, owner: &Pubkey) -> u64 {
        read::<HolderState>(&self.rpc, &pda::holder(mint, owner).0)
            .map(|h| h.pledged_amount)
            .unwrap_or(u64::MAX)
    }

    /// A call's derived status (data-model.md): Open or Disputed delivery calls below the
    /// agreement's `met_below_seq` are Met.
    fn call_status(&self, call: &Pubkey) -> Option<u8> {
        let c: CollateralCall = read(&self.rpc, call)?;
        let a: PledgeAgreement = read(&self.rpc, &c.agreement)?;
        let unresolved =
            c.status == CallStatus::Open.as_u8() || c.status == CallStatus::Disputed.as_u8();
        Some(if c.kind == 0 && unresolved && c.seq < a.met_below_seq {
            CallStatus::Met.as_u8()
        } else {
            c.status
        })
    }
}

/// `--feature f004`.
pub fn run(mut run: Run) -> ExitCode {
    println!("F004 end-to-end run as {}", run.payer.pubkey());
    let issuer = run.payer.insecure_clone();
    let i = issuer.pubkey();
    let (mint, cash) = (Keypair::new(), Keypair::new());
    let (m, cm) = (mint.pubkey(), cash.pubkey());
    let (compliance, provider) = (Keypair::new(), Keypair::new());
    let (admin, ta, keeper) = (Keypair::new(), Keypair::new(), Keypair::new());
    let (giver, taker) = (Keypair::new(), Keypair::new());
    for k in [&compliance, &admin, &ta, &keeper, &giver, &taker] {
        run.fund(&k.pubkey(), 100_000_000);
    }

    // 1. A mock stablecoin and a launched fund priced at $1.00 today.
    let lamports = run
        .rpc
        .get_minimum_balance_for_rent_exemption(t22::state::Mint::LEN)
        .unwrap_or(0);
    run.send(
        "F004-1. create the mock stablecoin",
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
    let mut terms_uri = [0u8; 128];
    terms_uri[..30].copy_from_slice(b"https://example.org/terms.pdf\0");
    let terms = fund::FundTerms {
        fund_id: *b"MMF-DEVNET-4",
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
        "F004-1. init_roles + create_fund",
        &[
            ix_governance::init_roles(&i, &m),
            ix_fund::create_fund(&i, &m, &cm, terms, "Devnet Collateral MMF", "DCMMF"),
        ],
        &[&mint],
        Expect::Ok,
    );
    run.send(
        "F004-1. register rules, roles, launch",
        &[
            ix_hook::init_registry(&i, &m),
            ix_hook::set_rules(&i, &m, providers, allowed, 0),
            ix_governance::set_roles(&i, &m, &roles),
            ix_fund::launch(&i, &m),
        ],
        &[],
        Expect::Ok,
    );
    let registry: Option<compliance_hook::Registry> = read(&run.rpc, &pda::registry(&m).0);
    run.check(
        "F004-1. the new Registry names the collateral program",
        registry.map(|r| r.collateral_program) == Some(mmf_common::ids::COLLATERAL_PROGRAM_ID),
        String::new(),
    );
    run.send(
        "F004-1. publish $1.00",
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

    // Settlement setup and the two investors.
    let fs = pda::fund_state(&m).0;
    let treasury = ata(&ta.pubkey(), &cm);
    let escrow = pda::escrow_authority(&fs).0;
    let c = compliance.pubkey();
    run.send(
        "F004-1. treasury cash account, set_cash_treasury, init_dvp",
        &[
            ix_hook::create_holding_account(&i, &cm, &ta.pubkey()),
            ix_fund::set_cash_treasury(&i, &m, &treasury, &cm),
            ix_dvp::init_dvp(&i, &i, &m, &cm, &TOKEN_2022_PROGRAM_ID, None),
        ],
        &[],
        Expect::Ok,
    );
    run.send(
        "F004-1. compliance approves the escrow; thaw",
        &[
            ix_hook::register_holder(&c, &c, &m, &escrow, 2),
            ix_hook::approve_fund_account(&c, &m, &escrow),
            ix_hook::thaw_holding_account(&i, &m, &escrow, &pda::unit_escrow(&fs, &m)),
        ],
        &[&compliance],
        Expect::Ok,
    );
    for (name, k) in [("G", &giver), ("T", &taker)] {
        let o = k.pubkey();
        run.send(
            &format!("F004-1. {name} registers itself"),
            &[ix_hook::register_holder(&o, &o, &m, &o, 0)],
            &[k],
            Expect::Ok,
        );
        let signed = kyc::sign(&provider, &kyc::statement(m, o, *b"US", 1, now() + YEAR));
        let ixs = ix_hook::onboarding_tx(&i, &c, &m, &o, &signed);
        run.send(
            &format!("F004-1. compliance approves {name}"),
            &ixs,
            &[&compliance],
            Expect::Ok,
        );
    }

    // G buys 2 000 000 units by subscription (F003).
    let g = giver.pubkey();
    run.send(
        "F004-1. G gets $2 000 000",
        &[
            ix_hook::create_holding_account(&i, &cm, &g),
            t22::instruction::mint_to(
                &TOKEN_2022_PROGRAM_ID,
                &cm,
                &ata(&g, &cm),
                &i,
                &[],
                GIVER_UNITS,
            )
            .unwrap(),
        ],
        &[],
        Expect::Ok,
    );
    let sub = CreateArgs {
        kind: 0,
        seller: fs,
        buyer: g,
        units: GIVER_UNITS,
        cash_amount: 0,
        not_before_ts: 0,
        expires_at_ts: 0,
        external_ref: r(1),
    };
    let d = Deal {
        mint: m,
        cash_mint: cm,
        cash_program: TOKEN_2022_PROGRAM_ID,
        kind: 0,
        seller: fs,
        buyer: g,
        created_by: g,
        external_ref: r(1),
    };
    run.send(
        "F004-1. G subscribes for 2 000 000 units",
        &[ix_dvp::create_instruction(&g, &i, &m, sub)],
        &[&giver],
        Expect::Ok,
    );
    run.send(
        "F004-1. transfer agent affirms",
        &[ix_dvp::affirm_fund_side(&ta.pubkey(), &d)],
        &[&ta],
        Expect::Ok,
    );
    run.send(
        "F004-1. G funds the cash",
        &[ix_dvp::fund_cash(&g, &d, &ata(&g, &cm))],
        &[&giver],
        Expect::Ok,
    );
    run.send(
        "F004-1. keeper settles",
        &[ix_dvp::settle_subscription(&keeper.pubkey(), &d, &treasury)],
        &[&keeper],
        Expect::Ok,
    );
    let held = run.balance(&m, &g);
    run.check(
        "F004-1. G holds 2 000 000 units",
        held == GIVER_UNITS,
        format!("{held}"),
    );

    // 2. The worked agreement.
    let t = taker.pubkey();
    let a = Agr {
        mint: m,
        giver: g,
        taker: t,
        nonce: 1,
    };
    let args = collateral::ProposeArgs {
        giver: g,
        taker: t,
        nonce: 1,
        mode: 0,
        income_to: 0,
        haircut_bps: 200,
        max_nav_age_secs: 93_600,
        call_cure_secs: 14_400,
        enforcement_grace_secs: 7_200,
        min_transfer_value: 10_000 * UNIT,
        concentration_cap_units: 5_000_000 * UNIT,
        taker_may_call_unilaterally: true,
        agreement_hash: [7; 32],
    };
    run.send(
        "F004-2. T proposes the agreement",
        &[ixc::propose_agreement(&t, &i, &m, args)],
        &[&taker],
        Expect::Ok,
    );
    run.send(
        "F004-2. G signs it",
        &[ixc::sign_agreement(&g, &a)],
        &[&giver],
        Expect::Ok,
    );

    // 3. A $1M call, met by a pledge prepared on G's durable nonce.
    run.send(
        "F004-3. T sets exposure $1 000 000",
        &[ixc::update_exposure(&t, &i, &a, MILLION, r(1))],
        &[&taker],
        Expect::Ok,
    );
    let call = a.call(&r(1));
    let opened: Option<CollateralCall> = read(&run.rpc, &call);
    run.check(
        "F004-3. a call opened for $1 000 000 needing 1 020 408.163266 units",
        opened.as_ref().map(|c| (c.value_delta, c.units_required)) == Some((MILLION, PLEDGE)),
        format!("{:?}", opened.map(|c| (c.value_delta, c.units_required))),
    );
    let nonce = Keypair::new();
    let rent = run
        .rpc
        .get_minimum_balance_for_rent_exemption(80)
        .unwrap_or(0);
    run.send(
        "F004-3. G's durable nonce account",
        &solana_system_interface::instruction::create_nonce_account(&i, &nonce.pubkey(), &g, rent),
        &[&nonce],
        Expect::Ok,
    );
    let Some(value) = run
        .rpc
        .get_account(&nonce.pubkey())
        .ok()
        .and_then(|acc| prepare::read_nonce(&acc.data))
    else {
        println!("F004-3. cannot read the nonce value");
        run.failures += 1;
        return finish(&run);
    };
    let message = prepare::with_durable_nonce(
        &[ixc::pledge(&g, &g, &a, PLEDGE, r(9))],
        &nonce.pubkey(),
        &g,
        &g,
        value,
    );
    println!("F004-3. pledge prepared; waiting 130 s (past any recent blockhash) before signing…");
    std::thread::sleep(Duration::from_secs(130));
    run.send_prepared(
        "F004-3. G signs and submits the prepared pledge",
        message,
        &[&giver],
        Expect::Ok,
    );
    let (held, lock) = (run.balance(&m, &g), run.locked(&m, &g));
    run.check(
        "F004-3. nothing moved; the units are locked",
        held == GIVER_UNITS && lock == PLEDGE,
        format!("balance {held}, locked {lock}"),
    );
    let status = run.call_status(&call);
    run.check(
        "F004-3. the call is Met",
        status == Some(CallStatus::Met.as_u8()),
        format!("{status:?}"),
    );

    // 4. The token refuses to move locked units.
    let locked_err: u32 = HookError::PledgedUnitsLocked.into();
    run.send(
        "F004-4. G sends 979 591.836735 units (one over the lock)",
        &[transfer::transfer_between_owners(&m, &g, &t, UNLOCKED + 1)],
        &[&giver],
        Expect::Err(locked_err, "PledgedUnitsLocked"),
    );
    run.send(
        "F004-4. G sends 979 591.836734 units",
        &[transfer::transfer_between_owners(&m, &g, &t, UNLOCKED)],
        &[&giver],
        Expect::Ok,
    );

    // 5. Exposure falls; the keeper releases the excess against G's return call.
    run.send(
        "F004-5. T lowers exposure to $400 000",
        &[ixc::update_exposure(&t, &i, &a, 400_000 * UNIT, r(2))],
        &[&taker],
        Expect::Ok,
    );
    run.send(
        "F004-5. G opens a return call",
        &[ixc::issue_call(&g, &i, &a, 1, r(3))],
        &[&giver],
        Expect::Ok,
    );
    let ret = a.call(&r(3));
    let k = keeper.pubkey();
    run.send(
        "F004-5. keeper releases 612 244.897959 units",
        &[ixc::release(
            &k,
            &a,
            &a.pledge(&r(9)),
            Some(ret),
            RETURN_UNITS,
        )],
        &[&keeper],
        Expect::Ok,
    );
    let lock = run.locked(&m, &g);
    let status = run.call_status(&ret);
    run.check(
        "F004-5. G's lock is 408 163.265307 and the return call is Met",
        lock == LOCK_AFTER && status == Some(CallStatus::Met.as_u8()),
        format!("locked {lock}, status {status:?}"),
    );
    println!("\nFund mint {m}, agreement {}", a.agreement());
    finish(&run)
}
