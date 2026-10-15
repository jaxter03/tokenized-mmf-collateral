//! T072 (SC-009, FR-037): an auditor rebuilds every agreement, call (with its derived status),
//! pledge and holder lock from the records alone, and the result equals the on-chain state. The
//! scripted sequence uses all 13 collateral instructions and `set_lien`.

use collateral::{CollateralCall, Pledge, PledgeAgreement, ProposeArgs};
use compliance_hook::HolderState;
use mmf_common::enums::ReasonCode;
use mmf_sdk::ix_collateral::{self as ix, Agr};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::collateral::{call_status, r};
use tests::fixtures::{self, read, worked_terms};
use tests::rebuild::{collateral_records, rebuild_collateral, CollateralRecord};
use tests::{Harness, TxResult};

const MILLION: u64 = 1_000_000_000_000;

/// Keeps every successful step's records, in order.
struct Ledger(Vec<CollateralRecord>);

impl Ledger {
    fn keep(&mut self, res: TxResult) {
        self.0
            .extend(collateral_records(&res.expect("step succeeds")));
    }
}

fn units_for(value: u64) -> u64 {
    mmf_common::collateral::units_for_value_up(value, 100_000_000, 200).unwrap()
}

#[test]
fn every_agreement_call_pledge_and_lock_can_be_rebuilt_from_the_records() {
    let mut h = Harness::new();
    let c = fixtures::collateral_base_fund(&mut h);
    let (g, t) = (c.giver.insecure_clone(), c.taker.insecure_clone());
    let m = c.m();
    let p = h.payer.pubkey();
    let mut l = Ledger(Vec::new());
    let send = |h: &mut Harness,
                ix: anchor_lang::solana_program::instruction::Instruction,
                k: &Keypair| { h.send(&[ix], &[k]) };

    // Two agreements: A1 with the worked terms, A2 where a raise waits for the giver.
    let a1 = Agr {
        mint: m,
        giver: g.pubkey(),
        taker: t.pubkey(),
        nonce: 1,
    };
    let a2 = Agr { nonce: 2, ..a1 };
    l.keep(send(
        &mut h,
        ix::propose_agreement(&t.pubkey(), &p, &m, worked_terms(&a1.giver, &a1.taker, 1)),
        &t,
    ));
    l.keep(send(&mut h, ix::sign_agreement(&g.pubkey(), &a1), &g));
    let two_step = ProposeArgs {
        taker_may_call_unilaterally: false,
        ..worked_terms(&a2.giver, &a2.taker, 2)
    };
    l.keep(send(
        &mut h,
        ix::propose_agreement(&g.pubkey(), &p, &m, two_step),
        &g,
    ));
    l.keep(send(&mut h, ix::sign_agreement(&t.pubkey(), &a2), &t));

    // A1: a $1M call, a partial pledge, a raise with its own call, which is disputed,
    // withdrawn, disputed again and cancelled by compliance.
    l.keep(send(
        &mut h,
        ix::update_exposure(&t.pubkey(), &p, &a1, MILLION, r(1)),
        &t,
    ));
    l.keep(send(
        &mut h,
        ix::pledge(&g.pubkey(), &p, &a1, units_for(600_000_000_000), r(101)),
        &g,
    ));
    l.keep(send(
        &mut h,
        ix::update_exposure(&t.pubkey(), &p, &a1, 3 * MILLION / 2, r(2)),
        &t,
    ));
    let c2 = a1.call(&r(2));
    l.keep(send(
        &mut h,
        ix::dispute_call(&g.pubkey(), &a1, &c2, r(50)),
        &g,
    ));
    l.keep(send(
        &mut h,
        ix::withdraw_dispute(&g.pubkey(), &a1, &c2),
        &g,
    ));
    l.keep(send(
        &mut h,
        ix::dispute_call(&g.pubkey(), &a1, &c2, r(51)),
        &g,
    ));
    let resolve = mmf_sdk::ix_governance::resolve_call_dispute(
        &c.compliance.pubkey(),
        &m,
        &a1.agreement(),
        &c2,
        5,
        ReasonCode::OperationalError as u8,
        [0xC5; 32],
    );
    l.keep(h.send(&[resolve], &[&c.compliance]));
    // Cover the rest (the first call is met), swap the first pledge for a larger one, re-check.
    l.keep(send(
        &mut h,
        ix::pledge(&g.pubkey(), &p, &a1, units_for(900_000_000_000), r(102)),
        &g,
    ));
    l.keep(send(
        &mut h,
        ix::substitute(
            &g.pubkey(),
            &p,
            &a1,
            &a1.pledge(&r(101)),
            r(103),
            700_000_000_000,
        ),
        &g,
    ));
    let auditor = c.auditor.insecure_clone();
    l.keep(send(
        &mut h,
        ix::check_coverage(&auditor.pubkey(), &a1),
        &auditor,
    ));
    // Exposure falls; the giver asks for the excess back, the keeper releases part of it, then
    // the giver cancels the rest of its return call.
    l.keep(send(
        &mut h,
        ix::update_exposure(&t.pubkey(), &p, &a1, 1_200_000_000_000, r(3)),
        &t,
    ));
    l.keep(send(
        &mut h,
        ix::issue_call(&g.pubkey(), &p, &a1, 1, r(4)),
        &g,
    ));
    let r4 = a1.call(&r(4));
    let keeper = c.keeper.insecure_clone();
    l.keep(send(
        &mut h,
        ix::release(
            &keeper.pubkey(),
            &a1,
            &a1.pledge(&r(103)),
            Some(r4),
            100_000_000_000,
        ),
        &keeper,
    ));
    l.keep(send(&mut h, ix::cancel_call(&g.pubkey(), &a1, &r4), &g));

    // A second return call, released in full by the taker: Met.
    l.keep(send(
        &mut h,
        ix::issue_call(&g.pubkey(), &p, &a1, 1, r(5)),
        &g,
    ));
    let r5 = a1.call(&r(5));
    let wanted = read::<CollateralCall>(&h, &r5).units_required;
    l.keep(send(
        &mut h,
        ix::release(&t.pubkey(), &a1, &a1.pledge(&r(103)), Some(r5), wanted),
        &t,
    ));

    // A2: a pending raise confirmed by the giver opens a call; the taker cancels it and calls
    // the shortfall again with `issue_call`; a pledge then meets that call.
    l.keep(send(
        &mut h,
        ix::update_exposure(&t.pubkey(), &p, &a2, 200_000_000_000, r(6)),
        &t,
    ));
    l.keep(send(
        &mut h,
        ix::confirm_exposure(&g.pubkey(), &p, &a2, 200_000_000_000, r(6)),
        &g,
    ));
    l.keep(send(
        &mut h,
        ix::cancel_call(&t.pubkey(), &a2, &a2.call(&r(6))),
        &t,
    ));
    l.keep(send(
        &mut h,
        ix::issue_call(&t.pubkey(), &p, &a2, 0, r(7)),
        &t,
    ));
    l.keep(send(
        &mut h,
        ix::pledge(&g.pubkey(), &p, &a2, units_for(200_000_000_000), r(104)),
        &g,
    ));

    // The rebuild equals the chain.
    let rebuilt = rebuild_collateral(&l.0);
    assert_eq!(rebuilt.agreements.len(), 2);
    for (key, a) in &rebuilt.agreements {
        let on: PledgeAgreement = read(&h, key);
        assert_eq!(
            (
                a.status,
                a.pledged_units,
                a.required_value,
                a.called_outstanding
            ),
            (
                on.status,
                on.pledged_units,
                on.required_value,
                on.called_outstanding
            ),
            "agreement {key}"
        );
        assert_eq!(
            (a.open_delivery_calls, a.met_below_seq, a.open_return_call),
            (
                on.open_delivery_calls,
                on.met_below_seq,
                on.open_return_call
            ),
            "agreement {key} calls"
        );
    }
    assert_eq!(rebuilt.calls.len(), 6);
    for (key, call) in &rebuilt.calls {
        assert_eq!(
            call.status,
            call_status(&h, key).as_u8(),
            "call {key} status"
        );
        let on: CollateralCall = read(&h, key);
        assert_eq!(
            (call.seq, call.kind, call.value_delta),
            (on.seq, on.kind, on.value_delta),
            "call {key}"
        );
    }
    assert_eq!(rebuilt.pledges.len(), 4);
    for (key, pl) in &rebuilt.pledges {
        let on: Pledge = read(&h, key);
        assert_eq!(
            (pl.remaining, pl.status),
            (on.units - on.released_units - on.enforced_units, on.status),
            "pledge {key}"
        );
    }
    let state: HolderState = read(&h, &mmf_sdk::pda::holder(&m, &g.pubkey()).0);
    let rebuilt_lock = rebuilt.locks.get(&g.pubkey()).copied().unwrap_or(0);
    assert_eq!(
        rebuilt_lock, state.pledged_amount,
        "the giver's lock, from the pledges"
    );
    assert_eq!(
        rebuilt.reported_locks[&g.pubkey()],
        state.pledged_amount,
        "the hook's last LienSet"
    );
}
