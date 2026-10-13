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
