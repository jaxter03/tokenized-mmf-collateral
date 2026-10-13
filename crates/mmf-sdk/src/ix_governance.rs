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

use mmf_common::ids::COMPLIANCE_HOOK_PROGRAM_ID;

fn holder_action_accounts(
    compliance: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
) -> governance::accounts::HolderAction {
    let fund_state = pda::fund_state(mint).0;
    governance::accounts::HolderAction {
        compliance: *compliance,
        role_registry: pda::role_registry(&fund_state).0,
        fund_state,
        registry: pda::registry(mint).0,
        holder_state: pda::holder(mint, owner).0,
        hook_admin: pda::hook_admin().0,
        hook_event_authority: pda::event_authority(&COMPLIANCE_HOOK_PROGRAM_ID).0,
        compliance_hook_program: COMPLIANCE_HOOK_PROGRAM_ID,
    }
}

fn pause_accounts(compliance: &Pubkey, mint: &Pubkey) -> governance::accounts::PauseAction {
    let fund_state = pda::fund_state(mint).0;
    governance::accounts::PauseAction {
        compliance: *compliance,
        role_registry: pda::role_registry(&fund_state).0,
        fund_state,
        registry: pda::registry(mint).0,
        hook_admin: pda::hook_admin().0,
        hook_event_authority: pda::event_authority(&COMPLIANCE_HOOK_PROGRAM_ID).0,
        compliance_hook_program: COMPLIANCE_HOOK_PROGRAM_ID,
    }
}

fn ix(accounts: impl ToAccountMetas, data: Vec<u8>) -> Instruction {
    Instruction {
        program_id: GOVERNANCE_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data,
    }
}

/// `suspend_holder`: Approved → Suspended.
pub fn suspend_holder(
    compliance: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    ix(
        holder_action_accounts(compliance, mint, owner),
        governance::instruction::SuspendHolder {
            reason,
            case_ref_hash,
        }
        .data(),
    )
}

/// `reinstate_holder`: Suspended → Approved.
pub fn reinstate_holder(
    compliance: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    ix(
        holder_action_accounts(compliance, mint, owner),
        governance::instruction::ReinstateHolder {
            reason,
            case_ref_hash,
        }
        .data(),
    )
}

/// `revoke_holder`: Pending / Approved / Suspended → Revoked (final).
pub fn revoke_holder(
    compliance: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    ix(
        holder_action_accounts(compliance, mint, owner),
        governance::instruction::RevokeHolder {
            reason,
            case_ref_hash,
        }
        .data(),
    )
}

/// `pause_transfers`.
pub fn pause_transfers(
    compliance: &Pubkey,
    mint: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    ix(
        pause_accounts(compliance, mint),
        governance::instruction::PauseTransfers {
            reason,
            case_ref_hash,
        }
        .data(),
    )
}

/// `unpause_transfers`.
pub fn unpause_transfers(
    compliance: &Pubkey,
    mint: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    ix(
        pause_accounts(compliance, mint),
        governance::instruction::UnpauseTransfers {
            reason,
            case_ref_hash,
        }
        .data(),
    )
}

use mmf_common::ids::FUND_PROGRAM_ID;

fn nav_override_accounts(
    compliance: &Pubkey,
    mint: &Pubkey,
) -> governance::accounts::NavOverrideAction {
    let fund_state = pda::fund_state(mint).0;
    governance::accounts::NavOverrideAction {
        compliance: *compliance,
        role_registry: pda::role_registry(&fund_state).0,
        fund_state,
        fund_caller: pda::fund_caller().0,
        fund_event_authority: pda::event_authority(&FUND_PROGRAM_ID).0,
        fund_program: FUND_PROGRAM_ID,
    }
}

/// `approve_nav_override` (F002): compliance allows `nav` at the next publish, beyond the daily
/// move limit. Reasons: OperationalError (3) or Other (6).
pub fn approve_nav_override(
    compliance: &Pubkey,
    mint: &Pubkey,
    nav: u64,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    ix(
        nav_override_accounts(compliance, mint),
        governance::instruction::ApproveNavOverride {
            nav,
            reason,
            case_ref_hash,
        }
        .data(),
    )
}

/// `withdraw_nav_override` (F002): compliance withdraws the pending override before use.
pub fn withdraw_nav_override(
    compliance: &Pubkey,
    mint: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    ix(
        nav_override_accounts(compliance, mint),
        governance::instruction::WithdrawNavOverride {
            reason,
            case_ref_hash,
        }
        .data(),
    )
}

/// `reject_settlement` (F003): compliance rejects an open settlement instruction. Reasons: the
/// F001 list (0, 1, 2, 3, 6).
pub fn reject_settlement(
    compliance: &Pubkey,
    mint: &Pubkey,
    settlement: &Pubkey,
    reason: u8,
    case_ref_hash: [u8; 32],
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let accounts = governance::accounts::RejectSettlement {
        compliance: *compliance,
        role_registry: pda::role_registry(&fund_state).0,
        fund_state,
        settlement: *settlement,
        dvp_caller: pda::dvp_caller().0,
        dvp_event_authority: pda::event_authority(&mmf_common::ids::DVP_SETTLEMENT_PROGRAM_ID).0,
        dvp_settlement_program: mmf_common::ids::DVP_SETTLEMENT_PROGRAM_ID,
    };
    Instruction {
        program_id: GOVERNANCE_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: governance::instruction::RejectSettlement {
            reason,
            case_ref_hash,
        }
        .data(),
    }
}
