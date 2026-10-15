# Implementation Plan: Collateral Agreements, Margin Calls and Pledges

**Branch**: `004-collateral-pledges` | **Date**: 2026-10-14 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/004-collateral-pledges/spec.md`, the shared plan input
(`docs/speckit-inputs.md` §4.1), the F004 plan note (§4.2), and the merged F001–F003 code and data
models ([F001](../001-fund-launch-onboarding/data-model.md),
[F002](../002-daily-price-accrual/data-model.md), [F003](../003-dvp-settlement/data-model.md)).
`/speckit-clarify` was not run; the spec's defaults stand, and the three F004 input questions are
answered in the spec's Assumptions.

## Summary

F004 adds the fifth and last program in the shared model's list, `collateral`. It lets a holder
pledge fund units to a counterparty without moving them:
- **Agreement**: giver and taker each sign a `PledgeAgreement` that mirrors the enforceable terms
  of their legal collateral agreement.
- **Exposure and calls**: the taker records its exposure. When the collateral value (total pledged
  units × the latest price × (1 − haircut), rounded down) falls short by more than the minimum
  transfer amount, a delivery call opens with a deadline and an enforce-after time.
- **Pledge**: the giver locks units. `collateral` asks `compliance_hook::set_lien` to raise
  `HolderState.pledged_amount`, and the hook checks that the balance covers it. Hook step 5 then
  refuses any transfer that would leave the balance below the lock. That covers direct sends,
  delegates and F003 escrow funding alike. Nothing moves, so interest is untouched.
- **Release, substitution, disputes**: the taker (or the keeper, against a return call) unlocks
  excess. The giver can swap a pledge for an equal or larger one in one step, and can dispute a
  call. Compliance resolves disputes through governance.

Key design choices (details in [research.md](research.md)):
- The lock lives on the HolderState, and only `set_lien` changes it. That instruction is signed
  by `collateral`'s `["hook-caller"]` and enforces I2 against the real balance (R2).
- Hook step 5 arrives without the enforcement bypass, which is F005's step 2 (R3).
- The agreement stores the **demanded part** of the shortfall. All open delivery calls are Met
  together when a valuing step finds coverage ≥ 0, so steps never need the list of open calls and
  prepared pledges stay valid (R5).
- Pledges and calls are addressed by party-chosen references, not sequence numbers (R6, the F003
  R2 pattern).
- A raise and the call it opens are one instruction, the named Constitution IV exception (R7).
- Compliance resolves disputes through governance's new `["collateral-caller"]`, like F003's
  reject (R10).

## Technical Context

**Language/Version**: unchanged. Rust 1.90 (pinned) with Anchor 1.1.2. The user input names
TypeScript for the client and UI; that layer is F008, and F004 ships Rust SDK builders only.

**Primary Dependencies**:
- Unchanged: Token-2022 v11 (`transfer_checked` with the hook), Squads v4 for compliance, and
  `anchor-spl` 1.1.2.
- New internal edges:
  - `collateral` → `compliance_hook` (`cpi`) for `set_lien`;
  - `governance` → `collateral` (`cpi`) for `resolve_dispute`.
- No cash leg in F004.
- No off-chain service: axum, sqlx and Yellowstone arrive in F008.

**Storage**: on-chain accounts only. New, never closed: `PledgeAgreement` (432 B, one per
agreement), `Pledge` (196 B, one per pledge) and `CollateralCall` (268 B, one per call). Filled
without resizing: `HolderState.pledged_amount` and `active_liens`, and
`Registry.collateral_program`. No database.

**Testing**:
- LiteSVM 0.12.0 with the F001–F003 fixtures and a new `collateral_ready_fund` fixture.
- One rejection test per rule, asserting the exact error.
- `proptest` random collateral sequences against an independent reference model (R15).
- A lock-bypass suite and durable-nonce tests with the blockhash check on.
- Compute-unit equality (SC-007), the error-coverage script (SC-006) and one devnet run.
- Map in [quickstart.md](quickstart.md).

**Target Platform**: Solana devnet, mainnet-compatible. Local tests on LiteSVM.

**Project Type**: Anchor workspace (programs/), Rust support crates (crates/), test crate (tests/)
and devnet script (scripts/). `services/` and `app/` start in F008.

**Performance Goals** (R14):
- Every `collateral` instruction ≤ 80 000 CU. `set_lien` costs about 13 000 CU plus about
  1 500 per ATA bump-search step, which depends on the address (≤ 40 000 in tests; R14). Hook
  step 5 adds under 100 CU.
- Call depth ≤ 4.
- Every instruction uses a fixed account set whatever the number of holders, agreements, pledges
  or open calls (SC-007).
- `publish_daily` is untouched.

**Constraints**:
- No backend key: `check_coverage` is permissionless and decides nothing, and the keeper can
  only release what a return call asks, within coverage.
- Every party step works with durable nonces, at addresses known when it is prepared.
- Checked integer math with the shared-model §8 rounding: units for a call up; value and return
  units down.
- Fail closed: stale or missing price, ineligible actor, over-limit and under-coverage all abort.

**Scale/Scope**: one fund, up to about 10 000 holders and hundreds of agreements.

## Constitution Check

*GATE: checked before Phase 0 research and again after Phase 1 design. Both pass.*

| Principle | How F004 complies | Pre | Post |
|---|---|---|---|
| I. Correctness over features | Enforcement, holds, defaults (F005), termination, title transfer and netting are out. Oldest-first call meeting was dropped for a rule every step can apply without the call list (R5). Substitution is limited to equal or larger (R9) | ✅ | ✅ |
| II. Rules live on-chain | The lock is enforced by the token's own hook (step 5) on every transfer, and by `set_lien` against the real balance. Tests submit raw `transfer_checked`, delegate transfers, F003 funding and direct `set_lien` calls with no SDK in the path | ✅ | ✅ |
| III. Named signer | giver: propose/sign, confirm, return call, pledge, substitute, dispute, withdraw, cancel return. Taker: propose/sign, exposure, delivery call, release, cancel delivery. Keeper: release against a return call. Compliance through governance `["collateral-caller"]`: resolve. `collateral` `["hook-caller"]`: `set_lien`. `check_coverage` is permissionless and its outcome depends only on state and time. Shared-model §5.1 updated | ✅ | ✅ |
| IV. One instruction, one transition | **Named exceptions**: a raise and the call it opens (R7); substitution's new pledge and old release (spec FR-030). Observing coverage, and so marking calls Met, is bookkeeping of the step that changed coverage (R5). Events are bookkeeping | ✅ | ✅ |
| V. On-chain source of truth | Coverage, shortfall and excess are computed on use, never stored. A call's Met status is derived from `met_below_seq`. Three stored running totals: `pledged_amount` (I1), `PledgeAgreement.pledged_units` (I15) and `called_outstanding` (I16, history-dependent), each updated in the same instruction as its inputs and invariant-tested | ✅ | ✅ |
| VI. Failing test first | [quickstart.md](quickstart.md) maps every FR and error to a rejection test; tasks must order each test before its code. The enforcement bypass waits for F005 because it can't be tested yet (R3) | ✅ | ✅ |
| VII. Fail closed | Missing or stale price, missing or ineligible holder records, an empty `collateral_program`, over-limit, under-coverage and overflow all abort the transaction. No step skips part of its effect | ✅ | ✅ |
| VIII. No unpaid units, no silent powers | F004 mints and burns nothing. Dispute resolution is role-gated, reason-coded and recorded (`DisputeResolved`; ActionRecord kind 15 in F007). It is not destructive: it moves no units and changes no rule. No setter for `collateral_program` exists (R2) | ✅ | ✅ |
| IX. Money math is exact | Three formulas, each with a stated direction (§8 rows: value down, delivery units up, return units down). Checked `u128`, overflow aborts | ✅ | ✅ |
| X. Stated assumptions | Spec Assumptions: exposure from the taker's system, legal agreement off-chain, price trust, threshold reading, substitution meaning, keeper trust. Quickstart lists known behaviours | ✅ | ✅ |
| XI. Features ship in order | F001–F003 merged (PR #3). Only F004 fields are filled; F005 fields (`enforced_*`, holds, step 2) stay zero. F003 US2 scenario 3 and F002 US2 scenario 3 are verified here, as those specs deferred | ✅ | ✅ |
| XII. Earlier features stay green | All F001–F003 suites run. Expected changes are additions only: `hook_order.rs` rows, the event decoder, `layouts.rs` sizes and `invariants_prop.rs` I1/I2. Any other change to an earlier test is listed in the PR | ✅ | ✅ |
| XIII. One shared model | All changes below are made in `docs/shared-model.md` on this branch, before code. No second account for an existing concept: the lock stays on HolderState, the price is read from FundState, and roles from RoleRegistry | ✅ | ✅ |

No violations, so Complexity Tracking is empty.

## Shared-model changes

Made in `docs/shared-model.md` on this branch (Principle XIII):

| # | Section | Change | Why |
|---|---|---|---|
| 1 | §4.1 Registry | `collateral_program` is set by `init_registry` to the collateral program id; no setter | R2 |
| 2 | §4.4 PledgeAgreement | new fields `activated_ts`, `pending_raise_{value, ref, ts}`, `exposure_ref`, `exposure_updated_ts`, `called_outstanding`, `open_delivery_calls`, `met_below_seq`, `open_return_call`; counters start at 1 | R5, R7, R8 |
| 3 | §4.4 Pledge | seeds `["pledge", agreement, pledge_ref]`; new `pledge_ref`, `closed_at` | R6 |
| 4 | §4.4 CollateralCall | seeds `["call", agreement, external_ref]`; new `units_released`, `dispute_ref`, `disputed_at`, `closed_at`; derived Met rule | R5, R6, R8, R10 |
| 5 | §4.5 | new governance PDA `["collateral-caller"]`; ActionKind 15 `ResolveDispute` (record from F007) | R10 |
| 6 | §5.1 | F004 rows per instruction (see contracts), incl. `confirm_exposure`, `check_coverage` (anyone), `cancel_call`, `withdraw_dispute`, `resolve_dispute` (governance PDA only), `governance::resolve_call_dispute` (compliance), `compliance_hook::set_lien` | R7–R10 |
| 7 | §5.2 | new edge governance → `collateral::resolve_dispute`; `set_lien(LienChange)` callee checks | R2, R10 |
| 8 | §6 | step 5 lands in F004 without the bypass; the bypass is step 2 (F005) | R3 |
| 9 | §7 | new I15 (agreement total) and I16 (demanded part and open-call count) | R5, SC-002 |
| 10 | §8 | rows: collateral value on the agreement's total units (one rounding); return-call units rounded down; worked check restated at the smallest unit (1,020,408.163266) | R4 |
| 11 | §9 | events `CoverageChecked`, `CallCancelled`, `DisputeWithdrawn`, `DisputeResolved` added; `CallMet` replaced by `met_below_seq` on valuing events and `return_call_met` on `Released` | data model |
| 12 | §11 | F004 row as built (step 5 only; `init_registry` fills `collateral_program`) | R2, R3 |

`docs/design.md` §6.5 differs in ways the shared model now settles:
- per-call `units_required` recomputed on cure, against a demanded part with derived Met (R5);
- a two-step-only exposure, against the unilateral term (R7);
- a substitution with the taker's affirmation, against equal-or-larger by the giver (R9).

design.md is left as is, and the differences are recorded in research.

## Spec adjustments made during planning

Planning surfaced facts the spec has to state (workflow rule: fix the spec before code). The spec
was updated on this branch:

| Spec section | Change | Why |
|---|---|---|
| Terms, US3 scenario 1, Edge Cases (several calls, price rises), FR-015, FR-018 | Oldest-first meeting replaced by: the agreement tracks the demanded part of the shortfall, a new call covers only the rest, and all open delivery calls are Met together when a valuing step finds coverage ≥ 0. F005 limits enforcement by the call's own value | R5 |
| FR-015 (old), US4 scenario 7 | A return call is no longer cancelled when a delivery call opens | R8 |
| FR-028, US4 scenario 3 | Only releases made against the return call count towards it; the keeper's always do | R8 |
| FR-022, Key Entities | each pledge carries a non-empty giver reference, unique per agreement (its address) | R6 |
| Assumptions (Scale) | no limit on unresolved calls per agreement | R5 |

## Deployment and authorities

- **New program** `collateral`. Upgrade authority: the issuer's Squads vault with its time lock
  (as for the earlier programs). `compliance_hook` and `governance` are upgraded in place; `fund`
  and `dvp_settlement` don't change.
- **No migration**: no account changes size. Registries created before F004 keep an empty
  `collateral_program`, so liens are unavailable on those test funds (R16). The F004 devnet run
  creates a new fund.
- **New authorities**:
  - `collateral`'s `["hook-caller"]` signs only `set_lien`;
  - governance's `["collateral-caller"]` signs only `resolve_dispute`;
  - the permanent delegate still signs nothing (F005).

## Project Structure

### Documentation (this feature)

```text
specs/004-collateral-pledges/
├── plan.md              # this file
├── research.md          # Phase 0
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1: validation guide and FR → test map
├── contracts/           # Phase 1
│   ├── collateral.md        # all collateral instructions
│   ├── compliance_hook.md   # set_lien, hook step 5, init_registry
│   └── governance.md        # resolve_call_dispute
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks (not created here)
```

### Source Code (repository root)

```text
programs/
├── collateral/                       # new program
│   ├── Cargo.toml                    # + compliance_hook (cpi), mmf-common
│   └── src/
│       ├── lib.rs
│       ├── state.rs                  # PledgeAgreement, Pledge, CollateralCall, kinds and statuses
│       ├── errors.rs                 # CollateralError
│       ├── events.rs
│       ├── valuation.rs              # fresh, observe, derived call status, settle_status
│       ├── lien.rs                   # set_lien CPI signed by ["hook-caller"]
│       ├── call_account.rs           # create a call account by PDA only when needed (R7)
│       └── instructions/
│           ├── propose_agreement.rs
│           ├── sign_agreement.rs
│           ├── exposure.rs           # update_exposure, confirm_exposure
│           ├── issue_call.rs
│           ├── check_coverage.rs
│           ├── pledge.rs
│           ├── release.rs
│           ├── substitute.rs
│           ├── cancel_call.rs
│           └── dispute.rs            # dispute_call, withdraw_dispute, resolve_dispute
├── compliance_hook/src/
│   ├── hook/execute.rs               # + step 5
│   ├── instructions/set_lien.rs      # new
│   ├── instructions/init_registry.rs # fills collateral_program
│   └── errors.rs, events.rs          # appended
└── governance/src/
    ├── instructions/resolve_call_dispute.rs   # new
    └── Cargo.toml                    # + collateral (cpi)
crates/
├── mmf-common/src/
│   ├── ids.rs                        # + COLLATERAL_PROGRAM_ID
│   ├── seeds.rs                      # + AGREEMENT, PLEDGE, CALL, HOOK_CALLER, COLLATERAL_CALLER
│   ├── collateral.rs                 # new: value, units up/down, coverage, fresh, derived status
│   └── views.rs                      # FundStateView + nav_ts
└── mmf-sdk/src/
    ├── ix_collateral.rs              # new: builders for every collateral instruction
    ├── ix_hook.rs, ix_governance.rs  # + set_lien (negative tests), resolve_call_dispute
    └── pda.rs                        # + agreement, pledge, call, hook_caller, collateral_caller
tests/
├── src/
│   ├── collateral_model.rs           # independent reference model (R15)
│   ├── fixtures.rs                   # + collateral_ready_fund and variants
│   ├── events.rs                     # + collateral events, LienSet
│   └── rebuild.rs                    # + agreements, calls, locks (SC-009)
└── tests/
    ├── f004_agreement.rs, f004_exposure.rs, f004_calls.rs        # US1, US2
    ├── f004_pledge.rs, f004_lock.rs, f004_lien.rs                # US3, lock rules
    ├── f004_release.rs, f004_substitute.rs, f004_dispute.rs      # US4–US6
    ├── f004_status.rs, f004_interest.rs, f004_slow_signers.rs
    ├── collateral_prop.rs            # SC-002, SC-003, SC-005; I1, I2, I15, I16
    ├── hook_order.rs                 # + step 5 rows
    ├── invariants_prop.rs            # + I1, I2
    ├── layouts.rs                    # + three new account sizes
    └── scale.rs                      # + SC-007
scripts/devnet-e2e/                   # + --feature f004
scripts/check-error-coverage.sh       # + CollateralError
Anchor.toml                           # + collateral
```

**Structure Decision**: the same workspace as F001–F003, plus the `collateral` program planned in
shared-model §2. The valuation formulas live in `mmf-common`, so the program and the SDK share one
implementation; the test model keeps its own. No `services/` or `app/` until F008.

## Complexity Tracking

No constitution violations to justify. The two Constitution IV exceptions are named above, as the
principle requires.
