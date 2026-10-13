//! T075 — randomized sequences checked against a reference model (SC-001, SC-007, I3, I10).
//!
//! Random operations are sent directly to LiteSVM (no SDK shortcuts beyond instruction building).
//! After every step:
//! - the outcome (success / failure) matches the model's prediction;
//! - SC-001 / I3: a transfer that delivered units went to a receiver that was approved with
//!   unexpired KYC at that moment, and no never-approved owner holds units;
//! - SC-007 / I10: `holder_count` equals investors with `last_balance > 0`, and every
//!   `last_balance` equals the holding account's balance;
//! - `holder_count` never exceeds the cap.
//!
//! F002 (T033): `Publish` operations are interleaved, so the F001 invariants are checked with the
//! interest counter moving, and I12 (index and period continuity) is checked after every step.

use anchor_lang::prelude::Pubkey;
use compliance_hook::{HolderState, Registry};
use mmf_common::enums::{HolderStatus, ReasonCode};
use mmf_sdk::{ix_governance, ix_hook, pda};
use proptest::prelude::*;
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::fixtures::{self, FundCtx};
use tests::model::{Investor, Model};
use tests::{clock, units, Harness};

const INVESTORS: usize = 5;
const ONBOARDED_AT_START: usize = 4;
const CAP: u32 = 2;
const DAY: i64 = 24 * 60 * 60;
const REASON: u8 = ReasonCode::OperationalError as u8;

#[derive(Clone, Debug)]
enum Op {
    Transfer {
        from: usize,
        to: usize,
        amount: u64,
    },
    Suspend(usize),
    Reinstate(usize),
    Revoke(usize),
    Renew {
        who: usize,
        days: i64,
    },
    Pause,
    Unpause,
    Onboard(usize),
    Warp {
        days: i64,
    },
    /// F002: publish for the next uncovered day through today, at `rate`.
    Publish {
        rate: i64,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let who = 0..INVESTORS;
    prop_oneof![
        12 => (0..INVESTORS, 0..INVESTORS, prop_oneof![Just(0u64), 1u64..400, 900u64..1_200])
            .prop_map(|(from, to, amount)| Op::Transfer { from, to, amount }),
        1 => who.clone().prop_map(Op::Suspend),
        1 => who.clone().prop_map(Op::Reinstate),
        1 => who.clone().prop_map(Op::Revoke),
        1 => (who.clone(), -30i64..400).prop_map(|(who, days)| Op::Renew { who, days }),
        1 => Just(Op::Pause),
        1 => Just(Op::Unpause),
        1 => who.prop_map(Op::Onboard),
        3 => (1i64..400).prop_map(|days| Op::Warp { days }),
        2 => (0i64..=500_000_000).prop_map(|rate| Op::Publish { rate }),
    ]
}

struct World {
    h: Harness,
    ctx: FundCtx,
    keys: Vec<Keypair>,
    model: Model,
    /// F002: last UTC day covered by a publish.
    period_end: Option<u32>,
}

impl World {
    fn new() -> Self {
        let mut h = Harness::new();
        let ctx = fixtures::launched_fund(&mut h, CAP);
        let keys: Vec<Keypair> = (0..INVESTORS).map(|_| h.funded_keypair()).collect();
        let now = clock::now(&h.svm);
        let mut investors = vec![Investor::unregistered(); INVESTORS];
        for (i, k) in keys.iter().enumerate().take(ONBOARDED_AT_START) {
            fixtures::onboard_investor(&mut h, &ctx, k, *b"US");
            investors[i] = Investor {
                registered: true,
                status: HolderStatus::Approved,
                kyc_expiry: now + fixtures::YEAR,
                balance: 0,
                thawed: true,
            };
        }
        units::seed_units(&mut h, &ctx.m(), &keys[0].pubkey(), 1_000);
        investors[0].balance = 1_000;
        let model = Model {
            investors,
            paused: false,
            max_holders: CAP,
            now,
        };
        Self {
            h,
            ctx,
            keys,
            model,
            period_end: None,
        }
    }

    fn owner(&self, i: usize) -> Pubkey {
        self.keys[i].pubkey()
    }

    fn compliance(&mut self, ix: anchor_lang::solana_program::instruction::Instruction) -> bool {
        let c = self.ctx.compliance.insecure_clone();
        self.h.send(&[ix], &[&c]).is_ok()
    }

    /// Runs one operation; returns (predicted, actual) success.
    fn step(&mut self, op: &Op) -> (bool, bool) {
        let m = self.ctx.m();
        let c = self.ctx.compliance.pubkey();
        match *op {
            Op::Transfer { from, to, amount } => {
                let predicted = self.model.transfer_ok(from, to, amount);
                // SC-001: snapshot the receiver's on-chain approval before the transfer.
                let receiver_approved = self.onchain_approved(to);
                let sender = self.keys[from].insecure_clone();
                let to_owner = self.owner(to);
                let actual =
                    fixtures::transfer(&mut self.h, &self.ctx, &sender, &to_owner, amount).is_ok();
                if actual && from != to && amount > 0 {
                    assert!(
                        receiver_approved,
                        "SC-001: units delivered to a receiver that wasn't approved"
                    );
                }
                if actual {
                    self.model.apply_transfer(from, to, amount);
                }
                (predicted, actual)
            }
            Op::Suspend(i) => {
                let predicted = self.model.suspend_ok(i);
                let actual = self.compliance(ix_governance::suspend_holder(
                    &c,
                    &m,
                    &self.owner(i),
                    REASON,
                    [0; 32],
                ));
                if actual {
                    self.model.investors[i].status = HolderStatus::Suspended;
                }
                (predicted, actual)
            }
            Op::Reinstate(i) => {
                let predicted = self.model.reinstate_ok(i);
                let actual = self.compliance(ix_governance::reinstate_holder(
                    &c,
                    &m,
                    &self.owner(i),
                    REASON,
                    [0; 32],
                ));
                if actual {
                    self.model.investors[i].status = HolderStatus::Approved;
                }
                (predicted, actual)
            }
            Op::Revoke(i) => {
                let predicted = self.model.revoke_ok(i);
                let actual = self.compliance(ix_governance::revoke_holder(
                    &c,
                    &m,
                    &self.owner(i),
                    REASON,
                    [0; 32],
                ));
                if actual {
                    self.model.investors[i].status = HolderStatus::Revoked;
                }
                (predicted, actual)
            }
            Op::Renew { who, days } => {
                let expiry = self.model.now + days * DAY;
                let predicted = self.model.renew_ok(who, expiry);
                let o = self.owner(who);
                let signed = fixtures::signed_statement(&self.ctx, &o, *b"US", expiry);
                let renew = ix_hook::renew_investor(&c, &m, &o, signed.attestation_hash);
                let comp = self.ctx.compliance.insecure_clone();
                let actual = self.h.send(&[signed.ix.clone(), renew], &[&comp]).is_ok();
                if actual {
                    self.model.investors[who].kyc_expiry = expiry;
                }
                (predicted, actual)
            }
            Op::Pause => {
                let predicted = !self.model.paused;
                let actual =
                    self.compliance(ix_governance::pause_transfers(&c, &m, REASON, [0; 32]));
                if actual {
                    self.model.paused = true;
                }
                (predicted, actual)
            }
            Op::Unpause => {
                let predicted = self.model.paused;
                let actual =
                    self.compliance(ix_governance::unpause_transfers(&c, &m, REASON, [0; 32]));
                if actual {
                    self.model.paused = false;
                }
                (predicted, actual)
            }
            Op::Onboard(i) => {
                if self.model.investors[i].registered {
                    return (false, false); // nothing to do
                }
                let k = self.keys[i].insecure_clone();
                fixtures::onboard_investor(&mut self.h, &self.ctx, &k, *b"GB");
                self.model.investors[i] = Investor {
                    registered: true,
                    status: HolderStatus::Approved,
                    kyc_expiry: self.model.now + fixtures::YEAR,
                    balance: 0,
                    thawed: true,
                };
                (true, true)
            }
            Op::Warp { days } => {
                clock::advance(&mut self.h.svm, days * DAY);
                self.model.now += days * DAY;
                (true, true)
            }
            Op::Publish { rate } => {
                // The fund is always Open here; a publish succeeds whenever its period has begun.
                let today = mmf_common::accrual::utc_day(self.model.now).unwrap();
                let start = self.period_end.map_or(today, |end| end + 1);
                let predicted = start <= today;
                let days = if predicted {
                    (today - start + 1) as u16
                } else {
                    1
                };
                let actual =
                    fixtures::publish(&mut self.h, &self.ctx, 100_000_000, rate, days).is_ok();
                if actual {
                    self.period_end = Some(today);
                }
                (predicted, actual)
            }
        }
    }

    fn onchain_approved(&self, i: usize) -> bool {
        let key = pda::holder(&self.ctx.m(), &self.owner(i)).0;
        self.h.account(&key).is_some_and(|_| {
            let rec: HolderState = fixtures::read(&self.h, &key);
            rec.approved(clock::now(&self.h.svm))
        })
    }

    fn check_invariants(&self, after: &Op) {
        let reg: Registry = fixtures::read(&self.h, &self.ctx.registry());
        let mut counted = 0u32;
        for i in 0..INVESTORS {
            let owner = self.owner(i);
            let balance = units::balance(&self.h, &self.ctx.m(), &owner);
            assert_eq!(
                balance, self.model.investors[i].balance,
                "balance of investor {i} after {after:?}"
            );
            let key = pda::holder(&self.ctx.m(), &owner).0;
            if self.h.account(&key).is_none() {
                assert_eq!(balance, 0, "I3: a never-registered owner holds units");
                continue;
            }
            let rec: HolderState = fixtures::read(&self.h, &key);
            assert_eq!(
                rec.last_balance, balance,
                "I10: last_balance of investor {i} after {after:?}"
            );
            counted += u32::from(rec.last_balance > 0);
        }
        assert_eq!(
            reg.holder_count, counted,
            "I10: holder_count after {after:?}"
        );
        assert_eq!(
            reg.holder_count,
            self.model.holder_count(),
            "model holder count after {after:?}"
        );
        assert!(
            reg.holder_count <= CAP,
            "holder_count exceeds the cap after {after:?}"
        );
        assert_eq!(reg.transfers_paused, self.model.paused);
        fixtures::check_i12(&self.h, &self.ctx);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn random_sequences_keep_every_invariant(ops in prop::collection::vec(op(), 15..60)) {
        let mut w = World::new();
        w.check_invariants(&Op::Warp { days: 0 });
        for op in &ops {
            let (predicted, actual) = w.step(op);
            prop_assert_eq!(predicted, actual, "outcome of {:?} (model {:?})", op, w.model);
            w.check_invariants(op);
        }
    }
}
