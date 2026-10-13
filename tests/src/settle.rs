//! Settlement test helpers (F003): instruction arguments, the `Deal` the SDK builders need, and
//! one-line senders for each step. They build instructions with `mmf_sdk::ix_dvp` and send them
//! directly, so the programs' own checks are what is tested (Constitution II).

use anchor_lang::prelude::Pubkey;
use dvp_settlement::{CreateArgs, SettlementInstruction};
use mmf_sdk::ix_dvp::{self, Deal};
use solana_keypair::Keypair;
use solana_signer::Signer;

use crate::fixtures::{self, read, SettleCtx};
use crate::{Harness, TxResult};

pub const SUBSCRIPTION: u8 = 0;
pub const REDEMPTION: u8 = 1;
pub const SECONDARY: u8 = 2;

/// One whole unit or one dollar (6 dp).
pub const ONE: u64 = 1_000_000;

/// An external reference made of one repeated byte (non-zero for `n > 0`).
pub fn ext(n: u8) -> [u8; 32] {
    [n; 32]
}

pub fn sub_args(s: &SettleCtx, buyer: &Pubkey, units: u64, n: u8) -> CreateArgs {
    CreateArgs {
        kind: SUBSCRIPTION,
        seller: s.fund_state(),
        buyer: *buyer,
        units,
        cash_amount: 0,
        not_before_ts: 0,
        expires_at_ts: 0,
        external_ref: ext(n),
    }
}

pub fn red_args(s: &SettleCtx, seller: &Pubkey, units: u64, n: u8) -> CreateArgs {
    CreateArgs {
        kind: REDEMPTION,
        seller: *seller,
        buyer: s.fund_state(),
        ..sub_args(s, seller, units, n)
    }
}

pub fn sec_args(seller: &Pubkey, buyer: &Pubkey, units: u64, cash: u64, n: u8) -> CreateArgs {
    CreateArgs {
        kind: SECONDARY,
        seller: *seller,
        buyer: *buyer,
        units,
        cash_amount: cash,
        not_before_ts: 0,
        expires_at_ts: 0,
        external_ref: ext(n),
    }
}

/// The SDK's view of an instruction created by `creator` with `args`.
pub fn deal(s: &SettleCtx, creator: &Pubkey, args: &CreateArgs) -> Deal {
    Deal {
        mint: s.m(),
        cash_mint: s.cash_mint,
        cash_program: s.cash_program,
        kind: args.kind,
        seller: args.seller,
        buyer: args.buyer,
        created_by: *creator,
        external_ref: args.external_ref,
    }
}

pub fn create(h: &mut Harness, s: &SettleCtx, creator: &Keypair, args: CreateArgs) -> TxResult {
    let ix = ix_dvp::create_instruction(&creator.pubkey(), &h.payer.pubkey(), &s.m(), args);
    h.send(&[ix], &[creator])
}

/// Creates and returns the deal; panics if creation fails.
pub fn created(h: &mut Harness, s: &SettleCtx, creator: &Keypair, args: CreateArgs) -> Deal {
    create(h, s, creator, args).expect("create_instruction");
    deal(s, &creator.pubkey(), &args)
}

pub fn read_si(h: &Harness, d: &Deal) -> SettlementInstruction {
    read(h, &d.settlement())
}

/// The transfer agent affirms the Fund side of a subscription.
pub fn affirm(h: &mut Harness, s: &SettleCtx, d: &Deal) -> TxResult {
    let ix = ix_dvp::affirm_fund_side(&s.transfer_agent.pubkey(), d);
    h.send(&[ix], &[&s.transfer_agent])
}

/// `party` funds the cash leg from its own cash ATA.
pub fn fund_cash(h: &mut Harness, s: &SettleCtx, d: &Deal, party: &Keypair) -> TxResult {
    let source = fixtures::cash_ata(h, &party.pubkey(), &s.cash_mint);
    let ix = ix_dvp::fund_cash(&party.pubkey(), d, &source);
    h.send(&[ix], &[party])
}

/// `by` settles a subscription (cash paid to the fund's treasury cash account).
pub fn settle_sub(h: &mut Harness, s: &SettleCtx, d: &Deal, by: &Keypair) -> TxResult {
    let ix = ix_dvp::settle_subscription(&by.pubkey(), d, &s.cash_treasury);
    h.send(&[ix], &[by])
}

/// An Affirmed subscription: `investor` creates it, the transfer agent affirms, the investor funds.
pub fn affirmed_subscription(
    h: &mut Harness,
    s: &SettleCtx,
    investor: &Keypair,
    units: u64,
    n: u8,
) -> Deal {
    let d = created(h, s, investor, sub_args(s, &investor.pubkey(), units, n));
    affirm(h, s, &d).expect("affirm");
    fund_cash(h, s, &d, investor).expect("fund cash");
    d
}

/// Cash in `owner`'s cash ATA.
pub fn cash_of(h: &Harness, s: &SettleCtx, owner: &Pubkey) -> u64 {
    fixtures::cash_balance(h, &fixtures::cash_ata(h, owner, &s.cash_mint))
}

/// The seller funds the unit leg (redemption or secondary transfer).
pub fn fund_units(h: &mut Harness, d: &Deal, seller: &Keypair) -> TxResult {
    let ix = ix_dvp::fund_units(&seller.pubkey(), d);
    h.send(&[ix], &[seller])
}

/// The transfer agent funds a redemption's cash from the treasury cash account.
pub fn fund_redemption_cash(h: &mut Harness, s: &SettleCtx, d: &Deal) -> TxResult {
    let ix = ix_dvp::fund_redemption_cash(&s.transfer_agent.pubkey(), d, &s.cash_treasury);
    h.send(&[ix], &[&s.transfer_agent])
}

/// `by` settles a redemption.
pub fn settle_red(h: &mut Harness, d: &Deal, by: &Keypair) -> TxResult {
    let ix = ix_dvp::settle_redemption(&by.pubkey(), d);
    h.send(&[ix], &[by])
}

/// Puts `amount` of cash in the fund's treasury cash account.
pub fn fill_treasury(h: &mut Harness, s: &SettleCtx, amount: u64) {
    fixtures::mint_cash(h, &s.cash_mint, &s.transfer_agent.pubkey(), amount);
}

/// An Affirmed redemption: the investor creates it and funds its units, the transfer agent funds
/// the cash from the treasury (which must hold enough).
pub fn affirmed_redemption(
    h: &mut Harness,
    s: &SettleCtx,
    investor: &Keypair,
    units: u64,
    n: u8,
) -> Deal {
    let d = created(h, s, investor, red_args(s, &investor.pubkey(), units, n));
    fund_units(h, &d, investor).expect("fund units");
    fund_redemption_cash(h, s, &d).expect("fund cash");
    d
}
