# F003 pull request notes (draft, for T081)

Collected while implementing, so the PR description can list them (Constitution XII).

## Baseline before F003 (T001, 2026-10-13)

- `cargo test -p tests`: 157 passed, 2 ignored (`scale`), 0 failed
- `cargo test -p mmf-common`: 23 passed

## F001/F002 test and helper edits

| Task | File | Change | Why |
|---|---|---|---|
| T011 | `tests/tests/layouts.rs` | FundState test sets the F003 fields, checks size still equals F001's, `units_at_day_start` after `pending_override_nav`, 44 reserved bytes; views decode the new FundState and Registry fields; HolderState test also loads `HolderStateView`; new DvpConfig and SettlementInstruction layout tests | FundState took 8 more reserved bytes (research R9); new views (T010) |

## Other notes

- `FundError::UnauthorizedCaller` message changed from "Only governance may call this instruction" to "Caller is not the program allowed to call this instruction" (T007): the dvp escrow PDA now also calls `fund`. The error code is unchanged.
