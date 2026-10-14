//! T071: random settlement sequences against the independent model (`tests/src/dvp_model.rs`),
//! SC-001, SC-002, SC-003, SC-005; invariants I4, I6, I10, I13, I14.
//!
//! Each case sets up a fund with four investors, then runs random operations: create (all
//! kinds, sometimes by the wrong party), fund either side, settle, cancel, withdraw a cancel
//! request, expire, reject, return either leg, suspend and reinstate, pause and unpause, publish,
//! and move the clock. Every operation goes to the programs and to the model, and their outcomes
//! must agree, including which error comes first. After every operation, every balance must
//! match the model and the invariants must hold.
//!
//! Errors are compared by name: the variant name from Anchor's `Error Code:` log line, or
//! `InsufficientFunds` / `AlreadyInUse` for the token and system programs. Publishes use a rate of
//! 0; exact interest is property-tested by `accrual_prop.rs` (F002) and checked across mint and
//! burn by `f003_accrual.rs`. `PROPTEST_CASES` raises the number of cases (default 24); the
//! `#[ignore]`d variant runs 200 in release mode.

use compliance_hook::Registry;
use dvp_settlement::SettlementInstruction;
use fund::FundState;
use mmf_sdk::ix_dvp::{self, Deal};
use proptest::prelude::*;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use solana_transaction_error::TransactionError;
use tests::dvp_model::{self as model, Actor, CreateArgs as MArgs, Kind, Leg, Model, Status, Who};
use tests::fixtures::{self, read, SettleCtx, NAV_ONE};
use tests::settle::{self, ONE};
use tests::{clock, units, Harness, TxResult};

const INVESTORS: usize = 4;
const START_UNITS: u64 = 1_000 * ONE;
const START_CASH: u64 = 1_000 * ONE;
const TREASURY: u64 = 5_000 * ONE;
const GATE_BPS: u64 = 1_000;
const CASE: [u8; 32] = [0xC5; 32];

#[derive(Clone, Debug)]
enum Op {
    Create {
        kind: u8,
        a: u8,
        b: u8,
        units: u16,
        cash: u16,
        creator: u8,
    },
    Fund {
        id: u8,
        side: u8,
        right_party: bool,
    },
    Settle {
        id: u8,
    },
    Cancel {
        id: u8,
        side: u8,
        right_party: bool,
    },
    Withdraw {
        id: u8,
        side: u8,
    },
    Expire {
        id: u8,
    },
    Reject {
        id: u8,
        reason: u8,
        by_compliance: bool,
    },
    Return {
        id: u8,
        leg: u8,
    },
    Suspend {
        i: u8,
    },
    Reinstate {
        i: u8,
    },
    Pause,
    Unpause,
    Publish {
        nav: u8,
    },
    Advance {
        step: u8,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0u8..3, 0u8..4, 0u8..4, 0u16..500, 0u16..300, 0u8..8).prop_map(
            |(kind, a, b, units, cash, creator)| Op::Create { kind, a, b, units, cash, creator }
        ),
        8 => (any::<u8>(), 0u8..2, prop::bool::weighted(0.9))
            .prop_map(|(id, side, right_party)| Op::Fund { id, side, right_party }),
        4 => any::<u8>().prop_map(|id| Op::Settle { id }),
        2 => (any::<u8>(), 0u8..2, prop::bool::weighted(0.9))
            .prop_map(|(id, side, right_party)| Op::Cancel { id, side, right_party }),
        1 => (any::<u8>(), 0u8..2).prop_map(|(id, side)| Op::Withdraw { id, side }),
        2 => any::<u8>().prop_map(|id| Op::Expire { id }),
        1 => (any::<u8>(), prop::sample::select(vec![1u8, 3, 4]), prop::bool::weighted(0.9))
            .prop_map(|(id, reason, by_compliance)| Op::Reject { id, reason, by_compliance }),
        3 => (any::<u8>(), 0u8..2).prop_map(|(id, leg)| Op::Return { id, leg }),
        1 => (0u8..4).prop_map(|i| Op::Suspend { i }),
        1 => (0u8..4).prop_map(|i| Op::Reinstate { i }),
        1 => Just(Op::Pause),
        1 => Just(Op::Unpause),
        1 => (0u8..3).prop_map(|nav| Op::Publish { nav }),
        2 => (0u8..4).prop_map(|step| Op::Advance { step }),
    ]
}

struct World {
    h: Harness,
    s: SettleCtx,
    inv: Vec<Keypair>,
    stranger: Keypair,
    m: Model,
    deals: Vec<Deal>,
    next_ref: u8,
    paused: bool,
}

/// An operation's outcome by name (module docs).
fn outcome(res: &TxResult) -> Result<(), String> {
    let f = match res {
        Ok(_) => return Ok(()),
        Err(f) => f,
    };
    if let Some(code) = f.meta.logs.iter().find_map(|l| {
        l.split("Error Code: ")
            .nth(1)
            .map(|rest| rest.split('.').next().unwrap_or("").to_string())
    }) {
        return Err(code);
    }
    let logs = f.meta.logs.join("\n");
    match &f.err {
        TransactionError::InstructionError(_, InstructionError::Custom(1))
            if logs.contains("insufficient funds") =>
        {
            Err("InsufficientFunds".into())
        }
        _ if logs.contains("already in use") => Err("AlreadyInUse".into()),
        other => Err(format!("{other:?}")),
    }
}

fn model_out(r: model::ModelResult<impl Sized>) -> Result<(), String> {
    r.map(|_| ()).map_err(str::to_string)
}

fn status_of(s: Status) -> u8 {
    match s {
        Status::Instructed => 0,
        Status::SellerFunded => 1,
        Status::BuyerFunded => 2,
        Status::Affirmed => 3,
        Status::Settled => 4,
        Status::Cancelled => 5,
        Status::Expired => 6,
        Status::Rejected => 7,
    }
}

impl World {
    fn new() -> Self {
        let mut h = Harness::new();
        let s = fixtures::settlement_ready_fund(&mut h);
        let inv: Vec<Keypair> = (0..INVESTORS)
            .map(|_| fixtures::investor_with_units(&mut h, &s, START_UNITS, START_CASH))
            .collect();
        settle::fill_treasury(&mut h, &s, TREASURY);
        let stranger = h.funded_keypair();
        let mut m = Model::new(
            clock::now(&h.svm),
            &[(START_UNITS, START_CASH); INVESTORS],
            GATE_BPS,
        );
        m.treasury_cash = TREASURY;
        m.publish(NAV_ONE, 1).expect("mirror the fixture's publish");
        Self {
            h,
            s,
            inv,
            stranger,
            m,
            deals: Vec::new(),
            next_ref: 1,
            paused: false,
        }
    }

    fn key(&self, a: Actor) -> &Keypair {
        match a {
            Actor::Inv(i) => &self.inv[i],
            Actor::TransferAgent => &self.s.transfer_agent,
            Actor::Keeper => &self.s.keeper,
            Actor::Compliance => &self.s.compliance,
            Actor::Other => &self.stranger,
        }
    }

    fn who_key(&self, w: Who) -> anchor_lang::prelude::Pubkey {
        match w {
            Who::Fund => self.s.fund_state(),
            Who::Inv(i) => self.inv[i].pubkey(),
        }
    }

    /// The party of `side`, or (if `!right`) someone else.
    fn side_actor(&self, id: usize, side: model::Side, right: bool) -> Actor {
        let ins = &self.m.instrs[id];
        let who = match side {
            model::Side::Seller => ins.seller,
            model::Side::Buyer => ins.buyer,
        };
        let party = match who {
            Who::Fund => Actor::TransferAgent,
            Who::Inv(i) => Actor::Inv(i),
        };
        if right {
            party
        } else {
            match party {
                Actor::Inv(i) => Actor::Inv((i + 1) % INVESTORS),
                _ => Actor::Keeper,
            }
        }
    }

    fn pick(&self, id: u8) -> Option<usize> {
        (!self.deals.is_empty()).then(|| id as usize % self.deals.len())
    }

    fn step(&mut self, op: &Op) -> (Result<(), String>, Result<(), String>) {
        let s = &self.s;
        match *op {
            Op::Create {
                kind,
                a,
                b,
                units,
                cash,
                creator,
            } => {
                let (kind, seller, buyer) = match kind {
                    0 => (Kind::Sub, Who::Fund, Who::Inv(b as usize)),
                    1 => (Kind::Red, Who::Inv(a as usize), Who::Fund),
                    _ => (Kind::Sec, Who::Inv(a as usize), Who::Inv(b as usize)),
                };
                let n = self.next_ref;
                self.next_ref = self.next_ref.wrapping_add(1).max(1);
                // Mostly the right creator; sometimes the transfer agent, keeper or a stranger.
                let right = match kind {
                    Kind::Sub => buyer,
                    Kind::Red => seller,
                    Kind::Sec if creator % 2 == 0 => seller,
                    Kind::Sec => buyer,
                }
                .into_actor();
                let creator = match creator {
                    0..=3 => right,
                    4 | 5 => Actor::TransferAgent,
                    6 => Actor::Keeper,
                    _ => Actor::Other,
                };
                let units = u64::from(units) * ONE / 2;
                let cash = if kind == Kind::Sec {
                    u64::from(cash) * ONE / 2
                } else {
                    0
                };
                let args = dvp_settlement::CreateArgs {
                    kind: match kind {
                        Kind::Sub => settle::SUBSCRIPTION,
                        Kind::Red => settle::REDEMPTION,
                        Kind::Sec => settle::SECONDARY,
                    },
                    seller: self.who_key(seller),
                    buyer: self.who_key(buyer),
                    units,
                    cash_amount: cash,
                    not_before_ts: 0,
                    expires_at_ts: 0,
                    external_ref: [n; 32],
                };
                let ck = self.key(creator).insecure_clone();
                let chain = outcome(&settle::create(&mut self.h, s, &ck, args));
                let m = self.m.create(
                    creator,
                    MArgs {
                        kind,
                        seller,
                        buyer,
                        units,
                        cash,
                        not_before: 0,
                        expires: 0,
                        external_ref: [n; 32],
                    },
                );
                if chain.is_ok() && m.is_ok() {
                    self.deals.push(settle::deal(s, &ck.pubkey(), &args));
                }
                (chain, model_out(m))
            }
            Op::Fund {
                id,
                side,
                right_party,
            } => {
                let Some(id) = self.pick(id) else {
                    return (Ok(()), Ok(()));
                };
                let side = if side == 0 {
                    model::Side::Seller
                } else {
                    model::Side::Buyer
                };
                let actor = self.side_actor(id, side, right_party);
                let k = self.key(actor).insecure_clone();
                let d = self.deals[id];
                let ix = match (side, d.kind) {
                    (model::Side::Seller, 0) => ix_dvp::affirm_fund_side(&k.pubkey(), &d),
                    (model::Side::Seller, _) => ix_dvp::fund_units(&k.pubkey(), &d),
                    (model::Side::Buyer, 1) => {
                        ix_dvp::fund_redemption_cash(&k.pubkey(), &d, &s.cash_treasury)
                    }
                    (model::Side::Buyer, _) => {
                        let src = fixtures::cash_ata(&self.h, &k.pubkey(), &s.cash_mint);
                        ix_dvp::fund_cash(&k.pubkey(), &d, &src)
                    }
                };
                let chain = outcome(&self.h.send(&[ix], &[&k]));
                (chain, model_out(self.m.fund(id, side, actor)))
            }
            Op::Settle { id } => {
                let Some(id) = self.pick(id) else {
                    return (Ok(()), Ok(()));
                };
                let d = self.deals[id];
                let k = &s.keeper;
                let ix = match d.kind {
                    0 => ix_dvp::settle_subscription(&k.pubkey(), &d, &s.cash_treasury),
                    1 => ix_dvp::settle_redemption(&k.pubkey(), &d),
                    _ => ix_dvp::settle_secondary(&k.pubkey(), &d),
                };
                let chain = outcome(&self.h.send(&[ix], &[k]));
                (chain, model_out(self.m.settle(id)))
            }
            Op::Cancel {
                id,
                side,
                right_party,
            } => {
                let Some(id) = self.pick(id) else {
                    return (Ok(()), Ok(()));
                };
                let mside = if side == 0 {
                    model::Side::Seller
                } else {
                    model::Side::Buyer
                };
                let actor = self.side_actor(id, mside, right_party);
                let k = self.key(actor).insecure_clone();
                let chain = outcome(&settle::cancel(&mut self.h, &self.deals[id], &k, side));
                (chain, model_out(self.m.cancel(id, mside, actor)))
            }
            Op::Withdraw { id, side } => {
                let Some(id) = self.pick(id) else {
                    return (Ok(()), Ok(()));
                };
                let mside = if side == 0 {
                    model::Side::Seller
                } else {
                    model::Side::Buyer
                };
                let actor = self.side_actor(id, mside, true);
                let k = self.key(actor).insecure_clone();
                let chain = outcome(&settle::withdraw_cancel(
                    &mut self.h,
                    &self.deals[id],
                    &k,
                    side,
                ));
                (chain, model_out(self.m.withdraw_cancel(id, mside, actor)))
            }
            Op::Expire { id } => {
                let Some(id) = self.pick(id) else {
                    return (Ok(()), Ok(()));
                };
                let chain = outcome(&settle::expire(&mut self.h, &self.deals[id], &s.keeper));
                (chain, model_out(self.m.expire(id)))
            }
            Op::Reject {
                id,
                reason,
                by_compliance,
            } => {
                let Some(id) = self.pick(id) else {
                    return (Ok(()), Ok(()));
                };
                let actor = if by_compliance {
                    Actor::Compliance
                } else {
                    Actor::Keeper
                };
                let k = self.key(actor).insecure_clone();
                let ix = mmf_sdk::ix_governance::reject_settlement(
                    &k.pubkey(),
                    &s.m(),
                    &self.deals[id].settlement(),
                    reason,
                    CASE,
                );
                let chain = outcome(&self.h.send(&[ix], &[&k]));
                (chain, model_out(self.m.reject(id, actor, reason)))
            }
            Op::Return { id, leg } => {
                let Some(id) = self.pick(id) else {
                    return (Ok(()), Ok(()));
                };
                let d = self.deals[id];
                let (chain, m) = if leg == 0 {
                    (
                        settle::return_units(&mut self.h, &d, &s.keeper),
                        self.m.return_leg(id, Leg::Units),
                    )
                } else {
                    (
                        settle::return_cash(&mut self.h, s, &d, &s.keeper),
                        self.m.return_leg(id, Leg::Cash),
                    )
                };
                (outcome(&chain), model_out(m))
            }
            Op::Suspend { i } => {
                let i = i as usize;
                if self.m.investors[i].status != model::HolderStatus::Approved {
                    return (Ok(()), Ok(()));
                }
                let ix = mmf_sdk::ix_governance::suspend_holder(
                    &s.compliance.pubkey(),
                    &s.m(),
                    &self.inv[i].pubkey(),
                    1,
                    CASE,
                );
                self.m.suspend(i);
                (outcome(&self.h.send(&[ix], &[&s.compliance])), Ok(()))
            }
            Op::Reinstate { i } => {
                let i = i as usize;
                if self.m.investors[i].status != model::HolderStatus::Suspended {
                    return (Ok(()), Ok(()));
                }
                let ix = mmf_sdk::ix_governance::reinstate_holder(
                    &s.compliance.pubkey(),
                    &s.m(),
                    &self.inv[i].pubkey(),
                    1,
                    CASE,
                );
                self.m.reinstate(i);
                (outcome(&self.h.send(&[ix], &[&s.compliance])), Ok(()))
            }
            Op::Pause | Op::Unpause => {
                let pause = matches!(op, Op::Pause);
                if pause == self.paused {
                    return (Ok(()), Ok(()));
                }
                let c = s.compliance.pubkey();
                let ix = if pause {
                    mmf_sdk::ix_governance::pause_transfers(&c, &s.m(), 3, CASE)
                } else {
                    mmf_sdk::ix_governance::unpause_transfers(&c, &s.m(), 3, CASE)
                };
                self.paused = pause;
                self.m.set_paused(pause);
                (outcome(&self.h.send(&[ix], &[&s.compliance])), Ok(()))
            }
            Op::Publish { nav } => {
                let days = self.m.days_to_cover_today();
                if days < 1 {
                    return (Ok(()), Ok(()));
                }
                let nav = [NAV_ONE, 99_990_000, 100_010_000][nav as usize];
                let chain = outcome(&fixtures::publish(&mut self.h, s, nav, 0, days as u16));
                (chain, model_out(self.m.publish(nav, days)))
            }
            Op::Advance { step } => {
                let secs = [600, 3_600, 6 * 3_600, 25 * 3_600][step as usize];
                clock::advance(&mut self.h.svm, secs);
                self.m.advance(secs);
                (Ok(()), Ok(()))
            }
        }
    }

    fn check(&self, after: &Op) {
        let (h, s, m) = (&self.h, &self.s, &self.m);
        for (i, k) in self.inv.iter().enumerate() {
            assert_eq!(
                units::balance(h, &s.m(), &k.pubkey()),
                m.investors[i].units,
                "units of {i} after {after:?}"
            );
            assert_eq!(
                settle::cash_of(h, s, &k.pubkey()),
                m.investors[i].cash,
                "cash of {i} after {after:?}"
            );
        }
        assert_eq!(
            fixtures::cash_balance(h, &s.cash_treasury),
            m.treasury_cash,
            "treasury"
        );
        let escrow_units = units::token_balance(h, &s.unit_escrow);
        let escrow_cash = fixtures::cash_balance(h, &s.cash_escrow);
        assert_eq!(
            (escrow_units, escrow_cash),
            (m.escrow_units, m.escrow_cash),
            "escrow"
        );

        // I4 and SC-002: units outstanding is the supply, and units are conserved.
        let fs: FundState = read(h, &s.fund_state());
        assert_eq!(fs.units_outstanding, units::supply(h, &s.m()), "I4");
        assert_eq!(
            fs.units_outstanding, m.units_outstanding,
            "units outstanding"
        );
        assert!(m.units_conserved());
        // SC-001: cash is conserved across investors, treasury and escrow.
        let cash_total: u64 = self
            .inv
            .iter()
            .map(|k| settle::cash_of(h, s, &k.pubkey()))
            .sum::<u64>()
            + fixtures::cash_balance(h, &s.cash_treasury)
            + escrow_cash;
        assert_eq!(
            cash_total,
            INVESTORS as u64 * START_CASH + TREASURY,
            "cash conserved"
        );

        // I13 (equality: no stray deposits here) and statuses.
        let (mut u, mut c) = (0, 0);
        for (id, d) in self.deals.iter().enumerate() {
            let si: SettlementInstruction = read(h, &d.settlement());
            u += si.units_held();
            c += si.cash_held();
            assert_eq!(
                si.status,
                status_of(m.instrs[id].status),
                "status of {id} after {after:?}"
            );
        }
        assert_eq!((u, c), (escrow_units, escrow_cash), "I13");

        // I14 and I10.
        assert!(
            u128::from(fs.redeemed_today) * 10_000
                <= u128::from(fs.units_at_day_start) * u128::from(fs.terms.redemption_gate_bps),
            "I14"
        );
        assert!(m.redemption_limit_holds());
        let reg: Registry = read(h, &s.registry());
        let holding = self
            .inv
            .iter()
            .filter(|k| units::balance(h, &s.m(), &k.pubkey()) > 0)
            .count();
        assert_eq!(reg.holder_count as usize, holding, "I10");
    }
}

trait IntoActor {
    fn into_actor(self) -> Actor;
}

impl IntoActor for Who {
    fn into_actor(self) -> Actor {
        match self {
            Who::Fund => Actor::TransferAgent,
            Who::Inv(i) => Actor::Inv(i),
        }
    }
}

fn run(ops: &[Op]) {
    let mut w = World::new();
    for op in ops {
        let (chain, model) = w.step(op);
        assert_eq!(chain, model, "program and model disagree on {op:?}");
        w.check(op);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(24),
        ..ProptestConfig::default()
    })]

    #[test]
    fn settlement_matches_the_model(ops in prop::collection::vec(op(), 50..150)) {
        run(&ops);
    }
}

#[test]
#[ignore = "long: run with --release -- --ignored"]
fn settlement_matches_the_model_many_cases() {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 200,
        ..ProptestConfig::default()
    });
    runner
        .run(&prop::collection::vec(op(), 100..200), |ops| {
            run(&ops);
            Ok(())
        })
        .unwrap();
}
