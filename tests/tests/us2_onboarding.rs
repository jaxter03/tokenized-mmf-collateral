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

// ---------------------------------------------------------------------------------------------
// T046: approve_investor and the KYC statement checks (FR-016, FR-017, contracts/kyc-statement.md)
// ---------------------------------------------------------------------------------------------

use compliance_hook::events::HolderApproved;
use mmf_common::statement::{self, Statement};
use mmf_sdk::kyc::{self, tamper, SignedStatement};
use tests::clock;
use tests::{Ix, TxResult};

/// `approve_investor` account index of the instructions sysvar.
const APPROVE_INSTRUCTIONS_SYSVAR: usize = 5;

/// A registered (Pending) investor.
fn registered(h: &mut Harness, ctx: &FundCtx) -> solana_keypair::Keypair {
    let inst = h.funded_keypair();
    let o = inst.pubkey();
    h.send(
        &[ix_hook::register_holder(&o, &o, &ctx.m(), &o, INVESTOR)],
        &[&inst],
    )
    .unwrap();
    inst
}

fn stmt(h: &Harness, ctx: &FundCtx, owner: &Pubkey) -> Statement {
    kyc::statement(
        ctx.m(),
        *owner,
        *b"US",
        2,
        clock::now(&h.svm) + fixtures::YEAR,
    )
}

fn approve_ix(ctx: &FundCtx, owner: &Pubkey, hash: [u8; 32]) -> Ix {
    ix_hook::approve_investor(&ctx.compliance.pubkey(), &ctx.m(), owner, hash)
}

/// `[ed25519, approve_investor]` signed by compliance.
fn approve(h: &mut Harness, ctx: &FundCtx, owner: &Pubkey, signed: &SignedStatement) -> TxResult {
    h.send(
        &[
            signed.ix.clone(),
            approve_ix(ctx, owner, signed.attestation_hash),
        ],
        &[&ctx.compliance],
    )
}

/// Asserts the approval failed with `expected` and the record is byte-for-byte unchanged.
#[track_caller]
fn assert_rejected(
    h: &Harness,
    ctx: &FundCtx,
    owner: &Pubkey,
    before: &[u8],
    res: TxResult,
    expected: HookError,
) {
    assert_anchor_error(&res, expected);
    assert_eq!(
        h.data(&pda::holder(&ctx.m(), owner).0),
        before,
        "record unchanged after {expected:?}"
    );
}

#[test]
fn compliance_approves_with_a_valid_statement() {
    // US2-2, FR-016, FR-017.
    let (mut h, ctx) = setup();
    let o = registered(&mut h, &ctx).pubkey();
    let s = stmt(&h, &ctx, &o);
    let signed = kyc::sign(&ctx.provider, &s);
    let meta = approve(&mut h, &ctx, &o, &signed).expect("approve");

    let rec = holder(&h, &ctx, &o);
    assert_eq!(rec.status, HolderStatus::Approved.as_u8());
    assert_eq!(rec.jurisdiction, *b"US");
    assert_eq!(rec.investor_type, 2);
    assert_eq!(rec.kyc_expiry_ts, s.expiry_ts);
    assert_eq!(rec.attestation_hash, statement::fingerprint(&s.encode()));
    assert_eq!(rec.attested_by, ctx.provider.pubkey());
    let ev = one_cpi_event::<HolderApproved>(&meta);
    assert_eq!(
        (ev.owner, ev.renewal, ev.kyc_expiry_ts, ev.attested_by),
        (o, false, s.expiry_ts, ctx.provider.pubkey())
    );
    assert_eq!(ev.signer, ctx.compliance.pubkey());
}

#[test]
fn approval_authority_status_and_class_checks() {
    let (mut h, ctx) = setup();
    let o = registered(&mut h, &ctx).pubkey();
    let before = h.data(&pda::holder(&ctx.m(), &o).0);
    let signed = kyc::sign(&ctx.provider, &stmt(&h, &ctx, &o));

    // Unauthorized: the issuer is not compliance.
    let ix = ix_hook::approve_investor(&ctx.issuer.pubkey(), &ctx.m(), &o, signed.attestation_hash);
    let res = h.send(&[signed.ix.clone(), ix], &[&ctx.issuer]);
    assert_rejected(&h, &ctx, &o, &before, res, HookError::Unauthorized);

    // InvalidHolderStatus: already Approved.
    approve(&mut h, &ctx, &o, &signed).expect("first approval");
    let res = approve(&mut h, &ctx, &o, &signed);
    assert_anchor_error(&res, HookError::InvalidHolderStatus);

    // NotInvestor: a fund treasury record can't be approved with a statement.
    let c = ctx.compliance.pubkey();
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
    .unwrap();
    let t = kyc::sign(&ctx.provider, &stmt(&h, &ctx, &treasury));
    assert_anchor_error(
        &approve(&mut h, &ctx, &treasury, &t),
        HookError::NotInvestor,
    );
}

#[test]
fn statement_must_be_the_ed25519_instruction_just_before() {
    let (mut h, ctx) = setup();
    let o = registered(&mut h, &ctx).pubkey();
    let before = h.data(&pda::holder(&ctx.m(), &o).0);
    let signed = kyc::sign(&ctx.provider, &stmt(&h, &ctx, &o));

    // No statement at all.
    let res = h.send(
        &[approve_ix(&ctx, &o, signed.attestation_hash)],
        &[&ctx.compliance],
    );
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMissing);

    // A statement, but not immediately before.
    let filler = solana_system_interface::instruction::transfer(
        &h.payer.pubkey(),
        &Pubkey::new_unique(),
        1_000_000,
    );
    let res = h.send(
        &[
            signed.ix.clone(),
            filler,
            approve_ix(&ctx, &o, signed.attestation_hash),
        ],
        &[&ctx.compliance],
    );
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMissing);

    // A fake instructions sysvar account.
    let mut ix = approve_ix(&ctx, &o, signed.attestation_hash);
    ix.accounts[APPROVE_INSTRUCTIONS_SYSVAR].pubkey = Pubkey::new_unique();
    let res = h.send(&[signed.ix.clone(), ix], &[&ctx.compliance]);
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMissing);
}

#[test]
fn malformed_statement_instructions_are_rejected() {
    let (mut h, ctx) = setup();
    let o = registered(&mut h, &ctx).pubkey();
    let before = h.data(&pda::holder(&ctx.m(), &o).0);
    let s = stmt(&h, &ctx, &o);
    let signed = kyc::sign(&ctx.provider, &s);
    let hash = signed.attestation_hash;

    // Two signatures in one instruction.
    let res = h.send(
        &[tamper::two_signatures(&signed), approve_ix(&ctx, &o, hash)],
        &[&ctx.compliance],
    );
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMalformed);

    // Zero signatures (the precompile accepts it trivially).
    let res = h.send(
        &[tamper::zero_signatures(), approve_ix(&ctx, &o, hash)],
        &[&ctx.compliance],
    );
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMalformed);

    // Offsets pointing into another instruction (the Relay bridge bug): instruction 0 carries a
    // valid signed statement, instruction 1 points at it, approve inspects instruction 1.
    let res = h.send(
        &[
            signed.ix.clone(),
            tamper::referencing(0, &signed),
            approve_ix(&ctx, &o, hash),
        ],
        &[&ctx.compliance],
    );
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMalformed);

    // A validly signed 94-byte message.
    let short = kyc::sign_bytes(&ctx.provider, &s.encode()[..94]);
    let res = approve(&mut h, &ctx, &o, &short);
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMalformed);

    // A validly signed message with the wrong domain tag.
    let mut wrong = s.encode();
    wrong[0] ^= 1;
    let res = approve(&mut h, &ctx, &o, &kyc::sign_bytes(&ctx.provider, &wrong));
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementMalformed);
}

#[test]
fn statement_content_checks_in_contract_order() {
    let (mut h, ctx) = setup();
    let o = registered(&mut h, &ctx).pubkey();
    let before = h.data(&pda::holder(&ctx.m(), &o).0);
    let base = stmt(&h, &ctx, &o);
    let now = clock::now(&h.svm);

    let check =
        |h: &mut Harness, signer: &solana_keypair::Keypair, s: Statement, expected: HookError| {
            let res = approve(h, &ctx, &o, &kyc::sign(signer, &s));
            assert_rejected(h, &ctx, &o, &before, res, expected);
        };

    let rogue = solana_keypair::Keypair::new();
    check(&mut h, &rogue, base, HookError::ProviderNotRecognised); // US2-3
    check(
        &mut h,
        &ctx.provider,
        Statement {
            mint: Pubkey::new_unique(),
            ..base
        },
        HookError::StatementWrongFund,
    );
    check(
        &mut h,
        &ctx.provider,
        Statement {
            owner: Pubkey::new_unique(),
            ..base
        },
        HookError::StatementOwnerMismatch,
    ); // US2-5
    check(
        &mut h,
        &ctx.provider,
        Statement {
            expiry_ts: now,
            ..base
        },
        HookError::StatementExpired,
    ); // US2-4
    check(
        &mut h,
        &ctx.provider,
        Statement {
            expiry_ts: now - 1,
            ..base
        },
        HookError::StatementExpired,
    );
    check(
        &mut h,
        &ctx.provider,
        Statement {
            jurisdiction: *b"AA",
            ..base
        },
        HookError::UnknownJurisdiction,
    );
    check(
        &mut h,
        &ctx.provider,
        Statement {
            jurisdiction: *b"FR",
            ..base
        },
        HookError::JurisdictionNotAllowed,
    ); // US2-6
    check(
        &mut h,
        &ctx.provider,
        Statement {
            investor_type: 0,
            ..base
        },
        HookError::InvalidInvestorType,
    );

    // The hash argument must match the statement actually signed (Squads binding, research R8).
    let signed = kyc::sign(&ctx.provider, &base);
    let other = kyc::sign(
        &ctx.provider,
        &Statement {
            investor_type: 3,
            ..base
        },
    );
    let res = h.send(
        &[
            signed.ix.clone(),
            approve_ix(&ctx, &o, other.attestation_hash),
        ],
        &[&ctx.compliance],
    );
    assert_rejected(&h, &ctx, &o, &before, res, HookError::StatementHashMismatch);

    // Several failures at once: the first in contract order wins.
    let res = approve(
        &mut h,
        &ctx,
        &o,
        &kyc::sign(
            &rogue,
            &Statement {
                expiry_ts: now,
                investor_type: 0,
                ..base
            },
        ),
    );
    assert_rejected(&h, &ctx, &o, &before, res, HookError::ProviderNotRecognised);
}
