//! Interest across mint and burn (F002 FR-013; research R8, R16). T033 covers `accrue` and the
//! mint; T045 (US2) adds the burn.
//!
//! Expected interest is written out by hand (`units × rate × 10^6 × days`), not computed with
//! `mmf_common::accrual`, so the test doesn't share code with what it tests.

use anchor_lang::prelude::{AccountInfo, Pubkey};
use anchor_lang::AccountSerialize;
use anchor_spl::token_2022::spl_token_2022 as t22;
use compliance_hook::events::Accrued;
use compliance_hook::instructions::accrue::check_holder_accounts;
use compliance_hook::{HolderState, HookError};
use fund::FundState;
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use solana_program_pack::Pack;
use solana_signer::Signer;
use tests::assert::assert_anchor_error;
use tests::events::log_events;
use tests::fixtures::{self, read, NAV_ONE, SETTLE_DAY};
use tests::settle::{self, ONE};
use tests::{clock, Harness};

/// 4.5 % a year, per day, × 10^12.
const RATE: u64 = 123_287_671;

#[test]
fn only_the_funds_own_state_may_call_accrue() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    let stranger = h.funded_keypair();
    let ix = mmf_sdk::ix_hook::accrue(&stranger.pubkey(), &s.m(), &s.escrow, 5);
    let res = h.send(&[ix], &[&stranger]);
    assert_anchor_error(&res, HookError::UnauthorizedCaller);
}

#[test]
fn escrow_interest_is_settled_before_the_mint_and_new_units_earn_nothing_in_escrow() {
    let mut h = Harness::new();
    let s = fixtures::settlement_ready_fund(&mut h);
    // 4 units sit in the escrow outside any instruction (an approved holder sent them).
    let x = fixtures::investor_with_units(&mut h, &s, 10 * ONE, 0);
    fixtures::transfer(&mut h, &s, &x, &s.escrow, 4 * ONE).expect("send to escrow");
    let y = fixtures::investor_with_cash(&mut h, &s, 3 * ONE);

    // Next day: one day at RATE.
    clock::set_utc_day(&mut h.svm, SETTLE_DAY + 1, 60);
    fixtures::publish(&mut h, &s, NAV_ONE, RATE as i64, 1).unwrap();
    let d = settle::affirmed_subscription(&mut h, &s, &y, 3 * ONE, 1);
    let meta = settle::settle_sub(&mut h, &s, &d, &s.keeper).expect("settle");

    let fs: FundState = read(&h, &s.fund_state());
    let escrow: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &s.escrow).0);
    let earned = u128::from(4 * ONE) * u128::from(RATE) * 1_000_000;
    assert_eq!(
        escrow.accrued_e18, earned,
        "4 units × one day, settled before the mint"
    );
    assert_eq!(escrow.index_at_last_touch, fs.accrual_index);
    assert_eq!(
        escrow.last_balance,
        4 * ONE,
        "the 3 minted units left in the same step"
    );

    let ev = log_events::<Accrued>(&meta);
    assert_eq!(ev.len(), 1);
    assert_eq!(
        (
            ev[0].fund,
            ev[0].owner,
            ev[0].pre_balance,
            ev[0].post_balance
        ),
        (s.fund_state(), s.escrow, 4 * ONE, 7 * ONE)
    );
    assert_eq!(
        (ev[0].accrual_index, ev[0].accrued_e18),
        (fs.accrual_index, earned)
    );

    // The subscriber held nothing at the publish, so it has earned nothing.
    let yh: HolderState = read(&h, &mmf_sdk::pda::holder(&s.m(), &y.pubkey()).0);
    assert_eq!(yh.accrued_e18, 0);
    assert_eq!(yh.index_at_last_touch, fs.accrual_index);
}

// ---- the account check, called directly -----------------------------------------------------
// `fund` always passes the escrow's own record and holding account, so `WrongHolder` and
// `ClassNotAccrued` can't be reached by a transaction; the check is called with forged accounts.

struct Acct {
    key: Pubkey,
    owner: Pubkey,
    data: Vec<u8>,
    lamports: u64,
}

impl Acct {
    fn info(&mut self) -> AccountInfo<'_> {
        AccountInfo::new(
            &self.key,
            false,
            false,
            &mut self.lamports,
            &mut self.data,
            &self.owner,
            false,
        )
    }
}

fn record(mint: &Pubkey, owner: &Pubkey, class: u8) -> Acct {
    let (key, bump) = mmf_sdk::pda::holder(mint, owner);
    let r = HolderState {
        version: 1,
        mint: *mint,
        owner: *owner,
        status: 1,
        class,
        jurisdiction: [0; 2],
        investor_type: 0,
        kyc_expiry_ts: i64::MAX,
        attestation_hash: [0; 32],
        attested_by: Pubkey::default(),
        pledged_amount: 0,
        active_liens: 0,
        index_at_last_touch: 0,
        accrued_e18: 0,
        last_balance: 0,
        last_slot: 0,
        last_paid_cycle: 0,
        bump,
        _reserved: [0; 64],
    };
    let mut data = Vec::new();
    r.try_serialize(&mut data).unwrap();
    Acct {
        key,
        owner: COMPLIANCE_HOOK_PROGRAM_ID,
        data,
        lamports: 1,
    }
}

fn holding(mint: &Pubkey, owner: &Pubkey) -> Acct {
    let mut data = vec![0u8; t22::state::Account::LEN];
    t22::state::Account {
        mint: *mint,
        owner: *owner,
        amount: 7,
        state: t22::state::AccountState::Initialized,
        ..Default::default()
    }
    .pack_into_slice(&mut data);
    Acct {
        key: mmf_sdk::pda::holding_account(owner, mint),
        owner: TOKEN_2022_PROGRAM_ID,
        data,
        lamports: 1,
    }
}

fn code(res: anchor_lang::Result<()>) -> u32 {
    match res {
        Ok(()) => 0,
        Err(anchor_lang::error::Error::AnchorError(e)) => e.error_code_number,
        Err(other) => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn accrue_accepts_only_the_escrows_own_record_and_holding_account() {
    let mint = Pubkey::new_unique();
    let escrow = mmf_sdk::pda::escrow_authority(&mmf_sdk::pda::fund_state(&mint).0).0;
    let investor = Pubkey::new_unique();
    let wrong: u32 = HookError::WrongHolder.into();
    let class: u32 = HookError::ClassNotAccrued.into();

    let (mut r, mut a) = (record(&mint, &escrow, 2), holding(&mint, &escrow));
    assert_eq!(
        code(check_holder_accounts(&mint, &r.info(), &a.info())),
        0,
        "genuine"
    );

    // An investor's genuine record and holding account: not accrued here in F003.
    let (mut r, mut a) = (record(&mint, &investor, 0), holding(&mint, &investor));
    assert_eq!(
        code(check_holder_accounts(&mint, &r.info(), &a.info())),
        class
    );

    // The escrow's record with another owner's holding account.
    let (mut r, mut a) = (record(&mint, &escrow, 2), holding(&mint, &investor));
    assert_eq!(
        code(check_holder_accounts(&mint, &r.info(), &a.info())),
        wrong
    );
    // A holding account at an address other than the owner's ATA.
    let (mut r, mut a) = (record(&mint, &escrow, 2), holding(&mint, &escrow));
    a.key = Pubkey::new_unique();
    assert_eq!(
        code(check_holder_accounts(&mint, &r.info(), &a.info())),
        wrong
    );
    // A record copied to another address.
    let (mut r, mut a) = (record(&mint, &escrow, 2), holding(&mint, &escrow));
    r.key = Pubkey::new_unique();
    assert_eq!(
        code(check_holder_accounts(&mint, &r.info(), &a.info())),
        wrong
    );
    // Another mint's holding account.
    let (mut r, mut a) = (
        record(&mint, &escrow, 2),
        holding(&Pubkey::new_unique(), &escrow),
    );
    assert_eq!(
        code(check_holder_accounts(&mint, &r.info(), &a.info())),
        wrong
    );
    // Holding account not owned by Token-2022.
    let (mut r, mut a) = (record(&mint, &escrow, 2), holding(&mint, &escrow));
    a.owner = Pubkey::new_unique();
    assert_eq!(
        code(check_holder_accounts(&mint, &r.info(), &a.info())),
        wrong
    );
}
