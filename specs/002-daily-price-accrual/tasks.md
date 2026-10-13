---
description: "Task list for F002: Daily Price and Interest Accrual"
---

# Tasks: Daily Price and Interest Accrual

**Input**: Design documents from `specs/002-daily-price-accrual/`: [plan.md](plan.md),
[spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md),
[contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: REQUIRED. Constitution Principle VI says every rule needs a test that is written and
seen to fail before its code, with at least one rejection test asserting the exact error. SC-006
says the same.
- **Order within each story**: interface stubs → tests (must fail) → implementation.
- **Stub handlers** return `Ok(())` and do nothing, so happy-path tests fail on missing state and
  rejection tests fail on a missing error.
- **US2's tests** fail because hook step 8 is still a no-op.

**Organization**: by user story. Both stories are P1. US2 builds on US1: interest only moves when
a price is published. Each story phase ends with a checkpoint where that story's independent test
passes.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: US1 or US2, from spec.md
- Paths are from the repository root

## Path conventions (from plan.md)

- Programs: `programs/{fund,compliance_hook,governance}/src/`
- Shared crate: `crates/mmf-common/src/`; client SDK: `crates/mmf-sdk/src/`
- LiteSVM tests: `tests/src/` (harness, model, helpers) and `tests/tests/` (test files)
- Scripts: `scripts/`

## Dates used in tests

UTC day numbers (`unix_timestamp.div_euclid(86_400)`, research R2): 2026-10-13 = **20 739**
(Tuesday), 2026-10-16 = **20 742** (Friday), 2026-10-19 = **20 745** (Monday).

---

## Phase 1: Setup

**Purpose**: confirm a green F001 baseline and add the one new crate edge.

- [X] T001 Run `anchor build && cargo test -p mmf-common && cargo test -p tests` on the branch
  before any change and confirm F001 is green (Constitution XII baseline). Record the test count
  in the PR description draft
- [X] T002 Add `fund = { path = "../fund", features = ["cpi", "no-entrypoint"] }` to
  `programs/governance/Cargo.toml` (contracts/governance.md "New cross-program edge"). Confirm
  `fund` doesn't depend on `governance` (`cargo tree -p fund | grep governance` is empty, so no
  cycle) and that `anchor build` still succeeds

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: shared math, layouts, views, SDK helpers and the independent reference model that
both stories use.

**⚠️ CRITICAL**: no story work starts until Phase 2 is complete, and T012 shows F001 still green.

- [X] T003 [P] Create `crates/mmf-common/src/accrual.rs` and export it from `lib.rs`. All
  functions are pure, use checked `u128` math, return `None` on overflow, and never divide except
  in `utc_day`:
  - `utc_day(ts: i64) -> Option<u32>` = `ts.div_euclid(86_400)`, `None` if negative or > `u32::MAX`
  - `index_increment(daily_rate_e12: u64, days_covered: u16) -> Option<u128>` =
    "rate × 10^6 × days_covered"
  - `settle(accrued_e18: u128, index_at_last_touch: u128, pre_balance: u64, index: u128) -> Option<(u128, u128)>`
    returning `(accrued_e18 + pre_balance × (index − index_at_last_touch), index)`; `None` if
    `index < index_at_last_touch`
  - `earned_e18(accrued_e18, index_at_last_touch, last_balance, accrual_index) -> Option<u128>`
    (FR-017)
  - `price_move_ok(prev: u64, nav: u64, max_nav_move_bps: u16) -> bool` =
    "`|nav − prev| × 10_000 ≤ prev × max_nav_move_bps`", cross-multiplied with no division, equal
    passes

  Unit tests:
  - $1.00 = `100_000_000`;
  - 4.5 %/yr = `123_287_671` gives an increment of `123_287_671_000_000` for one day (FR-005);
  - `utc_day(1_791_849_600) == 20_739`;
  - `price_move_ok`: target `100_000_000`, 50 bps, accepts `99_500_000` and `100_500_000`, rejects
    `99_499_999` and `100_500_001`;
  - `settle` with pre-balance 0 only moves the checkpoint;
  - overflow cases return `None`
- [X] T004 [P] In `crates/mmf-common/src/seeds.rs`, add `NAV = b"nav"` (fund:
  `["nav", fund_state, seq.to_le_bytes()]` → NavRecord) and `FUND_CALLER = b"fund-caller"`
  (governance: signs `fund::set_/clear_nav_override`). In `crates/mmf-common/src/enums.rs`, add
  `ReasonCode::allowed_for_nav_override()`, true only for `OperationalError` (3) and `Other` (6),
  with a unit test that the allowed list is exactly `[3, 6]`. `allowed_in_f001` stays unchanged
- [X] T005 Extend `programs/fund/src/state.rs`, `errors.rs` and `events.rs` per
  [data-model.md](data-model.md):
  - **FundState**: after `bump`, add `nav_period_end_day: u32` ("last UTC day covered by any
    publish; 0 = none") and `pending_override_nav: u64` ("price allowed by an approved override;
    0 = none"), and change `_reserved` to `[u8; 52]`. The total size must be unchanged
  - **New NavRecord account**: "version u8 · seq u64 · nav u64 · daily_rate_e12 u64 ·
    days_covered u16 · period_start_day u32 · index_after u128 · as_of_ts i64 ·
    published_by Pubkey · override_used bool · bump u8", 97 bytes with the discriminator, no
    `_reserved`, `VERSION = 1`
  - **Errors**: **append** (never reorder) `FundNotOpen`, `ZeroPrice`, `NegativeRate`,
    `ZeroDaysCovered`, `PeriodNotStarted`, `PriceMoveTooLarge`, `Overflow`, `UnauthorizedCaller`,
    `OverrideAlreadyPending`, `NoOverridePending`. Change only the message of `Unauthorized` to
    "Signer does not hold the required role"
  - **Events**:
    - `NavPublished { fund, seq, nav, daily_rate_e12, days_covered, period_start_day, index_after, publisher, override_used, override_cleared_nav, slot, unix_ts }`
    - `NavOverrideApproved { fund, nav, reason, case_ref_hash, actor, slot, unix_ts }`
    - `NavOverrideWithdrawn { fund, nav, reason, case_ref_hash, actor, slot, unix_ts }`
- [X] T006 [P] In `programs/compliance_hook/src/events.rs`, extend `TransferValidated` with
  `accrual_index: u128`, `source_accrued_e18: u128`, `destination_accrued_e18: u128`, placed after
  `holder_count`. Until T030, `execute.rs` fills them with the holders' current stored values so
  F001 still compiles and passes
- [X] T007 In `crates/mmf-common/src/views.rs`, add `accrual_index: u128` to `FundStateView`. Its
  offset is `FUND_STATUS + 1 + 8 + 8 + 8` (after `nav`, `nav_ts`, `nav_seq`); raise `load`'s
  minimum length to cover it, and decode it as little-endian `u128` (depends on T005)
- [X] T008 Update `tests/tests/layouts.rs` (depends on T005, T007):
  - FundState's serialized size is unchanged from F001;
  - `nav_period_end_day` sits right after `bump`, then `pending_override_nav`, then `_reserved`
    52 bytes;
  - NavRecord serializes to 97 bytes;
  - `FundStateView.accrual_index` matches a program-serialized FundState with a non-zero index.

  This is one of the F001 test edits the PR must list (Constitution XII)
- [X] T009 [P] In `crates/mmf-sdk/src/pda.rs`, add `nav_record(fund_state, seq: u64)` and
  `fund_caller()`. Add `crates/mmf-sdk/src/accrual.rs` with
  `earned_e18(holder: &HolderState, fund: &FundState) -> Option<u128>` (read-only, FR-017; `None` only for inconsistent accounts or overflow; using
  `mmf_common::accrual::earned_e18`), and export it from `lib.rs`
- [X] T010 Update `seed_units` in `tests/src/units.rs` (research R12). Before raising
  `last_balance`, read `FundState.accrual_index` and settle the holder:
  `accrued_e18 += last_balance × (index − index_at_last_touch); index_at_last_touch = index`. This
  is exactly what a hook-checked transfer in would do. Update the module doc comment. This is an
  F001 helper edit the PR must list (depends on T005)
- [X] T011 [P] Add the independent accrual reference model in `tests/src/model.rs` (research R11).
  It **must not** use `mmf_common::accrual`:
  - `AccrualModel { balances: Vec<u64>, earned_e18: Vec<u128>, index_e18: u128, period_end_day: Option<u32>, last_nav: Option<u64> }`;
  - `next_start(today) -> u32` (`today` before the first publish, else the day after the last
    covered day);
  - `publish(start_day: u32, nav: u64, rate_e12: u64, days: u16)` loops **once per covered
    day**, adding `balance × rate_e12 × 1_000_000` to every holder, and advances the index the
    same way; it also records `period_end_day` and `last_nav`;
  - `transfer(from, to, amount)` moves balances only;
  - `seed(to, amount)`;
  - `total_earned()`.

  Unit-test it against a 3-day hand-written table
- [X] T012 Run `cargo test -p mmf-common && cargo test -p tests` and confirm every F001 test still
  passes with the new layouts and helpers (Constitution XII)

**Checkpoint**: math, layouts, views and model are ready, and F001 is still green.

---

## Phase 3: User Story 1 — Publish daily price and interest (Priority: P1) 🎯 MVP part 1

**Goal**: the fund admin publishes a price, rate and days covered, kept permanently by sequence
number. Coverage is contiguous; an out-of-limit price needs a compliance override, approved
through governance (FR-001 to FR-011, FR-020 for publish and override records).

**Independent Test**: on an Open fund, publish three days of figures, then read back the full
history by sequence number and check each record's price, rate, days covered, time and publisher
(spec US1).

### Interface stubs for User Story 1

- [X] T013 [US1] Create `programs/fund/src/instructions/publish_daily.rs`. Add an Accounts struct
  exactly as in [contracts/fund.md](contracts/fund.md), with `#[event_cpi]`:
  - `fund_admin` signer;
  - `payer` signer mut;
  - `fund_state` mut;
  - `role_registry` (unchecked, verified in the handler);
  - `nav_record` `init`, seeds `[NAV, fund_state, (fund_state.nav_seq + 1).to_le_bytes()]`,
    payer = `payer`, space `8 + NavRecord::INIT_SPACE`;
  - `system_program`.

  Add a stub handler, and wire `publish_daily(nav: u64, daily_rate_e12: i64, days_covered: u16)`
  in `programs/fund/src/lib.rs`
- [X] T014 [US1] Create `programs/fund/src/instructions/nav_override.rs` with Accounts
  `SetNavOverride { fund_caller: Signer, fund_state: mut }` (`#[event_cpi]`), used by both
  instructions, and stub handlers. Wire
  `set_nav_override(nav: u64, reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)` and
  `clear_nav_override(reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)` in
  `programs/fund/src/lib.rs` (depends on T013, which edits the same `lib.rs`)
- [X] T015 [P] [US1] Create `programs/governance/src/instructions/nav_override.rs`. Add Accounts
  `NavOverrideAction` per [contracts/governance.md](contracts/governance.md):
  - `compliance` signer;
  - `role_registry: Account<RoleRegistry>`;
  - `fund_state` mut, unchecked, `address = role_registry.fund_state`;
  - `fund_caller`, seeds `[FUND_CALLER]` with bump;
  - `fund_event_authority`, unchecked;
  - `fund_program: Program<Fund>`.

  Add stub handlers, and wire `approve_nav_override(nav, reason, case_ref_hash)` and
  `withdraw_nav_override(reason, case_ref_hash)` in `programs/governance/src/lib.rs`
- [X] T016 [US1] Add SDK builders (depends on T013–T015). Accounts go in contract order,
  including the event authority and program:
  - `crates/mmf-sdk/src/ix_fund.rs`: `publish_daily(fund_admin, payer, mint, next_seq, nav, rate: i64, days)`
    and raw `set_nav_override` / `clear_nav_override` builders taking any signer (for the
    direct-call tests);
  - `crates/mmf-sdk/src/ix_governance.rs`: `approve_nav_override(compliance, mint, nav, reason, case_ref_hash)`
    and `withdraw_nav_override(...)`
- [X] T017 [P] [US1] Add test helpers:
  - `clock::set_utc_day(svm, day: u32, secs_into_day: i64)` in `tests/src/clock.rs`;
  - `fixtures::publish(h, ctx, nav, rate: i64, days) -> Result<TxMeta, TxFailure>` in
    `tests/src/fixtures.rs`, signed by the fund's `fund_admin` key and reading
    `FundState.nav_seq` for the next seq;
  - `fixtures::read_nav_record(h, ctx, seq)` in `tests/src/fixtures.rs`;
  - decoding for `NavPublished`, `NavOverrideApproved` and `NavOverrideWithdrawn`. The existing
    generic `tests::events::cpi_events::<T>` already handles them, so no new decoder was needed;
  - `fixtures::check_i12(h, ctx)` (from T020), placed in `tests/src/fixtures.rs` so T032 can
    reuse it

### Tests for User Story 1 (write first; they must fail) ⚠️

- [X] T018 [US1] Write the US1 happy path in `tests/tests/f002_publish.rs` (FR-002–FR-004, US1-1,
  US1-5, SC-005):
  - on a launched fund, publish on days 20 739, 20 740 and 20 741 with three different rates;
  - assert NavRecords 1, 2 and 3 field by field: seq, nav, `daily_rate_e12`,
    `days_covered = 1`, `period_start_day`, `index_after` = cumulative `rate × 10^6`,
    `as_of_ts`, `published_by = fund_admin`, `override_used = false`;
  - assert FundState `nav`, `nav_ts`, `nav_seq = 3`, `daily_rate_e12`, `accrual_index` and
    `nav_period_end_day = 20 741`;
  - assert three `NavPublished` events with full payloads;
  - after 90 more daily publishes, records 1–3 are byte-for-byte unchanged
- [X] T019 [US1] Add `publish_daily` rejection tests to `tests/tests/f002_publish.rs`, in contract
  order. Each asserts the exact error and that FundState is unchanged and no NavRecord was
  created:
  - `RoleRegistryMismatch` (another fund's registry; a non-governance account);
  - `Unauthorized` from the issuer, compliance, keeper, transfer agent and a stranger (FR-001);
  - `FundNotOpen` on a fund still in Setup;
  - `ZeroPrice`;
  - `NegativeRate` for −1, for `i64::MIN`, and for a hand-built instruction whose rate bytes are
    `u64::MAX` (FR-006, research R6);
  - `ZeroDaysCovered`;
  - `PeriodNotStarted`: a second publish on the same day; a publish on Saturday 20 743 after a
    Friday 20 742 publish with `days_covered = 3`;
  - `PriceMoveTooLarge`: the first publish is measured from the target price (50 bps limit:
    `100_500_001` and `99_499_999` rejected); later publishes are measured from the previous
    price, up and down;
  - publish `Overflow` by patching `FundState.accrual_index` to `u128::MAX − 1` and, separately,
    `nav_seq` to `u64::MAX`, each with `fixtures::patch` and a comment explaining that no
    instruction can reach it;
  - a `nav_record` address for the wrong seq fails Anchor `ConstraintSeeds` (2006)
- [X] T020 [US1] Add the coverage and acceptance tests to `tests/tests/f002_publish.rs`
  (FR-007, FR-009, FR-009b, FR-010, I12):
  - a move exactly at the limit is accepted (`100_500_000` and `99_500_000` from $1.00);
  - Friday 20 742 with `days_covered = 3`, then Monday 20 745 is accepted with
    `period_start_day = 20 745`;
  - outage: publish on 20 739; skip 20 740; on 20 741 a publish with `days_covered = 2` starts at
    20 740;
  - catch-up as two records: on 20 741, publish 1 day (start 20 740), then 1 day (start 20 741),
    both accepted;
  - a 0 rate leaves `accrual_index` unchanged but still stores the record;
  - rate `i64::MAX` with `days_covered = u16::MAX` is accepted (no cap, FR-009b);
  - every HolderState account's data is byte-identical before and after a publish (FR-010);
  - I12 holds after every publish in this file (a `check_i12(h, ctx)` helper reading every
    NavRecord)
- [X] T021 [US1] Write `tests/tests/f002_override.rs` (FR-008, US1-4):
  - **Happy path**: compliance `approve_nav_override(99_000_000, Other, case)` gives
    `NavOverrideApproved` and `pending_override_nav = 99_000_000`; the publish at `99_000_000`
    (1 % from $1.00, limit 50 bps) is accepted with `override_used = true`,
    `override_cleared_nav = 99_000_000` and the pending override back to 0.
  - **Next publish uses it up**: approve `99_000_000`, then publish `98_000_000` gives
    `PriceMoveTooLarge` and the override is **still pending** (the failed tx changed nothing).
    Then a within-limit publish of `100_200_000` is accepted with `override_used = false`,
    `override_cleared_nav = 99_000_000`, pending 0.
  - **Equal but not needed**: an override equal to a within-limit price gives
    `override_used = false`, and the override is cleared.
  - **Withdraw**: `withdraw_nav_override` gives `NavOverrideWithdrawn { nav: 99_000_000 }`, and
    a publish at `99_000_000` is then rejected.
  - **Rejections**:
    - `OverrideAlreadyPending`, `NoOverridePending`;
    - `ZeroPrice` for `approve_nav_override(0)`;
    - `InvalidReason` for codes 0, 1, 2, 4, 5 and 7;
    - `Unauthorized` from the fund admin, issuer and keeper;
    - `FundNotOpen` on a Setup fund;
    - `UnauthorizedCaller` for raw `set_nav_override` and `clear_nav_override` signed by the
      compliance key and by a stranger.
  - **Cross-fund**: fund B's compliance passing fund B's role registry with fund A's
    `fund_state` gets Anchor `ConstraintAddress` (2012); with fund A's role registry it gets
    `Unauthorized`.
  - **Squads**: with compliance as a 2-of-3 Squads vault, one approval leaves no override (Squads
    6008); two approvals set it. This is the depth-4 call chain (research R5).

### Implementation for User Story 1

- [X] T022 [US1] Implement `publish_daily` in `programs/fund/src/instructions/publish_daily.rs`.
  Checks run in contract order, using `mmf_common::accrual` and `RoleRegistryView`:
  1. `RoleRegistryMismatch`;
  2. `Unauthorized` (signer ≠ `role_registry.fund_admin`);
  3. `FundNotOpen`;
  4. `ZeroPrice`;
  5. `NegativeRate` (`daily_rate_e12 < 0`);
  6. `ZeroDaysCovered`;
  7. `PeriodNotStarted`: `start = today` if `nav_seq == 0`, else `nav_period_end_day + 1`;
     reject if `start > today`;
  8. `PriceMoveTooLarge`: `prev = nav_seq > 0 ? nav : terms.target_nav`; pass if
     `price_move_ok`, or if `pending_override_nav == nav`;
  9. `Overflow`: `start + days − 1`, `nav_seq + 1`, and the index add.

  Then write the NavRecord and update FundState. `override_used` is true only when the move check
  failed and the override matched. Set `pending_override_nav = 0` and emit `NavPublished` with
  `override_cleared_nav` = the previous pending value. Read no HolderState (FR-010)
- [X] T023 [US1] Implement `set_nav_override` and `clear_nav_override` in
  `programs/fund/src/instructions/nav_override.rs`:
  - `UnauthorizedCaller` unless `fund_caller.key() == Pubkey::find_program_address(&[FUND_CALLER], &GOVERNANCE_PROGRAM_ID).0`
    and it signed;
  - `FundNotOpen`;
  - set only: `ZeroPrice`, then `OverrideAlreadyPending`;
  - clear only: `NoOverridePending`.

  Emit `NavOverrideApproved` or `NavOverrideWithdrawn` (`actor` = the compliance key passed in)
- [X] T024 [US1] Implement `approve_nav_override` and `withdraw_nav_override` in
  `programs/governance/src/instructions/nav_override.rs`. Checks in order:
  1. `Unauthorized` (signer ≠ `role_registry.compliance`);
  2. `FundNotOpen` (`FundStateView` load fails or status ≠ Open);
  3. `InvalidReason` (`!allowed_for_nav_override()`).

  Then CPI `fund::cpi::set_nav_override` or `clear_nav_override`, signed with seeds
  `[FUND_CALLER, bump]`, passing `actor = compliance` (depends on T002, T023)
- [X] T025 [US1] Run `cargo test -p tests --test f002_publish --test f002_override` until green,
  then the full suite (Constitution XII)

**Checkpoint**: US1's independent test passes, and every publish and override rule rejects with
its exact error.

---

## Phase 4: User Story 2 — Fair interest for every holder (Priority: P1) 🎯 MVP part 2

**Goal**: every transfer settles both sides' interest on their old balances, exactly, before the
balance changes, so each holder earns for exactly the units held at each publish (FR-012 to
FR-019, FR-020/FR-021 for settlement records).

**Independent Test**: holder A holds 1 000 000 units for 10 publishes, sends half to B after
publish 10, and holds the rest for 10 more publishes, with a different rate each day. A's and B's
earned interest equal a day-by-day calculation exactly (spec US2).

### Tests for User Story 2 (write first; they must fail) ⚠️

- [X] T026 [US2] Write the spec's independent test in `tests/tests/f002_accrual.rs`:
  - seed A with `1_000_000_000_000` base units (1 000 000 units, 6 dp) before the first publish;
  - make 10 daily publishes with rates from a fixed list of 20 distinct `i64` values;
  - transfer `500_000_000_000` A → B;
  - make 10 more publishes;
  - then a 0-unit A → B transfer to settle both.

  Assert `HolderState.accrued_e18` for A and B equals expected values computed in the test from
  the explicit rate list. The expected values come from a plain loop
  `Σ balance × rate × 1_000_000`, **not** from `mmf_common::accrual` (US2-1, US2-2, US2-4)
- [X] T027 [US2] Add ordering and accrual-rule tests to `tests/tests/f002_accrual.rs`:
  - **Ordering**: transfer then publish → the receiver earns that publish on the received units;
    publish then transfer → the sender does. Both run in the same slot (no slot change between
    transactions) and in different slots (FR-013, US2-6).
  - **US2-1**: a holder unchanged for N publishes with mixed `days_covered` (1, 3, 2) earns
    `balance × Σ rate × 10^6 × days`.
  - **FR-016**: a holder registered and approved after 5 publishes, then receiving units, has
    `accrued_e18 = 0` and `index_at_last_touch` = the current index.
  - **FR-015**:
    - a holder suspended across 3 publishes, then reinstated and sending, is settled for all 3;
    - the same for KYC-expired then renewed;
    - transfers paused across 2 publishes, then unpaused, settles both;
    - an approved fund treasury holding units accrues like an investor.
  - **FR-017**: `mmf_sdk::accrual::earned_e18` read with no transaction equals the expected value,
    and equals stored `accrued_e18` after a 0-unit transfer to another approved holder.
  - **Failed transfer settles nothing**: a transfer rejected with `ReceiverNotApproved` leaves
    both HolderStates byte-identical.
  - **Records (FR-020)**: `TransferValidated` carries `accrual_index`, `source_accrued_e18` and
    `destination_accrued_e18` after settlement.
- [X] T028 [US2] Add the records test to `tests/tests/f002_accrual.rs` (FR-021, SC-007). From
  NavRecords and the `TransferValidated` events of a 15-publish scenario with 4 holders, rebuild
  every holder's balance history and earned interest using only those records. Assert they equal
  the stored `HolderState` figures. The rebuild lives in `tests/src/rebuild.rs` from the start, so
  T032 can reuse it
- [X] T029 [US2] Add hook step 8 cases to `tests/tests/hook_order.rs`:
  - **Ordering**: a transfer failing at steps 3, 4, 6 or 7 leaves `accrued_e18` and
    `index_at_last_touch` unchanged for both holders.
  - **Forged FundState**: patch the FundState account's owner to the system program with
    `set_account` (comment: unreachable by instruction) → `WrongMint`.
  - **Overflow**: patch the source `HolderState.index_at_last_touch` to `accrual_index + 1` (same
    comment) → hook `Overflow`, and both HolderStates are unchanged.

### Implementation for User Story 2

- [X] T030 [US2] Implement hook step 8 in `programs/compliance_hook/src/hook/execute.rs` per
  [contracts/compliance_hook.md](contracts/compliance_hook.md):
  - **Index**: after step 7, load
    `FundStateView::load(&a.fund_state, &a.registry.fund_state)`, mapping any error to
    `WrongMint`, and take `index = view.accrual_index`.
  - **Same owner**: `settle(src, post_src + post_dst, index)` once.
  - **Otherwise**: compute `pre_src = post_src + amount` and `pre_dst = post_dst − amount`
    (checked, `Overflow`), then settle each with `mmf_common::accrual::settle` (`None` →
    `Overflow`).
  - **Single write**: settle before `record_balance` (step 9) and still serialize each
    HolderState once.
  - **Event**: fill `TransferValidated.accrual_index`, `source_accrued_e18` and
    `destination_accrued_e18` with the values after settlement.
  - **Docs**: update the module doc's step list to "F002 builds step 8"
- [X] T031 [US2] Run `cargo test -p tests --test f002_accrual --test hook_order` until green, then
  the full suite (Constitution XII)

**Checkpoint**: US2's independent test passes exactly. US1 and US2 together are the MVP: published
figures plus fair interest.

---

## Phase 5: Polish & cross-cutting concerns

- [X] T032 [P] Write `tests/tests/accrual_prop.rs` (SC-001, SC-002, I11, I12, plus I3 and I10).
  `proptest`, 64 cases by default (`PROPTEST_CASES` to raise). Each case covers 90 days with 6
  investors plus the fund treasury.
  - **Operations**:
    - `Publish { rate in 0..=500_000_000i64, days: weighted 1 (most), 3 (Fridays), 2 (outage catch-up) }`;
    - `Transfer { from, to, amount }`;
    - `Suspend`, `Reinstate`, `Pause`, `Unpause`;
    - `Warp` to the next uncovered day;
    - `Seed` (only through `seed_units`).
  - **After every step**, compare with `AccrualModel` (T011):
    - each holder's `earned_e18` (stored + pending, via the SDK helper) equals the model
      **exactly**;
    - Σ over holders == `model.total_earned()`;
    - I12 holds;
    - `holder_count` and `last_balance` (I10) match.
  - **At the end of each case**: the earned interest rebuilt from records (T028's rebuild
    helper in `tests/src/rebuild.rs`) equals the stored figures (SC-007)
- [X] T033 [P] Extend `tests/tests/invariants_prop.rs` with `Op::Publish`, which publishes for
  the next uncovered day at a random rate, so the F001 checks (I3, I10, outcome prediction) run
  with publishes interleaved. The model in `tests/src/model.rs` treats a publish as always
  succeeding on an Open fund
- [X] T034 [P] Extend `tests/tests/scale.rs` (`#[ignore]`):
  - **SC-003**: `publish_daily` compute units and account count are identical with 10 and 10 000
    registered holders.
  - **SC-004**: hook compute units for a transfer from a holder last settled 1 publish ago and
    one last settled 90 publishes ago agree within a 16-CU band, at 10 and at 10 000 holders, and
    stay ≤ 60 000 CU. (SBF u128 arithmetic varies by 1–2 CU with the values; measured
    21 959–21 961 with 2 holders and nothing else changing. Any real growth costs ~1 500 CU or
    more.) Publish cost is compared by republishing the same seq after restoring FundState,
    because each NavRecord address has its own bump.
- [X] T035 Run `scripts/check-error-coverage.sh` and add any missing rejection test until it
  reports none. It must list every new `FundError` variant as covered (SC-006)
- [X] T036 [P] Extend `scripts/devnet-e2e/src/main.rs` with the F002 steps in
  [quickstart.md](quickstart.md#devnet-run-research-r13):
  - create the fund with `max_nav_move_bps = 50`;
  - rejected over-limit first publish (`PriceMoveTooLarge`);
  - non-admin publish (`Unauthorized`);
  - Squads compliance override → publish at `99_000_000` with `override_used`;
  - same-day re-publish (`PeriodNotStarted`);
  - approve and withdraw;
  - print NavRecord 1.

  Add `--continue <fund_state>`, which publishes the next record (3 days on a Friday, else 1) and
  prints the full history. Exit 0 only if every outcome matches
- [ ] T037 [P] Update the root `README.md`: F002 status, how the daily publish and override work
  (one paragraph each), and the new test files. Link
  [specs/002-daily-price-accrual/quickstart.md](quickstart.md)
- [ ] T038 [P] Write `specs/002-daily-price-accrual/review.md`, the human review checklist in the
  F001 `review.md` format. Items with code links:
  - money math against shared-model §8 (index increment, price check, settle; no division);
  - hook step 8 placement against §6 (after step 7, before step 9; single write);
  - signer checks against §5.1 (`publish_daily`, `set_/clear_nav_override` PDA check,
    `approve_/withdraw_nav_override`);
  - the CPI edge against §5.2;
  - invariants I11 and I12 against §7;
  - FundState offsets unchanged for F001 accounts;
  - every F001 test edit listed
- [ ] T039 Upgrade the three programs on devnet through the issuer's Squads vault (F001 plan
  "Deployment and authorities") and run `cargo run -p devnet-e2e`. Then run
  `cargo run -p devnet-e2e -- --continue <fund>` on each of the next two UTC days. Record program
  IDs, signatures and the three-record history in `specs/002-daily-price-accrual/devnet-run.md`
- [ ] T040 Run the full suite and validation:
  `anchor build && cargo test -p mmf-common && cargo test -p tests && cargo test -p tests --release -- --ignored`
  and [quickstart.md](quickstart.md). Open the pull request `002-daily-price-accrual` → `main`
  listing:
  - the shared-model changes (plan "Shared-model changes");
  - the spec adjustments;
  - every F001 test or helper edit (T006, T008, T010, T033) with the reason for each
    (Constitution XII).

  A human reviewer completes T038's checklist before merge

---

## Dependencies & execution order

### Phase dependencies

- **Setup (Phase 1)**: none.
- **Foundational (Phase 2)**: needs Setup. T007 needs T005; T008 needs T005 and T007; T010 needs
  T005; T012 needs all of T003–T011. Blocks both stories.
- **US1 (Phase 3)**: needs Foundational.
- **US2 (Phase 4)**: needs US1's `publish_daily` (T022), because interest only moves when a price
  is published. US2's test-writing tasks (T026–T029) may start once T017 exists.
- **Polish (Phase 5)**: needs both stories. T039 needs T036; T040 needs everything.

### Within each story

Interface stubs → SDK builders and helpers → tests (run them and see them fail) → implementation
→ checkpoint. Every rule's rejection test comes before the code that enforces it (Principle VI).

### Story order

```text
Setup → Foundational → US1 (publish + override) → US2 (accrual in the hook) → Polish
                                  └── US2 tests T026–T029 may be written once T017 exists
```

---

## Parallel opportunities

- **Foundational**: T003, T004, T006, T009 and T011 together, alongside T005; then T007 → T008,
  and T010.
- **US1**: T015 and T017 alongside T013 → T014; tests T018–T020 (one file, sequential) alongside
  T021 (separate file); implementation T022 and T023 together, then T024.
- **US2**: tests T026–T028 (one file) alongside T029 (`hook_order.rs`); one implementation task,
  T030.
- **Polish**: T032, T033, T034, T036, T037 and T038 together.

### Parallel example: User Story 1

```text
Task: "Implement publish_daily in programs/fund/src/instructions/publish_daily.rs"            (T022)
Task: "Implement set_/clear_nav_override in programs/fund/src/instructions/nav_override.rs"   (T023)
# then T024 approve_/withdraw_nav_override in governance, which CPIs into T023
```

### Parallel example: Polish

```text
Task: "Write tests/tests/accrual_prop.rs (SC-001, SC-002, I11, I12)"      (T032)
Task: "Extend tests/tests/invariants_prop.rs with Op::Publish"            (T033)
Task: "Extend tests/tests/scale.rs for SC-003, SC-004"                    (T034)
Task: "Extend scripts/devnet-e2e with the F002 steps and --continue"      (T036)
```

---

## Implementation strategy

### MVP first (US1 + US2, both P1)

1. Phase 1 Setup → Phase 2 Foundational. **Stop if T012 shows an F001 regression** and fix it
   first (Principle I).
2. Phase 3 US1 → validate: three days published and read back by seq; the override path works
   through Squads.
3. Phase 4 US2 → validate: the spec's 20-publish A/B test matches exactly. Prices without fair
   interest aren't a useful demo, so the MVP is US1 + US2.

### Then

4. Phase 5 → 90-day property tests, scale, error coverage, devnet over three days, review, merge.

Commit after each task or logical group, on `002-daily-price-accrual`.

---

## Notes

- `[P]` = different files and no dependency on an unfinished task.
- Tests that patch program state with `set_account` are allowed **only** for conditions no
  instruction can reach (T019 publish overflow; T029 forged FundState and hook overflow), and
  each says so in a comment.
- No task builds `compliance_hook::accrue`, mint, burn or payout. Those are F003 and F006
  (research R8; Principle XI).
- New error variants are appended, so F001's error codes don't move. Any F001 test edit beyond
  T006, T008, T010 and T033 must be justified in the PR (Principle XII).
- If a task needs a shared-model change not already in `docs/shared-model.md`, stop and make it
  there first (Principle XIII).
