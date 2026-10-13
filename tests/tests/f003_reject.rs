//! T059 (FR-025; contracts/governance.md `reject_settlement`): compliance rejects an open
//! instruction through governance, which signs dvp_settlement's `reject` with `["dvp-caller"]`.

use anchor_lang::prelude::Pubkey;
use anchor_lang::{InstructionData, ToAccountMetas};
use dvp_settlement::events::SettlementRejected;
use dvp_settlement::{CreateArgs, DvpError};
use governance::GovernanceError;
use mmf_common::enums::ReasonCode;
use mmf_common::ids::DVP_SETTLEMENT_PROGRAM_ID;
use mmf_sdk::ix_dvp::Deal;
use mmf_sdk::{ix_dvp, ix_fund, ix_governance, ix_hook, pda};
use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::assert::{assert_anchor_error, assert_squads_error};
use tests::events::{cpi_events, one_cpi_event};
use tests::fixtures::{self, SettleCtx, NAV_ONE, SETTLE_DAY};
use tests::settle::{self, sub_args, ONE};
use tests::squads::{self, INVALID_PROPOSAL_STATUS};
use tests::{clock, Harness, Ix};

const CASE: [u8; 32] = [0xC5; 32];
const SANCTIONS: u8 = ReasonCode::SanctionsHit as u8;

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

#[test]
fn compliance_rejects_in_every_open_status_and_the_funded_legs_are_earmarked() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 10 * ONE);

    let instructed = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    let seller_funded = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 2));
    settle::affirm(&mut h, &s, &seller_funded).unwrap();
    let buyer_funded = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), 2 * ONE, 3));
    settle::fund_cash(&mut h, &s, &buyer_funded, &a).unwrap();
    let affirmed = settle::affirmed_subscription(&mut h, &s, &a, 3 * ONE, 4);

    for (d, cash) in [
        (instructed, 0),
        (seller_funded, 0),
        (buyer_funded, 2 * ONE),
        (affirmed, 3 * ONE),
    ] {
        let meta = settle::reject(&mut h, &s, &d, SANCTIONS).expect("reject");
        let si = settle::read_si(&h, &d);
        assert_eq!(
            (si.status, si.units_to_return, si.cash_to_return),
            (7, 0, cash)
        );
        assert_eq!(si.closed_at, clock::now(&h.svm));
        let ev = one_cpi_event::<SettlementRejected>(&meta);
        assert_eq!(
            (ev.fund, ev.seq, ev.reason, ev.case_ref_hash, ev.actor),
            (
                s.fund_state(),
                si.seq,
                SANCTIONS,
                CASE,
                s.compliance.pubkey()
            )
        );
        assert_eq!((ev.units_to_return, ev.cash_to_return), (0, cash));
    }
}

#[test]
fn only_compliance_rejects() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    for who in [
        &s.issuer,
        &s.fund_admin,
        &s.transfer_agent,
        &s.keeper,
        &s.auditor,
        &a,
    ] {
        let ix = ix_governance::reject_settlement(
            &who.pubkey(),
            &s.m(),
            &d.settlement(),
            SANCTIONS,
            CASE,
        );
        assert_anchor_error(&h.send(&[ix], &[who]), GovernanceError::Unauthorized);
    }
    assert_eq!(settle::read_si(&h, &d).status, 0);
}

#[test]
fn only_the_compliance_reasons() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    for reason in [4u8, 5, 7] {
        assert_anchor_error(
            &settle::reject(&mut h, &s, &d, reason),
            GovernanceError::InvalidReason,
        );
    }
    for reason in [0u8, 2, 3, 6] {
        let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 10 + reason));
        settle::reject(&mut h, &s, &d, reason).expect("allowed reason");
    }
}

#[test]
fn one_funds_compliance_cannot_reject_another_funds_instruction() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let other = fixtures::settlement_ready_fund(&mut h);
    let b = fixtures::investor_with_cash(&mut h, &other, ONE);
    let d = settle::created(&mut h, &other, &b, sub_args(&other, &b.pubkey(), ONE, 1));
    // `s`'s compliance, `s`'s fund, `other`'s instruction.
    let ix = ix_governance::reject_settlement(
        &s.compliance.pubkey(),
        &s.m(),
        &d.settlement(),
        SANCTIONS,
        CASE,
    );
    assert_anchor_error(&h.send(&[ix], &[&s.compliance]), DvpError::FundMismatch);
    assert_eq!(settle::read_si(&h, &d).status, 0);
}

#[test]
fn dvp_reject_accepts_only_governance() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::created(&mut h, &s, &a, sub_args(&s, &a.pubkey(), ONE, 1));
    // Compliance calling dvp_settlement directly, not through governance.
    let accounts = dvp_settlement::accounts::Reject {
        dvp_caller: s.compliance.pubkey(),
        fund_state: s.fund_state(),
        settlement: d.settlement(),
        event_authority: pda::event_authority(&DVP_SETTLEMENT_PROGRAM_ID).0,
        program: DVP_SETTLEMENT_PROGRAM_ID,
    };
    let ix = Ix {
        program_id: DVP_SETTLEMENT_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: dvp_settlement::instruction::Reject {
            reason: SANCTIONS,
            case_ref_hash: CASE,
            actor: s.compliance.pubkey(),
        }
        .data(),
    };
    assert_anchor_error(
        &h.send(&[ix], &[&s.compliance]),
        DvpError::UnauthorizedCaller,
    );
}

#[test]
fn a_party_suspended_after_funding_is_rejected_and_refunded() {
    // Spec edge case: suspension mid-flight, then compliance rejects and the cash goes back.
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, 5 * ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, 5 * ONE, 1);
    let ix =
        ix_governance::suspend_holder(&s.compliance.pubkey(), &s.m(), &a.pubkey(), SANCTIONS, CASE);
    h.send(&[ix], &[&s.compliance]).unwrap();
    settle::reject(&mut h, &s, &d, SANCTIONS).expect("reject");
    settle::return_cash(&mut h, &s, &d, &s.keeper).expect("cash goes back even while suspended");
    assert_eq!(settle::cash_of(&h, &s, &a.pubkey()), 5 * ONE);
}

// ---- compliance as a Squads 2-of-3 vault (call depth 4: Squads → governance → dvp → emit_cpi) --

#[test]
fn a_multisig_compliance_rejects_only_at_its_threshold() {
    let mut h = Harness::new();
    clock::set_utc_day(&mut h.svm, SETTLE_DAY, 3_600);
    let mut ms = squads::create(&mut h, 2, 3);
    h.airdrop(&ms.vault, 7_000_000_000); // not 10 SOL: squads::create already sent that exact airdrop
    let (issuer, mint, cash_mint) = fixtures::create_standard_fund(&mut h);
    let m = mint.pubkey();
    let i = issuer.pubkey();
    let admin = h.funded_keypair();
    let ta = h.funded_keypair();
    let provider = Keypair::new();
    let roles = ix_governance::Roles {
        fund_admin: admin.pubkey(),
        transfer_agent: ta.pubkey(),
        compliance: ms.vault,
        keeper: Pubkey::new_unique(),
        auditor: Pubkey::new_unique(),
    };
    let providers = [
        provider.pubkey(),
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    h.send(
        &[
            ix_hook::init_registry(&i, &m),
            ix_hook::set_rules(&i, &m, providers, fixtures::jurisdictions(&[*b"US"]), 0),
            ix_governance::set_roles(&i, &m, &roles),
            ix_fund::launch(&i, &m),
        ],
        &[&issuer],
    )
    .unwrap();
    let ix = ix_fund::publish_daily(&admin.pubkey(), &h.payer.pubkey(), &m, 1, NAV_ONE, 0, 1);
    h.send(&[ix], &[&admin]).unwrap();
    let cash_program = fixtures::token_program_of(&h, &cash_mint);
    let treasury = fixtures::create_cash_ata(&mut h, &ta.pubkey(), &cash_mint);
    h.send(
        &[
            ix_fund::set_cash_treasury(&i, &m, &treasury, &cash_mint),
            ix_dvp::init_dvp(&i, &h.payer.pubkey(), &m, &cash_mint, &cash_program, None),
        ],
        &[&issuer],
    )
    .unwrap();

    // The vault registers and approves the escrow, then approves an investor.
    let fs = pda::fund_state(&m).0;
    let escrow = pda::escrow_authority(&fs).0;
    let v = ms.vault;
    squads::squads_execute(
        &mut h,
        &mut ms,
        &[
            ix_hook::register_holder(&v, &v, &m, &escrow, 2),
            ix_hook::approve_fund_account(&v, &m, &escrow),
        ],
        2,
        &[],
    )
    .expect("escrow via vault");
    let p = h.payer.pubkey();
    h.send(
        &[ix_hook::thaw_holding_account(
            &p,
            &m,
            &escrow,
            &pda::unit_escrow(&fs, &m),
        )],
        &[],
    )
    .unwrap();
    let inv = h.funded_keypair();
    let o = inv.pubkey();
    h.send(&[ix_hook::register_holder(&o, &o, &m, &o, 0)], &[&inv])
        .unwrap();
    let stmt = mmf_sdk::kyc::sign(
        &provider,
        &mmf_sdk::kyc::statement(m, o, *b"US", 1, clock::now(&h.svm) + fixtures::YEAR),
    );
    squads::squads_execute(
        &mut h,
        &mut ms,
        &[ix_hook::approve_investor(&v, &m, &o, stmt.attestation_hash)],
        2,
        std::slice::from_ref(&stmt.ix),
    )
    .expect("investor via vault");
    h.send(
        &[
            ix_hook::create_holding_account(&p, &m, &o),
            ix_hook::thaw_holding_account(&p, &m, &o, &pda::holding_account(&o, &m)),
        ],
        &[],
    )
    .unwrap();
    fixtures::mint_cash(&mut h, &cash_mint, &o, ONE);

    // A funded subscription.
    let args = CreateArgs {
        kind: settle::SUBSCRIPTION,
        seller: fs,
        buyer: o,
        units: ONE,
        cash_amount: 0,
        not_before_ts: 0,
        expires_at_ts: 0,
        external_ref: settle::ext(1),
    };
    h.send(&[ix_dvp::create_instruction(&o, &p, &m, args)], &[&inv])
        .unwrap();
    let d = Deal {
        mint: m,
        cash_mint,
        cash_program,
        kind: 0,
        seller: fs,
        buyer: o,
        created_by: o,
        external_ref: settle::ext(1),
    };
    let src = fixtures::cash_ata(&h, &o, &cash_mint);
    h.send(&[ix_dvp::fund_cash(&o, &d, &src)], &[&inv]).unwrap();

    // One approval: nothing happens. Two: rejected, recorded with the vault as actor.
    let reject = ix_governance::reject_settlement(&v, &m, &d.settlement(), SANCTIONS, CASE);
    let (index, remaining) = ms.propose(&mut h, &[reject]);
    ms.approve(&mut h, index, 0).unwrap();
    assert_squads_error(
        &ms.execute(&mut h, index, &remaining, &[]),
        INVALID_PROPOSAL_STATUS,
    );
    let si: dvp_settlement::SettlementInstruction = fixtures::read(&h, &d.settlement());
    assert_eq!(si.status, 2, "still BuyerFunded");
    ms.approve(&mut h, index, 1).unwrap();
    let meta = ms
        .execute(&mut h, index, &remaining, &[])
        .expect("executes at threshold");
    let si: dvp_settlement::SettlementInstruction = fixtures::read(&h, &d.settlement());
    assert_eq!((si.status, si.cash_to_return), (7, ONE));
    assert_eq!(cpi_events::<SettlementRejected>(&meta)[0].actor, v);
}
