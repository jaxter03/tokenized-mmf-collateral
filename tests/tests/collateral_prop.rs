//! T069: random collateral sequences against the independent model
//! (`tests/src/collateral_model.rs`); SC-002, SC-003, SC-005; invariants I1, I2, I15, I16.
//!
//! Each case sets up a fund with a giver G (2 000 000 units), a second giver B (1 000 000 units)
//! and a taker T, and two Active agreements, G→T and B→T. It then runs random operations:
//! - exposure up and down; delivery and return calls; re-checks;
//! - pledges (sometimes by the wrong party); releases by the taker or the keeper, with or
//!   without a return call; substitutions; cancels (sometimes by the wrong party); disputes and
//!   withdrawals;
//! - transfers between the holders; price publishes up and down; clock moves; suspensions and
//!   reinstatements.
//!
//! Every operation goes to the programs and to the model, and their outcomes must agree,
//! including which error comes first (by name: the variant from Anchor's `Error Code:` log line,
//! or `InsufficientFunds` / `AlreadyInUse` for the token and system programs).
//!
//! After every operation, these must match the model:
//! - every balance, lock and agreement field;
//! - every call's (derived) status.
//!
//! And I1, I2, I15 and I16 must hold on chain. `PROPTEST_CASES` sets the number of cases
//! (default 64); the `#[ignore]`d variant runs 256 in release mode.
//!
//! SC-005 is checked separately: for random values, prices and haircuts, the formulas the program
//! runs (`mmf_common::collateral`) never leave the taker under-covered.

use anchor_lang::prelude::Pubkey;
use collateral::events::ExposureUpdated;
use collateral::PledgeAgreement;
use compliance_hook::HolderState;
use mmf_common::enums::CallStatus as ChainStatus;
use mmf_sdk::ix_collateral::{self as ix, Agr};
use proptest::prelude::*;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::InstructionError;
use solana_transaction_error::TransactionError;
use tests::collateral::{call_status, propose, sign, Tracker};
use tests::collateral_model::{self as model, Actor, CallKind, CallStatus, Model, Terms};
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, CollatBase, GIVER_UNITS};
use tests::{clock, units, Harness, TxResult};

const B_UNITS: u64 = 1_000_000_000_000;
const NAVS: [u64; 5] = [
    99_000_000,
    99_500_000,
    100_000_000,
    100_500_000,
    101_000_000,
];
const STEPS: [i64; 4] = [600, 3_600, 6 * 3_600, 20 * 3_600];

#[derive(Clone, Debug)]
enum Op {
    Exposure {
        a: u8,
        k: u16,
    },
    Call {
        a: u8,
        ret: bool,
    },
    Check {
        a: u8,
    },
    Pledge {
        a: u8,
        k: u16,
        wrong: bool,
    },
    Release {
        p: u8,
        k: u16,
        keeper: bool,
        with_return: bool,
    },
    Substitute {
        p: u8,
        extra: u16,
    },
    Cancel {
        c: u8,
        wrong: bool,
    },
    Dispute {
        c: u8,
    },
    Withdraw {
        c: u8,
    },
    Transfer {
        from: u8,
        to: u8,
        k: u16,
    },
    Publish {
        nav: u8,
    },
    Advance {
        step: u8,
    },
    Suspend {
        i: u8,
    },
    Reinstate {
        i: u8,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0u8..2, 0u16..400).prop_map(|(a, k)| Op::Exposure { a, k }),
        4 => (0u8..2, prop::bool::weighted(0.4)).prop_map(|(a, ret)| Op::Call { a, ret }),
        2 => (0u8..2).prop_map(|a| Op::Check { a }),
        6 => (0u8..2, 0u16..120, prop::bool::weighted(0.1))
            .prop_map(|(a, k, wrong)| Op::Pledge { a, k, wrong }),
        5 => (any::<u8>(), 0u16..60, prop::bool::weighted(0.3), prop::bool::weighted(0.5))
            .prop_map(|(p, k, keeper, with_return)| Op::Release { p, k, keeper, with_return }),
        2 => (any::<u8>(), 0u16..30).prop_map(|(p, extra)| Op::Substitute { p, extra }),
        2 => (any::<u8>(), prop::bool::weighted(0.1)).prop_map(|(c, wrong)| Op::Cancel { c, wrong }),
        2 => any::<u8>().prop_map(|c| Op::Dispute { c }),
        1 => any::<u8>().prop_map(|c| Op::Withdraw { c }),
        3 => (0u8..3, 0u8..3, 0u16..80).prop_map(|(from, to, k)| Op::Transfer { from, to, k }),
        3 => (0u8..5).prop_map(|nav| Op::Publish { nav }),
        2 => (0u8..4).prop_map(|step| Op::Advance { step }),
        1 => (0u8..3).prop_map(|i| Op::Suspend { i }),
        2 => (0u8..3).prop_map(|i| Op::Reinstate { i }),
    ]
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

fn model_out<T>(r: &model::ModelResult<T>) -> Result<(), String> {
    r.as_ref().map(|_| ()).map_err(|e| e.to_string())
}

fn chain_status(s: CallStatus) -> ChainStatus {
    match s {
        CallStatus::Open => ChainStatus::Open,
        CallStatus::Met => ChainStatus::Met,
        CallStatus::Disputed => ChainStatus::Disputed,
        CallStatus::Cancelled => ChainStatus::Cancelled,
    }
}

fn worked_model_terms(haircut_bps: u16, min_transfer_value: u64, cap_units: u64) -> Terms {
    Terms {
        haircut_bps,
        min_transfer_value,
        cap_units,
        max_age_secs: 93_600,
        cure_secs: 14_400,
        grace_secs: 7_200,
        unilateral: true,
    }
}

struct World {
    h: Harness,
    c: CollatBase,
    /// Holders by model index: 0 G, 1 T, 2 B.
    keys: Vec<Keypair>,
    stranger: Keypair,
    agrs: Vec<Agr>,
    /// (model pledge index → its reference); same order as `m.pledges`.
    pledge_refs: Vec<[u8; 32]>,
    /// Model call index → its reference.
    call_refs: Vec<[u8; 32]>,
    tracker: Tracker,
    m: Model,
    next_ref: u32,
    /// Operations that succeeded on both sides (the generator must reach the happy paths).
    oks: usize,
}

impl World {
    fn new() -> Self {
        let mut h = Harness::new();
        let c = fixtures::collateral_base_fund(&mut h);
        let b = fixtures::investor_with_units(&mut h, &c, B_UNITS, 0);
        let stranger = h.funded_keypair();
        let (g, t) = (c.giver.pubkey(), c.taker.pubkey());

        // G→T: the worked terms. B→T: 5 % haircut, $5 000 MTA, 1 500 000-unit limit.
        let a0 = Agr {
            mint: c.m(),
            giver: g,
            taker: t,
            nonce: 1,
        };
        let a1 = Agr {
            mint: c.m(),
            giver: b.pubkey(),
            taker: t,
            nonce: 2,
        };
        propose(&mut h, &c.m(), &c.taker, fixtures::worked_terms(&g, &t, 1)).unwrap();
        sign(&mut h, &c.giver, &a0).unwrap();
        let args1 = collateral::ProposeArgs {
            haircut_bps: 500,
            min_transfer_value: 5_000_000_000,
            concentration_cap_units: 1_500_000_000_000,
            ..fixtures::worked_terms(&b.pubkey(), &t, 2)
        };
        propose(&mut h, &c.m(), &c.taker, args1).unwrap();
        sign(&mut h, &b, &a1).unwrap();

        let fs: fund::FundState = read(&h, &c.fund_state());
        let mut m = Model::new(&[GIVER_UNITS, 0, B_UNITS], fs.nav, clock::now(&h.svm));
        m.nav_ts = fs.nav_ts;
        m.nav_seq = fs.nav_seq;
        m.add_agreement(
            0,
            1,
            worked_model_terms(200, 10_000_000_000, 5_000_000_000_000),
        );
        m.add_agreement(
            2,
            1,
            worked_model_terms(500, 5_000_000_000, 1_500_000_000_000),
        );

        let mut tracker = Tracker::new(c.m());
        tracker.agreement(a0.agreement());
        tracker.agreement(a1.agreement());
        let keys = vec![c.giver.insecure_clone(), c.taker.insecure_clone(), b];
        Self {
            h,
            c,
            keys,
            stranger,
            agrs: vec![a0, a1],
            pledge_refs: Vec::new(),
            call_refs: Vec::new(),
            tracker,
            m,
            next_ref: 1,
            oks: 0,
        }
    }

    fn fresh_ref(&mut self) -> [u8; 32] {
        let mut r = [0u8; 32];
        r[..4].copy_from_slice(&self.next_ref.to_le_bytes());
        r[31] = 0xEE;
        self.next_ref += 1;
        r
    }

    fn agr_of(&self, a: usize) -> Agr {
        self.agrs[a]
    }

    fn signer(&self, a: usize, actor: Actor) -> Keypair {
        let ag = &self.m.agreements[a];
        match actor {
            Actor::Giver => self.keys[ag.giver].insecure_clone(),
            Actor::Taker => self.keys[ag.taker].insecure_clone(),
            Actor::Keeper => self.c.keeper.insecure_clone(),
            Actor::Other => self.stranger.insecure_clone(),
        }
    }

    fn send(
        &mut self,
        ixs: &[anchor_lang::solana_program::instruction::Instruction],
        signer: &Keypair,
    ) -> TxResult {
        self.h.send(ixs, &[signer])
    }

    fn agree(&mut self, chain: Result<(), String>, model: Result<(), String>, op: &Op) {
        assert_eq!(chain, model, "{op:?}");
        if chain.is_ok() {
            self.oks += 1;
        }
    }

    fn apply(&mut self, op: &Op) {
        match *op {
            Op::Exposure { a, k } => {
                let a = a as usize;
                let value = u64::from(k) * 5_000_000_000;
                let r = self.fresh_ref();
                let taker = self.signer(a, Actor::Taker);
                let agr = self.agr_of(a);
                let p = self.h.payer.pubkey();
                let res = self.send(
                    &[ix::update_exposure(&taker.pubkey(), &p, &agr, value, r)],
                    &taker,
                );
                let want = self.m.update_exposure(a, value, r);
                self.agree(outcome(&res), model_out(&want), op);
                if let (Ok(meta), Ok(opened)) = (&res, &want) {
                    let ev: ExposureUpdated = one_cpi_event(meta);
                    assert_eq!(
                        ev.opened_call.is_some(),
                        opened.is_some(),
                        "{op:?}: call opened"
                    );
                    if opened.is_some() {
                        self.call_refs.push(r);
                        self.tracker.call(agr.call(&r));
                    }
                }
            }
            Op::Call { a, ret } => {
                let a = a as usize;
                let r = self.fresh_ref();
                let (kind, actor) = if ret {
                    (CallKind::Return, Actor::Giver)
                } else {
                    (CallKind::Delivery, Actor::Taker)
                };
                let signer = self.signer(a, actor);
                let agr = self.agr_of(a);
                let p = self.h.payer.pubkey();
                let res = self.send(
                    &[ix::issue_call(&signer.pubkey(), &p, &agr, u8::from(ret), r)],
                    &signer,
                );
                let want = self.m.issue_call(a, kind, r);
                self.agree(outcome(&res), model_out(&want), op);
                if want.is_ok() {
                    self.call_refs.push(r);
                    self.tracker.call(agr.call(&r));
                }
            }
            Op::Check { a } => {
                let a = a as usize;
                let agr = self.agr_of(a);
                let s = self.stranger.insecure_clone();
                let res = self.send(&[ix::check_coverage(&s.pubkey(), &agr)], &s);
                let want = self.m.check_coverage(a);
                self.agree(outcome(&res), model_out(&want), op);
            }
            Op::Pledge { a, k, wrong } => {
                let a = a as usize;
                let units = u64::from(k) * 10_000_000_000 + u64::from(k % 7);
                let r = self.fresh_ref();
                let actor = if wrong { Actor::Taker } else { Actor::Giver };
                let signer = self.signer(a, actor);
                let agr = self.agr_of(a);
                let p = self.h.payer.pubkey();
                let res = self.send(&[ix::pledge(&signer.pubkey(), &p, &agr, units, r)], &signer);
                let want = self.m.pledge(a, actor, units, r);
                self.agree(outcome(&res), model_out(&want), op);
                if want.is_ok() {
                    self.pledge_refs.push(r);
                    self.tracker.pledge(agr.pledge(&r));
                }
            }
            Op::Release {
                p,
                k,
                keeper,
                with_return,
            } => {
                if self.m.pledges.is_empty() {
                    return;
                }
                let pi = p as usize % self.m.pledges.len();
                let a = self.m.pledges[pi].agreement;
                let units = u64::from(k) * 10_000_000_000 + u64::from(k % 3);
                let ret = if with_return {
                    self.m.agreements[a].open_return.or_else(|| {
                        self.m
                            .calls
                            .iter()
                            .rposition(|c| c.agreement == a && c.kind == CallKind::Return)
                    })
                } else {
                    None
                };
                let actor = if keeper { Actor::Keeper } else { Actor::Taker };
                let signer = self.signer(a, actor);
                let agr = self.agr_of(a);
                let pledge = agr.pledge(&self.pledge_refs[pi]);
                let ret_key = ret.map(|c| agr.call(&self.call_refs[c]));
                let res = self.send(
                    &[ix::release(&signer.pubkey(), &agr, &pledge, ret_key, units)],
                    &signer,
                );
                let want = self.m.release(pi, actor, units, ret);
                self.agree(outcome(&res), model_out(&want), op);
            }
            Op::Substitute { p, extra } => {
                if self.m.pledges.is_empty() {
                    return;
                }
                let pi = p as usize % self.m.pledges.len();
                let a = self.m.pledges[pi].agreement;
                let new_units = self.m.pledges[pi].remaining() + u64::from(extra) * 10_000_000_000;
                let r = self.fresh_ref();
                let giver = self.signer(a, Actor::Giver);
                let agr = self.agr_of(a);
                let old = agr.pledge(&self.pledge_refs[pi]);
                let payer = self.h.payer.pubkey();
                let res = self.send(
                    &[ix::substitute(
                        &giver.pubkey(),
                        &payer,
                        &agr,
                        &old,
                        r,
                        new_units,
                    )],
                    &giver,
                );
                let want = self.m.substitute(pi, Actor::Giver, new_units, r);
                self.agree(outcome(&res), model_out(&want), op);
                if want.is_ok() {
                    self.pledge_refs.push(r);
                    self.tracker.pledge(agr.pledge(&r));
                }
            }
            Op::Cancel { c, wrong } => {
                if self.m.calls.is_empty() {
                    return;
                }
                let ci = c as usize % self.m.calls.len();
                let a = self.m.calls[ci].agreement;
                let right = match self.m.calls[ci].kind {
                    CallKind::Delivery => Actor::Taker,
                    CallKind::Return => Actor::Giver,
                };
                let actor = match (wrong, right) {
                    (false, r) => r,
                    (true, Actor::Taker) => Actor::Giver,
                    (true, _) => Actor::Taker,
                };
                let signer = self.signer(a, actor);
                let agr = self.agr_of(a);
                let key = agr.call(&self.call_refs[ci]);
                let res = self.send(&[ix::cancel_call(&signer.pubkey(), &agr, &key)], &signer);
                let want = self.m.cancel_call(ci, actor);
                self.agree(outcome(&res), model_out(&want), op);
            }
            Op::Dispute { c } | Op::Withdraw { c } => {
                if self.m.calls.is_empty() {
                    return;
                }
                let ci = c as usize % self.m.calls.len();
                let a = self.m.calls[ci].agreement;
                let giver = self.signer(a, Actor::Giver);
                let agr = self.agr_of(a);
                let key = agr.call(&self.call_refs[ci]);
                let (res, want) = if matches!(op, Op::Dispute { .. }) {
                    let r = self.fresh_ref();
                    let res =
                        self.send(&[ix::dispute_call(&giver.pubkey(), &agr, &key, r)], &giver);
                    (res, self.m.dispute(ci, Actor::Giver, r))
                } else {
                    let res =
                        self.send(&[ix::withdraw_dispute(&giver.pubkey(), &agr, &key)], &giver);
                    (res, self.m.withdraw_dispute(ci, Actor::Giver))
                };
                self.agree(outcome(&res), model_out(&want), op);
            }
            Op::Transfer { from, to, k } => {
                let (from, to) = (from as usize, to as usize);
                if from == to {
                    return;
                }
                let units = u64::from(k) * 10_000_000_000 + u64::from(k % 5);
                let sender = self.keys[from].insecure_clone();
                let receiver = self.keys[to].pubkey();
                let c = &self.c;
                let res = fixtures::transfer(&mut self.h, c, &sender, &receiver, units);
                let want = self.m.transfer(from, to, units);
                self.agree(outcome(&res), model_out(&want), op);
            }
            Op::Publish { nav } => {
                let nav = NAVS[nav as usize];
                clock::advance(&mut self.h.svm, 86_400);
                self.m.advance(86_400);
                fixtures::publish(&mut self.h, &self.c, nav, 0, 1).expect("publish");
                self.m.publish(nav);
            }
            Op::Advance { step } => {
                let secs = STEPS[step as usize];
                clock::advance(&mut self.h.svm, secs);
                self.m.advance(secs);
            }
            Op::Suspend { i } | Op::Reinstate { i } => {
                let i = i as usize;
                let suspend = matches!(op, Op::Suspend { .. });
                if self.m.holders[i].approved != suspend {
                    return; // only valid status changes
                }
                let o = self.keys[i].pubkey();
                if suspend {
                    tests::collateral::suspend(&mut self.h, &self.c, &o);
                } else {
                    tests::collateral::reinstate(&mut self.h, &self.c, &o);
                }
                self.m.set_approved(i, !suspend);
            }
        }
        self.compare(op);
    }

    /// Every balance, lock, agreement field and call status equals the model's; the on-chain
    /// invariants hold.
    fn compare(&self, op: &Op) {
        let mint = self.c.m();
        for (i, k) in self.keys.iter().enumerate() {
            let o = k.pubkey();
            assert_eq!(
                units::balance(&self.h, &mint, &o),
                self.m.holders[i].balance,
                "{op:?}: holder {i} balance"
            );
            let s: HolderState = read(&self.h, &mmf_sdk::pda::holder(&mint, &o).0);
            assert_eq!(
                s.pledged_amount, self.m.holders[i].locked,
                "{op:?}: holder {i} lock"
            );
        }
        for (a, agr) in self.agrs.iter().enumerate() {
            let on: PledgeAgreement = read(&self.h, &agr.agreement());
            let mo = &self.m.agreements[a];
            assert_eq!(
                (on.pledged_units, on.required_value, on.called_outstanding),
                (mo.pledged, mo.required, mo.demanded),
                "{op:?}: agreement {a}"
            );
            assert_eq!(
                (
                    on.open_delivery_calls,
                    on.met_below_seq,
                    on.call_count,
                    on.pledge_count
                ),
                (
                    mo.open_delivery,
                    mo.met_below,
                    mo.call_count,
                    mo.pledge_count
                ),
                "{op:?}: agreement {a} counters"
            );
            let open_return = mo
                .open_return
                .map(|c| agr.call(&self.call_refs[c]))
                .unwrap_or_default();
            assert_eq!(
                on.open_return_call, open_return,
                "{op:?}: agreement {a} return call"
            );
        }
        for (ci, r) in self.call_refs.iter().enumerate() {
            let agr = self.agrs[self.m.calls[ci].agreement];
            let key: Pubkey = agr.call(r);
            assert_eq!(
                call_status(&self.h, &key),
                chain_status(self.m.call_status(ci)),
                "{op:?}: call {ci}"
            );
        }
        self.tracker.check(&self.h);
        self.m.check_invariants();
    }
}

/// Runs `ops` on both sides; returns how many succeeded.
fn run(ops: &[Op]) -> usize {
    let mut w = World::new();
    for op in ops {
        w.apply(op);
    }
    w.oks
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(64),
        ..ProptestConfig::default()
    })]

    #[test]
    fn collateral_matches_the_model(ops in prop::collection::vec(op(), 30..90)) {
        run(&ops);
    }

    /// SC-005: pledging a delivery call's units at its price always meets it, and releasing a
    /// return call's units at its price never leaves the taker under-covered. Checked on the
    /// formulas the program runs.
    #[test]
    fn rounding_never_under_covers_the_taker(
        value in 1u64..10_000_000_000_000,
        nav in 50_000_000u64..200_000_000,
        haircut in 0u16..5_000,
        pledged_extra in 0u64..5_000_000_000_000,
    ) {
        use mmf_common::collateral::{collateral_value, units_for_value_down, units_for_value_up};
        // Delivery: from nothing pledged, the units required are worth at least the value.
        let units = units_for_value_up(value, nav, haircut).unwrap();
        prop_assert!(collateral_value(units, nav, haircut).unwrap() >= value);
        // One unit less would fall short (the rounding is tight, not generous).
        if units > 0 {
            prop_assert!(collateral_value(units - 1, nav, haircut).unwrap() < value);
        }
        // Return: with `pledged` units covering `required` with excess E, releasing
        // units_for_value_down(E) keeps coverage ≥ 0.
        let pledged = units + pledged_extra;
        let worth = collateral_value(pledged, nav, haircut).unwrap();
        let required = value.min(worth);
        let excess = worth - required;
        let back = units_for_value_down(excess, nav, haircut).unwrap();
        prop_assert!(back <= pledged);
        prop_assert!(collateral_value(pledged - back, nav, haircut).unwrap() >= required);
    }
}

/// The generator reaches the happy paths: over a fixed-seed run of 16 cases, at least a quarter of
/// all operations succeed on both sides. (A floor per random case would be flaky; this one is
/// deterministic.) A generator that only produced rejections would prove little.
#[test]
fn the_generator_reaches_the_happy_paths() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{RngAlgorithm, TestRng, TestRunner};
    let mut runner = TestRunner::new_with_rng(
        ProptestConfig::default(),
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    let strategy = prop::collection::vec(op(), 30..90);
    let (mut oks, mut total) = (0, 0);
    for _ in 0..16 {
        let ops = strategy.new_tree(&mut runner).unwrap().current();
        oks += run(&ops);
        total += ops.len();
    }
    assert!(
        oks * 4 >= total,
        "only {oks} of {total} operations succeeded"
    );
}

#[test]
#[ignore = "long: run with --release -- --ignored"]
fn collateral_matches_the_model_many_cases() {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    });
    runner
        .run(&prop::collection::vec(op(), 60..150), |ops| {
            run(&ops);
            Ok(())
        })
        .unwrap();
}
