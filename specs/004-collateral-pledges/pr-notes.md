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
| T040 | `tests/tests/hook_order.rs` | new test `locked_units_are_checked_between_sender_and_receiver` **added**; earlier tests unchanged | hook step 5's place in the check order (FR-026 of F001) |
| T070 | `tests/tests/invariants_prop.rs` | I1/I2 asserted for every investor after every operation; operations, predictions and existing checks unchanged | I1 and I2 must hold in every suite from F004 (Constitution VI) |
| T071 | `tests/tests/scale.rs` | new test `collateral_cost_does_not_grow_with_holders_or_agreements` **added**; earlier tests unchanged | SC-007 |
| T010 | `tests/tests/layouts.rs` | FundStateView expectation gains `nav_ts`; new `collateral_account_layouts` test | `FundStateView` reads `nav_ts` for collateral's price age (research R4) |

## Other notes

- T015: the planted pre-F004 Registry test moved to T043; outside `collateral` no caller can sign as `["hook-caller"]`, so only a pledge reaches the empty-`collateral_program` check.
- T044: `accrue`'s holder-account check split into `owner_accounts` (shared with `set_lien`) plus the Escrow-class check; `accrue`'s behaviour and errors are unchanged.
- T046: the lien CPI is built by hand (`collateral::lien`), because Anchor's generated CPI passes an `UncheckedAccount` unsigned; `set_lien` keeps the caller unchecked so a wrong or unsigned caller reports `NotCollateralCaller`.
- T047 / R14 revised: `set_lien` measured 13 141–23 641 CU over 40 runs (each owner's ATA needs a different number of bump-search steps), so the plan's 15 000 CU target was replaced by ≤ 40 000 in the test; the whole pledge stays ≤ 80 000 (max 51 084 measured).
- T069: the property test's success floor is an aggregate over a fixed-seed run of 16 cases (≥ 25%; measured 335/975), not a per-case floor, which made random cases fail although chain and model agreed.
- After T069: `cargo test -p tests` 373 passed, 5 ignored (release-only), 0 failed; `collateral_prop` 256 cases pass in release.
- T071 measured (release): pledge 40 582 / 40 582, return call 23 337 / 23 339, release 34 384 / 34 384 CU at 10 holders + 1 agreement vs 10 000 holders + 300 agreements; publish 18 963 both.
- Devnet script (T075) rehearsed on a local validator with the five programs and Token-2022 v11 (2026-10-15): 37 of 37 steps matched, including a pledge signed 130 s after it was prepared on a durable nonce, and `PledgedUnitsLocked` on a send one base unit over the lock. `scripts/devnet-e2e/src/f003.rs`: `send_prepared` made `pub(crate)` so the F004 run reuses it.
- T079: the SC-007 scale test uses its own 100-CU band (`COLLATERAL_JITTER_CU`): the release cost moved by 49 CU between runs, in either direction, from value-dependent `u128` division; growth would cost ≥ 1 500 CU.
- Final validation (T079, 2026-10-15): 72 unit tests and 375 LiteSVM tests pass (6 ignored); all 6 release-only tests pass (incl. 256 collateral property cases and SC-007); 160/160 errors covered; fmt and clippy clean; devnet script 37/37 on a local validator.
- MVP checkpoint (T047): `cargo test -p tests` 333 passed, 4 ignored, 0 failed.
- Phase 2 (T019): `cargo test -p tests` 285 passed, 4 ignored, 0 failed.
