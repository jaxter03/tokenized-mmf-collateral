//! Every F001 PDA (docs/shared-model.md §4, §4.5; data-model.md). Each function returns
//! `(address, bump)`.

use anchor_lang::prelude::Pubkey;
use mmf_common::ids::{
    ASSOCIATED_TOKEN_PROGRAM_ID, COMPLIANCE_HOOK_PROGRAM_ID, FUND_PROGRAM_ID,
    GOVERNANCE_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
use mmf_common::seeds;

/// fund `["fund", mint]` → FundState.
pub fn fund_state(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::FUND, mint.as_ref()], &FUND_PROGRAM_ID)
}

/// governance `["roles", fund_state]` → RoleRegistry.
pub fn role_registry(fund_state: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::ROLES, fund_state.as_ref()], &GOVERNANCE_PROGRAM_ID)
}

/// compliance_hook `["registry", mint]` → Registry (the mint's freeze authority).
pub fn registry(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::REGISTRY, mint.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
}

/// compliance_hook `["extra-account-metas", mint]` → ExtraAccountMetaList.
pub fn extra_metas(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::EXTRA_ACCOUNT_METAS, mint.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
}

/// compliance_hook `["holder", mint, owner]` → HolderState.
pub fn holder(mint: &Pubkey, owner: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::HOLDER, mint.as_ref(), owner.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
}

/// governance `["delegate", mint]` → the mint's permanent delegate.
pub fn delegate(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::DELEGATE, mint.as_ref()], &GOVERNANCE_PROGRAM_ID)
}

/// governance `["token-admin", mint]` → the mint's transfer hook authority.
pub fn token_admin(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::TOKEN_ADMIN, mint.as_ref()], &GOVERNANCE_PROGRAM_ID)
}

/// governance `["hook-admin"]` → signs compliance_hook `set_status` / `set_paused`.
pub fn hook_admin() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::HOOK_ADMIN], &GOVERNANCE_PROGRAM_ID)
}

/// fund `["treasury", fund_state]` → owner of the fund treasury holding account.
pub fn treasury(fund_state: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::TREASURY, fund_state.as_ref()], &FUND_PROGRAM_ID)
}

/// Anchor's `["__event_authority"]` PDA for `emit_cpi!` in `program`.
pub fn event_authority(program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"__event_authority"], program)
}

/// The owner's one holding account: its Token-2022 associated token account for `mint`
/// (data-model.md "Holding account rule").
pub fn holding_account(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            owner.as_ref(),
            TOKEN_2022_PROGRAM_ID.as_ref(),
            mint.as_ref(),
        ],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_account_is_the_token_2022_ata() {
        let (owner, mint) = (Pubkey::new_unique(), Pubkey::new_unique());
        let expected = spl_associated_token_account_interface::address::get_associated_token_address_with_program_id(
            &owner,
            &mint,
            &TOKEN_2022_PROGRAM_ID,
        );
        assert_eq!(holding_account(&owner, &mint), expected);
    }

    #[test]
    fn pdas_are_distinct_and_program_scoped() {
        let mint = Pubkey::new_unique();
        let fs = fund_state(&mint).0;
        let all = [
            fs,
            role_registry(&fs).0,
            registry(&mint).0,
            extra_metas(&mint).0,
            holder(&mint, &Pubkey::new_unique()).0,
            delegate(&mint).0,
            token_admin(&mint).0,
            hook_admin().0,
            treasury(&fs).0,
        ];
        for (i, a) in all.iter().enumerate() {
            assert!(!a.is_on_curve(), "PDA {i} must be off-curve");
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // Same seeds under a different program give a different address.
        let other =
            Pubkey::find_program_address(&[seeds::FUND, mint.as_ref()], &GOVERNANCE_PROGRAM_ID).0;
        assert_ne!(fs, other);
    }

    #[test]
    fn holder_depends_on_owner() {
        let mint = Pubkey::new_unique();
        assert_ne!(
            holder(&mint, &Pubkey::new_unique()).0,
            holder(&mint, &Pubkey::new_unique()).0
        );
    }
}
