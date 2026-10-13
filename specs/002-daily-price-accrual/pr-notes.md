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
| T010 | `tests/src/units.rs` | `seed_units` settles the holder's interest at the fund's current counter before raising `last_balance` | otherwise seeded units would earn for publishes before they arrived (research R12); F001 tests have counter 0, so behaviour for them is unchanged |

## After Phase 2 (T012, 2026-10-13)

- `cargo test -p tests`: 112 passed, 1 ignored, 0 failed (baseline 110 + `nav_record_layout` + `three_days_by_hand`)
- `cargo test -p mmf-common`: 23 passed (baseline 15 + 7 accrual + 1 override reasons)

## After US1 (T025, 2026-10-13)

- `cargo test -p tests`: 141 passed, 1 ignored, 0 failed (+17 `f002_publish`, +12 `f002_override`)
- `scripts/check-error-coverage.sh`: all 73 error variants asserted
- Test-only state patches so far (each for a condition no instruction reaches): `f002_publish::counter_and_sequence_overflow_fail_closed` (FundState `accrual_index`, `nav_seq`)
