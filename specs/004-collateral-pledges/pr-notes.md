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

## Other notes
