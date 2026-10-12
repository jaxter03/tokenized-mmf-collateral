//! Shared test setup: standard terms, a cash mint, and (from T039) a fully launched fund.

use anchor_lang::prelude::Pubkey;
use anchor_lang::AccountDeserialize;
use anchor_spl::token_2022::spl_token_2022 as t22;
use fund::FundTerms;
use mmf_common::ids::TOKEN_2022_PROGRAM_ID;
use mmf_common::jurisdiction::{self, JurisdictionSet, SET_WORDS};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_signer::Signer;

use crate::harness::Harness;

pub const CASH_DECIMALS: u8 = 6;

/// Terms that pass every FR-003 check.
pub fn standard_terms(cash_mint: Pubkey) -> FundTerms {
    let mut terms_uri = [0u8; 128];
    let uri = b"https://example.org/fund/terms.pdf";
    terms_uri[..uri.len()].copy_from_slice(uri);
    FundTerms {
        fund_id: *b"MMF-USD-0001",
        cash_mint,
        target_nav: 100_000_000,
        max_nav_move_bps: 50,
        redemption_gate_bps: 1_000,
        distribution_mode: 0,
        distribution_cadence_days: 30,
        terms_hash: [0x5a; 32],
        terms_uri,
    }
}

pub const FUND_NAME: &str = "Tokenized Government MMF";
pub const FUND_SYMBOL: &str = "TMMF";

/// A jurisdiction set from codes, e.g. `jurisdictions(&[*b"US", *b"GB"])`.
pub fn jurisdictions(codes: &[[u8; 2]]) -> JurisdictionSet {
    let mut set = [0u64; SET_WORDS];
    for c in codes {
        assert!(
            jurisdiction::set_insert(&mut set, *c),
            "{c:?} must be assigned"
        );
    }
    set
}

/// Creates a Token-2022 cash mint with `decimals` (6 for a valid cash asset).
pub fn create_cash_mint(h: &mut Harness, decimals: u8) -> Pubkey {
    let mint = Keypair::new();
    let len = t22::state::Mint::LEN;
    let lamports = h.svm.minimum_balance_for_rent_exemption(len);
    let ixs = [
        solana_system_interface::instruction::create_account(
            &h.payer.pubkey(),
            &mint.pubkey(),
            lamports,
            len as u64,
            &TOKEN_2022_PROGRAM_ID,
        ),
        t22::instruction::initialize_mint2(
            &TOKEN_2022_PROGRAM_ID,
            &mint.pubkey(),
            &h.payer.pubkey(),
            None,
            decimals,
        )
        .unwrap(),
    ];
    h.send(&ixs, &[&mint]).expect("create cash mint");
    mint.pubkey()
}

/// Deserializes an Anchor account (discriminator checked).
pub fn read<T: AccountDeserialize>(h: &Harness, key: &Pubkey) -> T {
    let data = h.data(key);
    T::try_deserialize(&mut data.as_slice()).expect("deserialize account")
}

/// Rewrites an Anchor account in place. Only for tests of conditions no instruction can reach
/// (documented exception, quickstart.md FR-023); each caller says why in a comment.
pub fn patch<T: AccountDeserialize + anchor_lang::AccountSerialize>(
    h: &mut Harness,
    key: &Pubkey,
    edit: impl FnOnce(&mut T),
) {
    let mut acc = h.account(key).expect("account to patch");
    let mut value = T::try_deserialize(&mut acc.data.as_slice()).expect("deserialize");
    edit(&mut value);
    let mut out = Vec::with_capacity(acc.data.len());
    value.try_serialize(&mut out).expect("serialize");
    assert_eq!(
        out.len(),
        acc.data.len(),
        "patch must not change the account size"
    );
    acc.data = out;
    h.svm.set_account(*key, acc).expect("set_account");
}

/// Raw TLV lookup on a Token-2022 mint (`spl-token-2022-interface` 2.1.0 can't enumerate a mint
/// carrying PermissionedBurn, research R8). Returns the extension's data bytes.
pub fn mint_tlv(data: &[u8], extension_type: u16) -> Option<Vec<u8>> {
    // Base mint padded to the account size (165) plus the 1-byte account type.
    let mut i = 166;
    while i + 4 <= data.len() {
        let ty = u16::from_le_bytes([data[i], data[i + 1]]);
        let len = u16::from_le_bytes([data[i + 2], data[i + 3]]) as usize;
        if ty == 0 && len == 0 {
            return None; // uninitialized tail
        }
        if ty == extension_type {
            return Some(data[i + 4..i + 4 + len].to_vec());
        }
        i += 4 + len;
    }
    None
}

/// Lowercase hex, as used for the `fund_id` and `terms_hash` metadata fields.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Sends init_roles + create_fund with standard terms. Returns `(issuer, mint, cash_mint)`.
pub fn create_standard_fund(h: &mut Harness) -> (Keypair, Keypair, Pubkey) {
    let issuer = h.funded_keypair();
    let mint = Keypair::new();
    let cash_mint = create_cash_mint(h, CASH_DECIMALS);
    let m = mint.pubkey();
    let ixs = [
        mmf_sdk::ix_governance::init_roles(&issuer.pubkey(), &m),
        mmf_sdk::ix_fund::create_fund(
            &issuer.pubkey(),
            &m,
            &cash_mint,
            standard_terms(cash_mint),
            FUND_NAME,
            FUND_SYMBOL,
        ),
    ];
    h.send(&ixs, &[&issuer, &mint]).expect("create fund");
    (issuer, mint, cash_mint)
}

/// A launched fund with every role held by its own keypair (T039). Used by US2–US4 tests.
pub struct FundCtx {
    pub issuer: Keypair,
    pub mint: Keypair,
    pub cash_mint: Pubkey,
    pub fund_admin: Keypair,
    pub transfer_agent: Keypair,
    pub compliance: Keypair,
    pub keeper: Keypair,
    pub auditor: Keypair,
    /// The one recognised mock KYC provider.
    pub provider: Keypair,
}

impl FundCtx {
    pub fn m(&self) -> Pubkey {
        self.mint.pubkey()
    }

    pub fn fund_state(&self) -> Pubkey {
        mmf_sdk::pda::fund_state(&self.m()).0
    }

    pub fn registry(&self) -> Pubkey {
        mmf_sdk::pda::registry(&self.m()).0
    }

    pub fn roles(&self) -> mmf_sdk::ix_governance::Roles {
        mmf_sdk::ix_governance::Roles {
            fund_admin: self.fund_admin.pubkey(),
            transfer_agent: self.transfer_agent.pubkey(),
            compliance: self.compliance.pubkey(),
            keeper: self.keeper.pubkey(),
            auditor: self.auditor.pubkey(),
        }
    }
}

/// Creates, configures and launches a fund: one provider, `US` and `GB` allowed, holder cap
/// `cap` (0 = no cap), five distinct role keypairs (all funded so they can pay fees).
pub fn launched_fund(h: &mut Harness, cap: u32) -> FundCtx {
    let (issuer, mint, cash_mint) = create_standard_fund(h);
    let ctx = FundCtx {
        issuer,
        mint,
        cash_mint,
        fund_admin: h.funded_keypair(),
        transfer_agent: h.funded_keypair(),
        compliance: h.funded_keypair(),
        keeper: h.funded_keypair(),
        auditor: h.funded_keypair(),
        provider: Keypair::new(),
    };
    let m = ctx.m();
    let i = ctx.issuer.pubkey();
    let providers = [
        ctx.provider.pubkey(),
        Pubkey::default(),
        Pubkey::default(),
        Pubkey::default(),
    ];
    let ixs = [
        mmf_sdk::ix_hook::init_registry(&i, &m),
        mmf_sdk::ix_hook::set_rules(&i, &m, providers, jurisdictions(&[*b"US", *b"GB"]), cap),
        mmf_sdk::ix_governance::set_roles(&i, &m, &ctx.roles()),
        mmf_sdk::ix_fund::launch(&i, &m),
    ];
    h.send(&ixs, &[&ctx.issuer])
        .expect("configure and launch fund");
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launched_fund_is_open_with_rules_and_roles() {
        let mut h = Harness::new();
        let c = launched_fund(&mut h, 2);
        let fs: fund::FundState = read(&h, &c.fund_state());
        assert_eq!(fs.status, mmf_common::enums::FundStatus::Open.as_u8());
        let reg: compliance_hook::Registry = read(&h, &c.registry());
        assert_eq!(reg.max_holders, 2);
        assert_eq!(reg.kyc_providers[0], c.provider.pubkey());
        assert!(jurisdiction::set_contains(
            &reg.allowed_jurisdictions,
            *b"US"
        ));
        let roles: governance::RoleRegistry = read(&h, &fs.role_registry);
        assert_eq!(roles.compliance, c.compliance.pubkey());
        assert_eq!(roles.issuer, c.issuer.pubkey());
    }
}
