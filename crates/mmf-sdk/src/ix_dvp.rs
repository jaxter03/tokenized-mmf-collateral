//! Instruction builders for `dvp_settlement` (F003). Builders do no validation: tests send
//! every instruction directly so the programs' own checks are what is tested (Constitution II).
//! The story phases add one builder per instruction (tasks T016, T027, T041, T052, T057).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{system_program, InstructionData, ToAccountMetas};
use mmf_common::ids::{
    ASSOCIATED_TOKEN_PROGRAM_ID, DVP_SETTLEMENT_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
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
