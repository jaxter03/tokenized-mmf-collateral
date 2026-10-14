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
