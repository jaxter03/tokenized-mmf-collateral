//! F004 US3, no path moves locked units (FR-023, FR-026, SC-003; hook step 5). T039: written
//! before the code. Every attempt is a transfer or settlement step built and sent directly.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_spl::token_2022::spl_token_2022;
use compliance_hook::HookError;
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::collateral::{agr, exposure, lock_of, pledge, pledge_on, r, Tracker};
use tests::fixtures::{self, worked_terms, CollatCtx, GIVER_UNITS};
use tests::settle::{create, deal, fund_units, red_args, sec_args};
use tests::{units, Harness};

const LOCKED: u64 = 1_020_408_163_266;
const UNLOCKED: u64 = GIVER_UNITS - LOCKED; // 979 591.836734

/// G holds 2 000 000 units with 1 020 408.163266 locked under the fixture's agreement.
fn locked() -> (Harness, CollatCtx, Tracker) {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    let mut t = Tracker::new(c.m());
    t.agreement(c.agreement());
    exposure(&mut h, &c, &c.taker, 1_000_000_000_000, r(1)).expect("$1M");
    t.call(c.agr.call(&r(1)));
    pledge(&mut h, &c, &c.giver, LOCKED, r(9)).expect("pledge");
    t.pledge(c.agr.pledge(&r(9)));
    (h, c, t)
}

/// `transfer_checked` from G's holding account, written out by hand: Token-2022's instruction
/// plus the hook's extra accounts in the order the ExtraAccountMetaList declares.
fn raw_transfer(c: &CollatCtx, authority: &Pubkey, to_owner: &Pubkey, amount: u64) -> Instruction {
    let m = c.m();
    let g = c.giver.pubkey();
    let mut ix = spl_token_2022::instruction::transfer_checked(
        &TOKEN_2022_PROGRAM_ID,
        &mmf_sdk::pda::holding_account(&g, &m),
        &m,
        &mmf_sdk::pda::holding_account(to_owner, &m),
        authority,
        &[],
        amount,
        6,
    )
    .unwrap();
    ix.accounts.extend([
        AccountMeta::new(mmf_sdk::pda::registry(&m).0, false),
        AccountMeta::new_readonly(c.fund_state(), false),
        AccountMeta::new(mmf_sdk::pda::holder(&m, &g).0, false),
        AccountMeta::new(mmf_sdk::pda::holder(&m, to_owner).0, false),
        AccountMeta::new_readonly(mmf_sdk::pda::delegate(&m).0, false),
        AccountMeta::new_readonly(COMPLIANCE_HOOK_PROGRAM_ID, false),
        AccountMeta::new_readonly(mmf_sdk::pda::extra_metas(&m).0, false),
    ]);
    ix
}

fn unchanged(h: &Harness, c: &CollatCtx, t: &Tracker, giver_balance: u64) {
    assert_eq!(units::balance(h, &c.m(), &c.giver.pubkey()), giver_balance);
    assert_eq!(lock_of(h, &c.m(), &c.giver.pubkey()).0, LOCKED);
    t.check(h);
}

#[test]
fn a_hand_built_transfer_over_the_lock_is_refused() {
    let (mut h, c, t) = locked();
    let to = c.taker.pubkey();
    let res = h.send(&[raw_transfer(&c, &c.giver.pubkey(), &to, UNLOCKED + 1)], &[&c.giver]);
    assert_anchor_error(&res, HookError::PledgedUnitsLocked);
    unchanged(&h, &c, &t, GIVER_UNITS);

    h.send(&[raw_transfer(&c, &c.giver.pubkey(), &to, UNLOCKED)], &[&c.giver])
        .expect("exactly the unlocked units");
    assert_eq!(units::balance(&h, &c.m(), &c.giver.pubkey()), LOCKED, "balance = lock");
    unchanged(&h, &c, &t, LOCKED);

    // Fully locked: a zero-unit transfer passes (the post-balance still covers the lock), one
    // base unit doesn't.
    h.send(&[raw_transfer(&c, &c.giver.pubkey(), &to, 0)], &[&c.giver]).expect("zero units");
    let res = h.send(&[raw_transfer(&c, &c.giver.pubkey(), &to, 1)], &[&c.giver]);
    assert_anchor_error(&res, HookError::PledgedUnitsLocked);
}

#[test]
fn a_delegate_cannot_move_locked_units() {
    let (mut h, c, t) = locked();
    let delegate = h.funded_keypair();
    let m = c.m();
    let approve = spl_token_2022::instruction::approve_checked(
        &TOKEN_2022_PROGRAM_ID,
        &mmf_sdk::pda::holding_account(&c.giver.pubkey(), &m),
        &m,
        &delegate.pubkey(),
        &c.giver.pubkey(),
        &[],
        GIVER_UNITS,
        6,
    )
    .unwrap();
    h.send(&[approve], &[&c.giver]).expect("approve the whole balance");
    let res = h.send(
        &[raw_transfer(&c, &delegate.pubkey(), &c.taker.pubkey(), UNLOCKED + 1)],
        &[&delegate],
    );
    assert_anchor_error(&res, HookError::PledgedUnitsLocked);
    unchanged(&h, &c, &t, GIVER_UNITS);
}

#[test]
fn settlement_funding_cannot_use_locked_units() {
    let (mut h, c, t) = locked();
    let g = c.giver.insecure_clone();
    // F003 US2 scenario 3: a redemption funded with locked units.
    let args = red_args(&c, &g.pubkey(), 1_000_000_000_000, 1);
    create(&mut h, &c, &g, args).expect("create redemption");
    let d = deal(&c, &g.pubkey(), &args);
    assert_anchor_error(&fund_units(&mut h, &d, &g), HookError::PledgedUnitsLocked);
    unchanged(&h, &c, &t, GIVER_UNITS);

    // A secondary sale of the same units.
    let args = sec_args(&g.pubkey(), &c.taker.pubkey(), 1_000_000_000_000, 1_000_000_000_000, 2);
    create(&mut h, &c, &g, args).expect("create secondary");
    let d = deal(&c, &g.pubkey(), &args);
    assert_anchor_error(&fund_units(&mut h, &d, &g), HookError::PledgedUnitsLocked);
    unchanged(&h, &c, &t, GIVER_UNITS);
}

#[test]
fn locked_units_cannot_be_pledged_elsewhere() {
    let (mut h, c, mut t) = locked();
    let other: Keypair = fixtures::new_investor(&mut h, &c);
    let args = worked_terms(&c.giver.pubkey(), &other.pubkey(), 7);
    tests::collateral::propose(&mut h, &c.m(), &other, args).expect("propose");
    let second = mmf_sdk::ix_collateral::Agr { taker: other.pubkey(), ..agr(&c, 7) };
    tests::collateral::sign(&mut h, &c.giver, &second).expect("sign");
    t.agreement(second.agreement());

    let res = pledge_on(&mut h, &second, &c.giver, UNLOCKED + 1, r(1));
    assert_anchor_error(&res, HookError::InsufficientUnlockedUnits);
    pledge_on(&mut h, &second, &c.giver, UNLOCKED, r(2)).expect("exactly the unlocked units");
    t.pledge(second.pledge(&r(2)));
    assert_eq!(lock_of(&h, &c.m(), &c.giver.pubkey()), (GIVER_UNITS, 2));
    t.check(&h);
}

#[test]
fn a_locked_holder_can_still_receive() {
    let (mut h, c, t) = locked();
    units::seed_units(&mut h, &c.m(), &c.taker.pubkey(), 5_000_000);
    fixtures::transfer(&mut h, &c, &c.taker, &c.giver.pubkey(), 5_000_000).expect("receive");
    unchanged(&h, &c, &t, GIVER_UNITS + 5_000_000);
    // The units received are unlocked.
    let res = h.send(
        &[raw_transfer(&c, &c.giver.pubkey(), &c.taker.pubkey(), UNLOCKED + 5_000_000)],
        &[&c.giver],
    );
    res.expect("send everything unlocked");
    assert_eq!(units::balance(&h, &c.m(), &c.giver.pubkey()), LOCKED);
}
