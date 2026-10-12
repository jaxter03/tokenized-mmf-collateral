# Quickstart: validating F001

How to prove F001 works: local program tests first, then one run on devnet. Instruction details
live in [contracts/](contracts/); account layouts in [data-model.md](data-model.md); version
choices in [research.md](research.md) (R1, R8).

## Prerequisites

| Tool | Version | Note |
|---|---|---|
| Rust | 1.90 (pinned in `rust-toolchain.toml`) | configuration B (R8); ≥ 1.97.1 if the spike moves to configuration A |
| Anchor CLI | 1.1.2 (`avm use 1.1.2`) | |
| Solana CLI | 3.1.10 or newer | devnet runs Agave 4.4-beta; upgrade if deploys misbehave |
| devnet SOL | ~8 SOL | three programs at 1.5–2.6 SOL each plus an upgrade buffer; faucet.solana.com allows 2 requests per 8 h |

### Test fixtures (once; committed under `tests/fixtures/`)

All three come from **mainnet**, never devnet:

```bash
solana program dump -um TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb tests/fixtures/spl_token_2022.so
solana program dump -um SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf tests/fixtures/squads_multisig_program.so
solana account -um BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr --output json > tests/fixtures/squads_program_config.json
python3 -c "import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],'rb').read().rstrip(b'\\0')).hexdigest())" tests/fixtures/spl_token_2022.so
# hash with trailing zeros removed (solana-verify convention) must start 7ea94a02 = program@v11.0.0
```

## Build and test

```bash
anchor build                      # three programs: fund, compliance_hook, governance
cargo test -p tests               # LiteSVM suite, all stories
cargo test -p tests --release -- --ignored scale   # SC-006: 10 vs 10 000 holders
cargo test -p mmf-common          # ISO 3166 table (249 codes), statement encoding
scripts/check-error-coverage.sh   # SC-004: every error variant is asserted by at least one test
```

**Expected**: everything green. `check-error-coverage` lists any error code in the three programs
that no test asserts, and fails if the list isn't empty.

## Spike (task 1) — must pass before anything else

`tests/tests/spike_toolchain.rs` (task T011), under configuration B. It uses raw Token-2022
instructions, so it doesn't wait for `create_fund`:

1. A mint is created with TransferHook, DefaultAccountState(Frozen), PermanentDelegate,
   MetadataPointer, MintCloseAuthority and the hand-encoded PermissionedBurn, then initialized.
2. An owner's `BurnChecked` on a thawed account with a balance fails.
3. An Ed25519 verify instruction runs in LiteSVM (`precompiles` feature).
4. The empty `fund` program can be invoked.

If any of these can't be made to work, switch to configuration A and record it in research R8.
`tests/tests/mint_config.rs` (T030) later checks that `create_fund` produces the same configuration
with the authorities in the data model.

## Requirement → test map

Every row has at least one rejection test asserting the exact error (Constitution VI). File names
are under `tests/tests/`.

| Requirement | Test file | Rejections asserted |
|---|---|---|
| FR-001, FR-002, FR-007 | `us1_launch.rs` | — (reads back terms, issuer, roles, rules, status) |
| FR-003 | `us1_launch.rs` | `InvalidFundId`, `InvalidCashMint`, `ZeroTargetNav`, `ZeroMaxNavMove`, `InvalidRedemptionGate`, `InvalidDistributionMode`, `ZeroDistributionCadence`, `MissingTermsHash`, `MissingTermsUri`, `InvalidName`, `InvalidSymbol`, `RoleRegistryMismatch`, `Unauthorized` |
| FR-004, FR-035 | `mint_config.rs`, `direct_token_calls.rs` | Token-2022 `MintTo`, `SetAuthority`, `UpdateTransferHook`, `UpdateDefaultAccountState`, metadata `UpdateField` from the issuer's key; owner `Burn`/`BurnChecked` |
| FR-005, FR-006 | `us1_launch.rs` | `Unauthorized`, `FundNotInSetup`, `RolesIncomplete`, `RegisterNotInitialized`, `NoKycProvider`, `NoAllowedJurisdiction`; `init_roles` without the mint signature; `FundAlreadyExists` |
| FR-008, FR-009 | `us1_launch.rs` | `DuplicateProvider`, `UnknownJurisdiction`, `FundNotInSetup` (rules and roles after Open) |
| FR-010 | `us4_roles.rs` | `RoleUnassigned`, `KeeperNotSeparate` (each other role, incl. issuer), `ComplianceIsIssuer`; fund admin = transfer agent accepted |
| FR-011 | `us4_roles.rs` | every role-gated instruction from every other role and the issuer → `Unauthorized` |
| FR-012 | `us4_roles.rs` | Squads 2-of-3 compliance: 1 approval → `InvalidProposalStatus` (6008) and holder unchanged; 2 approvals → done |
| FR-013, FR-014 | `us2_onboarding.rs` | `FundNotOpen`, `InvalidClass`, `Unauthorized`, `SelfRegistrationClassNotAllowed`, `ClassNotAvailable`, `NotSystemControlled`, `HolderAlreadyRegistered` (incl. after revoke) |
| FR-015 | `us2_onboarding.rs` | Pending holder's account → `AccountFrozen`; second non-ATA account stays frozen; thaw `HolderNotApproved`, `NotHoldingAccount`, `AlreadyThawed` |
| FR-016, FR-017 | `us2_onboarding.rs` | `Unauthorized`, `FundNotOpen`, `HolderRevoked`, `InvalidHolderStatus`, `NotInvestor`, and all 12 statement errors in [kyc-statement.md](contracts/kyc-statement.md), incl. a fake Instructions sysvar, offsets into another instruction, zero signatures, and a swapped statement under Squads (`StatementHashMismatch`) |
| FR-018 | `us3_suspend_pause.rs` | `RenewalNotNewer`; renewing a Suspended holder → `InvalidHolderStatus` |
| FR-019 | `us2_onboarding.rs` | `NotFundAccount`, `NotSystemControlled` |
| FR-020 | `hook_order.rs` | transfer built by hand (no SDK); direct call to `execute` → `NotTransferring`; `WrongMint` |
| FR-021 | `hook_order.rs` | `TransfersPaused` |
| FR-022 | `hook_order.rs` | `SenderNotApproved` for suspended, revoked, KYC-expired and missing record |
| FR-023 | `hook_order.rs` | `ReceiverNotApproved` for suspended, revoked, KYC-expired. Jurisdiction-not-allowed and disallowed class can't be reached through instructions in F001 (rules are fixed after Open), so those two tests patch the `HolderState` with `set_account` to prove the defensive check fires — the only tests that edit program state directly |
| FR-024 | `hook_order.rs` | `HolderCapReached`; existing holder can still receive; place frees at zero; treasury not counted; no cap = no limit |
| FR-025 | `us3_suspend_pause.rs` | at `now == expiry` → rejected; at `expiry − 1` → accepted |
| FR-026 | `hook_order.rs` | pairs and triples of failing conditions → the first error in the documented order, including Token-2022 `AccountFrozen` before any hook error |
| FR-027–FR-029 | `us3_suspend_pause.rs` | `InvalidHolderStatus`, `HolderRevoked`, `UnauthorizedCaller` (calling `set_status` directly) |
| FR-030 | `us3_suspend_pause.rs` | `AlreadyPaused`, `NotPaused`; all admin actions succeed while paused |
| FR-031 | `us3_suspend_pause.rs` | `InvalidReason` for codes 4, 5 and ≥ 7 |
| FR-032, SC-002 | `us3_suspend_pause.rs` | the action and a transfer in consecutive transactions with no clock change |
| FR-033, FR-034, SC-003 | `us3_suspend_pause.rs` | every listed action emits its event; a holder's status history rebuilt from events equals the actual sequence |
| SC-001, SC-007 (I3, I10) | `invariants_prop.rs` | `proptest` over random sequences of register / approve / renew / suspend / reinstate / revoke / pause / unpause / transfer / clock warp, compared with a reference model after every step |
| SC-005 | `us2_onboarding.rs` | exactly two transactions: the institution's registration, then compliance's `[create ATA, ed25519, approve_investor, thaw]`; then a transfer to it succeeds |
| SC-006 | `scale.rs` | hook compute units identical at 10 and 10 000 holders; ≤ 60 000 CU |

## Devnet run (R16)

`scripts/devnet-e2e` (Rust binary using `mmf-sdk`), against a devnet wallet with ~8 SOL:

1. `anchor deploy --provider.cluster devnet`.
2. Move each program's upgrade authority to the issuer's Squads vault with a time lock (≥ 1 h on
   devnet; 24 h on mainnet). Program upgrade is a destructive power (Constitution VIII), and Squads
   proposals are visible and cancellable until they execute.
3. `init_roles` + `create_fund` (one transaction) → `init_registry` → `set_rules` (one mock provider,
   allow `US`, `GB`; cap 2) → `set_roles` → `launch`.
4. Institution A registers itself; compliance approves with a mock statement and thaws its ATA.
   Repeat for B. Register and approve the fund treasury.
5. Zero-unit `transfer_checked` A → B: **succeeds** and logs `TransferValidated`.
6. Suspend B → zero-unit A → B: **fails `ReceiverNotApproved`**. Reinstate B → succeeds again.
7. Pause → zero-unit A → B: **fails `TransfersPaused`**. Unpause → succeeds.
8. Unregistered C: zero-unit A → C: **fails `AccountFrozen`**.
9. Revoke B → `approve_investor` for B: **fails `HolderRevoked`**.
10. Print every event for A and B from the transaction history, and the status history rebuilt from
    those events.

**Expected**: each step prints its signature and outcome; the run exits 0 only if every outcome
matches. Transfers with units are covered by the LiteSVM suite, because F001 can't create units on
a real cluster (FR-035).
