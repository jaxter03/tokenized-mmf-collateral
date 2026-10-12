---
description: "Task list for F001: Fund Launch, Roles and Holder Onboarding"
---

# Tasks: Fund Launch, Roles and Holder Onboarding

**Input**: Design documents from `specs/001-fund-launch-onboarding/`: [plan.md](plan.md),
[spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md),
[contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: REQUIRED. Constitution Principle VI says every rule needs a test that is written and seen
to fail before its code, with at least one rejection test asserting the exact error. SC-004 says
the same. Within each story the order is: **interface stubs → tests (must fail) → implementation**.
Stub handlers return `Ok(())` and do nothing, so happy-path tests fail on missing state and
rejection tests fail on a missing error.

**Organization**: by user story, in the spec's priority order (US1, US2 are P1; US3, US4 are P2).
The stories build on each other: a holder needs a launched fund, and suspension needs a holder.
Each story phase still ends with a checkpoint where that story's independent test passes.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: US1–US4, from spec.md
- Paths are from the repository root

## Path conventions (from plan.md)

- Programs: `programs/{fund,compliance_hook,governance}/src/`
- Shared crate: `crates/mmf-common/src/`; client SDK: `crates/mmf-sdk/src/`
- LiteSVM tests: `tests/src/` (harness), `tests/tests/` (test files), `tests/fixtures/`
- Scripts: `scripts/`

---

## Phase 1: Setup (shared infrastructure)

**Purpose**: an empty, building workspace with fixtures and CI.

- [X] T001 Create the workspace root: `Anchor.toml` (programs `fund`, `compliance_hook`, `governance` under `[programs.localnet]` and `[programs.devnet]`; provider cluster localnet), root `Cargo.toml` (`members = ["programs/*", "crates/*", "tests", "scripts/devnet-e2e"]`, `resolver = "2"`, `[profile.release] overflow-checks = true`), `rust-toolchain.toml` (`channel = "1.90"`), and add `target/`, `.anchor/`, `test-ledger/`, `**/*-keypair.json` to `.gitignore`. Program keypairs must never be committed; the repository is public (plan "Deployment and authorities")
- [X] T002 [P] Create the three program crates `programs/fund/`, `programs/compliance_hook/`, `programs/governance/`, each with `Cargo.toml` (`crate-type = ["cdylib", "lib"]`; features `no-entrypoint`, `cpi`, `idl-build`; `anchor-lang = { version = "=1.1.2", features = ["event-cpi"] }`; `anchor-spl = { version = "=1.1.2", features = ["token_2022", "token_2022_extensions", "associated_token"] }`) and `src/lib.rs` with a `declare_id!` placeholder and an empty `#[program]` module. `compliance_hook` also depends on `spl-transfer-hook-interface = "2.1.0"`, `spl-tlv-account-resolution = "0.11.4"` and `sha2`. `governance` depends on `compliance_hook` with features `["cpi", "no-entrypoint"]`. This is the only program-to-program crate dependency (research R12)
- [X] T003 [P] Create `crates/mmf-common/` (`Cargo.toml` depending on `anchor-lang = "=1.1.2"` for `Pubkey`/Borsh and `sha2`; no program crates) with an empty `src/lib.rs`
- [X] T004 [P] Create `crates/mmf-sdk/` (`Cargo.toml` depending on `mmf-common`, the three program crates with `no-entrypoint`, `spl-transfer-hook-interface = "2.1.0"`, `solana-ed25519-program = "3.0.0"`, `ed25519-dalek`) with an empty `src/lib.rs`
- [X] T005 [P] Create the test crate `tests/Cargo.toml` (`litesvm = { version = "=0.12.0", features = ["precompiles"] }`, `proptest`, `mmf-common`, `mmf-sdk`, the three program crates with `no-entrypoint`) and an empty `tests/src/lib.rs`
- [X] T006 [P] Create `scripts/devnet-e2e/` (`Cargo.toml` depending on `mmf-sdk`; `src/main.rs` that prints usage and exits 0)
- [X] T007 Run `anchor keys sync` to generate the three program keypairs under `target/deploy/` (not committed). Write the IDs into each program's `declare_id!` and into `crates/mmf-common/src/ids.rs` as `FUND_PROGRAM_ID`, `COMPLIANCE_HOOK_PROGRAM_ID`, `GOVERNANCE_PROGRAM_ID`, plus `TOKEN_2022_PROGRAM_ID`, `ASSOCIATED_TOKEN_PROGRAM_ID`, `ED25519_PROGRAM_ID` and `SQUADS_PROGRAM_ID = SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf`. Confirm `anchor build` succeeds (depends on T002, T003)
- [X] T008 [P] Dump the test fixtures from **mainnet** into `tests/fixtures/`, using the commands in [quickstart.md](quickstart.md#test-fixtures-once-committed-under-testsfixtures): `spl_token_2022.so`, `squads_multisig_program.so`, `squads_program_config.json`. Check the Token-2022 SHA-256 against the `program@v11.0.0` release hash (starts `7ea94a02`). Record the full hashes and dump date in `tests/fixtures/README.md`
- [X] T009 [P] Write `scripts/check-error-coverage.sh`. It extracts every variant name from the `#[error_code]` enums in `programs/*/src/errors.rs` and exits non-zero, listing each variant that no file in `tests/tests/` references (SC-004)
- [X] T010 [P] Add `.github/workflows/ci.yml`: install Anchor CLI 1.1.2 via avm; `anchor build`; `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test -p mmf-common`; `cargo test -p tests`; `scripts/check-error-coverage.sh`; and `cargo tree -d` failing on duplicate `solana-address` versions (research R1)

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: prove the toolchain works, then build the shared types, account layouts and test
harness that every story uses.

**⚠️ CRITICAL**: T011 must pass before any other Phase 2 task, and no story work starts until
Phase 2 is complete.

- [X] T011 **Toolchain spike (research R3, R8).** In `crates/mmf-common/src/permissioned_burn.rs`, record the PermissionedBurn encoding: instruction tag 46, sub-instruction 0 = Initialize followed by the 32-byte authority, plus the extension's `ExtensionType` number and TLV data size, all read from the `spl-token-2022-interface` 3.1.2 source. Then write `tests/tests/spike_toolchain.rs`, which in LiteSVM 0.12 (`precompiles`), with `tests/fixtures/spl_token_2022.so` loaded over the bundled Token-2022, does the following using raw Token-2022 instructions:
  - (a) creates a mint with TransferHook, DefaultAccountState(Frozen), PermanentDelegate, MetadataPointer, MintCloseAuthority and the hand-encoded PermissionedBurn, then `InitializeMint2`;
  - (b) creates and thaws an ATA, patches its balance, and asserts that an owner `BurnChecked` fails;
  - (c) runs one Ed25519 verify instruction successfully;
  - (d) invokes the empty `fund` program.

  **If any step can't be made to pass**, switch to configuration A: `rust-toolchain.toml` ≥ 1.97.1, `anchor-lang`/`anchor-spl` `=1.2.1` with `solana-v4`, `litesvm = "=0.18.0"`. Rerun, then write the outcome under "Spike outcome" in [research.md](research.md) R8
- [X] T012 [P] Add the shared enums to `crates/mmf-common/src/enums.rs`:
  - `FundStatus` "0 Setup, 1 Open, 2 Gated, 3 Suspended, 4 WindingDown, 5 Closed";
  - `HolderStatus` "0 Pending, 1 Approved, 2 Suspended, 3 Revoked";
  - `HolderClass` "0 Investor, 1 FundTreasury, 2 Escrow, 3 CollateralVault [backlog]";
  - `ReasonCode` "0 CourtOrder, 1 SanctionsHit, 2 LostKey, 3 OperationalError, 4 CollateralDefault, 5 IssuerWindDown, 6 Other", with `allowed_in_f001()` true only for 0, 1, 2, 3 and 6;
  - `StatusAction { Suspend, Reinstate, Revoke }`.

  Add every PDA seed to `crates/mmf-common/src/seeds.rs`: `"fund"`, `"roles"`, `"registry"`, `"holder"`, `"extra-account-metas"`, `"delegate"`, `"token-admin"`, `"hook-admin"`, `"treasury"`
- [X] T013 [P] Write the jurisdiction support in `crates/mmf-common/src/iso3166.rs` and `crates/mmf-common/src/jurisdiction.rs`:
  - embed the 249 assigned ISO 3166-1 alpha-2 codes from Debian `iso-codes` `data/iso_3166-1.json` v4.20.1;
  - `index(c1, c2) = (c1 − 'A') × 26 + (c2 − 'A')`, range 0..675;
  - `ASSIGNED_ISO3166: [u64; 11]` 676-bit constant, where bit `i` lives in word `i / 64` at bit `i % 64`;
  - helpers `is_assigned([u8;2])`, `set_contains`, `set_insert`, `valid_set` (false if any bit ≥ 676 is set or any set bit isn't an assigned code).

  Unit tests: the count is exactly 249; `US`, `GB`, `SS` are assigned; `AA`, `ZZ`, lowercase `us` and non-letters are rejected
- [X] T014 [P] Write the KYC statement codec in `crates/mmf-common/src/statement.rs`, a fixed 95-byte message: "domain [u8; 20] b\"MMF-KYC-STATEMENT-V1\" | mint Pubkey | owner Pubkey | jurisdiction [u8; 2] | investor_type u8 | expiry_ts i64 LE". Provide `encode`, `parse` (errors: wrong length, wrong domain) and `fingerprint` (SHA-256 of the 95 bytes). Unit tests: round-trip and each parse error
- [X] T015 [P] Write the fund account and its events and errors in `programs/fund/src/state.rs`, `errors.rs` and `events.rs`:
  - `FundState` in its full final layout per [data-model.md](data-model.md): "version u8 = 1, mint, role_registry, terms FundTerms, status u8", the F002–F006 fields from `docs/shared-model.md` §4.2 left zero (`nav u64`, `nav_ts i64`, `nav_seq u64`, `accrual_index u128`, `daily_rate_e12 u64`, `units_outstanding u64`, `redeemed_today u64`, `day_start_ts i64`, `cash_treasury Pubkey`, `current_cycle u32`), then `bump u8`, `_reserved [u8; 64]`;
  - `FundTerms`: `fund_id [u8;12]`, `cash_mint`, `target_nav u64`, `max_nav_move_bps u16`, `redemption_gate_bps u16`, `distribution_mode u8`, `distribution_cadence_days u16`, `terms_hash [u8;32]`, `terms_uri [u8;128]`;
  - every error in [contracts/fund.md](contracts/fund.md);
  - events `FundCreated` and `FundLaunched`, each carrying `fund`, `slot`, `unix_ts`
- [X] T016 [P] Write the governance account and its events and errors in `programs/governance/src/state.rs`, `errors.rs` and `events.rs`:
  - `RoleRegistry`: "version, fund_state, issuer, fund_admin, transfer_agent, compliance, keeper, auditor, timelock_secs u32 (zero, F007), action_count u64 (zero, F007), bump, _reserved [u8; 64]";
  - errors `Unauthorized`, `FundNotInSetup`, `FundNotOpen`, `FundAlreadyExists`, `RoleUnassigned`, `KeeperNotSeparate`, `ComplianceIsIssuer`, `InvalidReason`;
  - events `RolesInitialized` and `RolesAssigned`
- [X] T017 [P] Write the compliance_hook accounts, predicates, events and errors in `programs/compliance_hook/src/state.rs`, `errors.rs` and `events.rs`:
  - `Registry`, with "allowed_jurisdictions [u64; 11] 676-bit set", "max_holders u32 0 = no cap" and `holder_count u32`;
  - `HolderState`, with "jurisdiction [u8; 2] ISO 3166-1 alpha-2, ASCII; zero for fund-operated accounts", "kyc_expiry_ts i64 investors: from the statement; fund-operated: i64::MAX", `last_balance u64`, `last_slot u64`, and every F002/F004/F006 field left zero, laid out per [data-model.md](data-model.md);
  - predicates `kyc_valid(h, now) := now < h.kyc_expiry_ts`, `approved(h, now) := h.status == Approved && kyc_valid(h, now)`, `jurisdiction_ok(h) := h.class != Investor || allowed(registry, h.jurisdiction)`, `class_allowed(h) := h.class ∈ {Investor, FundTreasury, Escrow}`;
  - every error in [contracts/compliance_hook.md](contracts/compliance_hook.md) and [contracts/kyc-statement.md](contracts/kyc-statement.md);
  - the eight events in the data model's events table
- [X] T018 Write read-only views in `crates/mmf-common/src/views.rs`: `FundStateView`, `RoleRegistryView`, `RegistryView`. Each `load(&AccountInfo, expected_address)` checks the owner program ID, the address, and the 8-byte Anchor discriminator (`sha256("account:<Name>")[..8]`), then decodes only the fields other programs read (status, role keys, mint, providers, jurisdictions) (depends on T015–T017)
- [X] T019 Write `tests/tests/layouts.rs`. It asserts each account's serialized size, that `version` is the first byte and `_reserved` the last 64, and that each view decodes a program-serialized account to the same field values. This guards against layout drift (research R12) (depends on T018)
- [X] T020 [P] Write the test harness in `tests/src/harness.rs`: `Harness::new()` builds LiteSVM 0.12 with `precompiles`, loads the three `target/deploy/*.so`, overrides Token-2022 with `tests/fixtures/spl_token_2022.so`, and loads the Squads program and its ProgramConfig account `BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr`. Helpers: `send(ixs, signers) -> Result<TxMeta, TxFailure>`, `airdrop`, `account`
- [X] T021 [P] Write the remaining test helpers in `tests/src/assert.rs`, `tests/src/events.rs` and `tests/src/clock.rs`:
  - `tests/src/assert.rs`: `assert_anchor_error(res, code)`, `assert_token_error(res, TokenError)`, `assert_squads_error(res, 6008)`;
  - `tests/src/events.rs`: decode `emit_cpi!` events from `inner_instructions` (data starts with `EVENT_IX_TAG_LE` `0x1d9acb512ea545e4`, then the 8-byte discriminator, then Borsh), and decode `emit!` events from `Program data:` log lines;
  - `tests/src/clock.rs`: `set_unix_timestamp(&mut svm, ts)` via `get_sysvar::<Clock>` / `set_sysvar`
- [X] T022 [P] Write `crates/mmf-sdk/src/pda.rs`, deriving every PDA: `fund_state(mint)`, `role_registry(fund_state)`, `registry(mint)`, `extra_metas(mint)`, `holder(mint, owner)`, `delegate(mint)`, `token_admin(mint)`, `hook_admin()`, `treasury(fund_state)`, `event_authority(program)`, `holding_account(owner, mint)` (Token-2022 ATA)

**Checkpoint**: the toolchain is proven, layouts are fixed, and the harness runs.

---

## Phase 3: User Story 1 — Launch a fund (Priority: P1) 🎯 MVP part 1

**Goal**: the issuer creates the fund and its token, sets holder rules and roles, and launches it
(FR-001 to FR-009, FR-004/FR-035 mint configuration).

**Independent Test**: create a fund, complete setup, launch it, then read back its terms, rules,
roles and status (spec US1).

### Interface stubs for User Story 1

- [X] T023 [P] [US1] In `programs/governance/src/instructions/init_roles.rs` and `set_roles.rs`, add the `#[derive(Accounts)]` structs exactly as in [contracts/governance.md](contracts/governance.md) (`init_roles`: issuer signer+payer, **mint signer**, unchecked `fund_state`, `role_registry` init at `["roles", fund_state]`; `set_roles`: issuer, role_registry mut, fund_state). Add `#[event_cpi]` and stub handlers returning `Ok(())`, wired in `lib.rs`
- [ ] T024 [P] [US1] In `programs/fund/src/instructions/create_fund.rs` and `launch.rs`, add the Accounts structs per [contracts/fund.md](contracts/fund.md) with `#[event_cpi]` and stub handlers, wired in `programs/fund/src/lib.rs`
- [ ] T025 [P] [US1] In `programs/compliance_hook/src/instructions/init_registry.rs` and `set_rules.rs`, add the Accounts structs per [contracts/compliance_hook.md](contracts/compliance_hook.md) with `#[event_cpi]` and stub handlers, wired in `programs/compliance_hook/src/lib.rs`
- [ ] T026 [US1] Add instruction builders to `crates/mmf-sdk/src/ix_governance.rs`, `ix_fund.rs` and `ix_hook.rs` for `init_roles`, `set_roles`, `create_fund`, `launch`, `init_registry` and `set_rules`. Accounts go in contract order, including the event authority and program accounts (depends on T023–T025)

### Tests for User Story 1 (write first; they must fail) ⚠️

- [ ] T027 [US1] Write the US1 happy path in `tests/tests/us1_launch.rs`:
  - `init_roles` and `create_fund` in one transaction;
  - assert `FundState.terms` equal the input, `status == Setup`, and `RoleRegistry.issuer == creator` (FR-001, FR-002, FR-007);
  - `RolesInitialized` and `FundCreated` events are emitted;
  - then `init_registry`, `set_rules` (one provider, `US`+`GB`, cap 0), `set_roles` (five distinct keys), and `launch`;
  - assert `status == Open` and a `FundLaunched` event
- [ ] T028 [US1] Add `create_fund` rejection tests to `tests/tests/us1_launch.rs`, in contract order, each asserting the exact error and that no `FundState` or mint was created:
  - `RoleRegistryMismatch`, `Unauthorized`;
  - `InvalidFundId` (all zero);
  - `InvalidCashMint` (not a mint; a mint with 9 decimals);
  - `ZeroTargetNav`, `ZeroMaxNavMove`;
  - `InvalidRedemptionGate` (0 and 10 001);
  - `InvalidDistributionMode` (2), `ZeroDistributionCadence`, `MissingTermsHash`;
  - `MissingTermsUri` (all zero; invalid UTF-8);
  - `InvalidName` (empty; 33 bytes);
  - `InvalidSymbol` (empty; 11 bytes)
- [ ] T029 [US1] Add setup and launch rejection tests to `tests/tests/us1_launch.rs`:
  - `init_roles`: without the mint signature (Anchor `AccountNotSigner`); `FundAlreadyExists`;
  - `init_registry`: `Unauthorized`, `FundNotInSetup`;
  - `set_rules`: `Unauthorized`, `FundNotInSetup` after Open (FR-009), `DuplicateProvider`, `UnknownJurisdiction` (unassigned `AA` bit; a bit ≥ 676);
  - `set_roles`: `Unauthorized`, `RoleUnassigned`, `FundNotInSetup` after Open (FR-009);
  - `launch`: `Unauthorized` (spec US1-3), `FundNotInSetup` on relaunch (US1-6), `RolesIncomplete`, `RegisterNotInitialized`, `NoKycProvider`, `NoAllowedJurisdiction` (US1-4)
- [ ] T030 [P] [US1] Write `tests/tests/mint_config.rs`. After `create_fund`, decode the mint and assert:
  - TransferHook program = `compliance_hook`, authority = `token_admin` PDA;
  - DefaultAccountState = Frozen;
  - PermanentDelegate = `delegate` PDA;
  - MetadataPointer → mint, authority `FundState`;
  - token metadata name, symbol, uri = `terms_uri`, fields `fund_id` and `terms_hash` in hex, update authority `FundState`;
  - MintCloseAuthority = `FundState`;
  - PermissionedBurn authority = `FundState`;
  - mint authority `FundState`, freeze authority `Registry` PDA, decimals 6
- [ ] T031 [P] [US1] Write `tests/tests/direct_token_calls.rs`. Raw Token-2022 instructions signed by the issuer's key must all fail (FR-004, Principle II): `MintTo`; `SetAuthority` for each mint authority type; `UpdateTransferHook`; `UpdateDefaultAccountState`; metadata `UpdateField`

### Implementation for User Story 1

- [ ] T032 [P] [US1] Implement `init_roles` in `programs/governance/src/instructions/init_roles.rs`. `fund_state` must equal `PDA(fund, ["fund", mint])` and hold no data, else `FundAlreadyExists`. Create `RoleRegistry { version: 1, fund_state, issuer: signer, other roles default }` and emit `RolesInitialized`
- [ ] T033 [P] [US1] Implement `set_roles` (basic) in `programs/governance/src/instructions/set_roles.rs`. Checks, in order: `Unauthorized` (signer ≠ `issuer`), `FundNotInSetup` (`FundStateView.status != Setup`), `RoleUnassigned` (any default pubkey). Replace the five roles and emit `RolesAssigned`. Separation rules come in T074
- [ ] T034 [P] [US1] Implement the mint setup CPIs in `programs/fund/src/token_setup.rs`, in this exact order (research R2):
  1. `create_account` with length = `try_calculate_account_len::<Mint>(&[TransferHook, DefaultAccountState, PermanentDelegate, MetadataPointer, MintCloseAuthority])` + the PermissionedBurn TLV size from `mmf-common::permissioned_burn`;
  2. `transfer_hook_initialize` (authority `token_admin` PDA, program `compliance_hook`);
  3. `default_account_state_initialize(Frozen)`;
  4. `permanent_delegate_initialize(delegate PDA)`;
  5. `metadata_pointer_initialize` (authority `FundState`, address = mint);
  6. `mint_close_authority_initialize(FundState)`;
  7. hand-encoded PermissionedBurn initialize (authority `FundState`);
  8. `initialize_mint2` (decimals 6, mint authority `FundState`, freeze authority `Registry` PDA);
  9. transfer lamports for the final size, including metadata;
  10. `token_metadata_initialize` (name, symbol, uri, update authority `FundState`, signed with `FundState` seeds);
  11. `token_metadata_update_field` for `fund_id` (hex), then `terms_hash` (hex)
- [ ] T035 [US1] Implement `create_fund` in `programs/fund/src/instructions/create_fund.rs`.
  - Check, in order: `RoleRegistryMismatch` (owner governance, address `["roles", fund_state]`, `role_registry.fund_state == fund_state`), then `Unauthorized` (signer ≠ issuer).
  - Then validate terms in this order: "fund_id not all zero → InvalidFundId"; "cash_mint an initialized SPL Token or Token-2022 mint with 6 decimals → InvalidCashMint"; "target_nav > 0 → ZeroTargetNav"; "max_nav_move_bps > 0 → ZeroMaxNavMove"; "redemption_gate_bps 1 ..= 10 000 → InvalidRedemptionGate"; "distribution_mode 0 Reinvest or 1 Cash → InvalidDistributionMode"; "distribution_cadence_days > 0 → ZeroDistributionCadence"; "terms_hash not all zero → MissingTermsHash"; "terms_uri at least one non-zero byte, zero-padded, valid UTF-8 → MissingTermsUri"; "name non-empty, ≤ 32 bytes → InvalidName"; "symbol non-empty, ≤ 10 bytes → InvalidSymbol".
  - Then call `token_setup`, create `FundState` (status Setup, `role_registry`), and emit `FundCreated` (depends on T034)
- [ ] T036 [P] [US1] Implement `launch` in `programs/fund/src/instructions/launch.rs`. Checks, in order: `Unauthorized`; `FundNotInSetup`; `RolesIncomplete` (any of the five roles default, via `RoleRegistryView`); `RegisterNotInitialized` (`RegistryView` and the extra-account-metas account owned by `compliance_hook` with data); `NoKycProvider`; `NoAllowedJurisdiction`. Set `status = Open` and emit `FundLaunched`
- [ ] T037 [P] [US1] Implement `init_registry` and `programs/compliance_hook/src/extra_metas.rs`.
  - Checks: `Unauthorized` (signer ≠ `RoleRegistryView.issuer`), `FundNotInSetup`.
  - Create the `Registry`, and the `ExtraAccountMetaList` with exactly these five entries in order:
    1. Registry, literal, **writable**;
    2. FundState, literal, read-only;
    3. HolderState, seeds `[Literal "holder", AccountKey{1}, AccountData{account_index: 0, data_index: 32, length: 32}]`, writable;
    4. the same with `account_index: 2`, writable;
    5. governance `["delegate", mint]` PDA, literal, read-only.
  - Emit `RegistryInitialized`
- [ ] T038 [P] [US1] Implement `set_rules` in `programs/compliance_hook/src/instructions/set_rules.rs`. Checks, in order: `Unauthorized`; `FundNotInSetup`; `DuplicateProvider` (non-default providers must be distinct); `UnknownJurisdiction` (`mmf_common::jurisdiction::valid_set`). Replace the rules and emit `RegisterRulesSet`
- [ ] T039 [US1] Add `launched_fund(&mut Harness, cap: u32) -> FundCtx` to `tests/src/fixtures.rs`. It returns the issuer, mint, five distinct role keypairs and one mock KYC provider key, with rules allowing `US` and `GB`. Then run `cargo test -p tests --test us1_launch --test mint_config --test direct_token_calls --test layouts` and confirm all pass

**Checkpoint**: US1's independent test passes. A fund can be created, configured and launched,
and no key can mint, freeze or reconfigure its token.

---

## Phase 4: User Story 2 — Approve holders (Priority: P1) 🎯 MVP part 2

**Goal**: institutions register, compliance approves them with a provider-signed statement, and
the hook allows only approved holders to send or receive (FR-013 to FR-017, FR-019, FR-015, FR-020
to FR-026, FR-035 owner burn).

**Independent Test**: register an institution, approve it with a valid statement, send it units;
show every invalid statement and every unapproved receiver is rejected (spec US2).

### Interface stubs for User Story 2

- [ ] T040 [US2] Add Accounts structs and stub handlers per [contracts/compliance_hook.md](contracts/compliance_hook.md) in `programs/compliance_hook/src/instructions/`:
  - `register_holder.rs`: `holder_state` as an `UncheckedAccount`, created by hand;
  - `approve_investor.rs`: arg `attestation_hash: [u8; 32]`, plus the `instructions` sysvar;
  - `approve_fund_account.rs`;
  - `thaw_holding_account.rs`.

  Also add `programs/compliance_hook/src/hook/execute.rs` with `#[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)] pub fn execute(ctx, amount: u64)`. Its accounts: source, mint, destination, authority, extra-metas, then the five extras, with both `HolderState` entries as `UncheckedAccount` (research R4). Wire everything in `lib.rs`
- [ ] T041 [P] [US2] Write `crates/mmf-sdk/src/kyc.rs`. `MockProvider { keypair }::statement(mint, owner, jurisdiction, investor_type, expiry_ts) -> (message [u8; 95], ed25519 Instruction, attestation_hash)`, using `mmf_common::statement::encode` and `solana_ed25519_program::new_ed25519_instruction_with_signature`. Add tamper helpers for tests: a statement with two signatures, offsets into another instruction, zero signatures, and a wrong length
- [ ] T042 [P] [US2] Write `crates/mmf-sdk/src/transfer.rs`. `transfer_checked_with_hook(...)` resolves the extra accounts via `spl_transfer_hook_interface::offchain::add_extra_account_metas_for_execute`. `transfer_checked_manual(...)` builds the same account list by hand with `pda.rs`, with no offchain helper (FR-020)
- [ ] T043 [US2] Add builders to `crates/mmf-sdk/src/ix_hook.rs` for `register_holder`, `approve_investor`, `approve_fund_account` and `thaw_holding_account`. Add `onboarding_tx(owner, provider, …) -> Vec<Instruction>` returning `[create ATA idempotent, ed25519, approve_investor, thaw_holding_account]` (depends on T040)
- [ ] T044 [US2] Write test helpers in `tests/src/units.rs` and `tests/src/fixtures.rs`:
  - `tests/src/units.rs`: `place_units(svm, treasury_owner, amount)` patches the approved, thawed treasury ATA's `amount` and the mint's `supply` via `StateWithExtensionsMut`. These are the only edits to token state (research R8);
  - `tests/src/fixtures.rs`: `onboard_investor(ctx, owner, jurisdiction)` and `onboard_treasury(ctx)`

### Tests for User Story 2 (write first; they must fail) ⚠️

- [ ] T045 [US2] Write registration tests in `tests/tests/us2_onboarding.rs`:
  - an institution registers itself → Pending, `HolderRegistered` (US2-1);
  - compliance registers an investor and a treasury at `["treasury", fund_state]`;
  - rejections: `FundNotOpen` (fund in Setup); `InvalidClass` (3); `Unauthorized` (a third party registers someone else); `SelfRegistrationClassNotAllowed` (self as treasury); `ClassNotAvailable` (escrow by compliance); `NotSystemControlled` (treasury with any other owner); `HolderAlreadyRegistered`, with the existing record byte-for-byte unchanged (US2-8, FR-014). The institution registering a different owner is covered by US2-9
- [ ] T046 [US2] Add `approve_investor` tests to `tests/tests/us2_onboarding.rs`.
  - Happy path: Approved; stores jurisdiction, investor_type, kyc_expiry_ts, attestation_hash = SHA-256 of the message, attested_by (FR-017); `HolderApproved { renewal: false }`.
  - Rejections in contract order: `Unauthorized`; `InvalidHolderStatus` (already Approved); `NotInvestor` (treasury).
  - Statement rejections:
    - `StatementMissing`: no ed25519 instruction; ed25519 not immediately before; fake instructions-sysvar account;
    - `StatementMalformed`: two signatures; offsets into another instruction; zero signatures; 94-byte message; wrong domain;
    - `ProviderNotRecognised` (US2-3); `StatementWrongFund`; `StatementOwnerMismatch` (US2-5);
    - `StatementExpired`: `expiry_ts == now` (US2-4);
    - `UnknownJurisdiction` (`AA`); `JurisdictionNotAllowed` (`FR`, US2-6); `InvalidInvestorType` (0); `StatementHashMismatch`.
  - Each rejection leaves the record unchanged
- [ ] T047 [US2] Add fund-account, thaw and onboarding-count tests to `tests/tests/us2_onboarding.rs`:
  - `approve_fund_account`: happy path (`kyc_expiry_ts == i64::MAX`); `Unauthorized`; `InvalidHolderStatus`; `NotFundAccount` (investor); `NotSystemControlled`, by patching the record's owner with `set_account` (unreachable state, documented exception);
  - `thaw_holding_account`: happy path + `HoldingAccountThawed`; `HolderNotApproved` (Pending; KYC expired); `NotHoldingAccount` (a non-ATA account of the owner); `AlreadyThawed`;
  - a second, non-ATA account of an approved owner stays frozen, and a transfer to it fails `AccountFrozen` (FR-015);
  - SC-005: exactly two transactions (the institution's `register_holder`, then compliance's `onboarding_tx`) make an institution able to receive units
- [ ] T048 [P] [US2] Write transfer rule tests in `tests/tests/hook_order.rs`:
  - with `place_units`, treasury → A → B (both approved) succeeds, balances move, `TransferValidated` is logged;
  - to a Pending or unregistered owner → Token-2022 `AccountFrozen` (US2-7), no balance change;
  - KYC boundaries (FR-025): sender at `now == expiry` → `SenderNotApproved`; at `expiry − 1` → OK; receiver at `now == expiry` → `ReceiverNotApproved`;
  - `ReceiverNotApproved` for a disallowed jurisdiction and for class 3, and `WrongMint` (Registry `mint` patched), by patching state with `set_account` (documented exception, quickstart FR-023);
  - calling `execute` directly → `NotTransferring`;
  - zero-unit transfers run the checks: to KYC-expired → `ReceiverNotApproved`; to approved → OK;
  - a transfer from an account to itself → succeeds, nothing changes, no `TransferValidated` (spec edge case, R4);
  - a transfer built with `transfer_checked_manual` passes the same checks (FR-020)
- [ ] T049 [US2] Add holder cap and order tests to `tests/tests/hook_order.rs`:
  - with cap 2, A and B holding, a transfer to new C → `HolderCapReached`;
  - A can still receive more;
  - B sends everything to A → `holder_count` drops to 1, and C can now receive;
  - the treasury is never counted; cap 0 means no limit;
  - assert `holder_count` and each `last_balance` after every step (FR-024, I10);
  - FR-026 order: frozen receiver plus KYC-expired sender → `AccountFrozen`; expired sender plus expired receiver → `SenderNotApproved`; expired receiver at cap → `ReceiverNotApproved`
- [ ] T050 [P] [US2] Add owner-burn tests to `tests/tests/direct_token_calls.rs`: an approved investor holding units calls `Burn` and `BurnChecked` on its thawed ATA; both fail and its balance is unchanged (FR-035, R3)

### Implementation for User Story 2

- [ ] T051 [P] [US2] Implement statement verification in `programs/compliance_hook/src/statement.rs`, applying [contracts/kyc-statement.md](contracts/kyc-statement.md) checks 1–12 in order:
  1. the `instructions` account key is the Instructions sysvar, and the instruction at `load_current_index − 1` is owned by the Ed25519 program;
  2. `num_signatures == 1`; all three `*_instruction_index == u16::MAX`; offsets and sizes stay inside the instruction's data; message length 95;
  3. the domain;
  4. the pubkey is a non-default entry in `registry.kyc_providers`;
  5. mint; 6. owner; 7. `expiry_ts > now`; 8. assigned jurisdiction; 9. allowed jurisdiction; 10. `investor_type != 0`;
  11. renewal only: `expiry_ts > holder.kyc_expiry_ts`;
  12. `fingerprint == attestation_hash`.

  Return the parsed statement
- [ ] T052 [P] [US2] Implement `register_holder` in `programs/compliance_hook/src/instructions/register_holder.rs`. Checks, in order: `FundNotOpen`; `InvalidClass`; `Unauthorized` (authority is neither compliance nor `owner`); `SelfRegistrationClassNotAllowed`; `ClassNotAvailable` (Escrow); `NotSystemControlled` (FundTreasury owner ≠ `PDA(fund, ["treasury", fund_state])`); `HolderAlreadyRegistered` (account has data). Create the PDA `["holder", mint, owner]` via system CPI, paid by `payer`. Write `HolderState { version 1, status Pending, class, owner, mint }` and emit `HolderRegistered`
- [ ] T053 [US2] Implement `approve_investor` in `programs/compliance_hook/src/instructions/approve_investor.rs`. Checks, in order: `Unauthorized`; `FundNotOpen`; `HolderRevoked`; `InvalidHolderStatus` (not Pending); `NotInvestor`; then `statement::verify(renewal = false)`. Set status Approved; store jurisdiction, investor_type, kyc_expiry_ts, attestation_hash and attested_by; emit `HolderApproved { renewal: false }` (depends on T051)
- [ ] T054 [P] [US2] Implement `approve_fund_account` in `programs/compliance_hook/src/instructions/approve_fund_account.rs`. Checks, in order: `Unauthorized`; `FundNotOpen`; `HolderRevoked`; `InvalidHolderStatus`; `NotFundAccount`; `NotSystemControlled`. Set status Approved and `kyc_expiry_ts = i64::MAX`, leaving the jurisdiction, investor_type and attestation fields zero. Emit `HolderApproved { renewal: false }`
- [ ] T055 [P] [US2] Implement `thaw_holding_account` (permissionless) in `programs/compliance_hook/src/instructions/thaw_holding_account.rs`. Checks, in order: `HolderNotApproved` (`!approved(h, now)`); `NotHoldingAccount` (token account ≠ ATA(owner, mint, Token-2022)); `AlreadyThawed`. Then Token-2022 `ThawAccount`, signed with the `Registry` seeds `["registry", mint, bump]`, and emit `HoldingAccountThawed`
- [ ] T056 [P] [US2] Implement the hook in `programs/compliance_hook/src/hook/execute.rs`, following the hook step order in [contracts/compliance_hook.md](contracts/compliance_hook.md):
  - step 0: `transferring` flag set on both token accounts, else `NotTransferring`;
  - step 1: `registry.mint == mint` and the extras at their expected PDAs, else `WrongMint`;
  - step 4: `SenderNotApproved`;
  - step 6: `ReceiverNotApproved`;
  - step 7: `HolderCapReached` when "receiver is an Investor with `last_balance == 0`, `amount > 0`, `max_holders > 0` and `holder_count >= max_holders`";
  - step 9: for each Investor touched, once if both sides have the same owner: "`holder_count += (post_balance > 0) − (last_balance > 0)`", then `last_balance = post_balance`, `last_slot = slot`, with checked math (`Overflow`);
  - step 10: `emit!(TransferValidated)`.

  A missing or foreign `HolderState` counts as not approved. Load the `HolderState` accounts by hand and write each once. Leave step 3 (pause) for T068; steps 2 and 5 are out of scope (F005, F004)

**Checkpoint**: US1 + US2 pass. Only approved holders can hold units, and the fund's core
regulatory promise works (MVP).

---

## Phase 5: User Story 3 — Suspend, revoke and pause (Priority: P2)

**Goal**: compliance can stop a holder (temporarily or for good) and pause all transfers, each with
a reason and a permanent record; KYC renewal (FR-018, FR-021, FR-027 to FR-034).

**Independent Test**: suspend a holder and show it can't send or receive; reinstate it and show it
can; pause and show no transfer succeeds; unpause and show transfers work (spec US3).

### Interface stubs for User Story 3

- [ ] T057 [US3] Add Accounts structs and stub handlers in `programs/compliance_hook/src/instructions/`:
  - `set_status.rs`: `hook_admin` signer, `registry`, `holder_state` mut; args `action, reason, case_ref_hash, actor`;
  - `set_paused.rs`: args `paused, reason, case_ref_hash, actor`;
  - `renew_investor`, with arg `attestation_hash`, in `approve_investor.rs`.

  Wire them in `lib.rs`
- [ ] T058 [US3] Add Accounts structs and stub handlers in `programs/governance/src/instructions/compliance_actions.rs` for `suspend_holder`, `reinstate_holder`, `revoke_holder`, `pause_transfers` and `unpause_transfers`, each taking `(reason: u8, case_ref_hash: [u8; 32])`. Accounts follow [contracts/governance.md](contracts/governance.md): `hook_admin` PDA `["hook-admin"]` and `compliance_hook_program` (depends on T057)
- [ ] T059 [US3] Add builders to `crates/mmf-sdk/src/ix_governance.rs` and `ix_hook.rs` for the five compliance actions, `renew_investor`, and direct `set_status` / `set_paused` calls (the latter for rejection tests)

### Tests for User Story 3 (write first; they must fail) ⚠️

- [ ] T060 [US3] Write status tests in `tests/tests/us3_suspend_pause.rs`:
  - suspend: Approved → Suspended; the holder can neither send (`SenderNotApproved`) nor receive (`ReceiverNotApproved`) (US3-1);
  - reinstate: Suspended → Approved and transfers work again (US3-2); after reinstatement KYC expiry still applies (FR-028);
  - revoke from Pending, Approved and Suspended → Revoked; after revocation, `approve_investor`, `renew_investor` and `reinstate_holder` → `HolderRevoked`, and `register_holder` → `HolderAlreadyRegistered` (US3-3, FR-029);
  - a revoked holder's units stay put and can't move;
  - `InvalidHolderStatus`: suspend a Pending or Suspended holder; reinstate an Approved holder;
  - `Unauthorized` (non-compliance signer); `FundNotOpen` (pause in Setup)
- [ ] T061 [US3] Add reason and pause tests to `tests/tests/us3_suspend_pause.rs`:
  - `InvalidReason` for codes 4, 5, 7 and 255 on each of the five actions (FR-031);
  - pause → every transfer fails `TransfersPaused`, including zero-unit and treasury transfers (US3-4); `AlreadyPaused`; unpause → transfers succeed (US3-5); `NotPaused`;
  - while paused, register, approve, renew, suspend, reinstate and revoke all succeed (FR-030);
  - calling `set_status` / `set_paused` directly with any signer → `UnauthorizedCaller`
- [ ] T062 [US3] Add renewal and timing tests to `tests/tests/us3_suspend_pause.rs`:
  - renew an Approved investor with a later expiry → `HolderApproved { renewal: true }`;
  - `RenewalNotNewer` for equal and earlier expiry; renewing a Suspended holder → `InvalidHolderStatus`;
  - a KYC-expired holder can't send or receive (US3-6), and after renewal it can (US3-7);
  - a jurisdiction change on renewal is re-checked (`JurisdictionNotAllowed`);
  - FR-032 / SC-002: suspend, pause, approve and unpause each take effect for a transfer in the very next transaction, with no clock change
- [ ] T063 [US3] Add record tests to `tests/tests/us3_suspend_pause.rs`:
  - every suspension, reinstatement, revocation, pause and unpause emits an event with `reason`, `case_ref_hash`, `actor`, `fund`, `slot` and `unix_ts` (US3-8, FR-033);
  - for a holder taken through register → approve → suspend → reinstate → renew → revoke, the status history rebuilt from `HolderRegistered`, `HolderApproved` and `HolderStatusChanged` alone equals the actual sequence (FR-034, SC-003)
- [ ] T064 [P] [US3] Add pause and suspension combinations to `tests/tests/hook_order.rs`:
  - FR-026 order: paused + suspended sender → `TransfersPaused`; paused + frozen receiver → `AccountFrozen`; suspended sender + suspended receiver → `SenderNotApproved`;
  - a suspended holder's transfer from its account to itself still succeeds and changes nothing (R4)

### Implementation for User Story 3

- [ ] T065 [P] [US3] Implement `set_status` in `programs/compliance_hook/src/instructions/set_status.rs`. Checks, in order: `UnauthorizedCaller` (signer ≠ governance PDA `["hook-admin"]`); `HolderRevoked`; `InvalidHolderStatus`, applying "Suspend: status Approved · Reinstate: status Suspended · Revoke: status Pending, Approved or Suspended". Change status only; never freeze or thaw the token account (R13). Emit `HolderStatusChanged { owner, from, to, reason, case_ref_hash, actor }`
- [ ] T066 [P] [US3] Implement `set_paused` in `programs/compliance_hook/src/instructions/set_paused.rs`. Checks: `UnauthorizedCaller`, then `AlreadyPaused` / `NotPaused`. Set `registry.transfers_paused` and emit `TransfersPaused { paused, reason, case_ref_hash, actor }`
- [ ] T067 [P] [US3] Implement `renew_investor` in `programs/compliance_hook/src/instructions/approve_investor.rs`. Checks, in order: `Unauthorized`; `FundNotOpen`; `HolderRevoked`; `InvalidHolderStatus` (not Approved); `NotInvestor`; then `statement::verify(renewal = true)`, which includes `RenewalNotNewer`. Update the stored statement fields and emit `HolderApproved { renewal: true }`
- [ ] T068 [P] [US3] Add hook step 3 to `programs/compliance_hook/src/hook/execute.rs`: `if registry.transfers_paused → TransfersPaused`, between step 1 and step 4
- [ ] T069 [US3] Implement the five actions in `programs/governance/src/instructions/compliance_actions.rs`. Checks, in order: `Unauthorized` (signer ≠ `role_registry.compliance`); `FundNotOpen`; `InvalidReason` (`!ReasonCode::allowed_in_f001`). Then CPI `compliance_hook::set_status` or `set_paused`, signed with the `["hook-admin", bump]` seeds, passing `actor = compliance` (depends on T065, T066)

**Checkpoint**: US1–US3 pass. Compliance can react to sanctions hits and incidents immediately, and
every action is on record.

---

## Phase 6: User Story 4 — Roles (Priority: P2)

**Goal**: each action is accepted only from its role holder, roles are separated as clarified, and
a multisig group acts only at its own threshold (FR-010, FR-011, FR-012).

**Independent Test**: give every role to a different party, attempt each role-restricted action
from every party, and show only the right one succeeds (spec US4).

### Test support for User Story 4

- [ ] T070 [US4] Write `tests/src/squads.rs`, hand-encoding the Squads v4 instructions as an Anchor discriminator `sha256("global:<name>")[..8]` followed by Borsh arguments (research R8):
  - `multisig_create_v2`: threshold, members with Vote/Execute permissions, `time_lock = 0`, ProgramConfig treasury;
  - `vault_transaction_create`, with Squads' compact message format;
  - `proposal_create`; `proposal_approve`; `vault_transaction_execute`.

  Add a helper `squads_execute(ixs, approvals) -> Result`

### Tests for User Story 4 (write first; they must fail) ⚠️

- [ ] T071 [US4] Write role separation tests in `tests/tests/us4_roles.rs`:
  - `KeeperNotSeparate` when the keeper equals the issuer, fund_admin, transfer_agent, compliance or auditor (five cases);
  - `ComplianceIsIssuer`;
  - accepted overlaps: fund_admin == transfer_agent (US4-6); auditor == fund_admin;
  - a rejected assignment leaves the existing roles unchanged (US4-5)
- [ ] T072 [US4] Add the permission matrix to `tests/tests/us4_roles.rs` (US4-1, US4-2, FR-011):
  - instructions: issuer-only (`init_registry`, `set_rules`, `set_roles`, `launch`) and compliance-only (`register_holder` for another owner, `approve_investor`, `approve_fund_account`, `renew_investor`, and the five governance actions);
  - signers: the issuer, fund_admin, transfer_agent, compliance, keeper, auditor and an unrelated key;
  - assert `Unauthorized` for every wrong signer, including the issuer for compliance actions, and success for the right one
- [ ] T073 [US4] Add Squads tests to `tests/tests/us4_roles.rs`:
  - compliance = a 2-of-3 Squads vault. A `suspend_holder` proposal with one approval fails on execute with `InvalidProposalStatus` (6008), and the holder is unchanged (US4-3, FR-012); after a second approval, execute succeeds;
  - `[ed25519, vault_transaction_execute(approve_investor(hash))]` succeeds; with a different valid statement in the ed25519 instruction it fails `StatementHashMismatch` (R8)

### Implementation for User Story 4

- [ ] T074 [US4] Add the separation rules to `programs/governance/src/instructions/set_roles.rs`, after `RoleUnassigned`: "keeper differs from issuer, fund_admin, transfer_agent, compliance and auditor → else `KeeperNotSeparate`", then "compliance differs from issuer → else `ComplianceIsIssuer`". Other overlaps stay allowed

**Checkpoint**: all four stories pass on their own and together.

---

## Phase 7: Polish & cross-cutting concerns

- [ ] T075 [P] Write a reference model in `tests/src/model.rs` and `tests/tests/invariants_prop.rs`: a `proptest` over random sequences of register / approve / renew / suspend / reinstate / revoke / pause / unpause / transfer (random amounts, including zero) / clock warp, sent directly to LiteSVM. After every step, compare with the model and assert:
  - SC-001: no successful transfer delivered units to an owner that wasn't `approved` at that moment;
  - I3: only approved holders or treasury hold a non-zero balance;
  - SC-007 / I10: `holder_count` equals the number of Investors with `last_balance > 0`, and each `last_balance` equals its ATA balance;
  - `holder_count ≤ max_holders` when a cap is set
- [ ] T076 [P] Write `tests/tests/scale.rs` (`#[ignore]`, run with `--release -- --ignored`) for SC-006: register and approve 10 holders and measure a hooked transfer's compute units; repeat with 10 000 holders. Assert the two are identical and ≤ 60 000 CU
- [ ] T077 Run `scripts/check-error-coverage.sh` and add any missing rejection test until it reports none (SC-004)
- [ ] T078 [P] Implement `scripts/devnet-e2e/src/main.rs`: steps 3–10 of the [quickstart.md devnet run](quickstart.md#devnet-run-r16). Use zero-unit transfers, print each signature and outcome, and exit 0 only if every outcome matches. Finish by rebuilding A's and B's status history from their events
- [ ] T079 Deploy to devnet (`anchor deploy --provider.cluster devnet`). Move each program's upgrade authority to the issuer's Squads vault with a time lock ≥ 1 h (Constitution VIII; plan "Deployment and authorities"). Run `cargo run -p devnet-e2e`. Record the program IDs, vault address and run output in `specs/001-fund-launch-onboarding/devnet-run.md`
- [ ] T080 [P] Add a root `README.md`: what the project is, the repository layout, and how to build, test and run the devnet script. Link [quickstart.md](quickstart.md) and `docs/`
- [ ] T081 Constitution review gate: a human reviewer checks signer checks against `docs/shared-model.md` §5.1, hook check order against §6, cross-program calls against §5.2, invariants I3 and I10 against §7, and that no mint, burn or metadata-update path exists (FR-035). Record the sign-off in the pull request description
- [ ] T082 Run the full suite (`anchor build && cargo test -p mmf-common && cargo test -p tests && cargo test -p tests --release -- --ignored`) and the [quickstart.md](quickstart.md) validation. Open the pull request `001-fund-launch-onboarding` → `main` listing the shared-model changes, and merge when CI is green (Principle XII)

---

## Dependencies & execution order

### Phase dependencies

- **Setup (Phase 1)**: none. T007 needs T002 and T003.
- **Foundational (Phase 2)**: needs Setup. **T011 (spike) first**; T018 needs T015–T017; T019 needs
  T018. Blocks every story.
- **US1 (Phase 3)**: needs Foundational.
- **US2 (Phase 4)**: needs US1 (a launched fund, `fixtures::launched_fund`).
- **US3 (Phase 5)**: needs US2 (holders to suspend; hook to pause).
- **US4 (Phase 6)**: separation tests (T071, T074) only need US1; the matrix (T072) and Squads
  tests (T073) need US3's instructions.
- **Polish (Phase 7)**: needs all stories.

### Within each story

Interface stubs → SDK builders → tests (run them and see them fail) → implementation → checkpoint.
Every rule's rejection test comes before the code that enforces it (Principle VI).

### Story order

```text
Setup → Foundational (spike first) → US1 → US2 → US3 → US4 → Polish
                                       └──── US4 separation (T071, T074) may start after US1
```

---

## Parallel opportunities

- **Setup**: T002–T006 together; then T008, T009, T010 alongside T007.
- **Foundational** (after T011): T012, T013, T014, T015, T016, T017, T020, T021, T022 together.
- **US1**: stubs T023, T024, T025 together; tests T030, T031 alongside T027–T029 (different files);
  implementation T032, T033, T034, T036, T037, T038 together, then T035.
- **US2**: T041, T042 alongside T040; tests T048 and T050 alongside T045–T047; implementation
  T051, T052, T054, T055, T056 together, then T053.
- **US3**: T064 alongside T060–T063; implementation T065, T066, T067, T068 together, then T069.
- **Polish**: T075, T076, T078, T080 together.

### Parallel example: User Story 2 implementation

```text
Task: "Implement statement verification in programs/compliance_hook/src/statement.rs"        (T051)
Task: "Implement register_holder in programs/compliance_hook/src/instructions/register_holder.rs" (T052)
Task: "Implement approve_fund_account in .../instructions/approve_fund_account.rs"             (T054)
Task: "Implement thaw_holding_account in .../instructions/thaw_holding_account.rs"             (T055)
Task: "Implement the hook in programs/compliance_hook/src/hook/execute.rs"                     (T056)
# then T053 approve_investor, which needs T051
```

### Parallel example: User Story 1 implementation

```text
Task: "Implement init_roles in programs/governance/src/instructions/init_roles.rs"   (T032)
Task: "Implement set_roles (basic) in programs/governance/src/instructions/set_roles.rs" (T033)
Task: "Implement the mint setup CPIs in programs/fund/src/token_setup.rs"           (T034)
Task: "Implement launch in programs/fund/src/instructions/launch.rs"                (T036)
Task: "Implement init_registry and extra_metas.rs in programs/compliance_hook/src/" (T037)
Task: "Implement set_rules in programs/compliance_hook/src/instructions/set_rules.rs" (T038)
# then T035 create_fund, which needs T034
```

---

## Implementation strategy

### MVP first (US1 + US2, both P1)

1. Phase 1 Setup → Phase 2 Foundational. **Stop if the spike (T011) fails** and switch
   configuration before going further.
2. Phase 3 US1 → validate: a fund launches and its token can't be reconfigured.
3. Phase 4 US2 → validate: only approved holders hold units. This is the product's core regulatory
   promise, so US1 alone isn't a useful demo; the MVP is US1 + US2.

### Incremental delivery

4. Phase 5 US3 → suspend, revoke, pause, renew, records.
5. Phase 6 US4 → role separation, permission matrix, multisig threshold.
6. Phase 7 → property tests, scale test, error coverage, devnet run, review, merge.

Commit after each task or logical group, on `001-fund-launch-onboarding`.

---

## Notes

- `[P]` = different files and no dependency on an unfinished task.
- Tests that patch program state with `set_account` are allowed **only** for conditions no
  instruction can reach in F001 (T047, T048), and each says so in a comment.
- No task creates `dvp_settlement` or `collateral` code, or fills F002+ fields (Principle XI).
- If a task needs a shared-model change not already in `docs/shared-model.md`, stop and make it
  there first (Principle XIII).
