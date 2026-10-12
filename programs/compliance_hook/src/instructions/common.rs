//! Checks shared by the compliance-signed instructions.

use anchor_lang::prelude::*;
use mmf_common::enums::{FundStatus, HolderStatus};
use mmf_common::views::{FundStateView, RoleRegistryView};

use crate::errors::HookError;
use crate::state::{HolderState, Registry};

/// The signer is the fund's compliance role (an unverifiable role registry fails closed).
pub fn require_compliance(
    registry: &Registry,
    role_registry: &AccountInfo,
    signer: &Pubkey,
) -> Result<()> {
    let roles = RoleRegistryView::load(role_registry, &registry.role_registry)
        .map_err(|_| error!(HookError::Unauthorized))?;
    require_keys_eq!(*signer, roles.compliance, HookError::Unauthorized);
    Ok(())
}

/// The fund is Open (a missing fund fails closed).
pub fn require_open(registry: &Registry, fund_state: &AccountInfo) -> Result<()> {
    let open = FundStateView::load(fund_state, &registry.fund_state)
        .map(|f| f.status == FundStatus::Open.as_u8())
        .unwrap_or(false);
    require!(open, HookError::FundNotOpen);
    Ok(())
}

/// Not Revoked, then in `expected` status.
pub fn require_status(holder: &HolderState, expected: HolderStatus) -> Result<()> {
    require!(
        holder.status() != Some(HolderStatus::Revoked),
        HookError::HolderRevoked
    );
    require!(
        holder.status() == Some(expected),
        HookError::InvalidHolderStatus
    );
    Ok(())
}
