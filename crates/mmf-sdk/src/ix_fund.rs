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

/// `publish_daily` (F002): signed by the fund admin; `payer` funds the new NavRecord. `next_seq`
/// is `FundState.nav_seq + 1` (wrapping, matching the program's address derivation).
pub fn publish_daily(
    fund_admin: &Pubkey,
    payer: &Pubkey,
    mint: &Pubkey,
    next_seq: u64,
    nav: u64,
    daily_rate_e12: i64,
    days_covered: u16,
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = fund::accounts::PublishDaily {
        fund_admin: *fund_admin,
        payer: *payer,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        nav_record: pda::nav_record(&fund_state, next_seq).0,
        system_program: system_program::ID,
        event_authority: pda::event_authority(&FUND_PROGRAM_ID).0,
        program: FUND_PROGRAM_ID,
    };
    Instruction {
        program_id: FUND_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: fund::instruction::PublishDaily {
            nav,
            daily_rate_e12,
            days_covered,
        }
        .data(),
    }
}

fn override_accounts(fund_caller: &Pubkey, mint: &Pubkey) -> fund::accounts::SetNavOverride {
    fund::accounts::SetNavOverride {
        fund_caller: *fund_caller,
        fund_state: pda::fund_state(mint).0,
        event_authority: pda::event_authority(&FUND_PROGRAM_ID).0,
        program: FUND_PROGRAM_ID,
    }
}

/// Raw `set_nav_override` with any signer as `fund_caller`. Real calls come only from governance
/// (`ix_governance::approve_nav_override`); this builder exists for direct-call rejection tests.
pub fn set_nav_override_raw(
    fund_caller: &Pubkey,
    mint: &Pubkey,
    nav: u64,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: FUND_PROGRAM_ID,
        accounts: override_accounts(fund_caller, mint).to_account_metas(None),
        data: fund::instruction::SetNavOverride {
            nav,
            reason,
            case_ref_hash,
            actor: *actor,
        }
        .data(),
    }
}

/// Raw `clear_nav_override` with any signer as `fund_caller` (direct-call rejection tests).
pub fn clear_nav_override_raw(
    fund_caller: &Pubkey,
    mint: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: FUND_PROGRAM_ID,
        accounts: override_accounts(fund_caller, mint).to_account_metas(None),
        data: fund::instruction::ClearNavOverride {
            reason,
            case_ref_hash,
            actor: *actor,
        }
        .data(),
    }
}
