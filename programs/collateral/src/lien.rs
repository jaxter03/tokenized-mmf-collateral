//! The lien CPI (research R1, R2): every change to a holder's locked units goes to
//! `compliance_hook::set_lien`, signed by this program's `["hook-caller"]` PDA. The hook checks
//! the balance, so the lock can never exceed the units the holder has (invariant I2).

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke_signed;
use anchor_lang::InstructionData;
use compliance_hook::LienChange;
use mmf_common::seeds;

/// The accounts every lien change passes through (contracts/collateral.md "lien accounts").
pub struct LienAccounts<'a, 'info> {
    pub hook_caller: &'a AccountInfo<'info>,
    pub hook_caller_bump: u8,
    pub registry: &'a AccountInfo<'info>,
    pub holder_state: &'a AccountInfo<'info>,
    pub holding_account: &'a AccountInfo<'info>,
    pub hook_program: &'a AccountInfo<'info>,
}

pub fn set_lien(l: &LienAccounts, change: LienChange) -> Result<()> {
    // Built by hand: `set_lien` takes the caller as an unchecked account (so a wrong or unsigned
    // caller reports `NotCollateralCaller`), and Anchor's generated CPI would pass it unsigned.
    let ix = Instruction {
        program_id: l.hook_program.key(),
        accounts: vec![
            AccountMeta::new_readonly(l.hook_caller.key(), true),
            AccountMeta::new_readonly(l.registry.key(), false),
            AccountMeta::new(l.holder_state.key(), false),
            AccountMeta::new_readonly(l.holding_account.key(), false),
        ],
        data: compliance_hook::instruction::SetLien { change }.data(),
    };
    let bump = [l.hook_caller_bump];
    let signer: &[&[u8]] = &[seeds::HOOK_CALLER, &bump];
    invoke_signed(
        &ix,
        &[
            l.hook_caller.clone(),
            l.registry.clone(),
            l.holder_state.clone(),
            l.holding_account.clone(),
            l.hook_program.clone(),
        ],
        &[signer],
    )?;
    Ok(())
}

/// The holder's locked units after a CPI (read back from its HolderState).
pub fn locked_after(holder_state: &AccountInfo) -> Result<u64> {
    let data = holder_state.try_borrow_data()?;
    Ok(compliance_hook::HolderState::try_deserialize(&mut &data[..])?.pledged_amount)
}
