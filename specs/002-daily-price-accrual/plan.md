# Implementation Plan: Daily Price and Interest Accrual

**Branch**: `002-daily-price-accrual` | **Date**: 2026-10-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/002-daily-price-accrual/spec.md`, the shared plan
input (`docs/speckit-inputs.md` §4.1), the F002 plan note (§4.2), and F001's merged code and
[data model](../001-fund-launch-onboarding/data-model.md).

## Summary

F002 makes the fund admin publish one official price and daily interest rate for each covered
period, kept forever as a `NavRecord`. It credits interest to every holder without touching any
holder on publish:
- **Publishing**: each publish adds `rate × 10^6 × days_covered` to one global counter on the
  `FundState`, `accrual_index`.
- **Settlement**: the transfer hook's step 8 settles each side's interest on its old balance
  before the balance changes, so a holder's earned interest is always
  `balance × (counter now − counter at last settlement)`. Settlement is exact in units × 10^18;
  rounding happens only at payout (F006).
- **Price-move limit**: a price that moves more than the Fund's limit is accepted only if
  compliance approved an override for exactly that price. The approval goes through governance,
  the same path F001 uses for suspensions.

Key design choices (details in [research.md](research.md)):
- Coverage is contiguous: each publish starts the day after the previous period and may not start
  in the future (R3).
- A day is a UTC calendar day (R2).
- The rate parameter is signed, so a negative rate is a real rejection (R6).
- The override lives in FundState's reserved bytes and is cleared by the next publish (R5, R10).
- `compliance_hook::accrue` (before mint and burn) moves to F003, its first caller (R8).
- The tests' reference model is independent of the on-chain math (R11).

## Technical Context

**Language/Version**: unchanged from F001. Rust 1.90 (pinned) with Anchor 1.1.2 ("configuration
B", F001 research R1/R8). No TypeScript in F002; the client and UI layer is F008.

**Primary Dependencies**: unchanged from F001: `anchor-spl` 1.1.2, Token-2022 v11 on-chain,
Squads v4 for the compliance and fund-admin roles. New internal edge: `governance` depends on the
`fund` crate (`cpi`, `no-entrypoint`) for the override CPI.

**Storage**: on-chain accounts only. New: `NavRecord` (one per publish, about 97 bytes). Extended:
`FundState` (two fields taken from reserved bytes), `HolderState` (existing F002 fields now
written). No database (the indexer is F008).

**Testing**: LiteSVM 0.12.0 with the F001 fixtures. One rejection test per rule, asserting the
exact error. `proptest` 90-day accrual simulations against an independent reference model
(SC-001, SC-002). The F001 randomized suite keeps running with publishes interleaved. Compute-unit
equality tests for publish and settlement (SC-003, SC-004). The error-coverage script covers the
new variants (SC-006). One devnet run (R13). Map in [quickstart.md](quickstart.md).

**Target Platform**: Solana devnet, mainnet-compatible. Local tests on LiteSVM (F001 research R8).

**Project Type**: Anchor workspace (on-chain programs), plus Rust support crates and a test crate.

**Performance Goals**:
- `publish_daily` touches a fixed set of 8 accounts whatever the number of holders (FR-011,
  SC-003).
- The hook adds one account decode and two checked `u128` multiplications, staying ≤ 60 000 CU
  per transfer.
- Settlement effort doesn't depend on the number of holders or on publishes since the last
  settlement (FR-018, SC-004).
- Call depth is at most 4: Squads → governance → fund → `emit_cpi`.

**Constraints**:
- Checked integer math only, with no division in any F002 formula (shared-model §8 rows added).
- Fail closed: a missing or forged FundState in the hook is rejected, overflow aborts, and
  coverage that would start in the future is rejected.
- No mint, burn or interest payout.
- Every new instruction has a named signer in shared-model §5.1.

**Scale/Scope**: one fund, up to about 10 000 holders, one publish per business day (about 250 to
365 NavRecords a year).

## Constitution Check

*GATE: checked before Phase 0 research and again after Phase 1 design. Both pass.*

| Principle | How F002 complies | Pre | Post |
|---|---|---|---|
| I. Correctness over features | `accrue` for mint/burn is deferred to F003 rather than shipped untested (R8). Accrual is exact, so SC-001 demands equality, not a tolerance | ✅ | ✅ |
| II. Rules live on-chain | Price limit, override, coverage and accrual are all in `fund` and the hook. Tests send `publish_daily`, `set_nav_override` and raw `transfer_checked` directly | ✅ | ✅ |
| III. Named signer | `publish_daily`: fund_admin. `approve_` / `withdraw_nav_override`: compliance. `set_` / `clear_nav_override`: governance PDA `["fund-caller"]` only. All are added to shared-model §5.1. There is no permissionless instruction | ✅ | ✅ |
| IV. One instruction, one transition | Publish = one transition; clearing the pending override and moving the counter are bookkeeping within it. Approve and withdraw are separate instructions. Accrual inside a transfer is bookkeeping of that transfer (named in the constitution) | ✅ | ✅ |
| V. On-chain source of truth | The price and rate enter only through the fund admin's signed publish. `accrued_e18` / `index_at_last_touch` are the allowed accrual checkpoints, updated in the same instruction as the balance and invariant-tested (I11). `nav_period_end_day` is a checkpoint of the latest record, tested by I12. Current earned interest is computed on read (FR-017) | ✅ | ✅ |
| VI. Failing test first | [quickstart.md](quickstart.md) maps every FR and every new error to a rejection test. Tasks must order each test before its code | ✅ | ✅ |
| VII. Fail closed | Unverifiable FundState → `WrongMint`; overflow → abort with no partial write; a period not yet begun → `PeriodNotStarted`; an unknown reason → `InvalidReason` | ✅ | ✅ |
| VIII. No unpaid units, no silent powers | No mint path. The price override is role-gated (compliance), reason-coded (3 or 6 only), and recorded (`NavOverrideApproved` / `Withdrawn`, and the override's use in `NavPublished` and the NavRecord). It isn't destructive (moves no units, changes no rule), so it isn't time-locked | ✅ | ✅ |
| IX. Money math is exact | Index increment and settlement are exact, with no division. The price check is cross-multiplied and exact. Two rows are added to §8; payout rounding stays in F006 | ✅ | ✅ |
| X. Stated assumptions | The spec now states that the admin's rate and days are trusted with no cap (and what fails if they're wrong), that a day is a UTC day, record-date semantics, and no negative interest | ✅ | ✅ |
| XI. Features ship in order | F001 is merged. Only F002 fields are filled; F003+ fields stay zero; no new program | ✅ | ✅ |
| XII. Earlier features stay green | All F001 tests run unchanged. The only edits are the event decoder, the layout check and `seed_units` bookkeeping, listed in quickstart and to be listed in the PR | ✅ | ✅ |
| XIII. One shared model | All changes below are made in `docs/shared-model.md` on this branch, before code. No second account for an existing concept: the override and coverage live on `FundState` | ✅ | ✅ |

No violations, so Complexity Tracking is empty.

## Shared-model changes

Made in `docs/shared-model.md` on this branch (Principle XIII):

| # | Section | Change | Why |
|---|---|---|---|
| 1 | §1 Conventions | add **Day** = UTC day number (`u32`) | R2 |
| 2 | §4.2 FundState | `nav_period_end_day: u32` and `pending_override_nav: u64` after `bump`; `_reserved` 64 → 52 | R3, R5, R10 |
| 3 | §4.2 NavRecord | add `days_covered: u16`, `period_start_day: u32`, `override_used: bool`; immutable, never closed, no `_reserved`, separate payer; coverage rule stated | FR-002, FR-009, R9 |
| 4 | §4.5 | new governance PDA `["fund-caller"]`; NavOverride reasons limited to 3 and 6 | R5 |
| 5 | §5.1 | add `approve_nav_override`, `withdraw_nav_override` (compliance) and `set_nav_override`, `clear_nav_override` (governance PDA only); `publish_daily` with a separate payer | Clarification Q3 |
| 6 | §5.2 | new edge governance → `fund::set_` / `clear_nav_override`; `fund → accrue` tagged F003 | R5, R8 |
| 7 | §6 | step 8 made precise (FundState verified, settle formula); "F002 builds step 8" | R7 |
| 8 | §7 | new invariants I11 (per-holder interest) and I12 (index and period continuity) | SC-001, SC-002, Constitution V |
| 9 | §8 | rows: index increment (exact), price-move check (exact, cross-multiplied) | Constitution IX |
| 10 | §9 | new events `NavOverrideApproved`, `NavOverrideWithdrawn`; `TransferValidated` payload extended; `Accrued` tagged F003 | FR-020, R7 |
| 11 | §11 | F002 row updated; `compliance_hook::accrue` + CPI moved to the F003 row | R8 |

`docs/design.md` §12 says "intraday republish allowed". The shared model, which takes precedence,
never mentions one, and this plan allows none. design.md is left as is, and the difference is
recorded in research.

## Spec adjustments made during planning

Planning surfaced facts the spec has to state (workflow rule: fix the spec before code). None of
them changes a clarify decision:

| Spec section | Change | Why |
|---|---|---|
| Assumptions | a day is a UTC calendar day | R2 |
| Edge Cases: unused override | the next publish discards an override whether used or not; one pending at a time; reasons limited to operational error or other | R5 |
| FR-020 | a settlement's record is the record of the transfer that caused it | R7 |

## Deployment and authorities

- **Program upgrade**: the three programs are upgraded in place through the issuer's Squads vault
  with its time lock (F001 plan). FundState and HolderState keep their sizes and offsets (R10), so
  funds created under F001 need no migration. Their `accrual_index`, `nav_seq` and
  `nav_period_end_day` are 0, meaning "no publish yet".
- **No new authority**: the `["fund-caller"]` PDA signs only the two override CPIs, and `fund`
  accepts it only for those.

## Project Structure

### Documentation (this feature)

```text
specs/002-daily-price-accrual/
├── plan.md              # this file
├── research.md          # Phase 0
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1: validation guide and FR → test map
├── contracts/           # Phase 1
│   ├── fund.md              # publish_daily, set_/clear_nav_override
│   ├── governance.md        # approve_/withdraw_nav_override, new CPI edge
│   └── compliance_hook.md   # hook step 8, TransferValidated payload
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks (not created here)
```

### Source Code (repository root)

```text
programs/
├── fund/src/
│   ├── state.rs                    # + NavRecord; FundState nav_period_end_day, pending_override_nav
│   ├── errors.rs                   # + appended F002 variants
│   ├── events.rs                   # + NavPublished, NavOverrideApproved, NavOverrideWithdrawn
│   └── instructions/
│       ├── publish_daily.rs        # new
│       └── nav_override.rs         # new: set_nav_override, clear_nav_override
├── compliance_hook/src/
│   ├── hook/execute.rs             # step 8; FundState via FundStateView
│   └── events.rs                   # TransferValidated payload
└── governance/src/
    ├── instructions/nav_override.rs   # new: approve_/withdraw_nav_override
    └── Cargo.toml                  # + fund (cpi)
crates/
├── mmf-common/src/
│   ├── accrual.rs                  # new: utc_day, index_increment, settle, price_move_ok, earned_e18
│   ├── views.rs                    # FundStateView + accrual_index
│   ├── seeds.rs                    # + NAV ("nav"), FUND_CALLER ("fund-caller")
│   └── enums.rs                    # + ReasonCode::allowed_for_nav_override
└── mmf-sdk/src/                    # ix_fund::publish_daily, ix_governance override builders,
                                    # pda::nav_record / fund_caller, accrual read helper
tests/
├── src/
│   ├── model.rs                    # + independent accrual reference model (R11)
│   └── units.rs                    # seed_units settles accrual first (R12)
└── tests/
    ├── f002_publish.rs             # US1
    ├── f002_override.rs            # FR-008
    ├── f002_accrual.rs             # US2
    ├── accrual_prop.rs             # SC-001, SC-002, I11, I12
    ├── invariants_prop.rs          # + Publish op (I3, I10 still hold)
    ├── hook_order.rs               # + step 8 cases
    ├── layouts.rs                  # + FundState new offsets, NavRecord size
    └── scale.rs                    # + SC-003, SC-004
scripts/devnet-e2e/                 # + F002 steps and --continue (R13)
```

**Structure Decision**: same workspace as F001, with no new program or crate. `dvp_settlement` and
`collateral` still wait for F003 and F004 (Principle XI). The accrual formulas live in
`mmf-common` so the hook, `fund` and the SDK share one implementation; the test model stays
separate.

## Complexity Tracking

No constitution violations to justify.
