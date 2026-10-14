# F004 pull request notes (draft, for T079)

Collected while implementing, so the PR description can list them (Constitution XII).

## Baseline before F004 (T001, 2026-10-14)

- `cargo test -p tests`: 278 passed, 4 ignored (release-only), 0 failed
- `cargo test -p mmf-common`: 35 passed
- `scripts/check-duplicate-crates.sh`: passes (solana-address v1.1.0 + v2.9.0)

## F001–F003 test and helper edits

| Task | File | Change | Why |
|---|---|---|---|
| T004 | `tests/src/harness.rs` | loads `collateral.so` at `COLLATERAL_PROGRAM_ID` | fifth program (research R1) |
| T017 | `tests/src/fixtures.rs` | `create_standard_fund_with`, `launched_fund_with` and `settlement_ready_fund_opts` now delegate to new `create_fund_with_terms`, `launched_fund_terms` and `settlement_ready_fund_terms` (standard terms, same behaviour); F004 section added (`collateral_base_fund`, `worked_terms`) | the collateral fixtures need a 5 % price-move guard (tasks.md T017) |
| T010 | `tests/tests/layouts.rs` | FundStateView expectation gains `nav_ts`; new `collateral_account_layouts` test | `FundStateView` reads `nav_ts` for collateral's price age (research R4) |

## Other notes

- T015: the planted pre-F004 Registry test moved to T043; outside `collateral` no caller can sign as `["hook-caller"]`, so only a pledge reaches the empty-`collateral_program` check.
- Phase 2 (T019): `cargo test -p tests` 285 passed, 4 ignored, 0 failed.
