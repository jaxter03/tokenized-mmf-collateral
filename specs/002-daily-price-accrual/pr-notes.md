# F002 pull request notes (draft, for T040)

Collected while implementing, so the PR description can list them (Constitution XII).

## Baseline before F002 (T001, 2026-10-13)

- `cargo test -p tests`: 110 passed, 1 ignored (`scale`), 0 failed
- `cargo test -p mmf-common`: 15 passed

## F001 test and helper edits

| Task | File | Change | Why |
|---|---|---|---|
| T006 | `tests/src/events.rs` | `sample()` fills the three new `TransferValidated` fields | the event payload grew (shared-model §9); decoding tests unchanged |
| T008 | `tests/tests/layouts.rs` | FundState test sets the F002 fields, checks size equals F001's, `bump` unmoved, new fields after it, 52 reserved bytes; view decodes `accrual_index`; new NavRecord layout test | FundState took reserved bytes (research R10) |
| T029 | `tests/tests/hook_order.rs` | three tests **added** at the end (step 8 placement, forged FundState, accrual overflow); no existing test changed | hook step 8 (shared-model §6) |
| T033 | `tests/tests/invariants_prop.rs` | new `Op::Publish`; `check_i12` after every step; existing operations, predictions and checks unchanged | F001 invariants must hold with the counter moving |
| T034 | `tests/tests/scale.rs` | new test `publish_and_settlement_cost_do_not_grow` **added**; F001's test unchanged | SC-003, SC-004 |
| T010 | `tests/src/units.rs` | `seed_units` settles the holder's interest at the fund's current counter before raising `last_balance` | otherwise seeded units would earn for publishes before they arrived (research R12); F001 tests have counter 0, so behaviour for them is unchanged |

## After Phase 2 (T012, 2026-10-13)

- `cargo test -p tests`: 112 passed, 1 ignored, 0 failed (baseline 110 + `nav_record_layout` + `three_days_by_hand`)
- `cargo test -p mmf-common`: 23 passed (baseline 15 + 7 accrual + 1 override reasons)

## After US1 (T025, 2026-10-13)

- `cargo test -p tests`: 141 passed, 1 ignored, 0 failed (+17 `f002_publish`, +12 `f002_override`)
- `scripts/check-error-coverage.sh`: all 73 error variants asserted
- Test-only state patches so far (each for a condition no instruction reaches): `f002_publish::counter_and_sequence_overflow_fail_closed` (FundState `accrual_index`, `nav_seq`)

## After US2 (T031, 2026-10-13)

- `cargo test -p tests`: 156 passed, 1 ignored, 0 failed (+12 `f002_accrual`, +3 `hook_order`)
- `scale` (release, ignored): hook 18 960 CU with 10 and with 10 000 holders (ceiling 60 000)
- `scripts/check-error-coverage.sh`: all 73 error variants asserted
- More test-only state patches: `hook_order::a_fund_state_that_cannot_be_verified_is_rejected` (FundState owner), `hook_order::accrual_arithmetic_fails_closed` (HolderState `index_at_last_touch`)
- Note for review: "a rejected transfer settles nothing" is also guaranteed by transaction atomicity; the tests pin it so a future non-reverting path would be caught

## Polish (T032–T034, 2026-10-13)

- `accrual_prop` (64 cases × ~90 days, ~87 s debug): exact per-holder match with the day-by-day model, sum check, I10, I12, and records rebuild. Mutation check: crediting the receiver on its post-balance fails at the first transfer into a holder
- `scale` (release): publish CU identical at 10 and 10 000 holders (same seq republished after restoring FundState); settlement CU within 16 CU across 10/10 000 holders and 1/90 publishes since (SBF u128 cost varies ±2 CU with values)
