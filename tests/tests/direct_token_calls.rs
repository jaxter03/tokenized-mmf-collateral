//! T031 (FR-004, FR-035, Constitution II): no individual's key can mint, freeze, reconfigure or
//! re-point the fund's token. Each call is a raw Token-2022 instruction signed by the issuer's own
//! key, submitted directly with no program of ours in the path.

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022 as t22;
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use solana_keypair::Keypair;
use solana_signer::Signer;
use spl_token_metadata_interface::error::TokenMetadataError;
use spl_token_metadata_interface::state::Field;
use t22::error::TokenError;
use t22::extension::StateWithExtensionsMut;
use t22::instruction::AuthorityType;
use t22::state::{Account as TokenAccount, AccountState};
use tests::assert::{assert_token_error, custom_error};
use tests::fixtures;
use tests::{Harness, Ix};

struct Ctx {
    h: Harness,
    issuer: Keypair,
    mint: Pubkey,
}

fn ctx() -> Ctx {
    let mut h = Harness::new();
    let (issuer, mint, _) = fixtures::create_standard_fund(&mut h);
    Ctx {
        h,
        issuer,
        mint: mint.pubkey(),
    }
}

/// SetAuthority for an authority type `spl-token-2022-interface` 2.1.0 doesn't know
/// (PermissionedBurn = 17). Data: `[6, type, 1, new_authority]`.
fn set_authority_raw(mint: &Pubkey, current: &Pubkey, authority_type: u8, new: &Pubkey) -> Ix {
    let mut data = vec![6, authority_type, 1];
    data.extend_from_slice(new.as_ref());
    Ix {
        program_id: TOKEN_2022_PROGRAM_ID,
        accounts: vec![
            anchor_lang::solana_program::instruction::AccountMeta::new(*mint, false),
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(*current, true),
        ],
        data,
    }
}

#[test]
fn issuer_key_cannot_mint() {
    let mut c = ctx();
    // A holding account for the issuer. It starts frozen, and Token-2022 checks a frozen
    // destination before the authority, so the test unfreezes it directly to isolate the
    // authority check.
    let ata = mmf_sdk::pda::holding_account(&c.issuer.pubkey(), &c.mint);
    let create =
        spl_associated_token_account_interface::instruction::create_associated_token_account(
            &c.h.payer.pubkey(),
            &c.issuer.pubkey(),
            &c.mint,
            &TOKEN_2022_PROGRAM_ID,
        );
    c.h.send(&[create], &[]).expect("create ATA");
    let mut acc = c.h.account(&ata).unwrap();
    {
        let mut st = StateWithExtensionsMut::<TokenAccount>::unpack(&mut acc.data).unwrap();
        st.base.state = AccountState::Initialized;
        st.pack_base();
    }
    c.h.svm.set_account(ata, acc).unwrap();

    let ix = t22::instruction::mint_to(
        &TOKEN_2022_PROGRAM_ID,
        &c.mint,
        &ata,
        &c.issuer.pubkey(),
        &[],
        1,
    )
    .unwrap();
    assert_token_error(&c.h.send(&[ix], &[&c.issuer]), TokenError::OwnerMismatch);
}

#[test]
fn issuer_key_cannot_change_any_mint_authority() {
    let mut c = ctx();
    let new = Pubkey::new_unique();
    for ty in [
        AuthorityType::MintTokens,
        AuthorityType::FreezeAccount,
        AuthorityType::CloseMint,
        AuthorityType::PermanentDelegate,
        AuthorityType::TransferHookProgramId,
        AuthorityType::MetadataPointer,
    ] {
        let ix = t22::instruction::set_authority(
            &TOKEN_2022_PROGRAM_ID,
            &c.mint,
            Some(&new),
            ty.clone(),
            &c.issuer.pubkey(),
            &[],
        )
        .unwrap();
        let res = c.h.send(&[ix], &[&c.issuer]);
        assert_eq!(
            custom_error(&res).1,
            TokenError::OwnerMismatch as u32,
            "{ty:?}"
        );
    }
    // PermissionedBurn (17), hand-encoded.
    let ix = set_authority_raw(&c.mint, &c.issuer.pubkey(), 17, &new);
    assert_token_error(&c.h.send(&[ix], &[&c.issuer]), TokenError::OwnerMismatch);
}

#[test]
fn issuer_key_cannot_repoint_the_transfer_hook() {
    let mut c = ctx();
    let ix = t22::extension::transfer_hook::instruction::update(
        &TOKEN_2022_PROGRAM_ID,
        &c.mint,
        &c.issuer.pubkey(),
        &[],
        Some(Pubkey::new_unique()),
    )
    .unwrap();
    assert_token_error(&c.h.send(&[ix], &[&c.issuer]), TokenError::OwnerMismatch);
}

#[test]
fn issuer_key_cannot_change_the_default_account_state() {
    let mut c = ctx();
    let ix = t22::extension::default_account_state::instruction::update_default_account_state(
        &TOKEN_2022_PROGRAM_ID,
        &c.mint,
        &c.issuer.pubkey(),
        &[],
        &AccountState::Initialized,
    )
    .unwrap();
    assert_token_error(&c.h.send(&[ix], &[&c.issuer]), TokenError::OwnerMismatch);
}

#[test]
fn issuer_key_cannot_update_metadata() {
    let mut c = ctx();
    let ix = spl_token_metadata_interface::instruction::update_field(
        &TOKEN_2022_PROGRAM_ID,
        &c.mint,
        &c.issuer.pubkey(),
        Field::Uri,
        "https://evil.example".into(),
    );
    let res = c.h.send(&[ix], &[&c.issuer]);
    assert_eq!(
        custom_error(&res).1,
        TokenMetadataError::IncorrectUpdateAuthority as u32
    );
}

/// FR-035, research R3: an approved investor can't destroy its own units. Token-2022 rejects an
/// ordinary burn on a permissioned-burn mint with TokenError::InvalidInstruction.
#[test]
fn holder_cannot_burn_its_own_units() {
    let mut h = Harness::new();
    let (ctx, alice) = fixtures::fund_with_investor_units(&mut h, 0, 1_000);
    let m = ctx.m();
    let ata = mmf_sdk::pda::holding_account(&alice.pubkey(), &m);
    let burn =
        t22::instruction::burn(&TOKEN_2022_PROGRAM_ID, &ata, &m, &alice.pubkey(), &[], 10).unwrap();
    assert_token_error(&h.send(&[burn], &[&alice]), TokenError::InvalidInstruction);
    let burn = t22::instruction::burn_checked(
        &TOKEN_2022_PROGRAM_ID,
        &ata,
        &m,
        &alice.pubkey(),
        &[],
        10,
        6,
    )
    .unwrap();
    assert_token_error(&h.send(&[burn], &[&alice]), TokenError::InvalidInstruction);
    assert_eq!(tests::units::balance(&h, &m, &alice.pubkey()), 1_000);
    assert_eq!(tests::units::supply(&h, &m), 1_000);
}
