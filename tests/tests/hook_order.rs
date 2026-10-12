//! Transfer rules enforced by the hook (FR-020 to FR-026), contracts/compliance_hook.md
//! "Transfer hook". Every transfer here is a real Token-2022 `transfer_checked`.

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022::error::TokenError;
use compliance_hook::events::TransferValidated;
use compliance_hook::{HolderState, HookError, Registry};
use mmf_common::ids::COMPLIANCE_HOOK_PROGRAM_ID;
use mmf_sdk::{ix_hook, pda, transfer};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_token_error};
use tests::events::log_events;
use tests::fixtures::{self, FundCtx};
use tests::{clock, units, Harness, Ix};

/// Onboards `owner` with a statement expiring at `expiry_ts`.
fn onboard_until(h: &mut Harness, ctx: &FundCtx, owner: &Keypair, expiry_ts: i64) {
    let o = owner.pubkey();
    h.send(
        &[ix_hook::register_holder(&o, &o, &ctx.m(), &o, 0)],
        &[owner],
    )
    .unwrap();
    let signed = fixtures::signed_statement(ctx, &o, *b"US", expiry_ts);
    let ixs = ix_hook::onboarding_tx(
        &h.payer.pubkey(),
        &ctx.compliance.pubkey(),
        &ctx.m(),
        &o,
        &signed,
    );
    h.send(&ixs, &[&ctx.compliance]).unwrap();
}

fn holder(h: &Harness, ctx: &FundCtx, owner: &Pubkey) -> HolderState {
    fixtures::read(h, &pda::holder(&ctx.m(), owner).0)
}

fn registry(h: &Harness, ctx: &FundCtx) -> Registry {
    fixtures::read(h, &ctx.registry())
}

// ---------------------------------------------------------------------------------------------
// T048: who may send and receive (FR-020 to FR-023, FR-025)
// ---------------------------------------------------------------------------------------------

#[test]
fn approved_to_approved_transfer_succeeds_and_is_logged() {
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    let meta = fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 400).expect("transfer");
    assert_eq!(units::balance(&h, &ctx.m(), &alice.pubkey()), 600);
    assert_eq!(units::balance(&h, &ctx.m(), &bob.pubkey()), 400);
    let ev = log_events::<TransferValidated>(&meta);
    assert_eq!(ev.len(), 1);
    assert_eq!(
        (ev[0].source_owner, ev[0].destination_owner, ev[0].amount),
        (alice.pubkey(), bob.pubkey(), 400)
    );
    assert_eq!(ev[0].fund, ctx.fund_state());
}

#[test]
fn transfer_resolved_from_chain_data_follows_the_same_rules() {
    // FR-020: a wallet resolving the extra accounts from the on-chain list gets the same result.
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    let m = ctx.m();
    let (src, dst) = (
        pda::holding_account(&alice.pubkey(), &m),
        pda::holding_account(&bob.pubkey(), &m),
    );
    let svm = &h.svm;
    let fetch = |k: Pubkey| {
        let v = svm.get_account(&k).map(|a| a.data);
        async move { Ok(v) }
    };
    let ix = futures::executor::block_on(transfer::transfer_checked_with_hook(
        &src,
        &m,
        &dst,
        &alice.pubkey(),
        10,
        fetch,
    ))
    .expect("resolve extra accounts");
    h.send(&[ix], &[&alice]).expect("resolved transfer");
    assert_eq!(units::balance(&h, &m, &bob.pubkey()), 10);
}

#[test]
fn receivers_that_are_not_approved_are_frozen_out() {
    // US2-7: a Pending or unregistered owner's account is still frozen; Token-2022 rejects first.
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let m = ctx.m();
    let pending = h.funded_keypair();
    let p = pending.pubkey();
    h.send(&[ix_hook::register_holder(&p, &p, &m, &p, 0)], &[&pending])
        .unwrap();
    let stranger = Pubkey::new_unique();
    for owner in [p, stranger] {
        h.send(
            &[ix_hook::create_holding_account(
                &h.payer.pubkey(),
                &m,
                &owner,
            )],
            &[],
        )
        .unwrap();
        assert_token_error(
            &fixtures::transfer(&mut h, &ctx, &alice, &owner, 1),
            TokenError::AccountFrozen,
        );
        assert_token_error(
            &fixtures::transfer(&mut h, &ctx, &alice, &owner, 0),
            TokenError::AccountFrozen,
        );
    }
    assert_eq!(
        units::balance(&h, &m, &alice.pubkey()),
        1_000,
        "no balance changed"
    );
}

#[test]
fn kyc_expires_at_the_exact_second() {
    // FR-022, FR-023, FR-025.
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let now = clock::now(&h.svm);
    let alice = h.funded_keypair();
    onboard_until(&mut h, &ctx, &alice, now + 1_000);
    let bob = h.funded_keypair();
    onboard_until(&mut h, &ctx, &bob, now + 2_000);
    units::seed_units(&mut h, &ctx.m(), &alice.pubkey(), 100);

    clock::set_unix_timestamp(&mut h.svm, now + 999);
    fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1).expect("one second before expiry");

    clock::set_unix_timestamp(&mut h.svm, now + 1_000);
    let res = fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1);
    assert_anchor_error(&res, HookError::SenderNotApproved);
    // Zero-unit transfers run the same checks.
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 0),
        HookError::SenderNotApproved,
    );

    clock::set_unix_timestamp(&mut h.svm, now + 2_000);
    let res = fixtures::transfer(&mut h, &ctx, &bob, &alice.pubkey(), 1);
    assert_anchor_error(&res, HookError::SenderNotApproved);
}

#[test]
fn expired_receiver_is_rejected_even_for_zero_units() {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    let now = clock::now(&h.svm);
    let alice = h.funded_keypair();
    onboard_until(&mut h, &ctx, &alice, now + 10_000);
    let bob = h.funded_keypair();
    onboard_until(&mut h, &ctx, &bob, now + 1_000);
    units::seed_units(&mut h, &ctx.m(), &alice.pubkey(), 100);
    clock::set_unix_timestamp(&mut h.svm, now + 1_000);
    for amount in [5, 0] {
        assert_anchor_error(
            &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), amount),
            HookError::ReceiverNotApproved,
        );
    }
    assert_eq!(units::balance(&h, &ctx.m(), &bob.pubkey()), 0);
}

#[test]
fn receiver_jurisdiction_and_class_are_checked_on_every_transfer() {
    // Unreachable through instructions in F001 (rules are fixed after Open and approval already
    // checks the jurisdiction), so the receiver's record is patched to prove the defensive check
    // fires (quickstart FR-023).
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    let key = pda::holder(&ctx.m(), &bob.pubkey()).0;

    fixtures::patch::<HolderState>(&mut h, &key, |r| r.jurisdiction = *b"FR");
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::ReceiverNotApproved,
    );

    fixtures::patch::<HolderState>(&mut h, &key, |r| {
        r.jurisdiction = *b"US";
        r.class = 3; // CollateralVault [backlog]
    });
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::ReceiverNotApproved,
    );
}

#[test]
fn accounts_from_another_fund_are_rejected() {
    // Token-2022 resolves the extra accounts from this mint's list, so a mismatch can't happen
    // through a normal transfer; the Registry's mint is patched to prove the check fires.
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    fixtures::patch::<Registry>(&mut h, &ctx.registry(), |r| r.mint = Pubkey::new_unique());
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::WrongMint,
    );
}

#[test]
fn the_hook_cannot_be_called_directly() {
    // Outside a real transfer the token accounts aren't in the "transferring" state.
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    let m = ctx.m();
    let t = transfer::transfer_between_owners(&m, &alice.pubkey(), &bob.pubkey(), 1);
    // execute's accounts: source, mint, destination, authority, extra-account list, then the extras.
    let mut accounts = vec![
        t.accounts[0].clone(),
        t.accounts[1].clone(),
        t.accounts[2].clone(),
    ];
    let mut authority = t.accounts[3].clone();
    authority.is_signer = false;
    accounts.push(authority);
    accounts.push(
        anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
            pda::extra_metas(&m).0,
            false,
        ),
    );
    accounts.extend(t.accounts[4..9].iter().cloned());
    let ix = Ix {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts,
        data: spl_transfer_hook_interface::instruction::TransferHookInstruction::Execute {
            amount: 1,
        }
        .pack(),
    };
    assert_anchor_error(&h.send(&[ix], &[]), HookError::NotTransferring);
}

#[test]
fn transfer_to_the_same_account_skips_the_hook_and_changes_nothing() {
    // Spec edge case "Same sender and receiver" (research R4): Token-2022 returns before the hook.
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let before = (
        registry(&h, &ctx).holder_count,
        holder(&h, &ctx, &alice.pubkey()).last_balance,
    );
    let meta =
        fixtures::transfer(&mut h, &ctx, &alice, &alice.pubkey(), 10).expect("self-transfer");
    assert!(
        log_events::<TransferValidated>(&meta).is_empty(),
        "hook not invoked"
    );
    assert_eq!(units::balance(&h, &ctx.m(), &alice.pubkey()), 1_000);
    assert_eq!(
        (
            registry(&h, &ctx).holder_count,
            holder(&h, &ctx, &alice.pubkey()).last_balance
        ),
        before
    );
}

// ---------------------------------------------------------------------------------------------
// T049: holder cap and the fixed order of checks (FR-024, FR-026, I10)
// ---------------------------------------------------------------------------------------------

/// Asserts `holder_count` and each investor's `last_balance` against the actual balances.
#[track_caller]
fn assert_count(h: &Harness, ctx: &FundCtx, expected: u32, investors: &[&Keypair]) {
    assert_eq!(registry(h, ctx).holder_count, expected, "holder_count");
    for k in investors {
        let actual = units::balance(h, &ctx.m(), &k.pubkey());
        assert_eq!(
            holder(h, ctx, &k.pubkey()).last_balance,
            actual,
            "last_balance tracks the balance"
        );
    }
}

#[test]
fn holder_cap_counts_investors_with_units() {
    let mut h = Harness::new();
    let (ctx, a) = fixtures::fund_with_investor_units(&mut h, 2, 1_000);
    let b = fixtures::new_investor(&mut h, &ctx);
    let c = fixtures::new_investor(&mut h, &ctx);
    let all = [&a, &b, &c];
    assert_count(&h, &ctx, 1, &all);

    fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), 100).unwrap();
    assert_count(&h, &ctx, 2, &all);

    // At the cap: a new investor can't receive...
    let res = fixtures::transfer(&mut h, &ctx, &a, &c.pubkey(), 1);
    assert_anchor_error(&res, HookError::HolderCapReached);
    assert_count(&h, &ctx, 2, &all);
    // ...even a zero-unit transfer is fine because it doesn't make C a holder.
    fixtures::transfer(&mut h, &ctx, &a, &c.pubkey(), 0).expect("zero units don't add a holder");
    assert_count(&h, &ctx, 2, &all);

    // ...but existing holders can still receive more.
    fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), 50).unwrap();
    assert_count(&h, &ctx, 2, &all);

    // A place frees up when an investor's balance falls to zero.
    fixtures::transfer(&mut h, &ctx, &b, &a.pubkey(), 150).unwrap();
    assert_count(&h, &ctx, 1, &all);
    fixtures::transfer(&mut h, &ctx, &a, &c.pubkey(), 10).expect("place freed");
    assert_count(&h, &ctx, 2, &all);
}

#[test]
fn fund_treasury_is_never_counted() {
    let mut h = Harness::new();
    let (ctx, a) = fixtures::fund_with_investor_units(&mut h, 1, 1_000);
    let treasury = fixtures::onboard_treasury(&mut h, &ctx);
    // The cap (1) is reached by A alone, yet the treasury can receive and isn't counted.
    fixtures::transfer(&mut h, &ctx, &a, &treasury, 300).expect("treasury receives at the cap");
    assert_eq!(units::balance(&h, &ctx.m(), &treasury), 300);
    assert_count(&h, &ctx, 1, &[&a]);
    // A sending everything to the treasury frees A's place.
    fixtures::transfer(&mut h, &ctx, &a, &treasury, 700).unwrap();
    assert_count(&h, &ctx, 0, &[&a]);
}

#[test]
fn no_cap_means_no_limit() {
    let mut h = Harness::new();
    let (ctx, a) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let others: Vec<Keypair> = (0..5)
        .map(|_| fixtures::new_investor(&mut h, &ctx))
        .collect();
    for o in &others {
        fixtures::transfer(&mut h, &ctx, &a, &o.pubkey(), 10).unwrap();
    }
    let refs: Vec<&Keypair> = std::iter::once(&a).chain(others.iter()).collect();
    assert_count(&h, &ctx, 6, &refs);
}

#[test]
fn first_failure_is_reported_in_the_documented_order() {
    // FR-026: Token-2022's frozen check, then the hook's checks in contract order.
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 1);
    let now = clock::now(&h.svm);
    let a = h.funded_keypair();
    onboard_until(&mut h, &ctx, &a, now + 1_000);
    let b = h.funded_keypair();
    onboard_until(&mut h, &ctx, &b, now + 1_000);
    units::seed_units(&mut h, &ctx.m(), &a.pubkey(), 100); // A holds; the cap (1) is reached
    let frozen = Pubkey::new_unique();
    h.send(
        &[ix_hook::create_holding_account(
            &h.payer.pubkey(),
            &ctx.m(),
            &frozen,
        )],
        &[],
    )
    .unwrap();
    clock::set_unix_timestamp(&mut h.svm, now + 1_000); // A and B expire

    // Frozen receiver and expired sender → Token-2022's AccountFrozen comes first.
    assert_token_error(
        &fixtures::transfer(&mut h, &ctx, &a, &frozen, 1),
        TokenError::AccountFrozen,
    );
    // Expired sender and expired receiver (also new, at the cap) → SenderNotApproved.
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &a, &b.pubkey(), 1),
        HookError::SenderNotApproved,
    );

    // Valid sender, expired receiver that would also break the cap → ReceiverNotApproved.
    // A is expired and can't send, so C (valid KYC) is seeded directly; that puts the count at 2,
    // over the cap of 1, which the receiver check must report before the cap check is reached.
    let c = h.funded_keypair();
    onboard_until(&mut h, &ctx, &c, now + 10_000);
    let fresh = h.funded_keypair();
    onboard_until(&mut h, &ctx, &fresh, now + 10_000);
    units::seed_units(&mut h, &ctx.m(), &c.pubkey(), 50);
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &c, &b.pubkey(), 1),
        HookError::ReceiverNotApproved,
    );
    // Valid sender and valid new receiver at the cap → HolderCapReached.
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &c, &fresh.pubkey(), 1),
        HookError::HolderCapReached,
    );
}

// ---------------------------------------------------------------------------------------------
// T064: pause and suspension in the check order (FR-026), self-transfer while suspended
// ---------------------------------------------------------------------------------------------

use mmf_common::enums::ReasonCode;
use mmf_sdk::ix_governance;

const REASON: u8 = ReasonCode::CourtOrder as u8;

fn suspend(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey) {
    let ix =
        ix_governance::suspend_holder(&ctx.compliance.pubkey(), &ctx.m(), owner, REASON, [0; 32]);
    h.send(&[ix], &[&ctx.compliance]).expect("suspend");
}

fn pause(h: &mut Harness, ctx: &FundCtx) {
    let ix = ix_governance::pause_transfers(&ctx.compliance.pubkey(), &ctx.m(), REASON, [0; 32]);
    h.send(&[ix], &[&ctx.compliance]).expect("pause");
}

#[test]
fn pause_and_suspension_follow_the_documented_order() {
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    let frozen = Pubkey::new_unique();
    h.send(
        &[ix_hook::create_holding_account(
            &h.payer.pubkey(),
            &ctx.m(),
            &frozen,
        )],
        &[],
    )
    .unwrap();

    suspend(&mut h, &ctx, &alice.pubkey());
    suspend(&mut h, &ctx, &bob.pubkey());
    // Suspended sender and suspended receiver → SenderNotApproved (sender checked first).
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::SenderNotApproved,
    );

    pause(&mut h, &ctx);
    // Paused and suspended sender → TransfersPaused (pause checked before the sender).
    assert_anchor_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1),
        HookError::TransfersPaused,
    );
    // Paused and frozen receiver → Token-2022's AccountFrozen comes before any hook check.
    assert_token_error(
        &fixtures::transfer(&mut h, &ctx, &alice, &frozen, 1),
        TokenError::AccountFrozen,
    );
}

#[test]
fn suspended_holder_sending_to_itself_changes_nothing() {
    // Research R4: a same-account transfer never reaches the hook, so it succeeds even while
    // suspended, but moves nothing and leaves every record unchanged.
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    suspend(&mut h, &ctx, &alice.pubkey());
    let before = (
        registry(&h, &ctx).holder_count,
        holder(&h, &ctx, &alice.pubkey()).last_balance,
    );
    fixtures::transfer(&mut h, &ctx, &alice, &alice.pubkey(), 25).expect("self-transfer");
    assert_eq!(units::balance(&h, &ctx.m(), &alice.pubkey()), 1_000);
    assert_eq!(
        (
            registry(&h, &ctx).holder_count,
            holder(&h, &ctx, &alice.pubkey()).last_balance
        ),
        before
    );
    assert_eq!(
        holder(&h, &ctx, &alice.pubkey()).status,
        mmf_common::enums::HolderStatus::Suspended.as_u8()
    );
}

// ---------------------------------------------------------------------------------------------
// T077: checked arithmetic in the holder count (Constitution IX)
// ---------------------------------------------------------------------------------------------

#[test]
fn holder_count_arithmetic_is_checked() {
    // Unreachable through instructions (the count always matches recorded balances, I10), so the
    // Registry is patched to 0 holders while Alice still holds units: her emptying transfer would
    // take the count below zero and must abort instead of wrapping.
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let bob = fixtures::new_investor(&mut h, &ctx);
    fixtures::transfer(&mut h, &ctx, &alice, &bob.pubkey(), 1).unwrap(); // both hold units
    fixtures::patch::<Registry>(&mut h, &ctx.registry(), |r| r.holder_count = 0);
    let res = fixtures::transfer(&mut h, &ctx, &bob, &alice.pubkey(), 1); // Bob drops to zero
    assert_anchor_error(&res, HookError::Overflow);
    assert_eq!(
        units::balance(&h, &ctx.m(), &bob.pubkey()),
        1,
        "nothing moved"
    );
}
