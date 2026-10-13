//! FR-019, FR-020 (research R7): units are created and destroyed only by `fund`, only for an
//! Affirmed settlement of the right kind, only when called by this fund's settlement escrow.
//! T032 covers the mint; T044 (US2) adds the burn.
//!
//! `fund` re-checks the settlement account that `dvp_settlement` passes it. `dvp_settlement`
//! already checks the same account, and only its escrow PDA can sign, so no transaction a test
//! can send delivers a forged settlement to `fund`. Those checks are therefore tested by calling
//! `fund::instructions::mint_burn::check_settlement` directly with forged accounts. The checks
//! that a transaction can reach (a foreign signer, cash missing from escrow) are tested end to
//! end.

use anchor_lang::prelude::{AccountInfo, Pubkey};
use anchor_lang::AccountSerialize;
use dvp_settlement::SettlementInstruction;
use fund::instructions::mint_burn::check_settlement;
use fund::FundError;
use mmf_common::enums::SettlementKind;
use mmf_common::ids::{DVP_SETTLEMENT_PROGRAM_ID, FUND_PROGRAM_ID};
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::fixtures::{self, patch_token_amount, SettleCtx};
use tests::settle::{self, ONE};
use tests::{units, Harness};

fn ready() -> (Harness, SettleCtx) {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    (h, s)
}

// ---- end to end -----------------------------------------------------------------------------

#[test]
fn only_the_settlement_escrow_may_ask_for_units() {
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    // A stranger, the issuer and the transfer agent each try to mint directly.
    let stranger = h.funded_keypair();
    for who in [&stranger, &s.issuer, &s.transfer_agent] {
        let ix = mmf_sdk::ix_fund::mint_for_subscription(
            &who.pubkey(),
            &s.m(),
            &s.cash_escrow,
            &d.settlement(),
            ONE,
        );
        let res = h.send(&[ix], &[who]);
        assert_anchor_error(&res, FundError::UnauthorizedCaller);
    }
    assert_eq!(units::supply(&h, &s.m()), 0);
}

#[test]
fn no_units_without_the_cash_in_escrow() {
    // I5. Funding always puts the cash in escrow, so a short escrow can only be planted: the
    // escrow's cash balance is lowered with `set_account` (no instruction can do this).
    let (mut h, s) = ready();
    let a = fixtures::investor_with_cash(&mut h, &s, ONE);
    let d = settle::affirmed_subscription(&mut h, &s, &a, ONE, 1);
    patch_token_amount(&mut h, &s.cash_escrow, ONE - 1);
    assert_anchor_error(
        &settle::settle_sub(&mut h, &s, &d, &s.keeper),
        FundError::CashNotEscrowed,
    );
    assert_eq!(units::supply(&h, &s.m()), 0);
}

// ---- the settlement-account check, called directly --------------------------------------------

struct Forged {
    key: Pubkey,
    owner: Pubkey,
    data: Vec<u8>,
    lamports: u64,
}

impl Forged {
    fn check(&mut self, fund: &Pubkey, kind: SettlementKind, units: u64) -> u32 {
        let info = AccountInfo::new(
            &self.key,
            false,
            false,
            &mut self.lamports,
            &mut self.data,
            &self.owner,
            false,
        );
        match check_settlement(&info, fund, kind, units) {
            Ok(()) => 0,
            Err(anchor_lang::error::Error::AnchorError(e)) => e.error_code_number,
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }
}

/// An Affirmed subscription for `units`, serialized as dvp_settlement would store it, at its own
/// PDA. `edit` changes it before serializing.
fn forged(fund: Pubkey, units: u64, edit: impl FnOnce(&mut SettlementInstruction)) -> Forged {
    let created_by = Pubkey::new_unique();
    let external_ref = [9; 32];
    let (key, bump) = mmf_sdk::pda::settlement(&fund, &created_by, &external_ref);
    let mut si = SettlementInstruction {
        version: 1,
        fund_state: fund,
        seq: 1,
        kind: 0,
        seller: fund,
        buyer: Pubkey::new_unique(),
        units,
        cash_amount: units,
        nav_seq: 1,
        not_before_ts: 0,
        expires_at_ts: i64::MAX,
        status: 3,
        seller_funded_at: 1,
        buyer_funded_at: 1,
        external_ref,
        created_by,
        created_at: 1,
        closed_at: 0,
        cancel_requests: 0,
        units_to_return: 0,
        cash_to_return: 0,
        bump,
        _reserved: [0; 64],
    };
    edit(&mut si);
    let mut data = Vec::new();
    si.try_serialize(&mut data).unwrap();
    Forged {
        key,
        owner: DVP_SETTLEMENT_PROGRAM_ID,
        data,
        lamports: 1,
    }
}

#[test]
fn check_settlement_accepts_only_this_funds_affirmed_settlement_of_that_kind_and_size() {
    let fund = Pubkey::new_unique();
    let mismatch: u32 = FundError::SettlementMismatch.into();
    let sub = SettlementKind::Subscription;

    assert_eq!(
        forged(fund, ONE, |_| {}).check(&fund, sub, ONE),
        0,
        "genuine"
    );

    // Wrong owner (a copy owned by any other program).
    let mut f = forged(fund, ONE, |_| {});
    f.owner = FUND_PROGRAM_ID;
    assert_eq!(f.check(&fund, sub, ONE), mismatch, "wrong owner");
    // Right owner, wrong address (a copy of a genuine settlement elsewhere).
    let mut f = forged(fund, ONE, |_| {});
    f.key = Pubkey::new_unique();
    assert_eq!(f.check(&fund, sub, ONE), mismatch, "wrong address");
    // Another fund's settlement.
    let other = Pubkey::new_unique();
    assert_eq!(
        forged(other, ONE, |_| {}).check(&fund, sub, ONE),
        mismatch,
        "other fund"
    );
    // Not Affirmed (Instructed, BuyerFunded, Settled).
    for status in [0u8, 2, 4] {
        let mut f = forged(fund, ONE, |si| si.status = status);
        assert_eq!(f.check(&fund, sub, ONE), mismatch, "status {status}");
    }
    // Another kind.
    let mut f = forged(fund, ONE, |si| si.kind = 1);
    assert_eq!(f.check(&fund, sub, ONE), mismatch, "a redemption");
    // Another amount.
    assert_eq!(
        forged(fund, ONE, |_| {}).check(&fund, sub, ONE + 1),
        mismatch,
        "units"
    );
    // Garbage data.
    let mut f = forged(fund, ONE, |_| {});
    f.data.truncate(20);
    assert_eq!(f.check(&fund, sub, ONE), mismatch, "too short");
}
