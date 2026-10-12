//! T030 (FR-004, research R2, R3): after `create_fund`, the mint carries every extension and
//! authority in data-model.md "Fund mint". Extensions are read by type, never enumerated (R8).

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022 as t22;
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use mmf_common::permissioned_burn::PERMISSIONED_BURN_EXTENSION_TYPE;
use mmf_sdk::pda;
use solana_signer::Signer;
use spl_token_metadata_interface::state::TokenMetadata;
use t22::extension::{
    default_account_state::DefaultAccountState, metadata_pointer::MetadataPointer,
    mint_close_authority::MintCloseAuthority, permanent_delegate::PermanentDelegate,
    transfer_hook::TransferHook, BaseStateWithExtensions, StateWithExtensions,
};
use t22::state::{AccountState, Mint};
use tests::fixtures::{self, FUND_NAME, FUND_SYMBOL};
use tests::Harness;

fn key<T: Into<Option<Pubkey>>>(k: T) -> Option<Pubkey> {
    k.into()
}

#[test]
fn create_fund_configures_the_mint_completely() {
    let mut h = Harness::new();
    let (_issuer, mint, cash_mint) = fixtures::create_standard_fund(&mut h);
    let m = mint.pubkey();
    let fund_state = pda::fund_state(&m).0;
    let terms = fixtures::standard_terms(cash_mint);

    let acc = h.account(&m).expect("mint exists");
    assert_eq!(acc.owner, TOKEN_2022_PROGRAM_ID);
    let state = StateWithExtensions::<Mint>::unpack(&acc.data).expect("mint unpacks");

    // Base mint.
    assert!(state.base.is_initialized);
    assert_eq!(state.base.decimals, 6);
    assert_eq!(state.base.supply, 0, "no units exist (FR-035)");
    assert_eq!(
        key(state.base.mint_authority),
        Some(fund_state),
        "mint authority = FundState"
    );
    assert_eq!(
        key(state.base.freeze_authority),
        Some(pda::registry(&m).0),
        "freeze authority = Registry"
    );

    // Transfer hook: compliance_hook, authority = governance token-admin PDA.
    let hook = state.get_extension::<TransferHook>().expect("TransferHook");
    assert_eq!(key(hook.program_id), Some(COMPLIANCE_HOOK_PROGRAM_ID));
    assert_eq!(key(hook.authority), Some(pda::token_admin(&m).0));

    // New accounts start frozen.
    let das = state
        .get_extension::<DefaultAccountState>()
        .expect("DefaultAccountState");
    assert_eq!(das.state, AccountState::Frozen as u8);

    // Permanent delegate = governance delegate PDA.
    let pd = state
        .get_extension::<PermanentDelegate>()
        .expect("PermanentDelegate");
    assert_eq!(key(pd.delegate), Some(pda::delegate(&m).0));

    // Metadata pointer → the mint itself, authority FundState.
    let mp = state
        .get_extension::<MetadataPointer>()
        .expect("MetadataPointer");
    assert_eq!(key(mp.metadata_address), Some(m));
    assert_eq!(key(mp.authority), Some(fund_state));

    // Close authority = FundState.
    let mca = state
        .get_extension::<MintCloseAuthority>()
        .expect("MintCloseAuthority");
    assert_eq!(key(mca.close_authority), Some(fund_state));

    // Permissioned burn: authority = FundState (raw TLV, type 28).
    let pb =
        fixtures::mint_tlv(&acc.data, PERMISSIONED_BURN_EXTENSION_TYPE).expect("PermissionedBurn");
    assert_eq!(pb.as_slice(), fund_state.as_ref());

    // Token metadata.
    let md = state
        .get_variable_len_extension::<TokenMetadata>()
        .expect("TokenMetadata");
    assert_eq!(md.mint, m);
    assert_eq!(key(md.update_authority), Some(fund_state));
    assert_eq!(md.name, FUND_NAME);
    assert_eq!(md.symbol, FUND_SYMBOL);
    let uri_len = terms.terms_uri.iter().position(|b| *b == 0).unwrap_or(128);
    assert_eq!(md.uri.as_bytes(), &terms.terms_uri[..uri_len]);
    let field = |k: &str| {
        md.additional_metadata
            .iter()
            .find(|(name, _)| name == k)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(field("fund_id"), Some(fixtures::hex(&terms.fund_id)));
    assert_eq!(field("terms_hash"), Some(fixtures::hex(&terms.terms_hash)));
    assert_eq!(md.additional_metadata.len(), 2);
}
