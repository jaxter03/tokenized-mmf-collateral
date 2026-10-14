//! F004 `compliance_hook::set_lien` and `init_registry` (contracts/compliance_hook.md, research
//! R2; FR-024). Part 1 (T015): the Registry names the collateral program, and nothing but
//! collateral's `["hook-caller"]` PDA can change a lock. Part 2 (T043) drives `set_lien` through
//! `collateral` to reach its account and balance checks.

use anchor_lang::prelude::Pubkey;
use compliance_hook::{HookError, LienChange, Registry};
use mmf_common::ids::COLLATERAL_PROGRAM_ID;
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::fixtures::{self, read};
use tests::Harness;

fn lock(units: u64) -> LienChange {
    LienChange {
        lock: units,
        unlock: 0,
        opened: 1,
        closed: 0,
    }
}

#[test]
fn new_registry_names_the_collateral_program() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let reg: Registry = read(&h, &ctx.registry());
    assert_eq!(reg.collateral_program, COLLATERAL_PROGRAM_ID);
}

#[test]
fn set_lien_refuses_any_other_signer() {
    let mut h = Harness::new();
    let (ctx, investor) = fixtures::fund_with_investor_units(&mut h, 0, 1_000_000);
    let m = ctx.m();
    let o = investor.pubkey();
    let holder = mmf_sdk::pda::holder(&m, &o).0;
    let holding = mmf_sdk::pda::holding_account(&o, &m);

    // A random key in the caller position, signing.
    let rogue = h.funded_keypair();
    let ix = mmf_sdk::ix_hook::set_lien(&rogue.pubkey(), true, &ctx.registry(), &holder, &holding, lock(1));
    assert_anchor_error(&h.send(&[ix], &[&rogue]), HookError::NotCollateralCaller);

    // The holder itself, signing for its own lock.
    let ix = mmf_sdk::ix_hook::set_lien(&o, true, &ctx.registry(), &holder, &holding, lock(1));
    assert_anchor_error(&h.send(&[ix], &[&investor]), HookError::NotCollateralCaller);

    // The real `["hook-caller"]` address, but without its signature (only the program can sign).
    let caller: Pubkey = mmf_sdk::pda::hook_caller().0;
    let ix = mmf_sdk::ix_hook::set_lien(&caller, false, &ctx.registry(), &holder, &holding, lock(1));
    assert_anchor_error(&h.send(&[ix], &[]), HookError::NotCollateralCaller);

    let state: compliance_hook::HolderState = read(&h, &holder);
    assert_eq!((state.pledged_amount, state.active_liens), (0, 0));
}

// ---- part 2 (T043): `set_lien`'s checks reached through a real pledge -----------------------

use collateral::CollateralError;
use tests::collateral::r;

/// Account positions in `pledge` (`MakePledge`): registry, giver_holder, giver_holding.
const REGISTRY: usize = 5;
const GIVER_HOLDER: usize = 6;
const GIVER_HOLDING: usize = 7;

fn pledge_ix(h: &Harness, c: &fixtures::CollatCtx, units: u64, n: u8) -> tests::Ix {
    mmf_sdk::ix_collateral::pledge(&c.giver.pubkey(), &h.payer.pubkey(), &c.agr, units, r(n))
}

#[test]
fn holding_account_must_be_the_givers_for_this_mint() {
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    let m = c.m();

    // Another owner's holding account (the taker's) next to the giver's record.
    let mut ix = pledge_ix(&h, &c, 1, 1);
    ix.accounts[GIVER_HOLDING].pubkey = mmf_sdk::pda::holding_account(&c.taker.pubkey(), &m);
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), HookError::WrongHolder);

    // The giver's account for another fund's mint.
    let other = fixtures::launched_fund(&mut h, 0);
    let g = c.giver.pubkey();
    let create = mmf_sdk::ix_hook::create_holding_account(&h.payer.pubkey(), &other.m(), &g);
    h.send(&[create], &[]).expect("create the other mint's account");
    let mut ix = pledge_ix(&h, &c, 1, 2);
    ix.accounts[GIVER_HOLDING].pubkey = mmf_sdk::pda::holding_account(&g, &other.m());
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), HookError::WrongHolder);

    // Another owner's holder record: collateral refuses it before the CPI.
    let mut ix = pledge_ix(&h, &c, 1, 3);
    ix.accounts[GIVER_HOLDER].pubkey = mmf_sdk::pda::holder(&m, &c.taker.pubkey()).0;
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), CollateralError::WrongHolder);

    // Another fund's Registry: its mint differs from the holding account's.
    let mut ix = pledge_ix(&h, &c, 1, 4);
    ix.accounts[REGISTRY].pubkey = other.registry();
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), HookError::WrongHolder);

    let state: compliance_hook::HolderState = read(&h, &mmf_sdk::pda::holder(&m, &g).0);
    assert_eq!((state.pledged_amount, state.active_liens), (0, 0));
}

#[test]
fn registry_without_a_collateral_program_refuses_liens() {
    // Models a Registry created before F004 (research R16); no instruction can produce one now.
    let mut h = Harness::new();
    let c = fixtures::collateral_ready_fund(&mut h);
    fixtures::patch::<Registry>(&mut h, &c.registry(), |reg| reg.collateral_program = Pubkey::default());
    let ix = pledge_ix(&h, &c, 1, 1);
    assert_anchor_error(&h.send(&[ix], &[&c.giver]), HookError::NotCollateralCaller);
}
