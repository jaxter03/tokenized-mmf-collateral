//! T074 (SC-008, FR-031): an auditor rebuilds every instruction's status, cancel requests and
//! earmarks, the escrow's balances and the units created or destroyed, from the settlement
//! records alone, and the result equals the on-chain state.

use dvp_settlement::SettlementInstruction;
use fund::FundState;
use mmf_sdk::ix_dvp::Deal;
use solana_signer::Signer;
use tests::fixtures::{self, read};
use tests::rebuild::{rebuild_settlements, settlement_records, SettlementRecord};
use tests::settle::{self, red_args, sec_args, sub_args, BUYER, ONE, SELLER};
use tests::{clock, units, Harness, TxResult};

/// Keeps every successful step's records, in order.
struct Ledger(Vec<SettlementRecord>);

impl Ledger {
    fn keep(&mut self, res: TxResult) {
        self.0
            .extend(settlement_records(&res.expect("step succeeds")));
    }
}

#[test]
fn every_instruction_and_the_escrow_can_be_rebuilt_from_the_records() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let a = fixtures::investor_with_units(&mut h, &s, 1_000 * ONE, 100 * ONE);
    let b = fixtures::investor_with_units(&mut h, &s, 1_000 * ONE, 100 * ONE);
    settle::fill_treasury(&mut h, &s, 100 * ONE);
    let start_units = fixtures::read::<FundState>(&h, &s.fund_state()).units_outstanding;
    let mut l = Ledger(Vec::new());
    let mut deals: Vec<Deal> = Vec::new();
    let create = |h: &mut Harness, l: &mut Ledger, who: &solana_keypair::Keypair, args| {
        l.keep(settle::create(h, &s, who, args));
        settle::deal(&s, &who.pubkey(), &args)
    };

    // A settled subscription.
    let d = create(&mut h, &mut l, &a, sub_args(&s, &a.pubkey(), 5 * ONE, 1));
    l.keep(settle::affirm(&mut h, &s, &d));
    l.keep(settle::fund_cash(&mut h, &s, &d, &a));
    l.keep(settle::settle_sub(&mut h, &s, &d, &s.keeper));
    deals.push(d);
    // A settled redemption.
    let d = create(&mut h, &mut l, &b, red_args(&s, &b.pubkey(), 7 * ONE, 2));
    l.keep(settle::fund_units(&mut h, &d, &b));
    l.keep(settle::fund_redemption_cash(&mut h, &s, &d));
    l.keep(settle::settle_red(&mut h, &d, &s.keeper));
    deals.push(d);
    // A secondary cancelled after cash funding, its cash returned.
    let d = create(
        &mut h,
        &mut l,
        &b,
        sec_args(&a.pubkey(), &b.pubkey(), ONE, 2 * ONE, 3),
    );
    l.keep(settle::fund_cash(&mut h, &s, &d, &b));
    l.keep(settle::cancel(&mut h, &d, &a, SELLER));
    l.keep(settle::return_cash(&mut h, &s, &d, &s.keeper));
    deals.push(d);
    // A redemption rejected with units in escrow, not yet returned.
    let d = create(&mut h, &mut l, &a, red_args(&s, &a.pubkey(), 3 * ONE, 4));
    l.keep(settle::fund_units(&mut h, &d, &a));
    l.keep(settle::reject(&mut h, &s, &d, 3));
    deals.push(d);
    // An Affirmed subscription with one cancel request withdrawn and one pending.
    let d = create(&mut h, &mut l, &b, sub_args(&s, &b.pubkey(), ONE, 5));
    l.keep(settle::fund_cash(&mut h, &s, &d, &b));
    l.keep(settle::affirm(&mut h, &s, &d));
    l.keep(settle::cancel(&mut h, &d, &b, BUYER));
    l.keep(settle::withdraw_cancel(&mut h, &d, &b, BUYER));
    l.keep(settle::cancel(&mut h, &d, &s.transfer_agent, SELLER));
    deals.push(d);
    // A secondary with both legs in, left to expire; only its units returned so far.
    let d = create(
        &mut h,
        &mut l,
        &a,
        sec_args(&b.pubkey(), &a.pubkey(), 4 * ONE, 3 * ONE, 6),
    );
    l.keep(settle::fund_units(&mut h, &d, &b));
    l.keep(settle::fund_cash(&mut h, &s, &d, &a));
    clock::advance(&mut h.svm, 86_400);
    l.keep(settle::expire(&mut h, &d, &s.keeper));
    l.keep(settle::return_units(&mut h, &d, &s.keeper));
    deals.push(d);

    let rebuilt = rebuild_settlements(&l.0);
    assert_eq!(rebuilt.instructions.len(), deals.len());
    for d in &deals {
        let si: SettlementInstruction = read(&h, &d.settlement());
        let r = rebuilt.instructions[&si.seq];
        assert_eq!(
            (r.kind, r.units, r.cash, r.status),
            (si.kind, si.units, si.cash_amount, si.status),
            "instruction {}",
            si.seq
        );
        assert_eq!(
            (r.cancel_requests, r.units_to_return, r.cash_to_return),
            (si.cancel_requests, si.units_to_return, si.cash_to_return),
            "instruction {} earmarks",
            si.seq
        );
    }
    assert_eq!(
        rebuilt.escrow_units,
        units::token_balance(&h, &s.unit_escrow)
    );
    assert_eq!(
        rebuilt.escrow_cash,
        fixtures::cash_balance(&h, &s.cash_escrow)
    );
    let fs: FundState = read(&h, &s.fund_state());
    assert_eq!(
        i128::from(fs.units_outstanding) - i128::from(start_units),
        rebuilt.net_units
    );
    // The scenario left something in escrow (the rejected units, the expired cash).
    assert_eq!(
        (rebuilt.escrow_units, rebuilt.escrow_cash),
        (3 * ONE, 3 * ONE + ONE)
    );
}
