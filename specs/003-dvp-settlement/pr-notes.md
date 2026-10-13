# F003 pull request notes (draft, for T081)

Collected while implementing, so the PR description can list them (Constitution XII).

## Baseline before F003 (T001, 2026-10-13)

- `cargo test -p tests`: 157 passed, 2 ignored (`scale`), 0 failed
- `cargo test -p mmf-common`: 23 passed

## F001/F002 test and helper edits

| Task | File | Change | Why |
|---|---|---|---|
| T011 | `tests/tests/layouts.rs` | FundState test sets the F003 fields, checks size still equals F001's, `units_at_day_start` after `pending_override_nav`, 44 reserved bytes; views decode the new FundState and Registry fields; HolderState test also loads `HolderStateView`; new DvpConfig and SettlementInstruction layout tests | FundState took 8 more reserved bytes (research R9); new views (T010) |
| T013 | `tests/src/units.rs`, `tests/src/assert.rs` | `seed_units` also adds to `FundState.units_outstanding`; new `assert_i4` helper | otherwise invariant I4 (`units_outstanding == supply`) fails in every test that seeds units (research R18); F001/F002 assertions unchanged |
| T018 | `tests/tests/us2_onboarding.rs` | F001's "escrow → `ClassNotAvailable`" check replaced: escrow with a random owner or another fund's escrow PDA → `NotSystemControlled`; new test that compliance registers and approves this fund's escrow PDA | the settlement escrow exists from F003 (F001 research R17); `CollateralVault` still `InvalidClass` |

## Other notes

- `FundError::UnauthorizedCaller` message changed from "Only governance may call this instruction" to "Caller is not the program allowed to call this instruction" (T007): the dvp escrow PDA now also calls `fund`. The error code is unchanged.
- `HookError::ClassNotAvailable` is retired (T021): nothing returns it any more. It stays in the enum, marked `// retired`, so later error codes keep their numbers. `scripts/check-error-coverage.sh` now skips variants marked `// retired`.
- `mmf-common` gains a dependency on `anchor-spl` (token_2022 features) for the shared cash-token checks in `mmf_common::cash`, used by `fund` and `dvp_settlement` (T019, T020).
- `init_dvp` creates the escrow ATAs idempotently: anyone can create an ATA for any owner, so a pre-created escrow account must not block setup (test `init_dvp_is_not_blocked_by_escrow_accounts_created_in_advance`).
