//! Every PDA (F001–F004) (docs/shared-model.md §4, §4.5; data-model.md). Each function returns
//! `(address, bump)`.

use anchor_lang::prelude::Pubkey;
use mmf_common::ids::{
    ASSOCIATED_TOKEN_PROGRAM_ID, COLLATERAL_PROGRAM_ID, COMPLIANCE_HOOK_PROGRAM_ID, DVP_SETTLEMENT_PROGRAM_ID,
    FUND_PROGRAM_ID, GOVERNANCE_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
use mmf_common::seeds;

/// fund `["fund", mint]` → FundState.
pub fn fund_state(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::FUND, mint.as_ref()], &FUND_PROGRAM_ID)
}

/// fund `["nav", fund_state, seq.to_le_bytes()]` → NavRecord (F002).
pub fn nav_record(fund_state: &Pubkey, seq: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::NAV, fund_state.as_ref(), &seq.to_le_bytes()],
        &FUND_PROGRAM_ID,
    )
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

/// governance `["fund-caller"]` → signs fund `set_nav_override` / `clear_nav_override` (F002).
pub fn fund_caller() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::FUND_CALLER], &GOVERNANCE_PROGRAM_ID)
}

/// fund `["treasury", fund_state]` → owner of the fund treasury holding account.
pub fn treasury(fund_state: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::TREASURY, fund_state.as_ref()], &FUND_PROGRAM_ID)
}

/// dvp_settlement `["dvp", fund_state]` → DvpConfig (F003).
pub fn dvp_config(fund_state: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::DVP, fund_state.as_ref()],
        &DVP_SETTLEMENT_PROGRAM_ID,
    )
}

/// dvp_settlement `["escrow", fund_state]` → escrow authority; owns both escrows (F003).
pub fn escrow_authority(fund_state: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::ESCROW, fund_state.as_ref()],
        &DVP_SETTLEMENT_PROGRAM_ID,
    )
}

/// The unit escrow: the escrow authority's holding account for `mint` (Token-2022 ATA) (F003).
pub fn unit_escrow(fund_state: &Pubkey, mint: &Pubkey) -> Pubkey {
    holding_account(&escrow_authority(fund_state).0, mint)
}

/// The cash escrow: the escrow authority's ATA for `cash_mint` under the cash mint's token
/// program (`cash_token_program`, SPL Token or Token-2022) (F003).
pub fn cash_escrow(fund_state: &Pubkey, cash_mint: &Pubkey, cash_token_program: &Pubkey) -> Pubkey {
    ata(
        &escrow_authority(fund_state).0,
        cash_mint,
        cash_token_program,
    )
}

/// dvp_settlement `["settlement", fund_state, created_by, external_ref]` → SettlementInstruction
/// (F003, research R2). Known before creation, so a prepared step can name it.
pub fn settlement(
    fund_state: &Pubkey,
    created_by: &Pubkey,
    external_ref: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            seeds::SETTLEMENT,
            fund_state.as_ref(),
            created_by.as_ref(),
            external_ref,
        ],
        &DVP_SETTLEMENT_PROGRAM_ID,
    )
}

/// governance `["dvp-caller"]` → signs dvp_settlement `reject` (F003).
pub fn dvp_caller() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::DVP_CALLER], &GOVERNANCE_PROGRAM_ID)
}

/// collateral `["agreement", fund_state, giver, taker, nonce.to_le_bytes()]` → PledgeAgreement
/// (F004).
pub fn agreement(fund_state: &Pubkey, giver: &Pubkey, taker: &Pubkey, nonce: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            seeds::AGREEMENT,
            fund_state.as_ref(),
            giver.as_ref(),
            taker.as_ref(),
            &nonce.to_le_bytes(),
        ],
        &COLLATERAL_PROGRAM_ID,
    )
}

/// collateral `["pledge", agreement, pledge_ref]` → Pledge (F004, research R6).
pub fn pledge(agreement: &Pubkey, pledge_ref: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::PLEDGE, agreement.as_ref(), pledge_ref],
        &COLLATERAL_PROGRAM_ID,
    )
}

/// collateral `["call", agreement, external_ref]` → CollateralCall (F004, research R6).
pub fn call(agreement: &Pubkey, external_ref: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::CALL, agreement.as_ref(), external_ref],
        &COLLATERAL_PROGRAM_ID,
    )
}

/// collateral `["hook-caller"]`: signs `compliance_hook::set_lien` (F004).
pub fn hook_caller() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::HOOK_CALLER], &COLLATERAL_PROGRAM_ID)
}

/// governance `["collateral-caller"]`: signs `collateral::resolve_dispute` (F004).
pub fn collateral_caller() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::COLLATERAL_CALLER], &GOVERNANCE_PROGRAM_ID)
}

/// Any owner's associated token account for `mint` under `token_program` (cash accounts, F003).
pub fn ata(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
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
            dvp_config(&fs).0,
            escrow_authority(&fs).0,
            settlement(&fs, &Pubkey::new_unique(), &[1; 32]).0,
            dvp_caller().0,
            agreement(&fs, &Pubkey::new_unique(), &Pubkey::new_unique(), 1).0,
            pledge(&Pubkey::new_unique(), &[1; 32]).0,
            call(&Pubkey::new_unique(), &[1; 32]).0,
            hook_caller().0,
            collateral_caller().0,
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
    fn cash_ata_matches_the_ata_program_for_both_token_programs() {
        let (owner, mint) = (Pubkey::new_unique(), Pubkey::new_unique());
        for program in [mmf_common::ids::SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID] {
            let expected = spl_associated_token_account_interface::address::get_associated_token_address_with_program_id(
                &owner, &mint, &program,
            );
            assert_eq!(ata(&owner, &mint, &program), expected);
        }
        let fs = Pubkey::new_unique();
        assert_eq!(
            unit_escrow(&fs, &mint),
            holding_account(&escrow_authority(&fs).0, &mint)
        );
    }

    #[test]
    fn settlement_depends_on_creator_and_reference() {
        let fs = Pubkey::new_unique();
        let a = Pubkey::new_unique();
        let s = settlement(&fs, &a, &[1; 32]).0;
        assert_ne!(s, settlement(&fs, &Pubkey::new_unique(), &[1; 32]).0);
        assert_ne!(s, settlement(&fs, &a, &[2; 32]).0);
        assert_ne!(s, settlement(&Pubkey::new_unique(), &a, &[1; 32]).0);
    }

    #[test]
    fn holder_depends_on_owner() {
        let mint = Pubkey::new_unique();
        assert_ne!(
            holder(&mint, &Pubkey::new_unique()).0,
            holder(&mint, &Pubkey::new_unique()).0
        );
    }

    #[test]
    fn collateral_pdas_match_their_seeds() {
        let (fs, g, t) = (Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique());
        let a = agreement(&fs, &g, &t, 7);
        assert_eq!(
            a,
            Pubkey::find_program_address(
                &[b"agreement", fs.as_ref(), g.as_ref(), t.as_ref(), &7u64.to_le_bytes()],
                &COLLATERAL_PROGRAM_ID
            )
        );
        assert_ne!(a.0, agreement(&fs, &t, &g, 7).0, "giver and taker are ordered");
        assert_ne!(a.0, agreement(&fs, &g, &t, 8).0, "nonce is in the seeds");
        assert_eq!(
            pledge(&a.0, &[3; 32]),
            Pubkey::find_program_address(&[b"pledge", a.0.as_ref(), &[3; 32]], &COLLATERAL_PROGRAM_ID)
        );
        assert_eq!(
            call(&a.0, &[4; 32]),
            Pubkey::find_program_address(&[b"call", a.0.as_ref(), &[4; 32]], &COLLATERAL_PROGRAM_ID)
        );
        assert_eq!(
            hook_caller(),
            Pubkey::find_program_address(&[b"hook-caller"], &COLLATERAL_PROGRAM_ID)
        );
        assert_eq!(
            collateral_caller(),
            Pubkey::find_program_address(&[b"collateral-caller"], &GOVERNANCE_PROGRAM_ID)
        );
    }
}
