//! Instruction builders for `collateral` (F004, contracts/collateral.md). Builders do no
//! validation: tests send every instruction directly so the programs' own checks are what is
//! tested (Constitution II). The story phases add one builder per instruction (tasks T021, T028,
//! T037, T049, T056, T061).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{InstructionData, ToAccountMetas};
use mmf_common::ids::COLLATERAL_PROGRAM_ID;

use crate::pda;

/// What the builders need to know about one agreement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Agr {
    pub mint: Pubkey,
    pub giver: Pubkey,
    pub taker: Pubkey,
    pub nonce: u64,
}

impl Agr {
    pub fn fund_state(&self) -> Pubkey {
        pda::fund_state(&self.mint).0
    }

    /// The PledgeAgreement's address.
    pub fn agreement(&self) -> Pubkey {
        pda::agreement(&self.fund_state(), &self.giver, &self.taker, self.nonce).0
    }

    pub fn giver_holder(&self) -> Pubkey {
        pda::holder(&self.mint, &self.giver).0
    }

    pub fn taker_holder(&self) -> Pubkey {
        pda::holder(&self.mint, &self.taker).0
    }

    /// A pledge's address (research R6).
    pub fn pledge(&self, pledge_ref: &[u8; 32]) -> Pubkey {
        pda::pledge(&self.agreement(), pledge_ref).0
    }

    /// A call's address (research R6).
    pub fn call(&self, external_ref: &[u8; 32]) -> Pubkey {
        pda::call(&self.agreement(), external_ref).0
    }
}

fn event_authority() -> Pubkey {
    pda::event_authority(&COLLATERAL_PROGRAM_ID).0
}

fn ix(accounts: impl ToAccountMetas, data: impl InstructionData) -> Instruction {
    Instruction {
        program_id: COLLATERAL_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: data.data(),
    }
}

/// `propose_agreement(args)`: signed by the proposer (the giver or the taker) and the payer.
pub fn propose_agreement(
    proposer: &Pubkey,
    payer: &Pubkey,
    mint: &Pubkey,
    args: collateral::ProposeArgs,
) -> Instruction {
    let a = Agr {
        mint: *mint,
        giver: args.giver,
        taker: args.taker,
        nonce: args.nonce,
    };
    ix(
        collateral::accounts::ProposeAgreement {
            proposer: *proposer,
            payer: *payer,
            fund_state: a.fund_state(),
            giver_holder: a.giver_holder(),
            taker_holder: a.taker_holder(),
            agreement: a.agreement(),
            system_program: anchor_lang::system_program::ID,
            event_authority: event_authority(),
            program: COLLATERAL_PROGRAM_ID,
        },
        collateral::instruction::ProposeAgreement { args },
    )
}

/// `sign_agreement()`: signed by the party that didn't propose.
pub fn sign_agreement(signer: &Pubkey, a: &Agr) -> Instruction {
    ix(
        collateral::accounts::SignAgreement {
            signer: *signer,
            fund_state: a.fund_state(),
            agreement: a.agreement(),
            giver_holder: a.giver_holder(),
            taker_holder: a.taker_holder(),
            event_authority: event_authority(),
            program: COLLATERAL_PROGRAM_ID,
        },
        collateral::instruction::SignAgreement {},
    )
}

/// `update_exposure(required_value, external_ref)`: signed by the taker (and the payer, which
/// pays for a call account if one opens).
pub fn update_exposure(
    signer: &Pubkey,
    payer: &Pubkey,
    a: &Agr,
    required_value: u64,
    external_ref: [u8; 32],
) -> Instruction {
    ix(
        collateral::accounts::UpdateExposure {
            signer: *signer,
            payer: *payer,
            fund_state: a.fund_state(),
            agreement: a.agreement(),
            signer_holder: pda::holder(&a.mint, signer).0,
            call: a.call(&external_ref),
            system_program: anchor_lang::system_program::ID,
            event_authority: event_authority(),
            program: COLLATERAL_PROGRAM_ID,
        },
        collateral::instruction::UpdateExposure {
            required_value,
            external_ref,
        },
    )
}

/// `confirm_exposure(required_value, external_ref)`: signed by the giver (and the payer).
pub fn confirm_exposure(
    signer: &Pubkey,
    payer: &Pubkey,
    a: &Agr,
    required_value: u64,
    external_ref: [u8; 32],
) -> Instruction {
    ix(
        collateral::accounts::ConfirmExposure {
            signer: *signer,
            payer: *payer,
            fund_state: a.fund_state(),
            agreement: a.agreement(),
            signer_holder: pda::holder(&a.mint, signer).0,
            call: a.call(&external_ref),
            system_program: anchor_lang::system_program::ID,
            event_authority: event_authority(),
            program: COLLATERAL_PROGRAM_ID,
        },
        collateral::instruction::ConfirmExposure {
            required_value,
            external_ref,
        },
    )
}

/// `issue_call(kind, external_ref)`: kind 0 Delivery (the taker), 1 Return (the giver).
pub fn issue_call(
    signer: &Pubkey,
    payer: &Pubkey,
    a: &Agr,
    kind: u8,
    external_ref: [u8; 32],
) -> Instruction {
    ix(
        collateral::accounts::IssueCall {
            signer: *signer,
            payer: *payer,
            fund_state: a.fund_state(),
            agreement: a.agreement(),
            signer_holder: pda::holder(&a.mint, signer).0,
            call: a.call(&external_ref),
            system_program: anchor_lang::system_program::ID,
            event_authority: event_authority(),
            program: COLLATERAL_PROGRAM_ID,
        },
        collateral::instruction::IssueCall { kind, external_ref },
    )
}

/// `check_coverage()`: signed by any key.
pub fn check_coverage(triggered_by: &Pubkey, a: &Agr) -> Instruction {
    ix(
        collateral::accounts::CheckCoverage {
            triggered_by: *triggered_by,
            fund_state: a.fund_state(),
            agreement: a.agreement(),
            event_authority: event_authority(),
            program: COLLATERAL_PROGRAM_ID,
        },
        collateral::instruction::CheckCoverage {},
    )
}
