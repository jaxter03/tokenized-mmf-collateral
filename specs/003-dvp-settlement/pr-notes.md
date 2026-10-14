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
| T072 | `tests/tests/invariants_prop.rs` | `assert_i4` after every operation; new variant running the same sequences on a fund with settlement set up (an approved escrow holder exists); F001/F002 operations, predictions and checks unchanged | I4 holds in every suite (Constitution VI); F001 invariants must hold with the escrow present |
| T073 | `tests/tests/scale.rs` | new test `settlement_cost_does_not_grow_with_holders_or_open_instructions` **added**; earlier tests unchanged | SC-007 |

## Other notes

- `FundError::UnauthorizedCaller` message changed from "Only governance may call this instruction" to "Caller is not the program allowed to call this instruction" (T007): the dvp escrow PDA now also calls `fund`. The error code is unchanged.
- `HookError::ClassNotAvailable` is retired (T021): nothing returns it any more. It stays in the enum, marked `// retired`, so later error codes keep their numbers. `scripts/check-error-coverage.sh` now skips variants marked `// retired`.
- `mmf-common` gains a dependency on `anchor-spl` (token_2022 features) for the shared cash-token checks in `mmf_common::cash`, used by `fund` and `dvp_settlement` (T019, T020).
- `init_dvp` creates the escrow ATAs idempotently: anyone can create an ATA for any owner, so a pre-created escrow account must not block setup (test `init_dvp_is_not_blocked_by_escrow_accounts_created_in_advance`).
- `fund::instructions::mint_burn::check_settlement` and `compliance_hook::instructions::accrue::check_holder_accounts` are public so tests can call them with forged accounts: `dvp_settlement` already checks the same accounts, and only its PDAs can sign, so no transaction can deliver a forged settlement or holder to them (T032, T033).
- `fund` checks the hook and Token-2022 program IDs before signing CPIs with its FundState PDA (the mint authority): handing that signature to any other program could create units (T037).
- `accrue` takes the FundState as its signer and reads the counter from it (one account instead of the contract's two); `settle` takes no role registry. Contracts updated.
- Measured: a subscription `settle` uses 120 927 CU (budget 200 000, research R15).
- Measured (T051): funding units 69 436 CU (budget 90 000); a redemption `settle` 80 857 CU (budget 120 000).
- `cancel`, `expire`, `reject` and `return_leg` check the instruction's fund by its stored `fund_state` (`FundMismatch`) rather than by an Anchor seeds constraint, so a mismatched fund reports the contract's error (T056). `affirm_and_fund` and `settle` keep the seeds constraint.
- Measured (T073, release): a subscription `settle` 129 585 CU and a redemption 89 869 CU, identical with 10 vs 10 000 holders and 1 vs 1 000 open instructions (steady state, after the day's first mint and the buyer's first units).
- All 113 error variants are asserted by a test (T075).
- Devnet script (T077) rehearsed on a local validator with the four programs and Token-2022 v11: 31 of 31 steps matched, including a funding signed 130 s after it was prepared on a durable nonce. `--continue` (the next UTC day's redemption) not yet rehearsed; LiteSVM covers that path.
