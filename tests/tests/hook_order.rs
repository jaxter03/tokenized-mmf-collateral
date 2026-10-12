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
