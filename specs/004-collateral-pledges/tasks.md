---
description: "Task list for F004: Collateral Agreements, Margin Calls and Pledges"
---

# Tasks: Collateral Agreements, Margin Calls and Pledges

**Input**: Design documents from `specs/004-collateral-pledges/`: [plan.md](plan.md),
[spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md),
[contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: REQUIRED. Constitution Principle VI says every rule needs a test that is written and
seen to fail before its code, with at least one rejection test asserting the exact error. SC-006
says the same.
- **Order within each story**: interface stubs → SDK builders → tests (must fail) →
  implementation.
- **Stub handlers** return `Ok(())` and do nothing, so happy-path tests fail on missing state and
  rejection tests fail on a missing error.

**Organization**: by user story, in spec priority order: US1, US2 and US3 (P1), then US4 and US5
(P2), then US6 (P3).
- **Foundational**: the shared formulas, accounts, model and a base fixture.
- **Polish**: the cross-cutting requirements that span every story: party status and pause
  (FR-034, FR-035), slow signers (FR-036), properties, scale and records.
- **Checkpoints**: each story phase ends with one, where that story's independent test passes.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: US1–US6, from spec.md
- Paths are from the repository root

## Path conventions (from plan.md)

- Programs: `programs/{collateral,compliance_hook,governance}/src/` (`fund` and `dvp_settlement`
  don't change)
- Shared crate: `crates/mmf-common/src/`; client SDK: `crates/mmf-sdk/src/`
- LiteSVM tests: `tests/src/` (harness, fixtures, model, helpers) and `tests/tests/` (test files)
- Scripts: `scripts/`

## Fixed values used in tests

- **Decimals**: 6 for units and cash. Prices: $1.00 = `100_000_000`, $0.99 = `99_000_000`,
  $0.985 = `98_500_000`, $1.01 = `101_000_000`. The collateral fixtures set
  `max_nav_move_bps = 500` (T017). 2026-10-14 = UTC day **20 740**.
- **Worked terms** (`worked_terms()` in fixtures):

  | Term | Value |
  |---|---|
  | `haircut_bps` | `200` |
  | `min_transfer_value` | `10_000_000_000` ($10 000) |
  | `call_cure_secs` | `14_400` |
  | `enforcement_grace_secs` | `7_200` |
  | `concentration_cap_units` | `5_000_000_000_000` |
  | `max_nav_age_secs` | `93_600` (26 h) |
  | `taker_may_call_unilaterally` | `true` |
  | `agreement_hash` | `[7; 32]` |
  | `mode`, `income_to` | `0`, `0` |
- **Giver G** holds `2_000_000_000_000` units (2 000 000).
- **Worked figures** (data-model table):

  | Figure | Base units |
  |---|---|
  | $1M exposure | `1_000_000_000_000` |
  | units for $1M | `1_020_408_163_266` |
  | unlocked after that pledge | `979_591_836_734` (send `979_591_836_735` fails) |
  | return for $600 000 | `612_244_897_959` units |
  | lock left after the return | `408_163_265_307` |
  | shortfall at $0.99 | `10_000_000_000` (= MTA: no call) |
  | shortfall at $0.985 | `15_000_000_000` → `15_539_210_609` units |

---

## Phase 1: Setup

**Purpose**: confirm a green F001–F003 baseline, then add the fifth program and the new crate
edges.

- [X] T001 Run `anchor build && cargo test -p mmf-common && cargo test -p tests` on the branch
  before any change. Confirm F001–F003 are green (Constitution XII baseline), and record the test
  count in the PR description draft
- [X] T002 Create the `collateral` program (research R1):
  - `programs/collateral/Cargo.toml` with
    `anchor-lang = { version = "=1.1.2", features = ["event-cpi"] }`, `mmf-common`, and
    `compliance_hook = { path = "../compliance_hook", features = ["cpi", "no-entrypoint"] }`,
    plus the `cpi` / `no-entrypoint` / `idl-build` features copied from
    `programs/dvp_settlement/Cargo.toml`;
  - `programs/collateral/src/lib.rs` with `declare_id!`;
  - run `anchor keys sync` (the keypair stays in `target/deploy/`, never committed);
  - add the ID to `Anchor.toml` (`[programs.devnet]` and `[programs.localnet]`) and to
    `crates/mmf-common/src/ids.rs` as `COLLATERAL_PROGRAM_ID`, with the same doc comment as the
    others.

  `anchor build` must produce five `.so` files
- [X] T003 Add the crate edges from plan.md "Primary Dependencies":
  - `collateral = { path = "../collateral", features = ["cpi", "no-entrypoint"] }` in
    `programs/governance/Cargo.toml`;
  - `collateral = { path = "../programs/collateral", features = ["no-entrypoint"] }` in
    `tests/Cargo.toml`.

  Confirm there is no cycle: `cargo tree -p compliance_hook | grep collateral` and
  `cargo tree -p collateral | grep governance` must both be empty. `anchor build` succeeds and
  `scripts/check-duplicate-crates.sh` passes
- [X] T004 In `tests/src/harness.rs`, load `collateral.so` at `COLLATERAL_PROGRAM_ID` alongside
  the four existing programs. Run the suite: still green

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: the shared formulas, account layouts, views, the reference model, `init_registry`
filling `collateral_program`, and a base fixture that every story needs.

**⚠️ CRITICAL**: no story work starts until Phase 2 is complete and T019 shows F001–F003 still
green.

### Shared code

- [X] T005 [P] Create `crates/mmf-common/src/collateral.rs` and export it from `lib.rs`
  (data-model "Units and formulas", research R4, R5). All functions are pure and use checked
  `u128` math; each returns `None` on overflow or on a result that doesn't fit:
  - `collateral_value(units: u64, nav: u64, haircut_bps: u16) -> Option<u64>` =
    `floor(units × nav × (10_000 − h) / (10^8 × 10_000))` ("down: taker never over-counts");
  - `units_for_value_up(value: u64, nav: u64, haircut_bps: u16) -> Option<u64>` =
    `ceil(value × 10^8 × 10_000 / (nav × (10_000 − h)))` ("up: giver always posts enough");
  - `units_for_value_down(…) -> Option<u64>`, the same with floor ("down: taker keeps enough");
  - `coverage(pledged_units, nav, haircut_bps, required_value) -> Option<i128>`;
  - `price_age(nav_seq: u64, nav_ts: i64, now: i64, max_age_secs: u32) -> PriceAge`
    `{ Fresh, NoPrice, Stale }`, where Fresh iff `nav_seq > 0 ∧ now − nav_ts ≤ max_age_secs`
    (exactly at the max age is Fresh);
  - `Observed { called_outstanding: u64, open_delivery_calls: u16, met_below_seq: u64 }` and
    `observe(state: Observed, shortfall: u64, call_count: u64) -> Observed`:
    `called_outstanding = min(D, S)`, then if `S == 0` set
    `met_below_seq = call_count, D = 0, count = 0`;
  - `delivery_met(stored_status: u8, seq: u64, met_below_seq: u64) -> bool`, true iff the stored
    status is Open (0) or Disputed (2) and `seq < met_below_seq`.

  Unit tests:
  - every row of the data-model worked-checks table;
  - ceil vs floor at an exact multiple (equal) and one base unit above;
  - `haircut_bps = 9_999` works, and overflow gives `None`;
  - `price_age` at max age and at max age + 1;
  - `observe` with S above, equal to and below D, and with S = 0;
  - `delivery_met` for each status
- [X] T006 [P] In `crates/mmf-common/src/seeds.rs`, add:
  - `AGREEMENT = b"agreement"` (`["agreement", fund_state, giver, taker, nonce_le]`);
  - `PLEDGE = b"pledge"` (`["pledge", agreement, pledge_ref]`);
  - `CALL = b"call"` (`["call", agreement, external_ref]`);
  - `HOOK_CALLER = b"hook-caller"` (collateral; signs `set_lien`);
  - `COLLATERAL_CALLER = b"collateral-caller"` (governance; signs `resolve_dispute`).

  In `crates/mmf-common/src/enums.rs`, add `u8_enum!`s with `[F004]` doc comments:
  - `PledgeMode { Lien = 0, TitleTransfer = 1 }`;
  - `IncomeTo { Giver = 0, Taker = 1, TakerOwesGiver = 2 }`;
  - `AgreementStatus { Proposed = 0, Active = 1, Terminating = 2, Terminated = 3 }`;
  - `PledgeStatus { Active = 0, Released = 1, Enforced = 2, Substituted = 3 }`;
  - `CallKind { Delivery = 0, Return = 1 }`;
  - `CallStatus { Open = 0, Met = 1, Disputed = 2, Defaulted = 3, Enforced = 4, Cancelled = 5 }`;
  - `ExposureOutcome { Applied = 0, Pending = 1, Confirmed = 2 }`;
  - `ReasonCode::allowed_for_resolve_dispute()`, the same list as `allowed_in_f001`.

  Extend the round-trip unit test (unknown values are errors)
- [X] T007 [P] In `crates/mmf-common/src/views.rs`, add `nav_ts: i64` to `FundStateView`
  (offset `FUND_NAV + 8`; `FUND_NAV_SEQ` already sits after it) and decode it. Extend the
  FundState decode test with a known `nav_ts`
- [X] T008 Create `programs/collateral/src/state.rs`, `errors.rs` and `events.rs` per
  data-model.md:
  - **Accounts**: `PledgeAgreement`, `Pledge` and `CollateralCall`, with **exactly** the field
    order and types listed and `#[derive(InitSpace)]`. Sizes must be **432 B** (8 + 424), **196 B**
    and **268 B**. Keep the data-model annotations as doc comments (e.g. `haircut_bps`:
    "< 10_000", `concentration_cap_units`: "pledge limit; > 0", `external_ref`: "non-zero; unique
    per agreement (in the seeds)").
  - **Errors**: `CollateralError` with the variants in the order of contracts/collateral.md
    "Errors".
  - **Events**: every event in the data-model "Events" table. Each has `fund: Pubkey`,
    `slot: u64` and `unix_ts: i64`. Valuing events also carry `coverage: i128`, `nav_seq: u64`,
    `called_outstanding: u64` and `met_below_seq: u64`. `ExposureUpdated` carries
    `outcome: u8` and an `opened_call: Option<CallOpened>`.
- [X] T009 [P] In `programs/compliance_hook/src/errors.rs`, **append** (after `ClassNotAccrued`)
  `PledgedUnitsLocked`, `InsufficientUnlockedUnits`, `NotCollateralCaller` and `LienUnderflow`.
  Add the `LienSet` event to `events.rs` (`owner`, `lock`, `unlock`, `opened`, `closed`,
  `pledged_amount`, `active_liens`, `slot`, `unix_ts`). Add
  `#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)] pub struct LienChange { lock: u64, unlock: u64, opened: u16, closed: u16 }`
  to `state.rs`
- [X] T010 Update `tests/tests/layouts.rs` (depends on T007, T008): assert the three new account
  sizes and the `FundStateView` `nav_ts` offset against a serialized FundState
- [X] T011 [P] In `crates/mmf-sdk/src/pda.rs`, add `agreement(fund_state, giver, taker, nonce)`,
  `pledge(agreement, pledge_ref)`, `call(agreement, external_ref)`, `hook_caller()` and
  `collateral_caller()`, with a unit test that each matches `find_program_address` over the
  seeds from T006
- [X] T012 [P] Add the independent reference model `tests/src/collateral_model.rs` (research R15).
  It must **not** import `mmf_common::collateral`: it has its own copies of the three formulas
  and of `observe`.
  - **State**: per-holder balance and lock; agreements (terms, status, required value, pending
    raise, D, open-call count, `met_below_seq`, open return call); pledges; calls; the price
    (`nav`, `nav_seq`, `nav_ts`).
  - **Operations**: one method per instruction, returning `Ok(effects)` or the expected error
    name as `&'static str`. Plus `publish(nav)`, `transfer(from, to, units)` (fails when it
    would leave a balance below its lock) and `advance(secs)`.
  - **Checks**: `check_invariants()` asserts I1, I2, I15 and I16.

  Unit-test the model on the US2–US4 worked figures
- [X] T013 Create `programs/collateral/src/valuation.rs` (depends on T005, T007, T008):
  - `fresh(fund: &FundStateView, a: &PledgeAgreement, now) -> Result<()>` (`NoPrice` /
    `StalePrice`);
  - `shortfall` / `excess` from `mmf_common::collateral::coverage` (`Overflow` on `None`);
  - `observe(a: &mut PledgeAgreement, fund, now)`, which applies `mmf_common::collateral::observe`
    to the agreement's fields;
  - `settle_status(call: &mut CollateralCall, a: &PledgeAgreement, now)`, which stores Met and
    `closed_at` when `delivery_met` holds;
  - `eligible(holder: &HolderStateView, now) -> bool` (Investor, Approved, `kyc_expiry_ts > now`).

  Add Rust unit tests for the pure parts

### Registry and base fixture

- [ ] T014 Add the `compliance_hook::set_lien(change: LienChange)` **stub** (returns `Ok(())`),
  with the full account struct from contracts/compliance_hook.md, in
  `programs/compliance_hook/src/instructions/set_lien.rs`, and register it in `lib.rs`. Add the
  SDK builder `ix_hook::set_lien(hook_caller, registry, holder_state, holding_account, change)` in
  `crates/mmf-sdk/src/ix_hook.rs` (used by negative tests)
- [ ] T015 Write `tests/tests/f004_lien.rs` part 1 (tests must fail):
  - a freshly created fund's Registry has `collateral_program == COLLATERAL_PROGRAM_ID`;
  - `set_lien` signed by a random key in the `hook_caller` position → `NotCollateralCaller`;
  - the real `["hook-caller"]` address passed without a signature → `NotCollateralCaller`;
  - a Registry planted with an empty `collateral_program` (via `set_account`, commented: "models
    a pre-F004 registry, R16; no instruction can produce it") → `NotCollateralCaller`
- [ ] T016 In `programs/compliance_hook/src/instructions/init_registry.rs`, set
  `reg.collateral_program = COLLATERAL_PROGRAM_ID` next to `reg.governance`. In `set_lien`,
  implement **check 1 only** (signer) per contracts/compliance_hook.md. Checks 2–5 and the effect
  come in US3 (T044), after their tests. Make T015 pass
- [ ] T017 Add `collateral_base_fund(h) -> CollatBase` to `tests/src/fixtures.rs`, following
  `settlement_ready_fund`:
  - launch a fund with no holder cap and `max_nav_move_bps = 500`, so the test publishes of
    $0.985–$1.01 pass F002's price-move guard (the standard terms allow only 50 bps); onboard
    investors G (giver) and T (taker); publish $1.00;
  - `seed_units(G, 2_000_000_000_000)` (keeps I4);
  - expose `worked_terms()`, the role keys (keeper and compliance as in the F003 fixtures) and
    the `FundCtx`.

  Add an `unpriced` variant that skips the publish
- [ ] T018 Add assertion helpers to `tests/src/collateral_model.rs` (or a new
  `tests/src/collateral.rs`). Each reads on-chain accounts:
  - `assert_locks(h, mint, owners, agreements)`: I1, I2 and I15 against the actual Pledge
    accounts;
  - `assert_calls(h, agreement)`: I16 against the actual call accounts;
  - `call_status(h, call)`: the derived status, via `delivery_met`.
- [ ] T019 Run the whole suite. F001–F003 and the Phase 2 tests are green (the stubs make no
  state)

**Checkpoint**: foundation ready; user story work can begin.

---

## Phase 3: User Story 1 — Agree the rules (Priority: P1) 🎯 MVP part 1

**Goal**: giver and taker record the agreement's terms, and it becomes Active once both have
signed (FR-001–FR-007).

**Independent Test**: T proposes with the worked terms → Proposed with exactly those terms and T's
flag set. G signs → Active. An outsider's signature → `Unauthorized`; T signing twice →
`AlreadySigned`.

### Interface stubs for User Story 1

- [ ] T020 [US1] Add stubs (handlers return `Ok(())`), with full account structs from
  contracts/collateral.md and `#[event_cpi]`, for `propose_agreement(args: ProposeArgs)` and
  `sign_agreement()` in `programs/collateral/src/instructions/{propose_agreement,sign_agreement}.rs`,
  and register them in `lib.rs`. `ProposeArgs` has the fields of the contract's `args` block in
  that order
- [ ] T021 [P] [US1] Create `crates/mmf-sdk/src/ix_collateral.rs` with
  `propose_agreement(proposer, payer, fund_state, args)` and
  `sign_agreement(signer, fund_state, agreement, giver, taker)`. They derive every PDA (agreement,
  both HolderStates, event authority)

### Tests for User Story 1 (write first; they must fail) ⚠️

- [ ] T022 [US1] Write `tests/tests/f004_agreement.rs` (FR-001–FR-006; quickstart rows), in the
  contract check order:
  - **Happy path**: T proposes, then G signs. Assert every stored term, `status`, both flags,
    `pledge_count == call_count == 1` and `created_ts` / `activated_ts`. The reverse (G proposes,
    T signs) also works.
  - **Term checks**: `FundNotOpen` (a fund in Setup), `Unauthorized` (an outsider proposes),
    `SameParty`, `UnsupportedPledgeMode` (mode 1), `UnsupportedIncomeRouting` (1 and 2),
    `InvalidHaircut` (10 000 and 10 001; 9 999 passes), `InvalidCureTime` (0),
    `InvalidPledgeLimit` (0), `InvalidPriceAge` (0), `EmptyAgreementHash`. Zero grace and zero
    MTA are accepted.
  - **Eligibility** (`PartyNotEligible`): each party pending, suspended, revoked, KYC expired at
    the exact second, or a fund-operated account. An unregistered party → `WrongHolder`.
  - **Signing**: `AgreementNotProposed` (sign an Active agreement), `AlreadySigned`,
    `Unauthorized` (an outsider signs), and `PartyNotEligible` when G is suspended between the
    proposal and the signature.
  - **Identity**: the same `(fund, G, T, nonce)` twice → already in use; a new nonce → a second
    agreement.
  - **FR-005**: assert the IDL has no instruction that takes terms other than
    `propose_agreement`.
  - Check the `AgreementProposed` and `AgreementActive` payloads

### Implementation for User Story 1

- [ ] T023 [US1] Implement `propose_agreement` per its contract (checks 1–13 in order), using
  `valuation::eligible` and `HolderStateView::load`. Set the proposer's flag and the counters to
  1. Emit `AgreementProposed`
- [ ] T024 [US1] Implement `sign_agreement` per its contract (checks 1–5): set the flag, then
  `Active` and `activated_ts`. Emit `AgreementActive`
- [ ] T025 [US1] Add `collateral_ready_fund(h) -> CollatCtx` to `tests/src/fixtures.rs`:
  `collateral_base_fund`, then T proposes with `worked_terms()` and nonce 1, and G signs.
  Variants: `two_step` (`taker_may_call_unilaterally = false`), `unpriced`, and
  `proposed_only` (not signed, used by every FR-007 test)
- [ ] T026 [US1] Make T022 pass. Check that each instruction is ≤ 80 000 CU (R14)

**Checkpoint**: US1 independent test passes.

---

## Phase 4: User Story 2 — Margin call from exposure (Priority: P1) 🎯 MVP part 2

**Goal**: the taker records exposure, and a delivery call opens automatically when the uncalled
shortfall exceeds the MTA. A raise may wait for the giver's confirmation, and anyone can re-check
coverage (FR-008–FR-019 without pledges).

**Independent Test**: with nothing pledged, T sets $1M. One Delivery call opens: value
`1_000_000_000_000`, units `1_020_408_163_266`, deadline = now + 14 400, enforce-after =
now + 21 600, and `called_outstanding` equal to the value.

### Interface stubs for User Story 2

- [ ] T027 [US2] Add stubs, with account structs per contracts/collateral.md, for:
  - `update_exposure(required_value, external_ref)` and
    `confirm_exposure(required_value, external_ref)` in `instructions/exposure.rs`;
  - `issue_call(kind, external_ref)` in `instructions/issue_call.rs`;
  - `check_coverage()` in `instructions/check_coverage.rs`.

  In `update_exposure` and `confirm_exposure`, the `call` account is an `UncheckedAccount`
  constrained to its PDA; it is not `init`
- [ ] T028 [P] [US2] Add SDK builders in `crates/mmf-sdk/src/ix_collateral.rs` for the four
  instructions. Each derives the call PDA from the external reference

### Tests for User Story 2 (write first; they must fail) ⚠️

- [ ] T029 [US2] Write `tests/tests/f004_exposure.rs` (FR-011–FR-013):
  - **US2 independent test**: the event payload carries the opened call;
  - **Party and reference checks**: G or an outsider → `Unauthorized`; a zero reference →
    `ZeroExternalRef`;
  - **Threshold**: shortfall exactly at the MTA ($10 000) → Applied, no call, call account not
    created; at MTA + 1 base unit → a call;
  - **Raise**: on a stale price (advance 93 601 s) → `StalePrice`; by a suspended taker →
    `PartyNotEligible`;
  - **Lower**: on a stale price it applies; lowering to 0 marks every open call derived Met;
  - **Two-step** (`two_step` fixture):
    - a raise is Pending and changes nothing else;
    - `confirm_exposure` by G opens the call, with deadlines counted from the confirmation
      (advance 3 600 s first);
    - G confirming a value or reference that differs from the pending raise →
      `PendingRaiseMismatch`, and so does a confirmation after T has made a newer raise;
    - nothing pending → `NoPendingRaise`; T confirming → `Unauthorized`;
    - a lower exposure clears the pending raise;
    - confirmation on a stale price → `StalePrice`; by a suspended G → `PartyNotEligible`;
  - **FR-007**: both instructions on `proposed_only` → `AgreementNotActive`
- [ ] T030 [P] [US2] Write `tests/tests/f004_calls.rs` part 1 (FR-014, FR-015, FR-017–FR-019, no
  pledges):
  - after the $1M call, `issue_call(Delivery)` → `NothingToCall`;
  - T raises to $1.5M → a second call for $500 000 (units `units_for_value_up`), with
    `called_outstanding` = $1.5M and `open_delivery_calls` = 2;
  - a reused reference → `CallRefInUse` (`update_exposure`) / already in use (`issue_call`);
  - G issuing a Delivery → `Unauthorized`; kind 2 → `InvalidCallKind`;
  - `NoPrice` (`unpriced` fixture); `StalePrice` at max age + 1 s, while max age exactly passes;
  - `check_coverage` by a random key on a stale price → `StalePrice`;
  - lower the exposure to 0, then `check_coverage` by a random key and by the keeper in two
    identical setups → identical account states (FR-019), with both calls derived Met;
  - `assert_calls` after every step;
  - **FR-007**: `issue_call` and `check_coverage` on `proposed_only` → `AgreementNotActive`

### Implementation for User Story 2

- [ ] T031 [US2] Create `programs/collateral/src/call_account.rs`:
  `open_delivery_call(a, call_info, payer, system_program, external_ref, value, fund, now, bump)`.
  - It fails with `CallRefInUse` if the account already holds data.
  - It creates the account by `system_program::create_account` with `invoke_signed` over
    `["call", agreement, external_ref, bump]`, then serializes a `CollateralCall` per
    contracts/collateral.md "Opening a delivery call".
  - It updates `a.call_count`, `a.open_delivery_calls` and `a.called_outstanding`.
- [ ] T032 [US2] Implement `update_exposure` and `confirm_exposure` per their contracts (checks
  in order; effects per branch). Run `observe` before applying a raise (R7). Emit
  `ExposureUpdated` with the outcome
- [ ] T033 [US2] Implement `issue_call` for **Delivery** (checks 1–8D and 11). Until T053 the
  Return branch returns `InvalidCallKind`, with a `// T053` comment
- [ ] T034 [US2] Implement `check_coverage` (checks 1–3; `observe` only). Emit `CoverageChecked`
- [ ] T035 [US2] Make T029 and T030 pass. Each instruction is ≤ 80 000 CU

**Checkpoint**: US2 independent test passes.

---

## Phase 5: User Story 3 — Pledge without moving units (Priority: P1) 🎯 MVP part 3

**Goal**: the giver locks units with no transfer. The token's own hook then refuses any transfer
below the lock, and locked units keep earning interest (FR-021–FR-026, hook step 5, `set_lien`).

**Independent Test**: after the US2 call, G pledges `1_020_408_163_266` in one transaction.
- No token balance anywhere changes; G's lock equals the pledge.
- The call is derived Met in that transaction.
- Sending `979_591_836_735` → `PledgedUnitsLocked`; sending `979_591_836_734` succeeds.

### Interface stubs for User Story 3

- [ ] T036 [US3] Add the `pledge(units, pledge_ref)` stub with its account struct (`lien accounts`
  per contracts/collateral.md). Create `programs/collateral/src/lien.rs` with
  `set_lien_cpi(lien_accounts, change)`, signed by `["hook-caller"]` (this bump is the
  collateral program's)
- [ ] T037 [P] [US3] Add the SDK builder `ix_collateral::pledge(giver, payer, fund_state, agreement, pledge_ref, units)`,
  which derives G's HolderState, ATA, Registry and `hook_caller`

### Tests for User Story 3 (write first; they must fail) ⚠️

- [ ] T038 [US3] Write `tests/tests/f004_pledge.rs` (FR-021, FR-022, SC-001):
  - **US3 independent test**: snapshot every token account of the mint (and every cash account)
    before and after the pledge; all equal;
  - **Recorded fields**: `seq`, `pledge_ref`, `posted_at`, `nav_seq_at_post`, zero
    released/enforced, Active; `active_liens == 1`;
  - **Rejections**: `ZeroUnits`; `ZeroPledgeRef`; `PledgeLimitExceeded` (cap + 1 base unit across
    two pledges; exactly the cap passes, using a fixture variant with a 1 500 000-unit cap);
    `InsufficientUnlockedUnits` (unlocked + 1; exactly the unlocked amount passes); T or the
    keeper pledging → `Unauthorized`; a suspended G → `PartyNotEligible`; the same reference
    twice → already in use;
  - **Stale price**: advance 93 601 s, pledge → succeeds and the call stays Open; publish, then
    `check_coverage` → Met;
  - **FR-007**: `proposed_only` → `AgreementNotActive`;
  - `assert_locks` after every step
- [ ] T039 [P] [US3] Write `tests/tests/f004_lock.rs` (FR-023, FR-026, SC-003). With G holding
  2 000 000 units and `1_020_408_163_266` locked:
  - a `transfer_checked` built by hand (raw instruction, no SDK helper) of `979_591_836_735` →
    `PledgedUnitsLocked`;
  - a delegate approved by G for the whole balance transfers over the lock →
    `PledgedUnitsLocked`;
  - F003: G creates and funds a redemption of 1 000 000 units → `PledgedUnitsLocked` (F003 US2
    scenario 3); the same for a secondary sale;
  - a second agreement with another taker: G pledges `979_591_836_735` →
    `InsufficientUnlockedUnits`, and `979_591_836_734` passes;
  - a fully locked G: a zero-unit transfer succeeds, and receiving units succeeds and raises the
    unlocked amount;
  - after every attempt, balances and locks are unchanged (I2)
- [ ] T040 [P] [US3] Add the step-5 rows of contracts/compliance_hook.md to
  `tests/tests/hook_order.rs`:
  - a suspended sender with locked units → `SenderNotApproved`;
  - paused → `TransfersPaused`;
  - over the lock to an unapproved receiver → `PledgedUnitsLocked`.
- [ ] T041 [P] [US3] Write `tests/tests/f004_interest.rs` (SC-004, FR-026; also F002 US2
  scenario 3):
  - two investors each hold 1 000 000 units; one pledges all of them;
  - five publishes at different rates (advance a day each), and each holder makes one transfer of
    0 units (touch);
  - their `accrued_e18` values are equal to the last digit
- [ ] T042 [P] [US3] Write `tests/tests/f004_calls.rs` part 2 (FR-015, FR-018, with pledges):
  - **Two open calls** ($1M then +$500k): a partial pledge leaves both Open; a pledge covering
    the rest → both derived Met in that transaction;
  - **Demanded part**: a $1M call; G pledges value $600 000; T raises by $500 000 → a new call of
    exactly $500 000 (not folded in), and `called_outstanding` = the shortfall;
  - **Price moves**: from full cover at $1M, publish $0.99 → `issue_call` → `NothingToCall`;
    publish $0.985 → a call for `15_000_000_000` with units `15_539_210_609`; publish $1.00 again,
    then `check_coverage` by a random key → Met
- [ ] T043 [P] [US3] Write `tests/tests/f004_lien.rs` part 2 (`set_lien` checks 2–5 through
  `collateral`):
  - pledge passing another owner's ATA as `giver_holding` → `HookError::WrongHolder`;
  - another owner's HolderState as `giver_holder` → `CollateralError::WrongHolder`;
  - an ATA of another fund's mint → `HookError::WrongHolder`;
  - G's HolderState planted with `pledged_amount` 1 below an active pledge's remaining (via
    `set_account`, commented: "unreachable state; proves the underflow guard"), then the taker
    releases the whole pledge → `LienUnderflow`. This test runs once `release` exists (US4) and is
    `#[ignore]`d until T054

### Implementation for User Story 3

- [ ] T044 [US3] Complete `set_lien` (checks 2–5 and the effect) per contracts/compliance_hook.md.
  Load and store the HolderState by hand, as `accrue` does, and reuse its holding-account check.
  Emit `LienSet` with `emit!`
- [ ] T045 [US3] Add step 5 to `programs/compliance_hook/src/hook/execute.rs`, between steps 4 and
  6: `require!(post_src >= src.pledged_amount, HookError::PledgedUnitsLocked)`. Update the module
  doc comment's step list ("step 5 lands with F004; step 2 with F005")
- [ ] T046 [US3] Implement `pledge` per its contract (checks 1–9):
  - CPI `set_lien { lock: units, unlock: 0, opened: 1, closed: 0 }`;
  - create the Pledge;
  - `pledge_count += 1`, `pledged_units += units`;
  - if `fresh`, `observe`;
  - emit `Pledged` with `giver_locked_after`.
- [ ] T047 [US3] Make T038–T043 pass (except T043's ignored case). Check `pledge` ≤ 80 000 CU,
  `set_lien` ≤ 15 000 CU and call depth ≤ 3

**Checkpoint**: US1 + US2 + US3 (the MVP) pass. A $1M call is met by a pledge in one transaction
with zero units moving, and no test can move locked units.

---

## Phase 6: User Story 4 — Release excess (Priority: P2)

**Goal**: the giver opens a return call when the excess exceeds the MTA. The taker, or the keeper
against that call, unlocks units, and coverage never goes negative (FR-016, FR-020, FR-027–FR-029).

**Independent Test**: after the US3 pledge, T lowers the exposure to $400 000. G opens a return
call: value `600_000_000_000`, units `612_244_897_959`. The keeper releases them against it. G's
lock is then `408_163_265_307`, coverage is exactly 0, no balance changed, and the return call is
Met.

### Interface stubs for User Story 4

- [ ] T048 [US4] Add stubs for `release(units)`, with an optional `return_call` (Anchor
  `Option<Account>`), and for `cancel_call()`, with account structs per contracts/collateral.md
- [ ] T049 [P] [US4] Add the SDK builders `ix_collateral::release(signer, fund_state, agreement, pledge, return_call: Option<Pubkey>, units)`
  and `cancel_call(signer, fund_state, agreement, call)`

### Tests for User Story 4 (write first; they must fail) ⚠️

- [ ] T050 [US4] Write `tests/tests/f004_release.rs` (FR-016, FR-027–FR-029):
  - **US4 independent test**, plus `Released.return_call_met == true` and
    `open_return_call == default` afterwards;
  - **Opening a return call**: excess exactly at the MTA → `NoExcess`; a second return call →
    `ReturnCallAlreadyOpen`; T opening a Return → `Unauthorized`; a suspended G →
    `PartyNotEligible`;
  - **`ReturnTooSmall`**: an agreement with haircut 0 and MTA 0; G pledges `1_000_000`; publish
    $1.01 (value `1_010_000`); exposure `1_009_999` gives an excess of 1, and
    `floor(1 / 1.01) = 0`;
  - **Releases by T**: without a return call; one release not naming the open return call leaves
    its `units_released` unchanged;
  - **Releases by the keeper**: without a return call → `KeeperNeedsReturnCall`; more than the
    call's remainder → `ExceedsReturnCall`; a Cancelled return call, or one from another
    agreement → `WrongReturnCall`;
  - **Release rejections**: more than the pledge holds → `ExceedsPledgeRemaining`; a fully
    Released pledge → `PledgeNotActive`; coverage below 0 by 1 base unit →
    `WouldUndercollateralise`; G or an outsider → `Unauthorized`; zero → `ZeroUnits`;
  - **Price age**: exposure 0 on a stale price → the release succeeds; exposure > 0 on a stale
    price → `StalePrice`;
  - **FR-007**: `proposed_only` → `AgreementNotActive`;
  - `assert_locks` after every step
- [ ] T051 [P] [US4] Write `tests/tests/f004_calls.rs` part 3 (FR-015, FR-020):
  - T cancels an Open delivery call: `open_delivery_calls` −1 and `called_outstanding` reduced by
    `min(D, value)`;
  - with two calls, cancelling the first after a partial pledge → D per that rule (not below 0),
    and D = 0 once none remain;
  - G cancels its Open return call; G cancelling a Delivery → `Unauthorized`; T cancelling a
    Return → `Unauthorized`;
  - a Cancelled or derived-Met call → `CallNotOpen`, and in the derived-Met case nothing is
    stored (transaction rolled back)

### Implementation for User Story 4

- [ ] T052 [US4] Implement `cancel_call` per its contract (checks 1–4, `settle_status` first, then
  the bookkeeping table in data-model "Call states"). Emit `CallCancelled`
- [ ] T053 [US4] Implement the **Return** branch of `issue_call` (checks 8R–10R; `units_for_value_down`;
  `open_return_call = call`), replacing T033's placeholder
- [ ] T054 [US4] Implement `release` per its contract (checks 1–12):
  - CPI `set_lien { unlock, closed: 1 if the pledge empties }`;
  - pledge and agreement bookkeeping, then `observe` (when `required_value > 0` the price is
    fresh; at 0, run `observe` with S = 0);
  - return-call progress: Met at `units_required`, then clear `open_return_call`;
  - emit `Released`.

  Un-ignore T043's `LienUnderflow` case
- [ ] T055 [US4] Make T050, T051 and T043 pass

**Checkpoint**: US4 independent test passes.

---

## Phase 7: User Story 5 — Substitute (Priority: P2)

**Goal**: in one instruction, the giver replaces an active pledge with an equal or larger one
(FR-030).

**Independent Test**: G substitutes its `1_020_408_163_266` pledge with `1_100_000_000_000`.
- The old pledge is Substituted and the new one Active.
- The lock is `1_100_000_000_000` and no balance changed, all in one instruction.
- Substituting with `1_000_000_000_000` → `SubstituteTooSmall`.

- [ ] T056 [US5] Add the `substitute(new_units, new_pledge_ref)` stub (account struct per
  contracts/collateral.md) and the SDK builder
  `ix_collateral::substitute(giver, payer, fund_state, agreement, old_pledge, new_pledge_ref, new_units)`
- [ ] T057 [US5] Write `tests/tests/f004_substitute.rs` (tests must fail):
  - **US5 independent test**;
  - **Equal swap**: lock, `active_liens` and `pledged_units` all unchanged;
  - **Rejections**: `SubstituteTooSmall`; a net increase above the unlocked amount →
    `InsufficientUnlockedUnits`; above the cap → `PledgeLimitExceeded`; a Substituted or Released
    old pledge → `PledgeNotActive`; a pledge of another agreement → `PledgeNotActive`; T →
    `Unauthorized`; a suspended G → `PartyNotEligible`; `ZeroPledgeRef`;
  - **Observe**: an open call is derived Met when the swap raises coverage to ≥ 0;
  - **FR-007**: `proposed_only` → `AgreementNotActive`;
  - `assert_locks` after every step
- [ ] T058 [US5] Implement `substitute` per its contract (checks 1–9):
  - one CPI `set_lien { lock: new, unlock: remaining, opened: 1, closed: 1 }`;
  - the old pledge becomes Substituted; the new pledge is created;
  - `pledged_units` and `pledge_count` updated;
  - `observe` if `fresh`;
  - emit `Substituted`.
- [ ] T059 [US5] Make T057 pass

**Checkpoint**: US5 independent test passes.

---

## Phase 8: User Story 6 — Dispute (Priority: P3)

**Goal**: the giver disputes a delivery call. The dispute ends by withdrawal, by the taker
cancelling the call, by compliance resolution through governance, or when the call is covered
(FR-031–FR-033).

**Independent Test**:
1. The $1M call opens; G disputes it → Disputed, with its deadlines unchanged.
2. G withdraws → Open.
3. G disputes again; compliance resolves it Cancelled with reason 3 (operational error) →
   Cancelled.
4. T opens a new call for $1M.

### Interface stubs for User Story 6

- [ ] T060 [US6] Add stubs, with account structs per contracts/collateral.md and
  contracts/governance.md:
  - `dispute_call(dispute_ref)`, `withdraw_dispute()` and
    `resolve_dispute(outcome, reason, case_ref_hash, actor)` in
    `programs/collateral/src/instructions/dispute.rs`;
  - `governance::resolve_call_dispute(outcome, reason, case_ref_hash)` in
    `programs/governance/src/instructions/resolve_call_dispute.rs`, with the `collateral_caller`
    PDA account.
- [ ] T061 [P] [US6] Add the SDK builders `ix_collateral::{dispute_call, withdraw_dispute}` and
  `ix_governance::resolve_call_dispute(compliance, role_registry, fund_state, agreement, call, outcome, reason, case_ref_hash)`

### Tests for User Story 6 (write first; they must fail) ⚠️

- [ ] T062 [US6] Write `tests/tests/f004_dispute.rs` (FR-020 for Disputed calls, FR-031–FR-033).
  Run compliance through its Squads vault, as `f003_reject.rs` does:
  - **US6 independent test**;
  - **Dispute rejections**: a Return call → `WrongCallKind`; a Disputed, Cancelled or
    (derived) Met call → `CallNotOpen`; T disputing → `Unauthorized`; a zero `dispute_ref` →
    `ZeroExternalRef`;
  - **Withdrawal**: when the call is not Disputed → `NotDisputed`;
  - **Taker cancel**: T cancels a Disputed call → Cancelled, with the bookkeeping;
  - **Resolution to Open**: the call is Open with its original deadlines;
  - **Rejected resolutions**:
    - outcome 1 or 3 → `InvalidOutcome`;
    - reasons 4, 5 and 7 → `InvalidReason`;
    - signed by the issuer, fund admin, transfer agent, keeper, auditor, G or T →
      `GovernanceError::Unauthorized`;
    - the call is Open or Met → `NotDisputed`;
    - a call of another fund's agreement → `FundMismatch`;
    - `collateral::resolve_dispute` called directly by compliance → `UnauthorizedCaller`;
  - **Disputed calls still count**: a Disputed call stays in `called_outstanding` (`issue_call`
    → `NothingToCall`), and is derived Met once a pledge covers the shortfall;
  - check the `DisputeResolved` payload (reason, case reference, actor = the compliance vault)

### Implementation for User Story 6

- [ ] T063 [US6] Implement `dispute_call` and `withdraw_dispute` per their contracts
  (`settle_status` first). Emit `CallDisputed` / `DisputeWithdrawn`
- [ ] T064 [US6] Implement `collateral::resolve_dispute` (checks 1–4). The caller must be
  governance's `["collateral-caller"]`, derived from `GOVERNANCE_PROGRAM_ID`. Cancelled shares
  `cancel_call`'s delivery bookkeeping. Emit `DisputeResolved`
- [ ] T065 [US6] Implement `governance::resolve_call_dispute` (checks 1–3, then the CPI signed by
  `["collateral-caller"]`, with `actor = compliance`), following `reject_settlement`
- [ ] T066 [US6] Make T062 pass. Call depth through Squads ≤ 4

**Checkpoint**: all six stories pass.

---

## Phase 9: Polish & cross-cutting concerns

- [ ] T067 [P] Write `tests/tests/f004_status.rs` (FR-034, FR-035):
  - **Suspended or KYC-expired G**: pledge, substitute, confirm and return call →
    `PartyNotEligible`; release, dispute and cancelling its return call succeed;
  - **Suspended T**: a raise and a delivery call → `PartyNotEligible`; lowering, release and
    cancel succeed;
  - **Deadlines**: call deadlines are unchanged across a suspension and reinstatement;
  - **Paused transfers**: pledge, release and substitute succeed, while a transfer of unlocked
    units fails with `TransfersPaused`.

  No program change is expected; if one is needed, stop and record it in research.md
- [ ] T068 [P] Write `tests/tests/f004_slow_signers.rs` (FR-036, SC-008), using
  `tests/src/nonce.rs` and `mmf_sdk::prepare::with_durable_nonce`:
  - **Delayed pledge**: G's pledge prepared on its durable nonce and submitted after 6 h →
    succeeds;
  - **State changed after preparation**: a pledge prepared before T opens a second call, and a
    release prepared before G opens a return call, both succeed;
  - **Stale confirmation**: one prepared for a raise that T has since replaced →
    `PendingRaiseMismatch`;
  - **Replay**: the same signed pledge resubmitted → fails, and nothing changes;
  - **Control**: an ordinary-blockhash transaction after 6 h → `BlockhashNotFound`.

  No program change is expected; if one is needed, stop and record it
- [ ] T069 [P] Write `tests/tests/collateral_prop.rs` (proptest; SC-002, SC-003, SC-005):
  - **Sequences**: random ops from research R15 against `collateral_model.rs`, with 2–4 holders,
    1–3 agreements and prices in [$0.95, $1.05];
  - **Each step**: the on-chain outcome (Ok, or the error name) equals the model's, and I1, I2,
    I15 and I16 hold;
  - **SC-005**: pledging a call's `units_required` at its price leaves coverage ≥ 0; releasing a
    return call's units at its price leaves coverage ≥ 0;
  - **Volume**: 64 cases by default and 256 under `--release --ignored`.
- [ ] T070 [P] Extend `tests/tests/invariants_prop.rs` with `assert_i1_i2` after every op (holds
  trivially with no pledges, and catches any F001–F003 path that would break it)
- [ ] T071 [P] Extend `tests/tests/scale.rs` (SC-007, `#[ignore]`, release):
  - **Fixed cost**: the CU of `pledge`, `release` and `issue_call` is equal (±0) with 10 vs
    10 000 holders and 1 vs 300 agreements;
  - **No new publish cost**: `publish_daily` CU is unchanged from F003.
- [ ] T072 [P] Extend `tests/src/rebuild.rs` and add `tests/tests/f004_rebuild.rs` (SC-009,
  FR-037). Rebuild from events alone:
  - every agreement's status, exposure, D and `met_below_seq`;
  - every call's status (derived Met from `met_below_seq`);
  - every pledge's remaining units;
  - every holder's lock.

  Compare them with the on-chain accounts after a scripted sequence that covers all 13 collateral
  instructions and `set_lien`
- [ ] T073 Run `scripts/check-error-coverage.sh`. Every `CollateralError`, the four new
  `HookError`s and any new `GovernanceError` must be asserted by a test (SC-006). Add the missing
  tests
- [ ] T074 [P] Run `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`
  and `scripts/check-duplicate-crates.sh`, then fix the findings
- [ ] T075 [P] Extend `scripts/devnet-e2e` with `--feature f004` (in `src/f004.rs`), following
  `src/f003.rs` and quickstart.md "Devnet run":
  - **Agreement**: a new fund; G gets units by an F003 subscription; T proposes and G signs;
  - **Call and pledge**: T sets $1M; G's pledge is prepared on a durable nonce and signed 130 s
    later; the call is Met with no balance change;
  - **Lock**: sending over the lock → `PledgedUnitsLocked`, and sending the unlocked amount
    succeeds;
  - **Return**: T lowers to $400 000, G opens a return call, the keeper releases, and the lock is
    `408_163_265_307`.

  Exit 0 only if every outcome matches
- [ ] T076 [P] Update the root `README.md`:
  - F004 status;
  - one paragraph on how pledges lock units without moving them;
  - the new test counts.
- [ ] T077 [P] Write `specs/004-collateral-pledges/review.md`, the human review checklist in the
  F003 format:
  - **Money math**: the three formulas and their rounding directions (shared-model §8);
  - **Hook order**: step 5's place (§6);
  - **Signers**: per instruction (§5.1), and the new CPI edges (§5.2);
  - **Invariants**: I1, I2, I15, I16 (§7);
  - **Design**: the derived-Met rule (R5), and the two Constitution IV exceptions.
- [ ] T078 Devnet run. It needs the user's go-ahead, devnet SOL and the deployed programs. Deploy
  `collateral` and upgrade `compliance_hook` and `governance` (and, if F001–F003 are still not on
  devnet, do the first deployment of all five, as F003 T080 describes). Run
  `cargo run -p devnet-e2e -- --feature f004`, and record program IDs and signatures in
  `specs/004-collateral-pledges/devnet-run.md`
- [ ] T079 Run the full suite and validation:
  `anchor build && cargo test -p mmf-common && cargo test -p tests && cargo test -p tests --release -- --ignored`,
  plus [quickstart.md](quickstart.md). Open the pull request `004-collateral-pledges` → `main`,
  listing:
  - the shared-model changes (plan "Shared-model changes");
  - the spec adjustments (plan "Spec adjustments made during planning");
  - every F001–F003 test or helper edit (T004, T010, T016, T040, T070), each with its reason
    (Constitution XII).

  A human reviewer completes T077's checklist before merge

---

## Dependencies & execution order

### Phase dependencies

- **Setup (Phase 1)**: none. T003 and T004 need T002.
- **Foundational (Phase 2)**: needs Setup. It blocks every story.
  - T010 needs T007 and T008. T013 needs T005, T007 and T008.
  - T015 needs T014; T016 needs T015 (tests first). T017 needs T016.
  - T019 needs T005–T018.
- **US1 (Phase 3)**: needs Foundational. T025 needs T023 and T024.
- **US2 (Phase 4)**: needs US1's fixture (T025).
- **US3 (Phase 5)**: needs US2: its tests open calls to meet.
- **US4 (Phase 6)**: needs US3 (pledges to release). T055 also completes T043.
- **US5 (Phase 7)**: needs US3. It is independent of US4.
- **US6 (Phase 8)**: needs US2 (calls). Its cancel tests need T052 (US4), and its "Met once
  covered" test needs `pledge` (US3).
- **Polish (Phase 9)**: needs every story. T078 needs T075; T079 needs everything.

### Within each story

Interface stubs → SDK builders → tests (run them and see them fail) → implementation →
checkpoint. Every rule's rejection test comes before the code that enforces it (Principle VI).

### Story order

```text
Setup → Foundational → US1 (agree) → US2 (exposure, calls) → US3 (pledge, lock)  ← MVP
                                                │                 ├→ US4 (release, return calls, cancel)
                                                │                 └→ US5 (substitute)
                                                └→ US6 (dispute; cancel cases after US4)
→ Polish
```

---

## Parallel opportunities

- **Foundational**: T005, T006, T007, T009, T011 and T012 together; then T008 → T010 and T013;
  then T014 → T018.
- **US1**: T021 alongside T020.
- **US2**: T028 alongside T027; T030 alongside T029.
- **US3**: T037 alongside T036; tests T039–T043 in separate files alongside T038.
- **US4**: T049 alongside T048; T051 alongside T050.
- **US5 and US6**: can run in parallel once US3 is done (US6's cancel tests wait for T052).
- **Polish**: T067–T072 and T074–T077 together.

### Parallel example: User Story 3 tests

```text
Task: "Write tests/tests/f004_lock.rs (no path moves locked units)"     (T039)
Task: "Add step-5 rows to tests/tests/hook_order.rs"                   (T040)
Task: "Write tests/tests/f004_interest.rs (SC-004)"                    (T041)
Task: "Write tests/tests/f004_calls.rs part 2 (met together)"          (T042)
```

### Parallel example: Polish

```text
Task: "Write tests/tests/f004_status.rs (FR-034, FR-035)"              (T067)
Task: "Write tests/tests/f004_slow_signers.rs (FR-036, SC-008)"        (T068)
Task: "Write tests/tests/collateral_prop.rs (SC-002, SC-003, SC-005)"  (T069)
Task: "Extend tests/tests/scale.rs (SC-007)"                           (T071)
```

---

## Implementation strategy

### MVP first (US1 + US2 + US3, all P1)

1. Phase 1 Setup → Phase 2 Foundational. **Stop if T019 shows an F001–F003 regression** and fix
   it first (Principle I).
2. US1 → validate an Active agreement with exactly the worked terms.
3. US2 → validate the $1M call and its deadlines.
4. US3 → validate the one-transaction pledge with zero units moving, and the lock-bypass suite.
   A pledge that the token doesn't enforce is worthless, so the MVP is US1 + US2 + US3.

### Then

5. US4 (release), US5 (substitute) and US6 (dispute), each validated at its checkpoint.
6. Phase 9: status, slow signers, properties, scale, rebuild, error coverage, devnet, review and
   merge.

Commit after each group of 2–4 related tasks on `004-collateral-pledges` (for example
"F004 T005-T007: shared formulas, seeds, views"), and push as you go.

---

## Notes

- `[P]` = different files and no dependency on an unfinished task.
- Tests may plant state with `set_account` **only** for conditions no instruction can reach: a
  pre-F004 Registry (T015) and the `LienUnderflow` guard (T043). Each says so in a comment.
- **Out of scope** (Principle XI):
  - enforcement, `mark_defaulted`, holds and hook step 2 (F005);
  - termination of agreements;
  - title transfer and manufactured income (backlog);
  - ActionRecords (F007).
- New error variants are appended, so earlier error codes don't move. Any F001–F003 test edit
  beyond those listed in T079 must be justified in the PR (Principle XII).
- If a task needs a shared-model change not already in `docs/shared-model.md`, stop and make it
  there first (Principle XIII).
