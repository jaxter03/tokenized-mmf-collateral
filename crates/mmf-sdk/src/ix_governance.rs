//! Instruction builders for the governance program (contracts/governance.md).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{system_program, InstructionData, ToAccountMetas};
use mmf_common::ids::GOVERNANCE_PROGRAM_ID;

use crate::pda;

/// The five operating roles set by `set_roles`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Roles {
    pub fund_admin: Pubkey,
    pub transfer_agent: Pubkey,
    pub compliance: Pubkey,
    pub keeper: Pubkey,
    pub auditor: Pubkey,
}

/// `init_roles`: signed by the issuer and the mint keypair; run before `fund::create_fund`.
pub fn init_roles(issuer: &Pubkey, mint: &Pubkey) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = governance::accounts::InitRoles {
        issuer: *issuer,
        mint: *mint,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        system_program: system_program::ID,
        event_authority: pda::event_authority(&GOVERNANCE_PROGRAM_ID).0,
        program: GOVERNANCE_PROGRAM_ID,
    };
    Instruction {
        program_id: GOVERNANCE_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: governance::instruction::InitRoles {}.data(),
    }
}

/// `set_roles`: signed by the issuer while the fund is in Setup.
pub fn set_roles(issuer: &Pubkey, mint: &Pubkey, roles: &Roles) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = governance::accounts::SetRoles {
        issuer: *issuer,
        role_registry: pda::role_registry(&fund_state).0,
        fund_state,
        event_authority: pda::event_authority(&GOVERNANCE_PROGRAM_ID).0,
        program: GOVERNANCE_PROGRAM_ID,
    };
    Instruction {
        program_id: GOVERNANCE_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: governance::instruction::SetRoles {
            fund_admin: roles.fund_admin,
            transfer_agent: roles.transfer_agent,
            compliance: roles.compliance,
            keeper: roles.keeper,
            auditor: roles.auditor,
        }
        .data(),
    }
}
