//! T011 toolchain spike (research R3, R8). Proves, under configuration B (Rust 1.90, Anchor 1.1.2,
//! LiteSVM 0.12 with the mainnet Token-2022 v11 binary), that:
//! (a) a mint can be created with every F001 extension, including hand-encoded PermissionedBurn;
//! (b) an owner's ordinary burn fails while a burn co-signed by the burn authority succeeds;
//! (c) an Ed25519 verify instruction runs;
//! (d) the (still empty) fund program is loaded and executes.
//! It uses raw Token-2022 instructions, so it doesn't depend on `create_fund`.

use anchor_spl::token_2022::spl_token_2022 as t22;
use litesvm::LiteSVM;
use mmf_common::{ids, permissioned_burn};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_program_pack::Pack;
use solana_signer::Signer;
use solana_transaction::Transaction;
use t22::extension::{
    default_account_state, metadata_pointer, transfer_hook, BaseStateWithExtensions, ExtensionType,
    StateWithExtensions, StateWithExtensionsMut,
};
use t22::state::{Account as TokenAccount, AccountState, Mint};

type Ix = anchor_lang::solana_program::instruction::Instruction;
type Pubkey = anchor_lang::prelude::Pubkey;

const DECIMALS: u8 = 6;

fn svm() -> LiteSVM {
    let mut svm = LiteSVM::new();
    let root = env!("CARGO_MANIFEST_DIR");
    svm.add_program_from_file(
        ids::TOKEN_2022_PROGRAM_ID,
        format!("{root}/fixtures/spl_token_2022.so"),
    )
    .expect("load Token-2022 v11 fixture");
    svm.add_program_from_file(
        ids::FUND_PROGRAM_ID,
        format!("{root}/../target/deploy/fund.so"),
    )
    .expect("load fund.so (run `anchor build` first)");
    svm
}

fn send(
    svm: &mut LiteSVM,
    ixs: &[Ix],
    payer: &Keypair,
    signers: &[&Keypair],
) -> litesvm::types::TransactionResult {
    let mut all: Vec<&Keypair> = vec![payer];
    all.extend_from_slice(signers);
    let msg = Message::new(ixs, Some(&payer.pubkey()));
    let tx = Transaction::new(&all, msg, svm.latest_blockhash());
    svm.send_transaction(tx)
}

/// Creates a mint with TransferHook, DefaultAccountState(Frozen), PermanentDelegate,
/// MetadataPointer, MintCloseAuthority and PermissionedBurn. Returns the mint keypair.
fn create_mint(
    svm: &mut LiteSVM,
    payer: &Keypair,
    freeze: &Pubkey,
    burn_authority: &Pubkey,
) -> Keypair {
    let mint = Keypair::new();
    let m = mint.pubkey();
    let len = ExtensionType::try_calculate_account_len::<Mint>(&[
        ExtensionType::TransferHook,
        ExtensionType::DefaultAccountState,
        ExtensionType::PermanentDelegate,
        ExtensionType::MetadataPointer,
        ExtensionType::MintCloseAuthority,
    ])
    .unwrap()
        + permissioned_burn::PERMISSIONED_BURN_TLV_LEN;
    let lamports = svm.minimum_balance_for_rent_exemption(len);
    let ixs = vec![
        solana_system_interface::instruction::create_account(
            &payer.pubkey(),
            &m,
            lamports,
            len as u64,
            &ids::TOKEN_2022_PROGRAM_ID,
        ),
        transfer_hook::instruction::initialize(
            &ids::TOKEN_2022_PROGRAM_ID,
            &m,
            Some(Pubkey::new_unique()),
            Some(ids::COMPLIANCE_HOOK_PROGRAM_ID),
        )
        .unwrap(),
        default_account_state::instruction::initialize_default_account_state(
            &ids::TOKEN_2022_PROGRAM_ID,
            &m,
            &AccountState::Frozen,
        )
        .unwrap(),
        t22::instruction::initialize_permanent_delegate(
            &ids::TOKEN_2022_PROGRAM_ID,
            &m,
            &Pubkey::new_unique(),
        )
        .unwrap(),
        metadata_pointer::instruction::initialize(
            &ids::TOKEN_2022_PROGRAM_ID,
            &m,
            Some(Pubkey::new_unique()),
            Some(m),
        )
        .unwrap(),
        t22::instruction::initialize_mint_close_authority(
            &ids::TOKEN_2022_PROGRAM_ID,
            &m,
            Some(&Pubkey::new_unique()),
        )
        .unwrap(),
        permissioned_burn::initialize(&m, burn_authority),
        t22::instruction::initialize_mint2(
            &ids::TOKEN_2022_PROGRAM_ID,
            &m,
            &Pubkey::new_unique(),
            Some(freeze),
            DECIMALS,
        )
        .unwrap(),
    ];
    send(svm, &ixs, payer, &[&mint]).expect("create mint with all extensions");
    mint
}

/// Creates the owner's ATA (frozen by default), thaws it, and patches in a balance.
fn funded_holding_account(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint: &Pubkey,
    owner: &Pubkey,
    freeze: &Keypair,
    amount: u64,
) -> Pubkey {
    let ata = spl_associated_token_account_interface::address::get_associated_token_address_with_program_id(owner, mint, &ids::TOKEN_2022_PROGRAM_ID);
    let create =
        spl_associated_token_account_interface::instruction::create_associated_token_account(
            &payer.pubkey(),
            owner,
            mint,
            &ids::TOKEN_2022_PROGRAM_ID,
        );
    send(svm, &[create], payer, &[]).expect("create ATA");
    {
        let acc = svm.get_account(&ata).unwrap();
        let state = StateWithExtensions::<TokenAccount>::unpack(&acc.data).unwrap();
        assert_eq!(
            state.base.state,
            AccountState::Frozen,
            "default account state must be Frozen"
        );
    }
    let thaw = t22::instruction::thaw_account(
        &ids::TOKEN_2022_PROGRAM_ID,
        &ata,
        mint,
        &freeze.pubkey(),
        &[],
    )
    .unwrap();
    send(svm, &[thaw], payer, &[freeze]).expect("thaw ATA");

    let mut acc = svm.get_account(&ata).unwrap();
    {
        let mut state = StateWithExtensionsMut::<TokenAccount>::unpack(&mut acc.data).unwrap();
        state.base.amount = amount;
        state.pack_base();
    }
    svm.set_account(ata, acc).unwrap();
    let mut macc = svm.get_account(mint).unwrap();
    {
        let mut state = StateWithExtensionsMut::<Mint>::unpack(&mut macc.data).unwrap();
        state.base.supply += amount;
        state.pack_base();
    }
    svm.set_account(*mint, macc).unwrap();
    ata
}

fn balance(svm: &LiteSVM, ata: &Pubkey) -> u64 {
    let acc = svm.get_account(ata).unwrap();
    StateWithExtensions::<TokenAccount>::unpack(&acc.data)
        .unwrap()
        .base
        .amount
}

#[test]
fn spike_mint_with_permissioned_burn_blocks_owner_burn() {
    let mut svm = svm();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    let freeze = Keypair::new();
    let burn_authority = Keypair::new();
    let owner = Keypair::new();
    svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();

    let mint = create_mint(&mut svm, &payer, &freeze.pubkey(), &burn_authority.pubkey());
    let m = mint.pubkey();

    // The extension is really there.
    let macc = svm.get_account(&m).unwrap();
    let state = StateWithExtensions::<Mint>::unpack(&macc.data).unwrap();
    // Looking up a known extension by type still works on a mint carrying type 28...
    let hook = state
        .get_extension::<transfer_hook::TransferHook>()
        .expect("TransferHook readable");
    assert_eq!(
        Option::<Pubkey>::from(hook.program_id),
        Some(ids::COMPLIANCE_HOOK_PROGRAM_ID)
    );
    // ...but interface 2.1.0 can't enumerate the extensions: it doesn't know type 28.
    assert!(state.get_extension_types().is_err());
    let tlv = &macc.data[Mint::LEN.max(165) + 1..];
    let mut found = false;
    let mut i = 0;
    while i + 4 <= tlv.len() {
        let ty = u16::from_le_bytes([tlv[i], tlv[i + 1]]);
        let ln = u16::from_le_bytes([tlv[i + 2], tlv[i + 3]]) as usize;
        if ty == permissioned_burn::PERMISSIONED_BURN_EXTENSION_TYPE {
            assert_eq!(&tlv[i + 4..i + 4 + ln], burn_authority.pubkey().as_ref());
            found = true;
        }
        i += 4 + ln;
    }
    assert!(
        found,
        "PermissionedBurn TLV present with the burn authority"
    );

    let ata = funded_holding_account(&mut svm, &payer, &m, &owner.pubkey(), &freeze, 1_000_000);

    // (b) An owner's ordinary BurnChecked fails.
    let burn = t22::instruction::burn_checked(
        &ids::TOKEN_2022_PROGRAM_ID,
        &ata,
        &m,
        &owner.pubkey(),
        &[],
        1,
        DECIMALS,
    )
    .unwrap();
    let res = send(&mut svm, &[burn], &owner, &[]);
    let err = format!(
        "{:?}",
        res.expect_err("owner BurnChecked must fail under permissioned burn")
            .err
    );
    // Token-2022 v11 rejects ordinary burns on a permissioned-burn mint with
    // TokenError::InvalidInstruction (custom error 12).
    assert!(err.contains("Custom(12)"), "unexpected error: {err}");
    // ... and so does plain Burn.
    let burn = t22::instruction::burn(
        &ids::TOKEN_2022_PROGRAM_ID,
        &ata,
        &m,
        &owner.pubkey(),
        &[],
        1,
    )
    .unwrap();
    let err = format!(
        "{:?}",
        send(&mut svm, &[burn], &owner, &[])
            .expect_err("owner Burn must fail")
            .err
    );
    assert!(err.contains("Custom(12)"), "unexpected error: {err}");
    assert_eq!(balance(&svm, &ata), 1_000_000);

    // Positive control: the same burn co-signed by the burn authority succeeds.
    let pburn = permissioned_burn::burn_checked(
        &ata,
        &m,
        &burn_authority.pubkey(),
        &owner.pubkey(),
        1,
        DECIMALS,
    );
    send(&mut svm, &[pburn], &owner, &[&burn_authority])
        .expect("permissioned burn with authority succeeds");
    assert_eq!(balance(&svm, &ata), 999_999);
}

#[test]
fn spike_ed25519_precompile_runs() {
    let mut svm = svm();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    let provider = Keypair::new();
    let msg = [7u8; 95];
    let sig = provider.sign_message(&msg);
    let sig_bytes: [u8; 64] = sig.into();
    let pk: [u8; 32] = provider.pubkey().to_bytes();
    let ix = solana_ed25519_program::new_ed25519_instruction_with_signature(&msg, &sig_bytes, &pk);
    send(&mut svm, std::slice::from_ref(&ix), &payer, &[]).expect("valid ed25519 verify passes");

    // And a wrong signature is rejected, so the precompile really checks.
    let mut bad = ix;
    let last = bad.data.len() - 1;
    bad.data[last] ^= 1;
    assert!(
        send(&mut svm, &[bad], &payer, &[]).is_err(),
        "tampered message must fail"
    );
}

#[test]
fn spike_fund_program_executes() {
    let mut svm = svm();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    // The fund program has no instructions yet, so Anchor answers any call with
    // InstructionFallbackNotFound (custom error 101). Getting that error proves the binary ran.
    let ix = Ix {
        program_id: ids::FUND_PROGRAM_ID,
        accounts: vec![],
        data: vec![0; 8],
    };
    let err = send(&mut svm, &[ix], &payer, &[]).unwrap_err();
    let s = format!("{:?}", err.err);
    assert!(
        s.contains("Custom(101)"),
        "expected Anchor fallback error 101, got {s}"
    );
}
