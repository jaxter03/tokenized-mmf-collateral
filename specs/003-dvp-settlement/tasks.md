---
description: "Task list for F003: Subscription, Redemption and Transfer Settlement"
---

# Tasks: Subscription, Redemption and Transfer Settlement

**Input**: Design documents from `specs/003-dvp-settlement/`: [plan.md](plan.md),
[spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md),
[contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: REQUIRED. Constitution Principle VI says every rule needs a test that is written and
seen to fail before its code, with at least one rejection test asserting the exact error. SC-006
says the same.
- **Order within each story**: interface stubs → SDK builders → tests (must fail) →
  implementation.
- **Stub handlers** return `Ok(())` and do nothing, so happy-path tests fail on missing state and
  rejection tests fail on a missing error.

**Organization**: by user story, in spec priority order: US1 and US2 (P1), then US3, US4 and US5
(P2). Settlement setup (issuer and compliance steps every story needs) is Foundational. Each story
phase ends with a checkpoint where that story's independent test passes.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: US1–US5, from spec.md
- Paths are from the repository root

## Path conventions (from plan.md)

- Programs: `programs/{dvp_settlement,fund,compliance_hook,governance}/src/`
- Shared crate: `crates/mmf-common/src/`; client SDK: `crates/mmf-sdk/src/`
- LiteSVM tests: `tests/src/` (harness, fixtures, model, helpers) and `tests/tests/` (test files)
- Scripts: `scripts/`

## Fixed values used in tests

- UTC day numbers (`unix_timestamp.div_euclid(86_400)`): 2026-10-13 = **20 739** (Tuesday),
  2026-10-14 = **20 740**, 2026-10-16 = **20 742** (Friday), 2026-10-19 = **20 745** (Monday).
- Unit and cash decimals: 6 for both. $1.00 = nav `100_000_000`. 2 000 000 units =
  `2_000_000_000_000` base units, and $2M = `2_000_000_000_000` cash base units.
- Default expiry: `86_400` s. Standard fund redemption limit: the F001 `standard_terms` value of
  `redemption_gate_bps` (read it from the fixture; don't hard-code it).

---

## Phase 1: Setup

**Purpose**: confirm a green F001 + F002 baseline, add the fifth program and the new crate edges.

- [X] T001 Run `anchor build && cargo test -p mmf-common && cargo test -p tests` on the branch
  before any change. Confirm F001 and F002 are green (Constitution XII baseline), and record the
  test count in the PR description draft
- [X] T002 Create the `dvp_settlement` program (research R1):
  - `programs/dvp_settlement/Cargo.toml` with `anchor-lang = { version = "=1.1.2", features = ["event-cpi"] }`,
    `anchor-spl` (same features as `fund` plus `token`), `mmf-common`, and
    `fund = { path = "../fund", features = ["cpi", "no-entrypoint"] }`, plus the usual
    `cpi` / `no-entrypoint` / `idl-build` features copied from `programs/fund/Cargo.toml`;
  - `programs/dvp_settlement/src/lib.rs` with `declare_id!`;
  - run `anchor keys sync` to generate the keypair (it stays in `target/deploy/`, never
    committed);
  - add the ID to `Anchor.toml` (`[programs.devnet]` and `[programs.localnet]`) and to
    `crates/mmf-common/src/ids.rs` as `DVP_SETTLEMENT_PROGRAM_ID`, with the same doc comment as
    the others.

  `anchor build` must produce four `.so` files
- [X] T003 Add the crate edges from plan.md "Primary Dependencies":
  - `compliance_hook = { path = "../compliance_hook", features = ["cpi", "no-entrypoint"] }` in
    `programs/fund/Cargo.toml`;
  - `dvp_settlement = { path = "../dvp_settlement", features = ["cpi", "no-entrypoint"] }` in
    `programs/governance/Cargo.toml`;
  - `dvp_settlement = { path = "../programs/dvp_settlement", features = ["no-entrypoint"] }` in
    `tests/Cargo.toml` and `crates/mmf-sdk/Cargo.toml` (if the SDK uses program types; otherwise
    the SDK uses only `mmf-common`).

  Confirm there is no cycle (`cargo tree -p compliance_hook | grep -E "fund|dvp"` and
  `cargo tree -p dvp_settlement | grep governance` are both empty), and that `anchor build`
  succeeds
- [X] T004 In `tests/src/harness.rs`, load `dvp_settlement.so` at `DVP_SETTLEMENT_PROGRAM_ID`
  alongside the three existing programs. In `tests/src/fixtures.rs`, add cash helpers:
  - `cash_ata(h, owner, cash_mint) -> Pubkey`, under the cash mint's own token program
    (`create_cash_mint` makes a Token-2022 mint; the helpers also work for a classic SPL Token
    mint);
  - `create_cash_ata(h, owner, cash_mint)`;
  - `mint_cash(h, cash_mint, owner, amount)`, minting with the cash mint's authority from
    `create_cash_mint`;
  - `cash_balance(h, token_account) -> u64`.

  Run the suite: still green

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: shared formulas, layouts, views, the reference model, and the settlement setup that
every story needs (FR-001 to FR-003).

**⚠️ CRITICAL**: no story work starts until Phase 2 is complete, and T024 shows F001/F002 still
green.

### Shared code

- [X] T005 [P] Create `crates/mmf-common/src/settlement.rs` and export it from `lib.rs`. All
  functions are pure, use checked `u128` math and return `None` on overflow:
  - `subscription_cash(units: u64, nav: u64) -> Option<u64>` = `ceil(units × nav / 10^8)`
    ("rounds up: the Fund is never under-paid");
  - `redemption_cash(units: u64, nav: u64) -> Option<u64>` = `floor(units × nav / 10^8)`
    ("rounds down: the Fund never over-pays");
  - `redemption_ok(redeemed_today: u64, units: u64, at_day_start: u64, gate_bps: u16) -> Option<bool>`
    = `(redeemed_today + units) × 10_000 ≤ at_day_start × gate_bps`, exact and cross-multiplied
    (equal passes);
  - `price_fresh(nav_seq: u64, nav_period_end_day: u32, today: u32) -> bool` =
    `nav_seq > 0 ∧ nav_period_end_day + 1 ≥ today`;
  - `day_start(ts: i64) -> Option<i64>` = `utc_day(ts) × 86_400`.

  Unit tests with the data-model worked checks:
  - `2_000_000_000_000` units at `100_000_000` → `2_000_000_000_000` both ways;
  - 1 unit at `99_999_999` → subscription 1, redemption 0;
  - `1_000_001` at `99_999_999` → subscription `1_000_001`, redemption `1_000_000`;
  - `redemption_ok` exactly at the limit → true, one unit over → false, `at_day_start = 0` and
    `units > 0` → false;
  - `price_fresh` for today, yesterday (true) and the day before yesterday (false);
  - overflow returns `None`
- [X] T006 [P] In `crates/mmf-common/src/seeds.rs`, add:
  - `DVP = b"dvp"` (`["dvp", fund_state]` → DvpConfig);
  - `ESCROW = b"escrow"` (`["escrow", fund_state]` → escrow authority);
  - `SETTLEMENT = b"settlement"` (`["settlement", fund_state, created_by, external_ref]`);
  - `DVP_CALLER = b"dvp-caller"` (governance; signs `dvp_settlement::reject`).

  In `crates/mmf-common/src/enums.rs`, add `u8_enum!`s:
  - `SettlementKind { Subscription = 0, Redemption = 1, SecondaryTransfer = 2 }`;
  - `SettlementStatus { Instructed = 0, SellerFunded = 1, BuyerFunded = 2, Affirmed = 3, Settled = 4, Cancelled = 5, Expired = 6, Rejected = 7 }`
    with `is_open()` (true for 0..=3) and `is_returnable()` (true for 5, 6, 7);
  - `Side { Seller = 0, Buyer = 1 }`;
  - `Leg { Units = 0, Cash = 1 }`;
  - `ReasonCode::allowed_for_reject_settlement()`, the same list as `allowed_in_f001`.

  Extend the round-trip unit test (unknown values are errors)
- [X] T007 Extend `programs/fund/src/state.rs`, `errors.rs` and `events.rs`:
  - **FundState**: after `pending_override_nav`, add `units_at_day_start: u64` ("units_outstanding
    when the redemption day rolled over"), and change `_reserved` to `[u8; 44]`. The total size
    must be unchanged.
  - **Errors**: **append** `CashTreasuryAlreadySet`, `InvalidCashTreasury`, `UnsupportedCashMint`,
    `SettlementMismatch`, `CashNotEscrowed`, `RedemptionLimitExceeded`.
  - **Events** (each with `fund`, `slot`, `unix_ts`):
    - `CashTreasurySet { cash_treasury, signer }`
    - `SubscriptionMinted { settlement, units, units_outstanding }`
    - `RedemptionBurned { settlement, units, units_outstanding, redeemed_today, units_at_day_start }`
- [X] T008 Create `programs/dvp_settlement/src/state.rs`, `errors.rs` and `events.rs` per
  [data-model.md](data-model.md):
  - **DvpConfig**: "version u8 · fund_state Pubkey · next_seq u64 · default_expiry_secs u32 ·
    bump u8 · escrow_bump u8 · _reserved [u8; 64]", 119 bytes with the discriminator,
    `VERSION = 1`.
  - **SettlementInstruction**, fields in exactly this order: `version u8, fund_state Pubkey,
    seq u64, kind u8, seller Pubkey, buyer Pubkey, units u64, cash_amount u64, nav_seq u64,
    not_before_ts i64, expires_at_ts i64, status u8, seller_funded_at i64, buyer_funded_at i64,
    external_ref [u8; 32], created_by Pubkey, created_at i64, closed_at i64, cancel_requests u8,
    units_to_return u64, cash_to_return u64, bump u8, _reserved [u8; 64]`. 333 bytes, `VERSION = 1`.
    Add helpers `kind()`, `status()`, `units_held()` and `cash_held()` (data-model "Escrow held by
    an instruction") and `side_party(side, transfer_agent) -> Pubkey`.
  - **DvpError**: in the order listed in
    [contracts/dvp_settlement.md](contracts/dvp_settlement.md#errors-dvperror-new).
  - **Events**: `DvpInitialized`, `SettlementCreated`, `LegFunded`, `Settled`, `CancelRequested`,
    `CancelRequestWithdrawn`, `SettlementCancelled`, `SettlementExpired`, `SettlementRejected`,
    `LegReturned`, with the payloads in the data-model Events table plus `fund`, `slot`,
    `unix_ts`.
- [X] T009 [P] In `programs/compliance_hook/src/errors.rs`, **append** `WrongHolder` and
  `ClassNotAccrued`. In `events.rs`, define
  `Accrued { fund, owner, pre_balance: u64, post_balance: u64, accrual_index: u128, accrued_e18: u128, slot, unix_ts }`.
  Check it isn't already declared; F002 left the name reserved
- [X] T010 In `crates/mmf-common/src/views.rs` (depends on T007, T008):
  - extend `FundStateView` with `mint`, `role_registry`, `status`, `nav`, `nav_seq`,
    `nav_period_end_day`, `cash_treasury`, `terms.cash_mint` and `units_outstanding` at their
    serialized offsets;
  - extend `RegistryView` with `transfers_paused`;
  - extend `RoleRegistryView` with `transfer_agent` if missing;
  - add `HolderStateView::load(info, mint, owner)`: checks owner = compliance_hook, discriminator,
    and that the address is PDA `["holder", mint, owner]`; decodes `class`, `status`,
    `kyc_expiry_ts`; and offers `eligible(now)` = "class == Investor ∧ status == Approved ∧
    kyc_expiry_ts > now";
  - add `SettlementView::load(info, expected_fund)`: checks owner = `DVP_SETTLEMENT_PROGRAM_ID`,
    the discriminator, and that the address is the PDA of its own `fund_state, created_by,
    external_ref`; decodes the fields `fund` needs (`fund_state, kind, status, units,
    cash_amount`).

  Each `load` failure is a distinct `ViewError`. Unit-test each decode against a
  program-serialized account
- [X] T011 Update `tests/tests/layouts.rs` (depends on T007, T008, T010):
  - FundState size unchanged;
  - `units_at_day_start` right after `pending_override_nav`, then `_reserved` 44 bytes;
  - DvpConfig = 119 bytes; SettlementInstruction = 333 bytes;
  - every new view field matches a program-serialized account with non-zero values.

  F001/F002 test edit, to be listed in the PR (Constitution XII)
- [X] T012 [P] In `crates/mmf-sdk/src/pda.rs`, add:
  - `dvp_config(fund_state)`, `escrow_authority(fund_state)`;
  - `unit_escrow(fund_state, mint)` (Token-2022 ATA);
  - `cash_escrow(fund_state, cash_mint, cash_token_program)`;
  - `settlement(fund_state, created_by, external_ref)`;
  - `dvp_caller()`.

  Create `crates/mmf-sdk/src/ix_dvp.rs` (empty module, exported) for the builders that the story
  phases add
- [X] T013 Update `seed_units` in `tests/src/units.rs` so it also adds `amount` to
  `FundState.units_outstanding`, keeping I4 (research R18). Update the module comment. Add
  `assert_i4(h, ctx)` (`units_outstanding == supply`) to `tests/src/assert.rs`. F001/F002 helper
  edit, to be listed in the PR
- [X] T014 [P] Add the independent settlement reference model in `tests/src/dvp_model.rs`
  (research R18). It **must not** use `mmf_common::settlement` or program types:
  - it tracks per-party units and cash, escrow units and cash, units outstanding,
    `redeemed_today` / `units_at_day_start` per UTC day, and each instruction's kind, parties,
    amounts, status, funded flags, cancel bits and earmarks;
  - it has one method per operation: `create, fund(side), settle, cancel(side),
    withdraw_cancel(side), expire, reject, return_leg(leg), suspend, reinstate, pause, unpause,
    publish(nav), advance(secs)`;
  - each method returns `Ok` or the expected error name, computed from the spec's rules
    (FR-004–FR-030), with its own ceil and floor arithmetic.

  Unit-test the model with the spec's independent tests (US1 $2M, US2 $500 000, US3 $999 500)

### Settlement setup (FR-001–FR-003)

- [X] T015 Interface stubs (handlers return `Ok(())`), with full account structs from the
  contracts:
  - `fund::set_cash_treasury` in `programs/fund/src/instructions/set_cash_treasury.rs`
    ([contracts/fund.md](contracts/fund.md#set_cash_treasury-issuer));
  - `dvp_settlement::init_dvp(default_expiry_secs: Option<u32>)` in
    `programs/dvp_settlement/src/instructions/init_dvp.rs`, wired into `lib.rs`.

  `anchor build` succeeds
- [X] T016 [P] Add SDK builders: `ix_fund::set_cash_treasury(issuer, fund_state, cash_treasury,
  cash_mint)` in `crates/mmf-sdk/src/ix_fund.rs`, and `ix_dvp::init_dvp(issuer, payer,
  fund_state, mint, cash_mint, cash_token_program, default_expiry_secs)` in
  `crates/mmf-sdk/src/ix_dvp.rs`
- [X] T017 Write `tests/tests/f003_setup.rs` (tests must fail):
  - `set_cash_treasury`: `RoleRegistryMismatch`; non-issuer (fund admin, compliance, transfer
    agent, keeper, investor) → `Unauthorized`; second call → `CashTreasuryAlreadySet`; account of
    another mint or not a token account → `InvalidCashTreasury`; a Token-2022 cash mint with a
    transfer-fee extension → `UnsupportedCashMint`; success in Setup and in Open, with
    `CashTreasurySet` and the stored key;
  - `init_dvp`: `FundMismatch`; non-issuer → `Unauthorized`; before `set_cash_treasury` →
    `CashTreasuryNotSet`; `Some(0)` → `InvalidExpiry`; a second call fails (account in use);
    `None` stores `86_400` and `Some(3_600)` stores `3_600`; `next_seq == 1`, the correct
    `escrow_bump`, both escrow ATAs exist and the unit escrow is frozen; `DvpInitialized`
- [X] T018 In `tests/tests/us2_onboarding.rs`, replace F001's "escrow class →
  `ClassNotAvailable`" test with:
  - escrow registration by an investor (self-registration) → `SelfRegistrationClassNotAllowed`
    (F001's existing error for a non-investor class);
  - compliance registering an Escrow holder owned by a random key, or by another fund's escrow
    PDA → `NotSystemControlled`;
  - compliance registering this fund's escrow PDA → Pending, then `approve_fund_account` →
    Approved with `kyc_expiry_ts = i64::MAX`.

  `CollateralVault` is still rejected, as before, with `InvalidClass` (it is checked earlier).
  `ClassNotAvailable` is no longer returned by anything; it stays in `HookError` marked
  `// retired` so later codes keep their numbers, and `check-error-coverage.sh` skips retired
  variants. F001 test edit, to be listed in the PR. Tests must fail
- [X] T019 Implement `fund::set_cash_treasury` per its contract (checks 1–6 in order; the
  extension check reads the cash mint's TLV for TransferFeeConfig and TransferHook when it's a
  Token-2022 mint). Emit `CashTreasurySet`
- [X] T020 Implement `dvp_settlement::init_dvp` per its contract. Create DvpConfig, and create
  both escrow ATAs through the associated-token program (unit escrow under Token-2022, cash escrow
  under `cash_mint.owner`). Emit `DvpInitialized`
- [X] T021 In `programs/compliance_hook/src/instructions/register_holder.rs` and
  `approve_fund_account.rs`, accept class `Escrow` when
  "owner == PDA(["escrow", registry.fund_state], DVP_SETTLEMENT_PROGRAM_ID)", otherwise
  `NotSystemControlled`. In `approve_fund_account`, replace the treasury-only check with "class
  is FundTreasury or Escrow, else `NotFundAccount`", plus the per-class owner check. Update the
  R17 comment
- [X] T022 Add `settlement_ready_fund(h) -> SettleCtx` to `tests/src/fixtures.rs`, following
  quickstart "Setting up a fund for settlement":
  - a launched fund with a price of $1.00 published on day 20 739;
  - a cash treasury ATA owned by the transfer agent's key;
  - `init_dvp(None)`, then the escrow registered, approved and thawed.

  `SettleCtx` holds the `FundCtx` plus the dvp config, escrow PDA, unit and cash escrow, cash
  treasury, and the transfer agent and keeper keypairs. Add `investor_with_cash(h, ctx, cash) ->
  Keypair` (onboarded, cash ATA funded) and `investor_with_units(h, ctx, units, cash) -> Keypair`
  (via `seed_units`)
- [X] T023 Make T017 and T018 pass. FR-003's creation checks need `create_instruction` and are
  written in US1 (T029)
- [X] T024 Run the whole suite. F001, F002 and the Phase 2 tests are green, and I4 holds in every
  test that seeds units

**Checkpoint**: shared code, layouts, model and settlement setup are ready, and F001/F002 are
still green.

---

## Phase 3: User Story 1 — Subscribe (Priority: P1) 🎯 MVP part 1

**Goal**: an approved investor pays cash and receives newly created units at the price of a
specific price record. Units are created only at the moment the cash is paid to the treasury.

**Independent Test**: at $1.00, the investor creates a subscription for 2 000 000 units, the
transfer agent affirms, the investor funds $2M, and the keeper settles. The investor gains
2 000 000 units and loses $2M; the treasury gains $2M; the escrow holds nothing for the
instruction; `units_outstanding` = supply, up 2 000 000.

### Interface stubs for User Story 1

- [X] T025 [US1] Add stubs (handlers return `Ok(())`) with full account structs from
  [contracts/dvp_settlement.md](contracts/dvp_settlement.md):
  - `create_instruction(args)` in `programs/dvp_settlement/src/instructions/create_instruction.rs`.
    `seller_holder` and `buyer_holder` are Anchor `Option<UncheckedAccount>`, absent for the Fund
    side.
  - `affirm_and_fund(side: u8)` in `affirm_and_fund.rs`, with optional unit-leg and hook accounts.
  - `settle()` in `settle.rs`, with optional per-kind accounts.

  Wire them into `lib.rs`; `anchor build` succeeds
- [X] T026 [P] [US1] Add stubs for `fund::mint_for_subscription(units)` in
  `programs/fund/src/instructions/mint_burn.rs` and `compliance_hook::accrue(post_balance)` in
  `programs/compliance_hook/src/instructions/accrue.rs`, with the account structs from
  [contracts/fund.md](contracts/fund.md) and
  [contracts/compliance_hook.md](contracts/compliance_hook.md)
- [X] T027 [US1] Add SDK builders to `crates/mmf-sdk/src/ix_dvp.rs`:
  - `create_instruction(creator, payer, ctx, args)` (`CreateArgs { kind, seller, buyer, units,
    cash_amount, not_before_ts, expires_at_ts, external_ref }`), which derives the settlement PDA
    from creator + external_ref;
  - `affirm_fund_side(transfer_agent, settlement)` (subscription seller side);
  - `fund_cash(party, source, settlement)`;
  - `settle_subscription(triggered_by, settlement)`, which passes every account `fund` and the
    hook need: escrow holder, Registry (writable), fund program and event authority, hook extra
    accounts for escrow → buyer (reuse `mmf_sdk::transfer`'s extra-account resolution), cash
    treasury.

  Builders do no validation (Constitution II: tests hit the program directly)

### Tests for User Story 1 (write first; they must fail) ⚠️

- [X] T028 [P] [US1] Write `tests/tests/f003_create.rs` (FR-004–FR-009, contract check order),
  covering all three kinds:
  - `FundMismatch`; Setup fund → `FundNotOpen`; before setup or before escrow approval or thaw →
    `EscrowNotReady`;
  - kind 3 or the Fund in the wrong place → `InvalidKind`;
  - creators: investor and transfer agent accepted for Sub/Red; keeper, fund admin and another
    investor → `Unauthorized`; for Secondary, seller and buyer accepted and a third party →
    `Unauthorized`;
  - `ZeroUnits`; `ZeroExternalRef`; `SameParty`; no publish → `NoPrice`; on day 20 741 with the
    last record covering 20 739 → `StalePrice`; on Monday 20 745 with Friday's 3-day record →
    accepted;
  - Sub at `99_999_999` for 1 unit → cash 1; Red at the same price → `ZeroCash`; Secondary with
    0 cash → `ZeroCash`; Sub/Red with a non-zero cash arg → `CashAmountNotAllowed`;
  - `InvalidExpiry` for `exp ≤ now` and `exp ≤ not_before`; default expiry = `now + 86_400`;
  - `PartyNotEligible` for pending, suspended, KYC-expired and revoked investors, and for the
    fund treasury holder used as a party;
  - the same creator and external_ref twice → second fails; a different creator with the same
    ref → accepted;
  - seq 1, 2, 3 in creation order; `SettlementCreated` fields; every stored field equals the
    data-model layout values.
- [X] T029 [P] [US1] Add to `tests/tests/f003_setup.rs` the FR-003 case: a create before
  `init_dvp`, before escrow approval and before thaw, each → `EscrowNotReady`
- [X] T030 [US1] Write the cash-leg and Fund-side tests in `tests/tests/f003_fund.rs` (FR-011,
  FR-012, FR-014):
  - `InvalidSide` (2);
  - each final status → `InstructionFinal`; at `expires_at_ts` → `InstructionExpired`;
  - the wrong signer for each side (investor affirming the Fund side, transfer agent funding the
    investor's cash) → `Unauthorized`;
  - funding twice → `LegAlreadyFunded`;
  - a suspended investor funding cash → `PartyNotEligible`;
  - a cash source of the wrong mint, or the wrong escrow → `WrongAccount`;
  - insufficient cash → token-program error, nothing changed;
  - statuses: affirm then fund → SellerFunded → Affirmed; fund then affirm → BuyerFunded →
    Affirmed; `seller_funded_at` / `buyer_funded_at` set; `LegFunded` amount (0 for the
    affirmation).
- [X] T031 [US1] Write `tests/tests/f003_subscribe.rs`:
  - **spec independent test** (T022 fixture), with exact balances, escrow empty, I4, and I5 (the
    `SubscriptionMinted` and the treasury +$2M are in one transaction);
  - **acceptance scenarios 1–6**;
  - **settle preconditions**: Instructed / BuyerFunded → `NotAffirmed`; a final status →
    `InstructionFinal`; before `not_before_ts` → `NotYetValid`, at `not_before_ts` → OK; at
    `expires_at_ts` → `InstructionExpired`; paused → `TransfersPaused`; investor suspended after
    funding → `PartyNotEligible`;
  - **all-or-nothing**: a fund with a holder cap that is already reached and an investor holding
    no units: settle → `HolderCapReached`, nothing is minted, supply is unchanged and the cash is
    still in escrow;
  - **wrong destinations**: a cash treasury other than `fund_state.cash_treasury` →
    `WrongAccount`;
  - **twice**: settle twice → second `InstructionFinal`;
  - **anyone can trigger** (FR-015): keeper, a random key and the investor each settle an
    identical instruction with identical end state;
  - **price change after creation**: publish a new price before settle; the cash and units are
    unchanged (edge case).
- [X] T032 [P] [US1] Write the mint half of `tests/tests/f003_mint_burn_guard.rs` (FR-019):
  - direct `mint_for_subscription` signed by a random key, or by another fund's escrow PDA →
    `UnauthorizedCaller`;
  - forged settlement accounts (wrong owner, wrong address, other fund, Instructed / BuyerFunded
    / Settled status, wrong units, Redemption kind, truncated data) → `SettlementMismatch`.
    `dvp_settlement` checks the same account before calling `fund`, and only its escrow PDA can
    sign, so no transaction can deliver a forged settlement; the test calls
    `fund::instructions::mint_burn::check_settlement` directly with forged `AccountInfo`s (the
    file's header says why);
  - cash escrow short of `cash_amount` (planted) → `CashNotEscrowed`.
- [X] T033 [P] [US1] Write `tests/tests/f003_accrual.rs` (R8, R16):
  - direct `accrue` by any key → `UnauthorizedCaller`;
  - with the escrow holding units from a seeded unrelated transfer, and with publishes between,
    a subscription settle leaves the escrow's `accrued_e18`, `last_balance` and `earned_now`
    equal to the F002 reference model (units minted and delivered in one step earn nothing in
    escrow);
  - an `Accrued` event is logged with the pre and post balances;
  - `WrongHolder` and `ClassNotAccrued`: unreachable through `fund`, so the test calls
    `compliance_hook::instructions::accrue::check_holder_accounts` directly with forged accounts
    (contracts/compliance_hook.md).

### Implementation for User Story 1

- [X] T034 [US1] Implement `create_instruction` per its contract (checks 1–15 in order), using
  `mmf_common::settlement` for the cash amount and `price_fresh`, and `HolderStateView::eligible`.
  Set every field listed in T008, `next_seq += 1`, and emit `SettlementCreated`
- [X] T035 [US1] Implement `affirm_and_fund` for the **cash leg and the subscription Fund-side
  affirmation** (checks 1–9, contract order): cash `transfer_checked` through `token_interface`
  into the cash escrow, then the status update. The unit-leg branch (seller side of a Redemption
  or Secondary) returns `DvpError::InvalidSide` until T047 and is not tested before then. A
  redemption's buyer-side cash leg (from `cash_treasury`) is completed in T048
- [X] T036 [US1] Implement `compliance_hook::accrue` per its contract (checks 1–5): settle on the
  pre-balance read from the holding account with `mmf_common::accrual::settle`, set
  `last_balance` and `last_slot`, and `emit!(Accrued)`
- [X] T037 [US1] Implement `fund::mint_for_subscription` per its contract (checks 1–6), using
  `SettlementView`:
  1. day rollover (data-model pseudocode), in a helper `roll_day(fund, now)` in `mint_burn.rs`;
  2. CPI `accrue` signed by the FundState PDA;
  3. `mint_to` the unit escrow;
  4. `units_outstanding += units`;
  5. `emit_cpi!(SubscriptionMinted)`.
- [X] T038 [US1] Implement `settle` for **Subscription** (checks 1–8, contract order):
  1. CPI `fund::mint_for_subscription` signed by the escrow PDA (`dvp_config.escrow_bump`);
  2. `transfer_checked` unit escrow → buyer with the hook's extra accounts (escrow PDA signs);
  3. cash escrow → `cash_treasury`;
  4. `status = Settled`, `closed_at = now`;
  5. emit `Settled`.

  Redemption and Secondary branches return `DvpError::NotAffirmed` until T050 / T054 (they can't
  reach Affirmed before then)
- [X] T039 [US1] Make T028–T033 pass. Check CU of a subscription settle ≤ 200 000 and the call
  depth (research R15); if over budget, record the measurement in research R15 and adjust

**Checkpoint**: US1's independent test passes, every creation, cash-funding, subscription-settle
and mint rule rejects with its specific error, and I4/I5 hold.

---

## Phase 4: User Story 2 — Redeem (Priority: P1) 🎯 MVP part 2

**Goal**: an approved investor returns units and receives cash at the price of a specific price
record, within the daily redemption limit. Units are destroyed at the moment the cash is paid.

**Independent Test**: at $1.00, an investor holding 500 000 units creates a redemption, funds the
units, the transfer agent funds $500 000 from the treasury, and the keeper settles. The investor
has 500 000 fewer units and $500 000 more; those units no longer exist; `units_outstanding` fell
by 500 000; `redeemed_today` = 500 000.

### Interface stubs for User Story 2

- [X] T040 [US2] Add the `fund::burn_for_redemption(units)` stub (accounts per
  [contracts/fund.md](contracts/fund.md#burn_for_redemptionunits-dvp-escrow-pda-only-cpi)) in
  `programs/fund/src/instructions/mint_burn.rs`
- [X] T041 [P] [US2] Add SDK builders in `crates/mmf-sdk/src/ix_dvp.rs`:
  - `fund_units(seller, settlement)`, with the seller's holding account → unit escrow plus the
    hook extra accounts;
  - `fund_redemption_cash(transfer_agent, settlement)`, with source = `cash_treasury`;
  - `settle_redemption(triggered_by, settlement)`, with the seller's cash ATA, the escrow holder,
    and the fund program and event authority.

### Tests for User Story 2 (write first; they must fail) ⚠️

- [X] T042 [US2] Add the unit-leg and redemption-cash tests to `tests/tests/f003_fund.rs`
  (FR-011–FR-013):
  - units moved to escrow and the investor's balance falls; accrual settled on both sides (F002
    I11 via the reference model);
  - paused → `TransfersPaused` (hook);
  - a suspended investor → `PartyNotEligible`;
  - an investor with too few units → token error, nothing changed;
  - the transfer agent funding from a source other than `cash_treasury` → `WrongCashSource`;
  - the transfer agent funding a subscription's cash or a redemption's units → `Unauthorized`;
  - funding before `not_before_ts` is allowed.

  The F004 collateral-lock case (US2 scenario 3) is recorded as a comment in `f003_fund.rs`
  stating the test F004 adds (pledge a holder's only unit, then `fund_units` →
  `PledgedUnitsLocked`). An `#[ignore]`d placeholder that panics was tried and removed: it broke
  `cargo test -- --ignored`
- [X] T043 [US2] Write `tests/tests/f003_redeem.rs`:
  - **spec independent test**;
  - **acceptance scenarios 1, 2, 4, 6**; cash = `floor(units × nav / 10^8)` at a non-$1 price;
  - **limit**: with `units_at_day_start = N` and the fixture's `redemption_gate_bps`, a
    redemption of exactly the limit settles; a second of 1 unit → `RedemptionLimitExceeded`,
    stays Affirmed with escrow unchanged; after `set_utc_day(20 740, 60)` it settles and
    `units_at_day_start` is the new day's figure;
  - subscriptions settled during the day don't raise the limit;
  - first day (no units at day start) → `RedemptionLimitExceeded`;
  - paused → `TransfersPaused` even though a burn runs no hook;
  - redeemer suspended after funding → `PartyNotEligible`;
  - missing seller cash ATA → fails and nothing moves; it succeeds after anyone creates the ATA;
  - I4 after every settle; `RedemptionBurned` payload.
- [X] T044 [P] [US2] Write the burn half of `tests/tests/f003_mint_burn_guard.rs` (FR-020):
  - direct `burn_for_redemption` by a random key or another fund's escrow PDA →
    `UnauthorizedCaller`;
  - forged settlement accounts (as T032) → `SettlementMismatch`;
  - a holder trying to burn its own units through Token-2022's permissioned burn fails (F001
    behaviour, re-asserted).
- [X] T045 [P] [US2] Add to `tests/tests/f003_accrual.rs`: units funded to escrow, then a
  publish, then a redemption burn. The escrow's `accrued_e18`, `last_balance` (0 after the burn)
  and `earned_now` match the reference model, and the redeemer earns nothing for the publish
  while its units were in escrow (R16)

### Implementation for User Story 2

- [X] T046 [US2] Implement `fund::burn_for_redemption` per its contract (checks 1–5):
  1. `roll_day`;
  2. `redemption_ok` (`RedemptionLimitExceeded`);
  3. CPI `accrue(post_balance = escrow balance − units)`;
  4. permissioned `burn_checked` via `mmf_common::permissioned_burn::burn_checked` (FundState
     PDA signs as burn authority; the escrow PDA's signature carries through as owner);
  5. `units_outstanding −= units`; `redeemed_today += units`;
  6. `emit_cpi!(RedemptionBurned)`.
- [X] T047 [US2] Complete `affirm_and_fund`'s **unit-leg** branch (replacing T035's placeholder):
  check the seller is `eligible`, then `transfer_checked` from the seller's holding account to the
  unit escrow by CPI, with the seller as authority and the five hook extra accounts passed
  through (research R5)
- [X] T048 [US2] Complete `affirm_and_fund`'s **redemption buyer side**: signer = transfer agent;
  check 8 `source == fund_state.cash_treasury` (`WrongCashSource`); move the cash into the cash
  escrow
- [X] T049 [P] [US2] Write `tests/tests/f003_escrow.rs` (FR-030, I13): send units (from an
  approved investor) and cash directly to the escrows outside any instruction. Then run a full
  subscription and a redemption: none of them releases the stray amounts, and the escrow
  balances equal Σ held + the stray amounts after every step. (Returns don't exist until US4, so
  the "expired instruction's returns" step is added to this file in T060.)
- [X] T050 [US2] Implement `settle` for **Redemption**:
  1. the party checks, plus the explicit `Registry.transfers_paused` check (`TransfersPaused`,
     research R12);
  2. CPI `fund::burn_for_redemption` signed by the escrow PDA;
  3. cash escrow → seller's cash ATA (`WrongAccount` if the account passed isn't
     ATA(seller, cash_mint));
  4. Settled; emit `Settled`.
- [X] T051 [US2] Make T042–T045 and T049 pass (the F004 placeholder stays ignored)

**Checkpoint**: US1 + US2 are the MVP. Units can be created against cash and destroyed against
cash, within the daily limit, both-or-neither.

---

## Phase 5: User Story 3 — Secondary transfer against cash (Priority: P2)

**Goal**: two approved holders swap existing units for cash with the same both-or-neither
guarantee. No units are created or destroyed.

**Independent Test**: A (1 000 000 units) and B agree 1 000 000 units for $999 500. Both fund,
the keeper settles; B holds the units, A has the cash, and `units_outstanding` is unchanged.

- [X] T052 [P] [US3] Add `settle_secondary(triggered_by, settlement)` to
  `crates/mmf-sdk/src/ix_dvp.rs`, with the buyer's holding account, the seller's cash ATA and the
  hook extra accounts for escrow → buyer
- [X] T053 [US3] Write `tests/tests/f003_secondary.rs` (tests must fail):
  - **spec independent test**; acceptance scenarios 1–4 (scenario 5's `SameParty` / `ZeroCash`
    are in `f003_create.rs`, so reference them in a comment);
  - creation by the buyer, then funding in both orders;
  - only the seller funded → `NotAffirmed`;
  - buyer suspended after funding → `PartyNotEligible`;
  - the buyer has no units and the holder cap is reached → `HolderCapReached`, nothing moves;
  - after settle, a seller whose balance reached zero is no longer counted in `holder_count`
    (I10) and the buyer is;
  - supply and `units_outstanding` unchanged; both sides' interest settled (F002 I11 with a
    publish between funding and settle).
- [X] T054 [US3] Implement `settle` for **SecondaryTransfer**: unit escrow → buyer
  (`transfer_checked` with the hook's extra accounts, escrow PDA signs), cash escrow → seller's
  cash ATA, then Settled and `Settled`. Remove the T038 placeholder for this kind
- [X] T055 [US3] Make T053 pass

**Checkpoint**: all three kinds settle both-or-neither.

---

## Phase 6: User Story 4 — Cancel, expire, reject (Priority: P2)

**Goal**: unfunded or stuck instructions can be cancelled or expired, compliance can reject any
open instruction, and every funded leg goes back to its funder through a return step that anyone
can trigger and that can be retried (research R3).

**Independent Test**: three instructions with one side funded each: cancel the first, expire the
second after its expiry, have compliance reject the third, then trigger the returns. Every funder
ends with exactly its starting units and cash, and the escrows hold nothing for them.

Unit returns need US2's unit funding (T047). Cash-only cases need only US1.

### Interface stubs for User Story 4

- [X] T056 [US4] Add stubs, with account structs per
  [contracts/dvp_settlement.md](contracts/dvp_settlement.md):
  - `cancel(side)` and `withdraw_cancel(side)` in
    `programs/dvp_settlement/src/instructions/cancel.rs`;
  - `expire()` in `expire.rs`;
  - `reject(reason, case_ref_hash, actor)` in `reject.rs`;
  - `return_leg(leg)` in `return_leg.rs`;
  - `governance::reject_settlement(reason, case_ref_hash)` in
    `programs/governance/src/instructions/reject_settlement.rs`
    ([contracts/governance.md](contracts/governance.md)).
- [X] T057 [P] [US4] Add SDK builders:
  - `ix_dvp::{cancel, withdraw_cancel, expire, return_units, return_cash}`. `return_cash` picks
    ATA(buyer, cash_mint), or `cash_treasury` for a Redemption. `return_units` adds the hook
    extra accounts for escrow → seller;
  - `ix_governance::reject_settlement(compliance, ctx, settlement, reason, case_ref_hash)`;
  - `pda::dvp_caller` usage.

  Squads-wrapped variants follow `tests/src/squads.rs`'s existing pattern for compliance actions

### Tests for User Story 4 (write first; they must fail) ⚠️

- [X] T058 [US4] Write `tests/tests/f003_cancel_expire_reject.rs`:
  - **spec independent test**;
  - **cancel**: either side cancels Instructed, SellerFunded and BuyerFunded, with the earmarks
    equal to the held amounts; an outsider → `Unauthorized`; `InvalidSide`;
  - **Affirmed cancel**: one side → `CancelRequested`, status still Affirmed, and settle still
    succeeds with one request; that side again → `CancelAlreadyRequested`; both sides →
    Cancelled; `withdraw_cancel` clears the bit; withdraw with no own request, or after both →
    `NoCancelRequest`; cancel after expiry is allowed;
  - **expire**: before `expires_at_ts` → `NotExpired`; at it → Expired; Instructed → Expired with
    both earmarks 0; anyone can trigger; works while paused;
  - **final**: every step on each final status → `InstructionFinal` (all of `affirm_and_fund`,
    `settle`, `cancel`, `withdraw_cancel`, `expire` and `reject`);
  - **events**: `SettlementCancelled`, `SettlementExpired` and `closed_at`.
- [X] T059 [P] [US4] Write `tests/tests/f003_reject.rs` (FR-025; contracts/governance.md):
  - compliance via Squads rejects an instruction in each open status; earmarks are set; one
    `SettlementRejected` with `reason`, `case_ref_hash` and `actor`;
  - signer is the issuer, fund admin, transfer agent, keeper, auditor or an investor →
    `GovernanceError::Unauthorized`;
  - reasons 4, 5 and 7 → `InvalidReason`;
  - an instruction of another fund → `DvpError::FundMismatch`;
  - a final instruction → `InstructionFinal`;
  - `dvp_settlement::reject` called directly by compliance → `UnauthorizedCaller`;
  - the edge case "suspended after funding, then compliance rejects".
- [X] T060 [P] [US4] Write `tests/tests/f003_returns.rs` (FR-026, FR-027):
  - units and cash returned after cancel, expire and reject, for all three kinds (a redemption's
    cash goes back to `cash_treasury`);
  - `NothingToReturn` after a return and for an unfunded leg; `NotReturnable` on open and
    Settled instructions; `InvalidLeg`; a wrong recipient → `WrongAccount`;
  - **undeliverable returns**: a suspended seller's unit return fails (hook
    `ReceiverNotApproved`) and the earmark is kept; after reinstatement it succeeds. Paused →
    fails, then succeeds after unpause. A buyer without a cash ATA → fails, then succeeds after
    the ATA is created;
  - expire plus both returns in one transaction succeeds atomically; when the unit return can't
    be delivered, the keeper's fallback (expire alone, then the cash return) leaves the units
    earmarked;
  - `LegReturned` events;
  - in `tests/tests/f003_escrow.rs`: an expired instruction's returns release only its own
    earmarks, never the stray deposits (moved here from T049).

### Implementation for User Story 4

- [X] T061 [US4] (The signed transfer helpers already exist: US1 needed them for `settle`, so
  `programs/dvp_settlement/src/token.rs` has `transfer_cash` and `transfer_units` since T035/T038.
  This task adds only the earmarking.) Create `programs/dvp_settlement/src/escrow.rs`:
  - `earmark(s)`: sets `units_to_return = s.units_held()` and `cash_to_return = s.cash_held()`
    (computed before the status changes), then `closed_at`;
  - `escrow_transfer_units(...)` and `escrow_transfer_cash(...)`: signed CPI helpers with the
    escrow PDA seeds.

  Refactor `settle` (T038, T050, T054) to use the transfer helpers, with no behaviour change; US1
  to US3 tests stay green
- [X] T062 [US4] Implement `cancel` and `withdraw_cancel` per their contracts (R17): bit 0 seller
  side, bit 1 buyer side; `CancelRequested`, `CancelRequestWithdrawn` and `SettlementCancelled`
  (the last one when cancelled)
- [X] T063 [US4] Implement `expire` per its contract (checks 1–3; no Fund status or pause check),
  `earmark`, `SettlementExpired`
- [X] T064 [US4] Implement `dvp_settlement::reject` (signer must be governance's
  `["dvp-caller"]`, derived with `GOVERNANCE_PROGRAM_ID`, else `UnauthorizedCaller`; then
  `FundMismatch`, then `InstructionFinal`), `earmark`, and `SettlementRejected`. Implement
  `governance::reject_settlement` (checks 1–3, then a CPI signed by `["dvp-caller"]` with
  `actor = compliance`)
- [X] T065 [US4] Implement `return_leg` per its contract (checks 1–5): transfer the earmarked
  amount out of escrow through the `escrow.rs` helpers (units through the hook), zero the
  earmark, and emit `LegReturned`
- [X] T066 [US4] Make T058–T060 pass

**Checkpoint**: no funded leg can be stuck. Every unwind completes, and each return either
succeeds or stays earmarked and can be retried.

---

## Phase 7: User Story 5 — Slow signers (Priority: P2)

**Goal**: a custodian approves a prepared step hours later and it still succeeds, as long as the
instruction is open and not expired. A signed step never takes effect twice.

**Independent Test**: a funding step prepared with a durable nonce and submitted 6 h later
succeeds. One prepared for an instruction that is cancelled before submission is rejected.

- [X] T067 [P] [US5] Create `crates/mmf-sdk/src/prepare.rs` (research R13):
  `with_durable_nonce(ixs, nonce_account, nonce_authority, payer, nonce_value) -> Message`. It
  puts `AdvanceNonceAccount` at index 0 and the nonce value as the blockhash, and returns an
  unsigned legacy message. Add `read_nonce(account_data) -> Hash`. Export it from `lib.rs`
- [X] T068 [US5] Add `tests/src/nonce.rs`:
  - `harness_with_blockhash_check()`: a `Harness` whose LiteSVM is built with
    `with_blockhash_check(true)`;
  - `create_nonce_account(h, authority) -> Pubkey`;
  - `expire_recent_blockhashes(h)` (call `expire_blockhash` repeatedly).

  Check that the T022 fixture works under this harness (send helpers use the latest blockhash)
- [X] T069 [US5] Write `tests/tests/f003_slow_signers.rs` (FR-028, FR-029, SC-004):
  - prepare the investor's `fund_cash` step with a durable nonce, advance the clock 6 h and expire
    the blockhashes, then sign and submit → succeeds; the same for 23 h;
  - **control**: the same step with an ordinary recent blockhash after expiry →
    `BlockhashNotFound`;
  - prepare the step, cancel the instruction, then submit → `InstructionFinal`;
  - a prepared create with a known creator and external ref, submitted after another party's
    create has taken `seq` → succeeds (R2);
  - the same signed transaction submitted twice → the second fails (nonce advanced), and
    re-preparing it with the new nonce → `LegAlreadyFunded`;
  - a prepared step submitted after `expires_at_ts` → `InstructionExpired`.

  The prepare helper is the code under test, so write this test before T067's body (stub
  returning an empty message first)
- [X] T070 [US5] Make T069 pass. No program change is expected; if one is needed, stop and record
  why in research R13

**Checkpoint**: all five stories pass their independent tests.

---

## Phase 8: Polish & cross-cutting concerns

- [X] T071 [P] Write `tests/tests/dvp_prop.rs` (proptest, SC-001, SC-002, SC-003, SC-005):
  - random sequences of 50–200 operations over 4 investors and 3 UTC days, using the T014 model
    as the oracle;
  - operations: create (all kinds, random amounts and expiries), fund (each side), settle,
    cancel, withdraw_cancel, expire, reject, return_leg (each leg), suspend, reinstate, pause,
    unpause, publish (prices within the move limit), advance clock;
  - after every step: program result == model result (same error name); party units and cash ==
    model; I4, I5, I6 (for each final instruction whose returns are done), I13 (equality: no
    stray deposits), I14; and the F001/F002 invariants I3, I10, I11.

  A release-mode `#[ignore]` variant runs 200 cases of 100–200 operations (each case sets up a
  fund, so 1 000 would take far longer than the rest of the suite; the default run is 24 cases,
  raised with `PROPTEST_CASES`). Publishes use rate 0: exact interest is covered by
  `accrual_prop.rs` (F002) and `f003_accrual.rs`
- [X] T072 [P] Extend `tests/tests/invariants_prop.rs` with `assert_i4` after every op (seeded
  units only), and add a dvp-ready fixture variant. F001/F002 test edit, to be listed in the PR
- [X] T073 [P] Extend `tests/tests/scale.rs` (SC-007, `#[ignore]`, release): the CU of a
  subscription `settle` and a redemption `settle` is identical with 10 vs 10 000 registered
  holders, and with 1 vs 1 000 open instructions (created by the fixture)
- [X] T074 [P] Extend `tests/src/rebuild.rs` and add `tests/tests/f003_rebuild.rs` (SC-008, FR-031).
  From the settlement events alone (`SettlementCreated`, `LegFunded`, `Settled`,
  `CancelRequested`, `CancelRequestWithdrawn`, `SettlementCancelled`, `SettlementExpired`,
  `SettlementRejected`, `LegReturned`, `SubscriptionMinted`, `RedemptionBurned`), rebuild every
  instruction's status, earmarks and escrow movements, and compare them with on-chain state after
  a mixed scenario
- [X] T075 Run `scripts/check-error-coverage.sh`. Every `DvpError`, the new `FundError` and
  `HookError` variants, and the governance errors used by `reject_settlement` are asserted by a
  test (SC-006). Add any missing rejection test
- [X] T076 [P] Run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check`, and fix any issues
- [X] T077 [P] Extend `scripts/devnet-e2e` with `--feature f003` (in `src/f003.rs`), following
  [quickstart.md](quickstart.md#devnet-run-scripts-devnet-e2e---feature-f003):
  - mock stablecoin;
  - `set_cash_treasury`, `init_dvp`, then escrow approve and thaw (compliance as a plain key, as
    in F002's devnet steps; the Squads path is proven in `f003_reject.rs`);
  - a subscription whose funding is prepared with a real durable nonce and submitted after more
    than 2 minutes;
  - an expired one-sided secondary, then its return;
  - a redemption refused today by the first-day rule (research R9). `--feature f003 --continue
    <mint>` settles it on a later UTC day.

  Print the signatures and balances, and exit 0 only if every outcome matches. Rehearse on a
  local validator

- [X] T078 [P] Update the root `README.md`: F003 status, one paragraph on how settlement works
  (create → fund → settle; cancel, expire or reject → return), the new program, and the new test
  files. Link [quickstart.md](quickstart.md)
- [X] T079 [P] Write `specs/003-dvp-settlement/review.md`, the human review checklist in the F002
  `review.md` format, with code links for:
  - money math against shared-model §8 (subscription ceil, redemption floor, limit
    cross-multiplied);
  - the mint/burn guard against §5.2 (escrow PDA plus settlement-account checks);
  - signer checks against §5.1 for every new instruction;
  - CPI edges against §5.2 (dvp → fund, fund → accrue, governance → dvp);
  - invariants I4, I5, I6, I13 and I14;
  - the Constitution IV exception for `settle`;
  - FundState offsets unchanged;
  - every F001/F002 test edit (T011, T013, T018, T072).
- [ ] T080 *(not done: needs the user's go-ahead, devnet SOL and the deployed programs. Rehearsed on a local validator 2026-10-13: 31 of 31 steps of `--feature f003` matched; the `--continue` step needs a later UTC day)* Devnet run (needs the user's go-ahead, about 15–20 devnet SOL, and the issuer's
  Squads vault). No program is on devnet yet (F001 T079 and F002 T039 are also open), so this
  is a first deployment of all four programs: `anchor deploy --provider.cluster devnet`, then each
  upgrade authority moved to the issuer's Squads vault with a time lock ≥ 1 h. Then run
  `cargo run -p devnet-e2e` (F001/F002 steps) and `cargo run -p devnet-e2e -- --feature f003`,
  and `--feature f003 --continue <mint>` on a later UTC day. Record program IDs and signatures in
  `specs/003-dvp-settlement/devnet-run.md` **Devnet 2026-10-15**: all five programs deployed and the run passed; see [devnet-run.md](../004-collateral-pledges/devnet-run.md). Still open: the Squads upgrade-authority handover and the `--continue` step on a later UTC day.
- [ ] T081 Run the full suite and validation:
  `anchor build && cargo test -p mmf-common && cargo test -p tests && cargo test -p tests --release -- --ignored`
  and [quickstart.md](quickstart.md). Open the pull request `003-dvp-settlement` → `main`,
  listing:
  - the shared-model changes (plan "Shared-model changes");
  - the spec adjustments;
  - every F001/F002 test or helper edit (T011, T013, T018, T072), with its reason
    (Constitution XII).

  A human reviewer completes T079's checklist before merge.
  **Status 2026-10-14**: full suite green (278 + 45 tests; 4 release-only tests pass, incl.
  SC-007 and 200 property cases; 113 of 113 errors covered; clippy and fmt clean). PR
  [#3](https://github.com/jaxter03/tokenized-mmf-collateral/pull/3) is open. It is left unchecked
  until the reviewer signs off and it merges

---

## Dependencies & execution order

### Phase dependencies

- **Setup (Phase 1)**: none. T003 needs T002; T004 needs T002.
- **Foundational (Phase 2)**: needs Setup.
  - T010 needs T007 and T008; T011 needs T007, T008 and T010.
  - T017 and T018 need T015 and T016.
  - T019–T021 need T017 and T018 (tests first); T022 needs T019–T021.
  - T024 needs all of T005–T023.

  Blocks every story.
- **US1 (Phase 3)**: needs Foundational.
- **US2 (Phase 4)**: needs US1's `create_instruction` (T034), `accrue` (T036) and the `settle`
  skeleton (T038). T040–T045 may be written once T027 exists.
- **US3 (Phase 5)**: needs US2's unit funding (T047).
- **US4 (Phase 6)**: needs US1. Its unit-return tests need T047 (US2). T061 refactors `settle`,
  so do it after T054 if US3 is done first; otherwise extend the helpers when US3 lands.
- **US5 (Phase 7)**: needs US1 (funding) and T062 (cancel) for the "cancelled before submit"
  case.
- **Polish (Phase 8)**: needs every story. T080 needs T077; T081 needs everything.

### Within each story

Interface stubs → SDK builders → tests (run them and see them fail) → implementation →
checkpoint. Every rule's rejection test comes before the code that enforces it (Principle VI).

### Story order

```text
Setup → Foundational → US1 (subscribe) → US2 (redeem) → US3 (secondary)
                                     │                 └→ US4 unit returns
                                     ├→ US4 (cancel/expire/reject/return; cash cases after US1)
                                     └→ US5 (slow signers; needs cancel from US4)
→ Polish
```

---

## Parallel opportunities

- **Foundational**: T005, T006, T009, T012 and T014 together, alongside T007 → T008 → T010 →
  T011; then T013. In setup, T016 runs alongside T015.
- **US1**: T026 alongside T025; tests T028, T029, T032 and T033 in separate files; T030 and T031
  after T027.
- **US2**: T041 alongside T040; tests T044, T045 and T049 alongside T042 and T043.
- **US4**: T057 alongside T056; T059 and T060 alongside T058.
- **US5**: T067 alongside T068.
- **Polish**: T071–T074 and T076–T079 together.

### Parallel example: User Story 1 tests

```text
Task: "Write tests/tests/f003_create.rs (FR-004–FR-009)"                 (T028)
Task: "Write the mint half of tests/tests/f003_mint_burn_guard.rs"       (T032)
Task: "Write tests/tests/f003_accrual.rs (escrow accrue)"                (T033)
```

### Parallel example: User Story 4 tests

```text
Task: "Write tests/tests/f003_reject.rs (governance → dvp reject)"       (T059)
Task: "Write tests/tests/f003_returns.rs (earmarked returns)"           (T060)
```

---

## Implementation strategy

### MVP first (US1 + US2, both P1)

1. Phase 1 Setup → Phase 2 Foundational. **Stop if T024 shows an F001/F002 regression** and fix
   it first (Principle I).
2. Phase 3 US1 → validate the $2M subscription end to end, I4 and I5.
3. Phase 4 US2 → validate the $500 000 redemption and the daily limit. Units that can come in but
   never go out aren't a usable fund, so the MVP is US1 + US2.

### Then

4. US3 (secondary), US4 (unwinds and returns), US5 (slow signers), each validated at its
   checkpoint.
5. Phase 8: property tests, scale, rebuild, error coverage, devnet, review, merge.

Commit after each group of 2–4 related tasks on `003-dvp-settlement`, and push as you go.

---

## Notes

- `[P]` = different files and no dependency on an unfinished task.
- Tests that plant state with `set_account` are allowed **only** for conditions no instruction can
  reach (T032 forged settlements, T032/T044 short cash escrow). Each says so in a comment.
- No task builds investor `accrue`, `holder_count` updates in `accrue`, collateral locks (F004)
  or distributions (F006) (Principle XI; research R8).
- New error variants are appended, so earlier error codes don't move. Any F001/F002 test edit
  beyond T011, T013, T018 and T072 must be justified in the PR (Principle XII).
- If a task needs a shared-model change not already in `docs/shared-model.md`, stop and make it
  there first (Principle XIII).
