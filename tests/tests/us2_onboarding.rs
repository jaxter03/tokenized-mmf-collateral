//! US2 — Approve holders (FR-013 to FR-017, FR-019, FR-015). Independent test: register an
//! institution, approve it with a valid statement, send it units; every invalid statement and
//! every unapproved receiver is rejected.

use anchor_lang::prelude::Pubkey;
use compliance_hook::events::HolderRegistered;
use compliance_hook::{HolderState, HookError};
use mmf_common::enums::{HolderClass, HolderStatus};
use mmf_sdk::{ix_hook, pda};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::events::one_cpi_event;
use tests::fixtures::{self, read, FundCtx};
use tests::Harness;

const INVESTOR: u8 = 0;
const TREASURY: u8 = 1;
const ESCROW: u8 = 2;

fn setup() -> (Harness, FundCtx) {
    let mut h = Harness::new();
    let ctx = fixtures::launched_fund(&mut h, 0);
    (h, ctx)
}

fn holder(h: &Harness, ctx: &FundCtx, owner: &Pubkey) -> HolderState {
    read(h, &pda::holder(&ctx.m(), owner).0)
}

// ---------------------------------------------------------------------------------------------
// T045: registration (FR-013, FR-014, US2-1, US2-8, US2-9)
// ---------------------------------------------------------------------------------------------

#[test]
fn institution_registers_itself_as_pending_investor() {
    let (mut h, ctx) = setup();
    let inst = h.funded_keypair();
    let o = inst.pubkey();
    let meta = h
        .send(
            &[ix_hook::register_holder(&o, &o, &ctx.m(), &o, INVESTOR)],
            &[&inst],
        )
        .expect("register");

    let rec = holder(&h, &ctx, &o);
    assert_eq!(rec.status, HolderStatus::Pending.as_u8()); // US2-1
    assert_eq!(rec.class, HolderClass::Investor.as_u8());
    assert_eq!((rec.owner, rec.mint, rec.version), (o, ctx.m(), 1));
    assert_eq!(
        (rec.kyc_expiry_ts, rec.jurisdiction, rec.last_balance),
        (0, [0, 0], 0)
    );
    let ev = one_cpi_event::<HolderRegistered>(&meta);
    assert_eq!(
        (ev.fund, ev.owner, ev.class, ev.signer),
        (ctx.fund_state(), o, INVESTOR, o)
    );
}

#[test]
fn compliance_registers_investor_and_treasury() {
    let (mut h, ctx) = setup();
    let c = ctx.compliance.pubkey();
    let inst = Pubkey::new_unique();
    h.send(
        &[ix_hook::register_holder(&c, &c, &ctx.m(), &inst, INVESTOR)],
        &[&ctx.compliance],
    )
    .expect("compliance registers an investor");
    assert_eq!(
        holder(&h, &ctx, &inst).status,
        HolderStatus::Pending.as_u8()
    );

    let treasury = pda::treasury(&ctx.fund_state()).0;
    h.send(
        &[ix_hook::register_holder(
            &c,
            &c,
            &ctx.m(),
            &treasury,
            TREASURY,
        )],
        &[&ctx.compliance],
    )
    .expect("compliance registers the fund treasury");
    let rec = holder(&h, &ctx, &treasury);
    assert_eq!(
        (rec.status, rec.class),
        (
            HolderStatus::Pending.as_u8(),
            HolderClass::FundTreasury.as_u8()
        )
    );
}

#[test]
fn registration_needs_an_open_fund() {
    let mut h = Harness::new();
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    let m = mint.pubkey();
    h.send(&[ix_hook::init_registry(&issuer.pubkey(), &m)], &[&issuer])
        .unwrap();
    let inst = h.funded_keypair();
    let o = inst.pubkey();
    let res = h.send(
        &[ix_hook::register_holder(&o, &o, &m, &o, INVESTOR)],
        &[&inst],
    );
    assert_anchor_error(&res, HookError::FundNotOpen);
}

#[test]
fn registration_rejections_in_contract_order() {
    let (mut h, ctx) = setup();
    let m = ctx.m();
    let c = ctx.compliance.pubkey();

    // InvalidClass (the backlog CollateralVault class and anything above).
    for class in [3u8, 9] {
        let res = h.send(
            &[ix_hook::register_holder(
                &c,
                &c,
                &m,
                &Pubkey::new_unique(),
                class,
            )],
            &[&ctx.compliance],
        );
        assert_anchor_error(&res, HookError::InvalidClass);
    }

    // Unauthorized: someone who is neither compliance nor the owner (US2-9 "a different owner").
    let stranger = h.funded_keypair();
    let s = stranger.pubkey();
    let res = h.send(
        &[ix_hook::register_holder(
            &s,
            &s,
            &m,
            &Pubkey::new_unique(),
            INVESTOR,
        )],
        &[&stranger],
    );
    assert_anchor_error(&res, HookError::Unauthorized);

    // SelfRegistrationClassNotAllowed: an institution registering itself as treasury or escrow (US2-9).
    for class in [TREASURY, ESCROW] {
        let res = h.send(
            &[ix_hook::register_holder(&s, &s, &m, &s, class)],
            &[&stranger],
        );
        assert_anchor_error(&res, HookError::SelfRegistrationClassNotAllowed);
    }

    // ClassNotAvailable: escrow arrives with the settlement feature (F003, research R17).
    let escrow_owner = Pubkey::new_unique();
    let res = h.send(
        &[ix_hook::register_holder(&c, &c, &m, &escrow_owner, ESCROW)],
        &[&ctx.compliance],
    );
    assert_anchor_error(&res, HookError::ClassNotAvailable);

    // NotSystemControlled: a treasury record for any owner other than the fund's treasury PDA.
    let res = h.send(
        &[ix_hook::register_holder(
            &c,
            &c,
            &m,
            &Pubkey::new_unique(),
            TREASURY,
        )],
        &[&ctx.compliance],
    );
    assert_anchor_error(&res, HookError::NotSystemControlled);

    // None of these created a record.
    assert!(!h.exists(&pda::holder(&m, &s).0));
    assert!(!h.exists(&pda::holder(&m, &escrow_owner).0));
}

#[test]
fn second_registration_is_rejected_and_record_unchanged() {
    // US2-8, FR-014: by the owner itself and by compliance.
    let (mut h, ctx) = setup();
    let inst = h.funded_keypair();
    let o = inst.pubkey();
    let m = ctx.m();
    h.send(
        &[ix_hook::register_holder(&o, &o, &m, &o, INVESTOR)],
        &[&inst],
    )
    .unwrap();
    let key = pda::holder(&m, &o).0;
    let before = h.data(&key);

    let res = h.send(
        &[ix_hook::register_holder(&o, &o, &m, &o, INVESTOR)],
        &[&inst],
    );
    assert_anchor_error(&res, HookError::HolderAlreadyRegistered);
    let c = ctx.compliance.pubkey();
    let res = h.send(
        &[ix_hook::register_holder(&c, &c, &m, &o, INVESTOR)],
        &[&ctx.compliance],
    );
    assert_anchor_error(&res, HookError::HolderAlreadyRegistered);
    assert_eq!(
        h.data(&key),
        before,
        "existing record byte-for-byte unchanged"
    );
}

#[test]
fn prefunded_holder_address_can_still_be_registered() {
    // Anyone can send lamports to the future holder-record address; that must not block
    // registration (the record is created with allocate + assign instead of create_account).
    let (mut h, ctx) = setup();
    let inst = h.funded_keypair();
    let o = inst.pubkey();
    let key = pda::holder(&ctx.m(), &o).0;
    let griefer = h.funded_keypair();
    let gift = solana_system_interface::instruction::transfer(&griefer.pubkey(), &key, 1_000_000);
    h.send(&[gift], &[&griefer]).unwrap();

    h.send(
        &[ix_hook::register_holder(&o, &o, &ctx.m(), &o, INVESTOR)],
        &[&inst],
    )
    .expect("register");
    assert_eq!(holder(&h, &ctx, &o).status, HolderStatus::Pending.as_u8());
}
