//! Instruction builders for the compliance_hook program (contracts/compliance_hook.md).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{system_program, InstructionData, ToAccountMetas};
use mmf_common::ids::COMPLIANCE_HOOK_PROGRAM_ID;
use mmf_common::jurisdiction::JurisdictionSet;

use crate::pda;

/// `init_registry`: signed by the issuer while the fund is in Setup.
pub fn init_registry(issuer: &Pubkey, mint: &Pubkey) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = compliance_hook::accounts::InitRegistry {
        issuer: *issuer,
        mint: *mint,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        registry: pda::registry(mint).0,
        extra_account_meta_list: pda::extra_metas(mint).0,
        system_program: system_program::ID,
        event_authority: pda::event_authority(&COMPLIANCE_HOOK_PROGRAM_ID).0,
        program: COMPLIANCE_HOOK_PROGRAM_ID,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::InitRegistry {}.data(),
    }
}

/// `set_rules`: signed by the issuer while the fund is in Setup.
pub fn set_rules(
    issuer: &Pubkey,
    mint: &Pubkey,
    kyc_providers: [Pubkey; 4],
    allowed_jurisdictions: JurisdictionSet,
    max_holders: u32,
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = compliance_hook::accounts::SetRules {
        issuer: *issuer,
        registry: pda::registry(mint).0,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        event_authority: pda::event_authority(&COMPLIANCE_HOOK_PROGRAM_ID).0,
        program: COMPLIANCE_HOOK_PROGRAM_ID,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::SetRules {
            kyc_providers,
            allowed_jurisdictions,
            max_holders,
        }
        .data(),
    }
}
