//! `set_status` (contracts/compliance_hook.md): suspend, reinstate or revoke a holder. Called only
//! by governance (signer = governance PDA `["hook-admin"]`) on behalf of compliance.
//!
//! Status only: the token account is not frozen or thawed (research R13). The hook's status check
//! is the rule.

use anchor_lang::prelude::*;
use mmf_common::enums::{HolderStatus, StatusAction};
use mmf_common::ids::GOVERNANCE_PROGRAM_ID;
use mmf_common::seeds;

use crate::errors::HookError;
use crate::events::HolderStatusChanged;
use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
pub struct SetStatus<'info> {
    /// Must be governance's `["hook-admin"]` PDA (checked in the handler: `UnauthorizedCaller`).
    pub hook_admin: Signer<'info>,
    pub registry: Account<'info, Registry>,
    #[account(
        mut,
        seeds = [seeds::HOLDER, registry.mint.as_ref(), holder_state.owner.as_ref()],
        bump = holder_state.bump
    )]
    pub holder_state: Account<'info, HolderState>,
}

/// Governance's `["hook-admin"]` PDA: the only caller allowed for set_status / set_paused.
pub fn require_hook_admin(signer: &Pubkey) -> Result<()> {
    let hook_admin = Pubkey::find_program_address(&[seeds::HOOK_ADMIN], &GOVERNANCE_PROGRAM_ID).0;
    require_keys_eq!(*signer, hook_admin, HookError::UnauthorizedCaller);
    Ok(())
}

/// The transition table (data-model.md "State transitions").
fn next_status(action: StatusAction, from: HolderStatus) -> Option<HolderStatus> {
    use HolderStatus::*;
    match (action, from) {
        (StatusAction::Suspend, Approved) => Some(Suspended),
        (StatusAction::Reinstate, Suspended) => Some(Approved),
        (StatusAction::Revoke, Pending | Approved | Suspended) => Some(Revoked),
        _ => None,
    }
}

pub fn handle_set_status(
    ctx: Context<SetStatus>,
    action: u8,
    reason: u8,
    case_ref_hash: [u8; 32],
    actor: Pubkey,
) -> Result<()> {
    // 1. Only governance's hook-admin PDA.
    require_hook_admin(&ctx.accounts.hook_admin.key())?;

    // 2. Revocation is final (FR-029).
    let h = &mut ctx.accounts.holder_state;
    let from = h.status().ok_or(error!(HookError::InvalidHolderStatus))?;
    require!(from != HolderStatus::Revoked, HookError::HolderRevoked);

    // 3. Allowed transition (an unknown action fails closed).
    let to = StatusAction::try_from(action)
        .ok()
        .and_then(|a| next_status(a, from))
        .ok_or(error!(HookError::InvalidHolderStatus))?;
    h.status = to.as_u8();

    let clock = Clock::get()?;
    emit_cpi!(HolderStatusChanged {
        fund: ctx.accounts.registry.fund_state,
        owner: h.owner,
        from: from.as_u8(),
        to: to.as_u8(),
        reason,
        case_ref_hash,
        actor,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        use HolderStatus::*;
        use StatusAction::*;
        assert_eq!(next_status(Suspend, Approved), Some(Suspended));
        assert_eq!(next_status(Reinstate, Suspended), Some(Approved));
        for from in [Pending, Approved, Suspended] {
            assert_eq!(next_status(Revoke, from), Some(Revoked));
        }
        for (a, from) in [
            (Suspend, Pending),
            (Suspend, Suspended),
            (Reinstate, Approved),
            (Reinstate, Pending),
        ] {
            assert_eq!(next_status(a, from), None);
        }
    }
}
