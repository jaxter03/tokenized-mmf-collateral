//! Instruction builders for `dvp_settlement` (F003). Builders do no validation: tests send
//! every instruction directly so the programs' own checks are what is tested (Constitution II).
//! The story phases add one builder per instruction (tasks T016, T027, T041, T052, T057).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{system_program, InstructionData, ToAccountMetas};
use mmf_common::ids::{
    ASSOCIATED_TOKEN_PROGRAM_ID, COMPLIANCE_HOOK_PROGRAM_ID, DVP_SETTLEMENT_PROGRAM_ID,
    FUND_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};

use crate::pda;

/// `init_dvp`: signed by the issuer and the payer. `cash_token_program` is the cash mint's
/// owner (SPL Token or Token-2022). `None` keeps the 24 h default expiry.
pub fn init_dvp(
    issuer: &Pubkey,
    payer: &Pubkey,
    mint: &Pubkey,
    cash_mint: &Pubkey,
    cash_token_program: &Pubkey,
    default_expiry_secs: Option<u32>,
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = dvp_settlement::accounts::InitDvp {
        issuer: *issuer,
        payer: *payer,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        mint: *mint,
        cash_mint: *cash_mint,
        dvp_config: pda::dvp_config(&fund_state).0,
        escrow_authority: pda::escrow_authority(&fund_state).0,
        unit_escrow: pda::unit_escrow(&fund_state, mint),
        cash_escrow: pda::cash_escrow(&fund_state, cash_mint, cash_token_program),
        token_2022_program: TOKEN_2022_PROGRAM_ID,
        cash_token_program: *cash_token_program,
        associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
        system_program: system_program::ID,
        event_authority: pda::event_authority(&DVP_SETTLEMENT_PROGRAM_ID).0,
        program: DVP_SETTLEMENT_PROGRAM_ID,
    };
    Instruction {
        program_id: DVP_SETTLEMENT_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: dvp_settlement::instruction::InitDvp {
            default_expiry_secs,
        }
        .data(),
    }
}

/// What the builders need to know about a fund and one settlement instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deal {
    pub mint: Pubkey,
    pub cash_mint: Pubkey,
    /// The cash mint's token program (SPL Token or Token-2022).
    pub cash_program: Pubkey,
    /// `SettlementKind`: 0 Subscription, 1 Redemption, 2 SecondaryTransfer.
    pub kind: u8,
    /// Subscription: the FundState; otherwise the unit owner.
    pub seller: Pubkey,
    /// Redemption: the FundState; otherwise the unit receiver.
    pub buyer: Pubkey,
    pub created_by: Pubkey,
    pub external_ref: [u8; 32],
}

impl Deal {
    pub fn fund_state(&self) -> Pubkey {
        pda::fund_state(&self.mint).0
    }

    /// The SettlementInstruction's address (research R2).
    pub fn settlement(&self) -> Pubkey {
        pda::settlement(&self.fund_state(), &self.created_by, &self.external_ref).0
    }

    fn escrow(&self) -> Pubkey {
        pda::escrow_authority(&self.fund_state()).0
    }

    fn unit_escrow(&self) -> Pubkey {
        pda::unit_escrow(&self.fund_state(), &self.mint)
    }

    fn cash_escrow(&self) -> Pubkey {
        pda::cash_escrow(&self.fund_state(), &self.cash_mint, &self.cash_program)
    }

    /// The investor's HolderState for `party`, or `None` for the Fund side.
    fn holder_of(&self, party: &Pubkey) -> Option<Pubkey> {
        (*party != self.fund_state()).then(|| pda::holder(&self.mint, party).0)
    }
}

/// `create_instruction`: signed by `creator` and `payer`. The address is fixed by
/// `(creator, args.external_ref)`, so it is known before submission (research R2).
pub fn create_instruction(
    creator: &Pubkey,
    payer: &Pubkey,
    mint: &Pubkey,
    args: dvp_settlement::CreateArgs,
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let escrow = pda::escrow_authority(&fund_state).0;
    let holder = |party: &Pubkey| (*party != fund_state).then(|| pda::holder(mint, party).0);
    let accounts = dvp_settlement::accounts::CreateInstruction {
        creator: *creator,
        payer: *payer,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        dvp_config: pda::dvp_config(&fund_state).0,
        escrow_holder: pda::holder(mint, &escrow).0,
        unit_escrow: pda::unit_escrow(&fund_state, mint),
        seller_holder: holder(&args.seller),
        buyer_holder: holder(&args.buyer),
        settlement: pda::settlement(&fund_state, creator, &args.external_ref).0,
        system_program: system_program::ID,
        event_authority: pda::event_authority(&DVP_SETTLEMENT_PROGRAM_ID).0,
        program: DVP_SETTLEMENT_PROGRAM_ID,
    };
    Instruction {
        program_id: DVP_SETTLEMENT_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: dvp_settlement::instruction::CreateInstruction { args }.data(),
    }
}

fn affirm_and_fund_ix(accounts: dvp_settlement::accounts::AffirmAndFund, side: u8) -> Instruction {
    Instruction {
        program_id: DVP_SETTLEMENT_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: dvp_settlement::instruction::AffirmAndFund { side }.data(),
    }
}

fn affirm_base(party: &Pubkey, d: &Deal) -> dvp_settlement::accounts::AffirmAndFund {
    let fund_state = d.fund_state();
    dvp_settlement::accounts::AffirmAndFund {
        party: *party,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        dvp_config: pda::dvp_config(&fund_state).0,
        settlement: d.settlement(),
        party_holder: None,
        source: None,
        destination: None,
        leg_mint: None,
        token_program: None,
        registry: None,
        escrow_holder: None,
        delegate: None,
        hook_program: None,
        extra_account_meta_list: None,
        event_authority: pda::event_authority(&DVP_SETTLEMENT_PROGRAM_ID).0,
        program: DVP_SETTLEMENT_PROGRAM_ID,
    }
}

/// `affirm_and_fund(Seller)` for a subscription: the transfer agent affirms for the Fund. Nothing
/// moves; the units are created at settlement.
pub fn affirm_fund_side(transfer_agent: &Pubkey, d: &Deal) -> Instruction {
    affirm_and_fund_ix(affirm_base(transfer_agent, d), 0)
}

/// `affirm_and_fund(Buyer)`: `party` moves the cash amount from `source` into the cash escrow.
/// For a redemption, `party` is the transfer agent and `source` the treasury cash account.
pub fn fund_cash(party: &Pubkey, d: &Deal, source: &Pubkey) -> Instruction {
    let mut a = affirm_base(party, d);
    a.party_holder = d.holder_of(&d.buyer);
    a.source = Some(*source);
    a.destination = Some(d.cash_escrow());
    a.leg_mint = Some(d.cash_mint);
    a.token_program = Some(d.cash_program);
    affirm_and_fund_ix(a, 1)
}

fn settle_base(triggered_by: &Pubkey, d: &Deal) -> dvp_settlement::accounts::Settle {
    let fund_state = d.fund_state();
    let escrow = d.escrow();
    dvp_settlement::accounts::Settle {
        triggered_by: *triggered_by,
        fund_state,
        registry: pda::registry(&d.mint).0,
        dvp_config: pda::dvp_config(&fund_state).0,
        settlement: d.settlement(),
        escrow_authority: escrow,
        escrow_holder: pda::holder(&d.mint, &escrow).0,
        unit_escrow: d.unit_escrow(),
        cash_escrow: d.cash_escrow(),
        mint: d.mint,
        cash_mint: d.cash_mint,
        seller_holder: d.holder_of(&d.seller),
        buyer_holder: d.holder_of(&d.buyer),
        buyer_units: None,
        seller_cash: None,
        cash_treasury: None,
        extra_account_meta_list: pda::extra_metas(&d.mint).0,
        delegate: pda::delegate(&d.mint).0,
        token_2022_program: TOKEN_2022_PROGRAM_ID,
        cash_token_program: d.cash_program,
        hook_program: COMPLIANCE_HOOK_PROGRAM_ID,
        fund_program: FUND_PROGRAM_ID,
        fund_event_authority: pda::event_authority(&FUND_PROGRAM_ID).0,
        event_authority: pda::event_authority(&DVP_SETTLEMENT_PROGRAM_ID).0,
        program: DVP_SETTLEMENT_PROGRAM_ID,
    }
}

fn settle_ix(accounts: dvp_settlement::accounts::Settle) -> Instruction {
    Instruction {
        program_id: DVP_SETTLEMENT_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: dvp_settlement::instruction::Settle {}.data(),
    }
}

/// `settle` for a subscription: units minted into escrow and delivered to the buyer's holding
/// account, cash paid to `cash_treasury`.
pub fn settle_subscription(triggered_by: &Pubkey, d: &Deal, cash_treasury: &Pubkey) -> Instruction {
    let mut a = settle_base(triggered_by, d);
    a.buyer_units = Some(pda::holding_account(&d.buyer, &d.mint));
    a.cash_treasury = Some(*cash_treasury);
    settle_ix(a)
}
