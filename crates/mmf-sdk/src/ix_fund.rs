//! Instruction builders for the fund program (contracts/fund.md).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{system_program, InstructionData, ToAccountMetas};
use fund::FundTerms;
use mmf_common::ids::{FUND_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};

use crate::pda;

/// `create_fund`: signed by the issuer and the mint keypair. Normally sent in the same
/// transaction right after `ix_governance::init_roles`.
pub fn create_fund(
    issuer: &Pubkey,
    mint: &Pubkey,
    cash_mint: &Pubkey,
    terms: FundTerms,
    name: &str,
    symbol: &str,
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = fund::accounts::CreateFund {
        issuer: *issuer,
        mint: *mint,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        cash_mint: *cash_mint,
        token_2022_program: TOKEN_2022_PROGRAM_ID,
        system_program: system_program::ID,
        event_authority: pda::event_authority(&FUND_PROGRAM_ID).0,
        program: FUND_PROGRAM_ID,
    };
    Instruction {
        program_id: FUND_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: fund::instruction::CreateFund {
            terms,
            name: name.to_string(),
            symbol: symbol.to_string(),
        }
        .data(),
    }
}

/// `launch`: signed by the issuer; Setup → Open.
pub fn launch(issuer: &Pubkey, mint: &Pubkey) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = fund::accounts::Launch {
        issuer: *issuer,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        registry: pda::registry(mint).0,
        extra_account_meta_list: pda::extra_metas(mint).0,
        event_authority: pda::event_authority(&FUND_PROGRAM_ID).0,
        program: FUND_PROGRAM_ID,
    };
    Instruction {
        program_id: FUND_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: fund::instruction::Launch {}.data(),
    }
}
