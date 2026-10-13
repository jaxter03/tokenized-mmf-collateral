//! T049 (FR-030, I13): the escrow holds at least what open instructions hold, and units or cash
//! sent to it outside any instruction are never released by one. US4 (T060) extends this with
//! returns after cancel, expiry and rejection.

use dvp_settlement::SettlementInstruction;
use mmf_sdk::ix_dvp::Deal;
use solana_signer::Signer;
use tests::fixtures::{self, read, SettleCtx};
use tests::settle::{self, ONE};
use tests::{units, Harness};

/// Σ units and Σ cash the given instructions hold in escrow (data-model "Escrow held by an
/// instruction").
fn held(h: &Harness, deals: &[Deal]) -> (u64, u64) {
    deals.iter().fold((0, 0), |(u, c), d| {
        let si: SettlementInstruction = read(h, &d.settlement());
        (u + si.units_held(), c + si.cash_held())
    })
}

#[track_caller]
fn assert_escrow(h: &Harness, s: &SettleCtx, deals: &[Deal], stray: (u64, u64)) {
    let (u, c) = held(h, deals);
    assert_eq!(
        units::token_balance(h, &s.unit_escrow),
        u + stray.0,
        "unit escrow"
    );
    assert_eq!(
        fixtures::cash_balance(h, &s.cash_escrow),
        c + stray.1,
        "cash escrow"
    );
}

#[test]
fn stray_deposits_stay_in_escrow_and_every_instruction_releases_only_its_own() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let a = fixtures::investor_with_units(&mut h, &s, 1_000 * ONE, 100 * ONE);
    let b = fixtures::investor_with_cash(&mut h, &s, 100 * ONE);
    settle::fill_treasury(&mut h, &s, 100 * ONE);

    // Someone sends 7 units and $3 straight to the escrow accounts.
    fixtures::transfer(&mut h, &s, &a, &s.escrow, 7 * ONE).expect("units to escrow");
    let ix = anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked(
        &s.cash_program,
        &fixtures::cash_ata(&h, &a.pubkey(), &s.cash_mint),
        &s.cash_mint,
        &s.cash_escrow,
        &a.pubkey(),
        &[],
        3 * ONE,
        fixtures::CASH_DECIMALS,
    )
    .unwrap();
    h.send(&[ix], &[&a]).expect("cash to escrow");
    let stray = (7 * ONE, 3 * ONE);
    let mut deals = Vec::new();
    assert_escrow(&h, &s, &deals, stray);

    // A subscription, step by step.
    let d = settle::affirmed_subscription(&mut h, &s, &b, 10 * ONE, 1);
    deals.push(d);
    assert_escrow(&h, &s, &deals, stray);
    settle::settle_sub(&mut h, &s, &d, &s.keeper).unwrap();
    assert_escrow(&h, &s, &deals, stray);

    // A redemption, step by step.
    let d = settle::created(
        &mut h,
        &s,
        &a,
        settle::red_args(&s, &a.pubkey(), 20 * ONE, 2),
    );
    deals.push(d);
    settle::fund_units(&mut h, &d, &a).unwrap();
    assert_escrow(&h, &s, &deals, stray);
    settle::fund_redemption_cash(&mut h, &s, &d).unwrap();
    assert_escrow(&h, &s, &deals, stray);
    settle::settle_red(&mut h, &d, &s.keeper).unwrap();
    assert_escrow(&h, &s, &deals, stray);

    // Exactly the stray amounts remain, and the parties got exactly their own.
    assert_eq!(units::balance(&h, &s.m(), &b.pubkey()), 10 * ONE);
    assert_eq!(units::balance(&h, &s.m(), &a.pubkey()), 973 * ONE);
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 117 * ONE);
}
