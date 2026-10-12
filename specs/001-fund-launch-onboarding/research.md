# Research: Fund Launch, Roles and Holder Onboarding (F001)

Each entry gives the decision, why, and what else was considered. R1–R4 and R6–R8 depend on how
the current toolchain behaves and were checked against upstream sources (listed per entry).

## R1. Toolchain and crate versions

- **Decision**: Anchor `anchor-lang` / `anchor-spl` **=1.1.2**, matching the installed CLI 1.1.2.
  Token-2022 interface types come through `anchor-spl` (`spl-token-2022-interface` 2.1.0).
  `spl-transfer-hook-interface` 2.1.0 and `spl-tlv-account-resolution` 0.11.4 for the extra-account
  list; `solana-ed25519-program` 3.0.0 to build statement instructions in clients and tests. Pin the
  Rust toolchain in `rust-toolchain.toml`. Run `cargo tree -d` in CI to catch duplicate
  `solana-address` versions across these crates (not verified upstream).
- **Rationale**: the CLI already installed is 1.1.2, and its transfer-hook test is the reference
  used here. Anchor 1.2.1 (2026-10-06) is six days old; 2.0 is a release candidate.
- **Cluster note**: mainnet runs Agave 4.3.0 and devnet 4.4.0-beta, newer than the local Solana CLI
  3.1.10. Programs built with the 3.x platform tools run on 4.x clusters. If deployment tooling
  misbehaves, upgrade the CLI before the devnet run (R16).
- **Test runtime**: LiteSVM 0.12.0, the newest release that builds with Rust 1.90 (R8). LiteSVM
  0.17+ pulls Agave 4.3 crates that need Rust 1.97.1.
- **Alternatives**: configuration A (Rust ≥ 1.97.1, Anchor 1.2.1 `solana-v4`, LiteSVM 0.18) — closer
  to mainnet and has native PermissionedBurn types. It is the fallback if the task-1 spike fails
  (R8).
- **Sources**: crates.io `anchor-spl` 1.1.2; Anchor v1.2.1 CHANGELOG.

## R2. Creating the mint with every extension in one instruction

- **Decision**: `fund::create_fund` builds the mint with manual CPIs, in this order: create account
  sized for the fixed extensions → `transfer_hook_initialize` → `default_account_state_initialize(Frozen)`
  → `permanent_delegate_initialize` → `metadata_pointer_initialize` (to the mint) →
  `mint_close_authority_initialize` → PermissionedBurn initialize (R3) → `initialize_mint2` → top up
  lamports for the metadata → `token_metadata_initialize` (signed by `FundState`) →
  `token_metadata_update_field` for `fund_id` and `terms_hash`.
- **Rationale**: Anchor 1.1.2's `extensions::` constraints don't cover DefaultAccountState or
  TokenMetadata, and its `init` calls `InitializeMint2` itself, so the frozen default can't be set
  first. Every helper above exists in `anchor-spl` 1.1.2 except PermissionedBurn.
- **Gotchas recorded for implementation**: `InitializeMint2` needs the account length to equal the
  fixed-extension size exactly (don't pre-allocate metadata space); metadata init and update grow the
  account and expect the lamports to be there; a frozen default requires a freeze authority (we set
  the `Registry` PDA); in 1.x `CpiContext::new` takes a `Pubkey`.
- **Authorities that could change transfer behaviour** (all are program PDAs with no F001
  instruction using them): transfer hook authority (`["token-admin", mint]`), freeze authority (can
  also change the default state), permanent delegate (can reassign itself), mint close authority,
  burn authority, metadata pointer / update authority, the `ExtraAccountMetaList` (no update
  instruction in F001), and the three programs' upgrade authorities (an operational key until F007
  — see Assumptions in plan).
- **Sources**: Anchor v1.1.2 `lang/syn/src/parser/accounts/constraints.rs` and
  `tests/spl/token-extensions`; Token-2022 v11.0.0 processor.

## R3. Holders destroying their own units

- **Finding**: on a thawed account, the owner (or a delegate, or the permanent delegate) can burn
  units with no mint authority, and the hook doesn't run on burns. Left alone, this would let a
  holder silently change its balance: the holder count would go stale (SC-007), and in later
  features the supply invariant (I4) and pledged-unit locks (I2) could be broken.
- **Decision**: enable Token-2022's **PermissionedBurn** extension at mint creation, with burn
  authority = `FundState` PDA. While set, ordinary `Burn` / `BurnChecked` fail; a burn must use the
  permissioned instruction with the burn authority co-signing. No F001 instruction burns, and the
  fund exposes no instruction to change this authority.
- **Verified**: shipped in Token-2022 program v11.0.0 ("available on all networks"); the deployed
  mainnet binary matches v11.0.0 and devnet matches v11.1.0.
- **Implementation risk**: `spl-token-2022-interface` 2.1.0 (used by `anchor-spl` 1.1.2) doesn't
  know the extension. We hand-encode the initialize instruction (instruction 46, sub-instruction 0,
  authority) and add its TLV size to the mint length. The first task is a spike that creates the mint
  in LiteSVM against the real v11 Token-2022 binary and proves an owner burn fails. Fallback: Anchor
  1.2.1 with `solana-v4` (interface 3.1.2).
- **Alternatives**: accept the drift and correct it on the next transfer — leaves I2/I4 open for
  F003–F005; Pausable — blocks burns only while paused, and also blocks transfers and mints.
- **Sources**: token-2022 releases `program@v11.0.0`, `program@v11.1.0`; commit 919eacb.

## R4. The hook's `execute` handler

- **Decision**: `#[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)] pub fn
  execute(ctx, amount: u64)`. The two `HolderState` entries are `UncheckedAccount`s, decoded by hand
  after checking owner program, PDA address and discriminator. When the source owner equals the
  destination owner, the record is loaded once and written once.
- **Rationale**: this is the pattern in Anchor 1.1.2's own transfer-hook test (the old `#[interface]`
  attribute is gone). Since Anchor 1.0, the same writable typed account passed twice fails with
  error 2040 (`ConstraintDuplicateMutableAccount`). Unchecked loading also lets a missing record
  count as "not approved" with our own error code instead of a decode failure (Constitution VII).
- **Token-2022 behaviour this relies on** (verified in the v11.0.0 processor):
  - the hook runs for zero-amount `transfer_checked`;
  - a transfer from an account to itself returns before the hook, so no checks run and nothing moves
    (spec edge case updated);
  - frozen source and destination fail with `AccountFrozen` before the hook;
  - the base accounts reach the hook read-only and as non-signers;
  - permanent-delegate transfers also go through the hook (handled from F005).
- **Extra-account list**: `HolderState` seeds `[Literal "holder", AccountKey(1 = mint),
  AccountData{account_index: 0 or 2, data_index: 32, length: 32}]`; Registry, FundState and the
  delegate PDA as literal pubkeys. Signer flags on extras are always dropped. Any program that calls
  `transfer_checked` by CPI must pass the writable Registry (shared-model §12).
- **Sources**: Anchor v1.1.2 `tests/spl/transfer-hook`, `codegen/accounts/try_accounts.rs`;
  `tlv-account-resolution/src/state.rs`; `transfer-hook/interface/src/offchain.rs`.


## R5. Registry writable during transfers (holder count)

- **Decision**: mark `Registry` writable in the `ExtraAccountMetaList` so the hook can maintain
  `holder_count` in the same transfer that changes balances (FR-024, Clarification Q3).
- **Rationale**: the count of investors holding units can't be recomputed over ~10 000 holders in
  one transaction, so it's a stored running total. Constitution V allows that only if it is updated
  in the same instruction as its inputs and invariant-tested (I10, SC-007).
- **Effect (stated, as the user asked)**: every transfer of the fund's units write-locks the one
  `Registry` account, so this fund's transfers run one after another within a block instead of in
  parallel. Transfer volume for an institutional money market fund is low (tens to thousands per
  day), well inside a single account's per-block budget: about 330 transfers per block (R8).
- **Alternatives**: (a) count approved investors at approval time (no hook write) — rejected in
  Clarification Q3; (b) shard the counter across several accounts — adds accounts and complexity
  for throughput this fund doesn't need.

## R6. Verifying the KYC statement

- **Decision**: the statement is checked through the Instructions sysvar. The program requires the
  sysvar account to be the real Instructions sysvar and loads the instruction just before the current
  one. That instruction must belong to the Ed25519 program, contain exactly one signature, and have
  all three `*_instruction_index` fields set to `u16::MAX`. Its offsets must stay inside its own data,
  and the message must be exactly 95 bytes. The program then parses the message and checks it field
  by field ([contracts/kyc-statement.md](contracts/kyc-statement.md)).
- **Rationale**: these are the known ways to bypass Ed25519 introspection, such as offsets pointing
  at another instruction's data (the Relay bridge bug), a fake sysvar account, or zero signatures,
  which verify trivially. Parsing the signed bytes, instead of rebuilding them from instruction
  arguments, gives a specific error for each failure (wrong owner, wrong fund, expired, and so on).
- **Replay**: a statement names one owner and one fund. Approval needs status Pending, and renewal
  needs a later expiry (R14), so replaying a statement changes nothing.
- **Runtime**: strict Ed25519 verification (SIMD-0152) is active, and the offset semantics are
  unchanged in Agave 3.1.10. The Ed25519 instruction can't be called via CPI, so it must be a
  top-level instruction in the compliance transaction. When compliance is a Squads vault, the
  instruction also carries the statement's hash, so the vault's approval covers the exact statement
  (R8).
- **Sources**: Agave v3.1.10 `precompiles/src/ed25519.rs`; docs.rs `solana-ed25519-program` 3.0.0;
  blog.asymmetric.re "wrong offset" write-up.

## R7. Events and call depth

- **Decision**: admin and onboarding instructions emit events with `emit_cpi!` (`event-cpi`
  feature); the hook's `TransferValidated` uses `emit!` (logs only). FR-033's records are the
  `emit_cpi!` events.
- **Rationale**: `emit_cpi!` stores the event in an inner instruction, so it survives log
  truncation. That suits permanent records. The cost is one call level. The current runtime allows
  the top-level instruction plus 4 nested calls (Agave 4.3 `MAX_INSTRUCTION_STACK_DEPTH = 5`), and the
  proposal to raise it (SIMD-0268) isn't active on any cluster. Depth budget:

  | Call chain | Depth | |
  |---|---|---|
  | Squads execute → governance → compliance_hook `set_status` → `emit_cpi` | 4 | OK |
  | Squads execute → compliance_hook `approve_investor` → `emit_cpi` | 3 | OK |
  | Squads execute → fund (F003) → Token-2022 → hook → `emit_cpi` | 5 | over the limit |
  | Squads execute → fund (F003) → Token-2022 → hook (`emit!`) | 4 | OK |

  Using `emit!` in the hook keeps later features (F003 settlement, F005 enforcement) inside the
  limit, and needs no event-authority account in the `ExtraAccountMetaList`. `TransferValidated` isn't
  one of FR-033's records; the token balances themselves are the transfer record.
- **Sources**: Agave v4.3.0 `program-runtime/src/execution_budget.rs`; SIMD-0268; Anchor v1.1.2
  `lang/src/event.rs`.

## R8. Test harness

- **Decision (default configuration "B", the installed toolchain)**: Rust 1.90, Anchor 1.1.2,
  **LiteSVM 0.12.0** (Agave 3.1 runtime, `precompiles` feature for Ed25519). LiteSVM 0.12 bundles
  Token-2022 v10, which predates permissioned burn, so tests load the **mainnet Token-2022 v11.0.0
  binary** (dumped once) over the bundled one.
- **Spike first**: task 1 builds the workspace skeleton and one test that creates the mint (R2, R3)
  and shows an owner burn failing, under configuration B. If v11 won't run on LiteSVM 0.12, or the
  crate types don't line up, switch to **configuration "A"**: Rust ≥ 1.97.1, Anchor 1.2.1 with
  `solana-v4`, LiteSVM 0.18.0 (Agave 4.3 runtime, bundles Token-2022 11.0.0, mainnet rent and
  features). Record the outcome here. Programs are built by the platform tools either way; the host
  Rust version only affects tests and support crates.
- **Placing units in tests** (no mint path exists — FR-035): create the fund treasury's ATA with real
  instructions (so it has the TransferHookAccount extension), approve and thaw it, then patch its
  `amount` and the mint's `supply` with `set_account` (unpack with `StateWithExtensionsMut`). Units
  then move to investors only through real `transfer_checked` calls, so the hook maintains the holder
  count itself. No test edits `HolderState` or `Registry` directly.
- **Time**: KYC expiry tests read the `Clock` sysvar, set `unix_timestamp`, and write it back
  (`warp_to_slot` changes only the slot).
- **Events**: decoded from `inner_instructions`: an instruction to our own program whose data starts
  with Anchor's event tag (`0x1d9acb512ea545e4`, little-endian), then the 8-byte event discriminator
  and the Borsh payload. Status-history tests (FR-034) rebuild each holder's history from these
  alone.
- **Compute**: `compute_units_consumed` from transaction metadata. The scale test (SC-006) registers
  10 and then 10 000 holders and asserts the hook's compute use is identical. A CI ceiling of
  60 000 CU per hooked transfer stops regressions.
- **Squads v4 (multisig roles, FR-012, US4 scenario 3)**: load the mainnet program
  `SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf` (immutable since its last deploy; don't use devnet's
  build) and its ProgramConfig account `BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr` as fixtures. The
  published `squads-multisig` crate targets Solana 1.17 / Anchor 0.29, so the test crate builds the
  five instructions it needs by hand (Anchor discriminator + Borsh): `multisig_create_v2`,
  `vault_transaction_create`, `proposal_create`, `proposal_approve`, `vault_transaction_execute`.
  Executing below threshold leaves the proposal Active and fails with `InvalidProposalStatus` (6008).
  The test asserts that code against the real binary, and that the target holder is unchanged.
- **KYC statement under Squads**: the Ed25519 instruction must be top-level, so a compliance
  transaction is `[ed25519 verify, squads vault_transaction_execute]`. The statement sits outside the
  vault transaction the members approved, so `approve_investor` / `renew_investor` take the expected
  `attestation_hash` as an argument (inside the approved transaction). The program checks it against
  the signed message (`StatementHashMismatch`). Members thus approve one exact statement, and the
  executor can't swap in a different valid one.
- **Write-lock budget (R5)**: on Agave 4.3 a single writable account may use 20M CU per block. At
  ≤ 60 000 CU per transfer that's about 330 fund transfers per block (~1 500 per second at 215 ms
  slots). Clients set a tight compute-unit limit, because the scheduler charges the requested limit.
- **Transaction size**: legacy/v0 transactions stay at 1 232 bytes. F001's largest (create_fund with
  terms and metadata; Squads execute with a statement) fit; V1 transactions (4 096 bytes, SIMD-0385)
  aren't needed.
- **Sources**: crates.io `litesvm`; LiteSVM v0.18.0 CHANGELOG and `tests/precompiles.rs`; Squads v4
  `errors.rs`, `vault_transaction_execute.rs`; Agave v4.3.0 `cost-model/src/block_cost_limits.rs`.

## R9. One holding account per owner (the ATA)

- **Decision**: an owner holds units only in its Token-2022 associated token account for the fund
  mint. Accounts start frozen (default state), and only `thaw_holding_account` thaws them — only the
  ATA, only for an Approved owner. Any other account the owner opens stays frozen for good.
- **Rationale**: `HolderState` is per owner (Clarification Q2), but the hook only sees the balances
  of the two token accounts in the transfer. If an owner had two accounts, the per-owner holder
  count (FR-024) and F002's per-owner accrual (shared-model §6 step 8 uses the token account's
  pre-balance) would both go wrong. With one account per owner, "owner balance" equals "ATA
  balance", and the hook can read it directly. Enforcing this through thawing costs nothing on the
  transfer path: Token-2022 rejects frozen accounts before the hook runs.
- **Alternatives**: (a) track an owner-total balance in `HolderState` — duplicates what the token
  accounts already hold and must be kept in step on every path; (b) have the hook derive and compare the ATA
  address on every transfer — costs compute on every transfer for no extra safety.

## R10. Binding the issuer to the Fund

- **Decision**: `governance::init_roles` runs first, signed by the issuer **and the mint keypair**,
  and creates `RoleRegistry { issuer }` at `["roles", PDA("fund", mint)]`. `fund::create_fund`
  then requires `role_registry.issuer == signer`. Both normally go in one transaction.
- **Rationale**: FR-002 requires the creator to become the issuer, but `FundState` has no issuer
  field, and the shared model allows no fund → governance CPI. Requiring the mint keypair's
  signature means nobody can create the role list for someone else's mint address first. This
  matters when the issuer is a Squads vault, because the vault transaction (and its ephemeral
  signer address) is visible before it executes. The issuer is stored once (Constitution V).
- **Alternatives**: (a) add `issuer` to `FundState` — a second copy of the same fact; (b) new CPI
  edge fund → governance — a shared-model §5.2 change, more compute in an already heavy
  instruction; (c) rely only on bundling both instructions in one transaction — still open to
  front-running when Squads exposes the address early.

## R11. Jurisdiction encoding

- **Decision**: ISO 3166-1 alpha-2 codes stored as `[u8; 2]`. The allowed set is a 676-bit bitmap
  (`[u64; 11]`) indexed by `(c1−'A')×26 + (c2−'A')`. The assigned codes are a compile-time constant
  in the shared `mmf-common` crate.
- **Source**: 249 assigned codes, unchanged since 2011 apart from names. Embedded from Debian
  `iso-codes` `data/iso_3166-1.json` v4.20.1, with a unit test asserting 249 entries.
- **Rationale**: covers every country (Clarification Q5) with no lookup table to agree with
  providers and no index that shifts when a code is added. Both sides can read the stored code. The
  set is 88 bytes on one account.
- **Deviation from the requested approach**: the plan input suggested widening the bitmap using
  `Registry._reserved`. Nothing is deployed yet, so the field is instead defined at its final size
  and `_reserved [u8; 64]` stays whole for post-deployment additions (shared-model §1 "full layout
  from day one").
- **Alternatives**: dense index 0..248 over a sorted code list — needs a versioned table on- and
  off-chain; 256-bit set with a custom mapping — same problem.

## R12. Cross-program reads without crate cycles

- **Decision**: a shared crate `crates/mmf-common` holds program IDs, PDA seeds, enums (status,
  class, reason), the jurisdiction table, the KYC statement layout, and read-only views of
  `FundState`, `RoleRegistry` and `Registry`. Each view checks owner program, address and the
  8-byte Anchor discriminator before decoding.
- **Rationale**: compliance_hook reads `RoleRegistry` and `FundState`, fund reads `Registry` and
  `RoleRegistry`, and governance CPIs into compliance_hook. Depending on each other's program crates
  would create a cycle. Only governance → compliance_hook (CPI client) is a real crate dependency.
- **Alternatives**: one program for everything — contradicts the shared model's five programs.

## R13. Suspension and revocation change status only

- **Decision**: suspend, reinstate and revoke change `HolderState.status`. They do not freeze or
  thaw the token account. The token freeze is used only as the default gate for new accounts.
- **Rationale**: the hook's status check is the rule (Principle II), and it blocks both sending
  and receiving (FR-027). Freezing too would create a second source of truth. It would also block
  the permanent delegate in F005 enforcement and F007 recovery, since Token-2022 rejects frozen
  accounts even for the delegate. `FreezeHolder` / `ThawHolder` remain separate powers in the
  shared model's `ActionRecord` kinds, for F007 to define.
- **Alternatives**: freeze on suspend (as `docs/design.md` §6.2.3 sketches) — rejected for the
  reasons above.

## R14. Renewal means a later expiry

- **Decision**: `renew_investor` requires the new statement's `expiry_ts` to be strictly later than
  the recorded `kyc_expiry_ts` (`RenewalNotNewer`). Jurisdiction and investor type may change and are
  re-checked.
- **Rationale**: resolves "newer statement" in FR-018 (left open in clarify) with a testable rule.
  It stops an older, still-valid statement from being replayed to undo a newer one.
- **Alternatives**: accept any valid statement — allows replay; add an `issued_at` field — an extra
  field the provider must manage, for no extra protection.

## R15. Holder count semantics and the strict cap

- **Decision**: an Investor counts while `last_balance > 0`, adjusted at hook step 9 for each owner
  touched. The cap is checked before the count changes (step 7): a transfer that would give units to
  an investor with none is rejected when `holder_count >= max_holders`, even if the sender empties
  its own balance in the same transfer.
- **Rationale**: matches the spec's wording ("rejected once the number … equals the cap") and the
  shared-model step order. It's predictable and easy to test. Recorded and actual balances can't
  diverge, because owners can't burn (R3) and have only one usable account (R9).
- **Alternatives**: net-aware cap (allow when the sender drops out at the same time) — friendlier,
  but departs from the spec text and adds a branch; not worth it in F001.

## R16. Devnet end-to-end run without units

- **Decision**: the per-feature devnet run deploys the three programs and runs: init roles → create
  fund → init registry → set rules → set roles → launch → register → approve with a mock statement
  → thaw → **zero-unit** transfers (approved → approved succeeds; to a suspended holder fails; while
  paused fails) → suspend / reinstate / revoke → pause / unpause.
- **Rationale**: F001 has no way to create units (FR-035), and devnet can't have accounts edited
  directly the way LiteSVM can. A zero-unit `transfer_checked` still runs every hook check (spec
  Edge Cases), so it proves on a real cluster that the token enforces the rules. Transfers with
  units are covered in LiteSVM (R8).
- **Alternatives**: a test-only mint instruction behind a feature flag — forbidden (Constitution
  VIII: no other mint path); a local validator with pre-loaded accounts — the hand-made account
  must match a mint created at runtime, which is brittle.

## R17. Fund-operated accounts, and escrow before the settlement feature

- **Decision**: `FundTreasury` holders must be owned by the fund PDA `["treasury", fund_state]`.
  `Escrow` registration fails with `ClassNotAvailable` in F001. F003 enables it once the settlement
  program and its escrow authority PDA exist (shared-model §4.3 already places the escrow
  `HolderState` at F003 setup).
- **Rationale**: FR-019 allows KYC-free approval only for accounts the system itself controls. In
  F001 the only such account is the fund treasury; no escrow account can exist yet, so rejecting
  escrow is the rule applied correctly, not a gap (spec edge case updated). The check is "owner equals
  the expected PDA", which needs no list of program IDs in the Registry.
- **Alternatives**: generate the settlement program's ID now and accept escrow owners derived from
  it — that would mean creating (and safeguarding) a program keypair two features early; accept any
  off-curve owner — any program's PDA would qualify, not just ours.
