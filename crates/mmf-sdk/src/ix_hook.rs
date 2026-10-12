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

use mmf_common::ids::TOKEN_2022_PROGRAM_ID;

use crate::kyc::SignedStatement;

fn event_accounts() -> (Pubkey, Pubkey) {
    (
        pda::event_authority(&COMPLIANCE_HOOK_PROGRAM_ID).0,
        COMPLIANCE_HOOK_PROGRAM_ID,
    )
}

/// `register_holder`: `authority` is the owner itself (Investor only) or compliance.
pub fn register_holder(
    authority: &Pubkey,
    payer: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    class: u8,
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let (event_authority, program) = event_accounts();
    let accounts = compliance_hook::accounts::RegisterHolder {
        authority: *authority,
        payer: *payer,
        registry: pda::registry(mint).0,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        holder_state: pda::holder(mint, owner).0,
        system_program: system_program::ID,
        event_authority,
        program,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::RegisterHolder {
            owner: *owner,
            class,
        }
        .data(),
    }
}

/// `approve_investor`: must come immediately after the statement's Ed25519 instruction.
pub fn approve_investor(
    compliance: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    attestation_hash: [u8; 32],
) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let (event_authority, program) = event_accounts();
    let accounts = compliance_hook::accounts::ApproveInvestor {
        compliance: *compliance,
        registry: pda::registry(mint).0,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        holder_state: pda::holder(mint, owner).0,
        instructions: solana_instructions_sysvar::ID,
        event_authority,
        program,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::ApproveInvestor { attestation_hash }.data(),
    }
}

/// `approve_fund_account`: compliance approves a fund treasury (or, from F003, escrow) record.
pub fn approve_fund_account(compliance: &Pubkey, mint: &Pubkey, owner: &Pubkey) -> Instruction {
    let fund_state = pda::fund_state(mint).0;
    let (event_authority, program) = event_accounts();
    let accounts = compliance_hook::accounts::ApproveFundAccount {
        compliance: *compliance,
        registry: pda::registry(mint).0,
        fund_state,
        role_registry: pda::role_registry(&fund_state).0,
        holder_state: pda::holder(mint, owner).0,
        event_authority,
        program,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::ApproveFundAccount {}.data(),
    }
}

/// `thaw_holding_account` (permissionless) for `owner`'s ATA, or another `token_account` for
/// rejection tests.
pub fn thaw_holding_account(
    payer: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    token_account: &Pubkey,
) -> Instruction {
    let (event_authority, program) = event_accounts();
    let accounts = compliance_hook::accounts::ThawHoldingAccount {
        payer: *payer,
        registry: pda::registry(mint).0,
        holder_state: pda::holder(mint, owner).0,
        token_account: *token_account,
        mint: *mint,
        token_2022_program: TOKEN_2022_PROGRAM_ID,
        event_authority,
        program,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::ThawHoldingAccount {}.data(),
    }
}

/// Creates `owner`'s holding account (idempotent), paid by `payer`.
pub fn create_holding_account(payer: &Pubkey, mint: &Pubkey, owner: &Pubkey) -> Instruction {
    spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(
        payer,
        owner,
        mint,
        &TOKEN_2022_PROGRAM_ID,
    )
}

/// Compliance's onboarding transaction (SC-005): `[create ATA, ed25519, approve_investor, thaw]`.
/// The Ed25519 instruction sits immediately before `approve_investor`, as the program requires.
pub fn onboarding_tx(
    payer: &Pubkey,
    compliance: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    signed: &SignedStatement,
) -> Vec<Instruction> {
    vec![
        create_holding_account(payer, mint, owner),
        signed.ix.clone(),
        approve_investor(compliance, mint, owner, signed.attestation_hash),
        thaw_holding_account(payer, mint, owner, &pda::holding_account(owner, mint)),
    ]
}

/// `renew_investor`: must come immediately after the statement's Ed25519 instruction.
pub fn renew_investor(
    compliance: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    attestation_hash: [u8; 32],
) -> Instruction {
    let mut ix = approve_investor(compliance, mint, owner, attestation_hash);
    ix.data = compliance_hook::instruction::RenewInvestor { attestation_hash }.data();
    ix
}

/// A direct `set_status` call (normally only governance's hook-admin PDA signs it). For tests
/// proving any other signer is rejected.
pub fn set_status_direct(
    signer: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    action: u8,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Instruction {
    let (event_authority, program) = event_accounts();
    let accounts = compliance_hook::accounts::SetStatus {
        hook_admin: *signer,
        registry: pda::registry(mint).0,
        holder_state: pda::holder(mint, owner).0,
        event_authority,
        program,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::SetStatus {
            action,
            reason,
            case_ref_hash,
            actor,
        }
        .data(),
    }
}

/// A direct `set_paused` call (normally only governance's hook-admin PDA signs it).
pub fn set_paused_direct(
    signer: &Pubkey,
    mint: &Pubkey,
    paused: bool,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Instruction {
    let (event_authority, program) = event_accounts();
    let accounts = compliance_hook::accounts::SetPaused {
        hook_admin: *signer,
        registry: pda::registry(mint).0,
        event_authority,
        program,
    };
    Instruction {
        program_id: COMPLIANCE_HOOK_PROGRAM_ID,
        accounts: accounts.to_account_metas(None),
        data: compliance_hook::instruction::SetPaused {
            paused,
            reason,
            case_ref_hash,
            actor,
        }
        .data(),
    }
}
