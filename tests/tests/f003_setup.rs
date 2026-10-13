//! F003 settlement setup (FR-001–FR-003). T004 adds the cash-helper check; T017 adds the
//! `set_cash_treasury` and `init_dvp` rules.

use solana_keypair::Keypair;
use solana_signer::Signer;
use tests::fixtures::{self, CASH_DECIMALS};
use tests::Harness;

#[test]
fn cash_helpers_mint_to_an_owners_ata() {
    let mut h = Harness::new();
    let cash_mint = fixtures::create_cash_mint(&mut h, CASH_DECIMALS);
    let owner = Keypair::new().pubkey();
    let ata = fixtures::cash_ata(&h, &owner, &cash_mint);
    assert_eq!(fixtures::cash_balance(&h, &ata), 0, "no account yet");

    fixtures::mint_cash(&mut h, &cash_mint, &owner, 2_000_000_000_000);
    fixtures::mint_cash(&mut h, &cash_mint, &owner, 1);
    assert_eq!(fixtures::cash_balance(&h, &ata), 2_000_000_000_001);
    assert_eq!(fixtures::create_cash_ata(&mut h, &owner, &cash_mint), ata, "idempotent");
}
