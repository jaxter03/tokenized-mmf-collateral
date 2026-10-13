//! T032: randomized 90-day accrual simulations (SC-001, SC-002, SC-007, I11, I12; with I3/I10).
//!
//! Each case seeds random balances before the first publish, then runs publishes covering about
//! 90 days (mostly 1 day, Fridays 3, occasional 2-day catch-ups after an outage) with random
//! transfers, suspensions and pauses in between. After **every** step each holder's earned
//! interest must equal the independent day-by-day model (`tests::model::AccrualModel`) exactly.
//! At the end an auditor's rebuild from the records alone must agree too.
//!
//! `PROPTEST_CASES` raises the number of cases (default 64).

use std::collections::HashMap;

use anchor_lang::prelude::Pubkey;
use compliance_hook::events::TransferValidated;
use compliance_hook::{HolderState, Registry};
use fund::FundState;
use mmf_common::enums::ReasonCode;
use mmf_sdk::{ix_governance, pda};
use proptest::prelude::*;
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::events::log_events;
use tests::fixtures::{self, check_i12, read, read_nav_record, FundCtx};
use tests::model::AccrualModel;
use tests::rebuild::{rebuild, Record};
use tests::{clock, units, Harness};

const INVESTORS: usize = 6;
/// Holder index of the fund treasury: receives only (its owner is a fund PDA nobody signs for).
const TREASURY: usize = INVESTORS;
const HOLDERS: usize = INVESTORS + 1;
/// Tuesday 2026-10-13.
const FIRST_DAY: u32 = 20_739;
const SIM_DAYS: u32 = 90;
const ONE: u64 = 100_000_000;
const REASON: u8 = ReasonCode::OperationalError as u8;

#[derive(Clone, Debug)]
enum Op {
    /// Send `pct` % of `from`'s balance (0 % settles both sides and moves nothing).
    Transfer {
        from: usize,
        to: usize,
        pct: u64,
    },
    Suspend(usize),
    Reinstate(usize),
    Pause,
    Unpause,
}

#[derive(Clone, Debug)]
struct Publish {
    rate: i64,
    days: u16,
    /// Published late, on the period's last day, after an outage.
    late: bool,
}

fn publish_op() -> impl Strategy<Value = Publish> {
    (
        0i64..=500_000_000,
        prop_oneof![6 => Just(1u16), 2 => Just(3u16), 1 => Just(2u16)],
        any::<bool>(),
    )
        .prop_map(|(rate, days, late)| Publish {
            rate,
            days,
            late: late && days > 1,
        })
}

fn other_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        10 => (0..INVESTORS, 0..HOLDERS, prop_oneof![Just(0u64), 1u64..=100])
            .prop_map(|(from, to, pct)| Op::Transfer { from, to, pct }),
        1 => (0..INVESTORS).prop_map(Op::Suspend),
        2 => (0..INVESTORS).prop_map(Op::Reinstate),
        1 => Just(Op::Pause),
        2 => Just(Op::Unpause),
    ]
}

struct World {
    h: Harness,
    ctx: FundCtx,
    keys: Vec<Keypair>,
    owners: Vec<Pubkey>,
    model: AccrualModel,
    start: HashMap<Pubkey, u64>,
    records: Vec<Record>,
}

impl World {
    fn new(seed: &[u64]) -> Self {
        let mut h = Harness::new();
        clock::set_utc_day(&mut h.svm, FIRST_DAY, 15 * 3_600);
        let ctx = fixtures::launched_fund(&mut h, 0);
        let keys: Vec<Keypair> = (0..INVESTORS)
            .map(|_| fixtures::new_investor(&mut h, &ctx))
            .collect();
        let mut owners: Vec<Pubkey> = keys.iter().map(|k| k.pubkey()).collect();
        owners.push(fixtures::onboard_treasury(&mut h, &ctx));
        let mut model = AccrualModel::new(HOLDERS);
        let mut start = HashMap::new();
        for (i, amount) in seed.iter().enumerate() {
            if *amount > 0 {
                units::seed_units(&mut h, &ctx.m(), &owners[i], *amount);
                model.seed(i, *amount);
            }
            start.insert(owners[i], *amount);
        }
        Self {
            h,
            ctx,
            keys,
            owners,
            model,
            start,
            records: vec![],
        }
    }

    fn today(&self) -> u32 {
        mmf_common::accrual::utc_day(clock::now(&self.h.svm)).unwrap()
    }

    fn covered_days(&self) -> u32 {
        self.model
            .period_end_day
            .map_or(0, |end| end + 1 - FIRST_DAY)
    }

    fn publish(&mut self, p: &Publish) {
        let start = self.model.next_start(self.today());
        // The first publish always starts on its own day, so only later ones can be late.
        let late = p.late && self.model.period_end_day.is_some();
        let day = if late {
            start + u32::from(p.days) - 1
        } else {
            start
        };
        clock::set_utc_day(&mut self.h.svm, day, 15 * 3_600);
        fixtures::publish(&mut self.h, &self.ctx, ONE, p.rate, p.days).expect("publish");
        self.model.publish(start, ONE, p.rate as u64, p.days);
        let seq = read::<FundState>(&self.h, &self.ctx.fund_state()).nav_seq;
        self.records.push(Record::Nav {
            index_after: read_nav_record(&self.h, &self.ctx, seq).index_after,
        });
    }

    fn compliance(&mut self, ix: anchor_lang::solana_program::instruction::Instruction) {
        let c = self.ctx.compliance.insecure_clone();
        let _ = self.h.send(&[ix], &[&c]); // may fail (e.g. already paused); outcomes are F001's
    }

    fn step(&mut self, op: &Op) {
        let (m, c) = (self.ctx.m(), self.ctx.compliance.pubkey());
        match *op {
            Op::Transfer { from, to, pct } => {
                let amount = self.model.balances[from] / 100 * pct;
                let sender = self.keys[from].insecure_clone();
                let to_owner = self.owners[to];
                if let Ok(meta) =
                    fixtures::transfer(&mut self.h, &self.ctx, &sender, &to_owner, amount)
                {
                    if from != to {
                        self.model.transfer(from, to, amount);
                    }
                    for e in log_events::<TransferValidated>(&meta) {
                        self.records.push(Record::Transfer {
                            source: e.source_owner,
                            destination: e.destination_owner,
                            amount: e.amount,
                            source_accrued_after: e.source_accrued_e18,
                            destination_accrued_after: e.destination_accrued_e18,
                        });
                    }
                }
            }
            Op::Suspend(i) => {
                let o = self.owners[i];
                self.compliance(ix_governance::suspend_holder(&c, &m, &o, REASON, [0; 32]));
            }
            Op::Reinstate(i) => {
                let o = self.owners[i];
                self.compliance(ix_governance::reinstate_holder(&c, &m, &o, REASON, [0; 32]));
            }
            Op::Pause => self.compliance(ix_governance::pause_transfers(&c, &m, REASON, [0; 32])),
            Op::Unpause => {
                self.compliance(ix_governance::unpause_transfers(&c, &m, REASON, [0; 32]))
            }
        }
    }

    fn holder(&self, i: usize) -> HolderState {
        read(&self.h, &pda::holder(&self.ctx.m(), &self.owners[i]).0)
    }

    fn check(&self, after: &str) {
        let fs: FundState = read(&self.h, &self.ctx.fund_state());
        assert_eq!(
            fs.accrual_index, self.model.index_e18,
            "counter after {after}"
        );
        let mut total = 0u128;
        let mut counted = 0u32;
        for i in 0..HOLDERS {
            let rec = self.holder(i);
            // SC-001 / I11: exact, per holder.
            let earned = mmf_sdk::accrual::earned_e18(&rec, &fs).unwrap();
            assert_eq!(
                earned, self.model.earned_e18[i],
                "holder {i} earned after {after}"
            );
            assert!(
                rec.index_at_last_touch <= fs.accrual_index,
                "I12 checkpoint, holder {i}"
            );
            total += earned;
            // I10 still holds with publishes interleaved.
            let balance = units::balance(&self.h, &self.ctx.m(), &self.owners[i]);
            assert_eq!(
                balance, self.model.balances[i],
                "balance of holder {i} after {after}"
            );
            assert_eq!(rec.last_balance, balance, "I10 last_balance, holder {i}");
            if i != TREASURY {
                counted += u32::from(balance > 0);
            }
        }
        // SC-002: nothing lost or created across holders.
        assert_eq!(
            total,
            self.model.total_earned(),
            "sum over holders after {after}"
        );
        let reg: Registry = read(&self.h, &self.ctx.registry());
        assert_eq!(reg.holder_count, counted, "I10 holder_count after {after}");
        check_i12(&self.h, &self.ctx);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn ninety_days_match_the_day_by_day_calculation(
        seed in prop::collection::vec(prop_oneof![Just(0u64), 1u64..10_000_000_000_000], HOLDERS),
        steps in prop::collection::vec((publish_op(), prop::collection::vec(other_op(), 0..5)), 90),
    ) {
        let mut w = World::new(&seed);
        w.check("seeding");
        for (p, ops) in &steps {
            if w.covered_days() >= SIM_DAYS {
                break;
            }
            w.publish(p);
            w.check(&format!("{p:?}"));
            for op in ops {
                w.step(op);
                w.check(&format!("{op:?}"));
            }
        }
        prop_assert!(w.covered_days() >= SIM_DAYS, "covered {} days", w.covered_days());

        // SC-007: an auditor's rebuild from the records alone agrees with every holder.
        let rebuilt = rebuild(&w.start, &w.records);
        let fs: FundState = read(&w.h, &w.ctx.fund_state());
        for i in 0..HOLDERS {
            let stored = mmf_sdk::accrual::earned_e18(&w.holder(i), &fs).unwrap();
            prop_assert_eq!(rebuilt[&w.owners[i]], stored, "rebuild of holder {}", i);
        }
    }
}
