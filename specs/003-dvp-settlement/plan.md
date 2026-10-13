# Implementation Plan: Subscription, Redemption and Transfer Settlement

**Branch**: `003-dvp-settlement` | **Date**: 2026-10-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/003-dvp-settlement/spec.md`, the shared plan input
(`docs/speckit-inputs.md` §4.1), the F003 plan note (§4.2), and the merged F001 and F002 code and
data models ([F001](../001-fund-launch-onboarding/data-model.md),
[F002](../002-daily-price-accrual/data-model.md)).

## Summary

F003 adds the fifth program, `dvp_settlement`. It exchanges cash for fund units so that either both
legs move or neither does:
- **Agreeing a trade**: a party creates a `SettlementInstruction`, priced from the Fund's latest
  price record.
- **Funding**: each party funds its own leg into one per-fund escrow, at its own pace. Prepared
  steps stay valid for hours because nothing on-chain depends on a recent blockhash, and the
  instruction's address is known when it is prepared (R2, R13).
- **Settlement**: when both legs are in escrow, anyone (normally the keeper) triggers `settle`.
  It moves both legs in one instruction:
  - Subscription: `fund` mints the units into escrow, they are delivered to the investor, and the
    cash goes to the treasury.
  - Redemption: `fund` burns the units within the daily limit and the cash goes to the investor.
  - Secondary transfer: units and cash swap between the two holders.
- **Unwinding**: cancel, expire and compliance reject only change the instruction's state and
  earmark what must go back. A separate `return_leg` that anyone can trigger delivers each
  earmarked leg, so an unwind always completes even when a return can't be delivered yet (R3).

Key design choices (details in [research.md](research.md)):
- Instruction PDA from creator + external reference, not the sequence number (R2).
- One escrow per fund with per-instruction accounting; returns as a separate step (R3, R4).
- The transfer agent acts for the Fund; the issuer designates the treasury cash account (R6).
- `fund` checks the settlement account itself before minting or burning (R7).
- `accrue(post_balance)` covers the escrow only in F003 (R8).
- A lazy UTC-day rollover with a day-start snapshot enforces the redemption limit (R9).
- Compliance rejects through governance, like every other compliance action (R11).

## Technical Context

**Language/Version**: unchanged. Rust 1.90 (pinned) with Anchor 1.1.2. The user input names
TypeScript for the client and UI; that layer is F008, and F003 ships Rust SDK builders only.

**Primary Dependencies**:
- Unchanged: `anchor-spl` 1.1.2 (`token_interface` for the cash leg), Token-2022 v11, Squads v4
  for the issuer and compliance roles.
- New internal edges:
  - `dvp_settlement` → `fund` (`cpi`) for mint and burn;
  - `fund` → `compliance_hook` (`cpi`) for `accrue`;
  - `governance` → `dvp_settlement` (`cpi`) for reject.
- No off-chain service is built in F003 (axum, sqlx and Yellowstone are F008).

**Storage**: on-chain accounts only.
- New: `DvpConfig` (119 B, one per fund) and `SettlementInstruction` (333 B, one per instruction,
  never closed).
- Escrow: one PDA with two ATAs.
- Extended: `FundState` (one field from reserved bytes).
- No database.

**Testing**:
- LiteSVM 0.12.0 with the F001/F002 fixtures and a new `settlement_ready_fund` fixture.
- One rejection test per rule, asserting the exact error.
- `proptest` random settlement sequences against an independent reference model (R18).
- Durable-nonce tests with the blockhash check on (R13).
- Compute-unit equality for `settle` (SC-007), the error-coverage script (SC-006), and one devnet
  run.
- Map in [quickstart.md](quickstart.md).

**Target Platform**: Solana devnet, mainnet-compatible. Local tests on LiteSVM.

**Project Type**: Anchor workspace (programs/), Rust support crates (crates/), test crate (tests/),
devnet script (scripts/). `services/` and `app/` start in F008.

**Performance Goals** (R15):
- `settle` (subscription) ≤ 200 000 CU; funding ≤ 90 000 CU.
- Call depth ≤ 4 on every path.
- `settle` uses a fixed account set whatever the number of holders or open instructions
  (SC-007).
- The publish cost is untouched by F003.

**Constraints**:
- No backend key: settle, expire and return are permissionless and decide nothing.
- Durable-nonce-compatible steps.
- Checked integer math with the shared-model §8 rounding (subscription up, redemption down, limit
  exact).
- Fail closed: any failed leg aborts the step, and returns are retryable.

**Scale/Scope**: one fund, up to about 10 000 holders, up to about 1 000 open instructions.

## Constitution Check

*GATE: checked before Phase 0 research and again after Phase 1 design. Both pass.*

| Principle | How F003 complies | Pre | Post |
|---|---|---|---|
| I. Correctness over features | Investor `accrue` (and its `holder_count` branch) is deferred to F006, which can test it (R8). Payout of escrow interest is deferred (R16). Partial fills, netting and forward pricing are out of scope | ✅ | ✅ |
| II. Rules live on-chain | Eligibility, pricing, rounding, the daily limit, atomicity, expiry and escrow bounds are all in programs. Unit legs run the hook. Tests send every instruction and raw `mint_for_subscription` / `burn_for_redemption` directly | ✅ | ✅ |
| III. Named signer | issuer: `init_dvp`, `set_cash_treasury`. Parties: create, fund, cancel, withdraw. Compliance through governance `["dvp-caller"]`: reject. Escrow PDA: mint and burn CPIs. FundState PDA: `accrue`. `settle`, `expire` and `return_leg` are permissionless as §5.1 lists; their outcome depends only on state and time (FR-015). Shared-model §5.1 updated | ✅ | ✅ |
| IV. One instruction, one transition | Each step is one transition. **Named exception**: `settle` moves both legs (and, for subscriptions, mints) in one instruction, because atomicity is the point; splitting it brings back delivery risk. Returns are split out of cancel, expire and reject (R3). Accrual, day rollover and events are bookkeeping | ✅ | ✅ |
| V. On-chain source of truth | Price read from FundState (I12). Units held per instruction are derived from status and amounts, not stored as totals. `units_outstanding`, `redeemed_today` and `units_at_day_start` are running totals updated in the same instruction as mint and burn, tested by I4 and I14 | ✅ | ✅ |
| VI. Failing test first | [quickstart.md](quickstart.md) maps every FR and error to a rejection test. Tasks must put each test before its code | ✅ | ✅ |
| VII. Fail closed | Stale or missing price, ineligible party, pause, expiry, wrong accounts and overflow all abort. A settle with any failing leg moves nothing. Unwinds never depend on a return succeeding (R3) | ✅ | ✅ |
| VIII. No unpaid units, no silent powers | Only `mint_for_subscription` mints. It requires the escrow PDA, an Affirmed subscription whose cash is in escrow (I5), and pays that cash to the treasury in the same transaction. Reject is role-gated, reason-coded and recorded (`SettlementRejected`; ActionRecord in F007). It is protective (it returns legs), so it isn't time-locked | ✅ | ✅ |
| IX. Money math is exact | Cash amounts: subscription rounds up, redemption rounds down (existing §8 rows). Limit check is cross-multiplied (new §8 row). All checked `u128` | ✅ | ✅ |
| X. Stated assumptions | Spec assumptions: cash issuer trust, transfer agent behaviour, treasury control, price age, escrow interest, custodian behaviour. Quickstart lists known behaviours (CPI guard, missing cash ATA, first day, revoked seller) | ✅ | ✅ |
| XI. Features ship in order | F001 and F002 merged. Only F003 fields are filled. `collateral` (F004) isn't started; US2 scenario 3 is verified in F004 | ✅ | ✅ |
| XII. Earlier features stay green | All F001/F002 tests run. Changes to them: `seed_units` keeps I4; the Escrow `ClassNotAvailable` test is replaced; the layout test covers the new offset; the event decoder covers new events. All are listed in the PR | ✅ | ✅ |
| XIII. One shared model | All changes below are made in `docs/shared-model.md` on this branch, before code. No second account for an existing concept: the escrow uses the planned Escrow HolderState; `cash_treasury` stays on FundState | ✅ | ✅ |

No violations, so Complexity Tracking is empty.

## Shared-model changes

Made in `docs/shared-model.md` on this branch (Principle XIII):

| # | Section | Change | Why |
|---|---|---|---|
| 1 | §4.2 FundState | `units_at_day_start: u64` after `pending_override_nav`; `_reserved` 52 → 44; `day_start_ts` = 00:00 UTC of the redemption day | R9 |
| 2 | §4.3 DvpConfig | add `escrow_bump: u8`; `default_expiry_secs` defaults to 86 400 | R4, R20 |
| 3 | §4.3 Escrow | unit escrow and cash escrow are the escrow authority's ATAs; the escrow HolderState is registered and approved by compliance after `init_dvp` | R4 |
| 4 | §4.3 SettlementInstruction | seeds `["settlement", fund_state, created_by, external_ref]`; new fields `fund_state`, `created_at`, `closed_at`, `cancel_requests`, `units_to_return`, `cash_to_return`; never closed | R2, R3, R17 |
| 5 | §4.5 | new governance PDA `["dvp-caller"]`; RejectSettlement reasons = the F001 list | R11 |
| 6 | §5.1 | rows: `set_cash_treasury` (issuer), `init_dvp` (issuer), `withdraw_cancel` (parties), `return_leg` (anyone), `reject` (governance `["dvp-caller"]`), `governance::reject_settlement` (compliance), `mint_for_subscription` / `burn_for_redemption` (dvp escrow PDA), `accrue` (FundState PDA); `affirm_and_fund` parties per side (transfer agent for the Fund side) | R6, R11 |
| 7 | §5.2 | new edge governance → `dvp_settlement::reject`; `fund → accrue(post_balance)` signature; mint/burn callee checks name the settlement-account checks | R7, R8, R11 |
| 8 | §7 | I4/I5/I6 wording sharpened; new I13 (escrow ≥ Σ held) and I14 (daily limit) | SC-003, SC-005 |
| 9 | §8 | row: redemption limit check, exact, cross-multiplied | R9 |
| 10 | §9 | new events `DvpInitialized`, `CancelRequested`, `CancelRequestWithdrawn`, `LegReturned`, `CashTreasurySet`, `SubscriptionMinted`, `RedemptionBurned`; `Accrued` payload | data model |
| 11 | §11 | F003 row: as built (`accrue` escrow-only; investor accrue and `holder_count` in F006) | R8 |

`docs/design.md` §6.3 resets the redemption counter in `publish_daily`, and its §6.4 uses
per-instruction vaults. The shared model takes precedence (UTC-day rollover; one escrow, as §10
already records). design.md is left as is, and the differences are recorded in research R4 and
R9.

## Spec adjustments made during planning

Planning surfaced facts the spec has to state (workflow rule: fix the spec before code). None of
them changes a clarify decision:

| Spec section | Change | Why |
|---|---|---|
| FR-026, Edge Cases (refund not deliverable) | cancel, expire and reject earmark every funded leg; each return is its own step that anyone can trigger, so an unwind always completes | R3 |
| FR-030, SC-003 | the escrow holds at least what open instructions hold; amounts sent outside an instruction are never released by an instruction | R4 |
| FR-004, Edge Cases | an instruction is identified by its creator and external reference, which must be unique for that creator and not empty | R2 |
| FR-012 | the funding party (other than the Fund side) must be eligible at funding | R12 |
| Assumptions | cash is paid to each party's standard (associated) account for the cash asset; a fee-bearing or hooked cash token isn't supported | R14 |

## Deployment and authorities

- **New program** `dvp_settlement`. Upgrade authority: the issuer's Squads vault with its time
  lock (as for the F001 programs). `fund`, `compliance_hook` and `governance` are upgraded in
  place.
- **No migration**: FundState keeps its size and offsets. Existing devnet funds have supply 0, so
  `units_outstanding = 0` already satisfies I4 (R19).
- **New authorities**:
  - the escrow PDA signs only movements out of escrow and the two `fund` CPIs;
  - governance's `["dvp-caller"]` signs only `reject`;
  - the FundState PDA newly signs `accrue`.

## Project Structure

### Documentation (this feature)

```text
specs/003-dvp-settlement/
├── plan.md              # this file
├── research.md          # Phase 0
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1: validation guide and FR → test map
├── contracts/           # Phase 1
│   ├── dvp_settlement.md    # all settlement instructions
│   ├── fund.md              # set_cash_treasury, mint_for_subscription, burn_for_redemption
│   ├── compliance_hook.md   # Escrow class enabled; accrue
│   └── governance.md        # reject_settlement
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks (not created here)
```

### Source Code (repository root)

```text
programs/
├── dvp_settlement/                   # new program
│   ├── Cargo.toml                    # + fund (cpi), mmf-common, anchor-spl
│   └── src/
│       ├── lib.rs
│       ├── state.rs                  # DvpConfig, SettlementInstruction, Kind, Status, Side
│       ├── errors.rs                 # DvpError
│       ├── events.rs
│       ├── escrow.rs                 # held amounts, earmarking, signed transfers out of escrow
│       └── instructions/
│           ├── init_dvp.rs
│           ├── create_instruction.rs
│           ├── affirm_and_fund.rs
│           ├── settle.rs
│           ├── cancel.rs             # cancel, withdraw_cancel
│           ├── expire.rs
│           ├── reject.rs
│           └── return_leg.rs
├── fund/src/
│   ├── state.rs                      # + units_at_day_start
│   ├── errors.rs, events.rs          # appended
│   └── instructions/
│       ├── set_cash_treasury.rs      # new
│       └── mint_burn.rs              # new: mint_for_subscription, burn_for_redemption, day rollover
├── compliance_hook/src/
│   ├── instructions/register_holder.rs, approve_fund_account.rs   # Escrow class
│   └── instructions/accrue.rs        # new
└── governance/src/
    ├── instructions/reject_settlement.rs   # new
    └── Cargo.toml                    # + dvp_settlement (cpi)
crates/
├── mmf-common/src/
│   ├── ids.rs                        # + DVP_SETTLEMENT_PROGRAM_ID
│   ├── seeds.rs                      # + DVP, ESCROW, SETTLEMENT, DVP_CALLER
│   ├── settlement.rs                 # new: subscription_cash, redemption_cash, redemption_ok, eligible, price_fresh
│   └── views.rs                      # + HolderStateView, SettlementView (read by fund)
└── mmf-sdk/src/
    ├── ix_dvp.rs                     # new: builders for every dvp instruction
    ├── ix_fund.rs, ix_governance.rs  # + set_cash_treasury, reject_settlement
    ├── pda.rs                        # + dvp_config, escrow, settlement, dvp_caller
    └── prepare.rs                    # new: durable-nonce message builder (R13)
tests/
├── src/
│   ├── dvp_model.rs                  # independent settlement reference model (R18)
│   ├── fixtures.rs                   # + settlement_ready_fund, cash helpers
│   ├── units.rs                      # seed_units keeps units_outstanding (I4)
│   └── rebuild.rs                    # + settlement events (SC-008)
└── tests/
    ├── f003_setup.rs, f003_create.rs, f003_fund.rs, f003_settle.rs
    ├── f003_subscribe.rs, f003_redeem.rs, f003_secondary.rs        # US1–US3
    ├── f003_cancel_expire_reject.rs, f003_reject.rs, f003_returns.rs   # US4
    ├── f003_slow_signers.rs          # US5
    ├── f003_mint_burn_guard.rs, f003_escrow.rs, f003_accrual.rs
    ├── dvp_prop.rs                   # SC-001, SC-002, SC-003, SC-005; I4, I5, I6, I13, I14
    ├── layouts.rs                    # + FundState offset, DvpConfig and SettlementInstruction sizes
    └── scale.rs                      # + SC-007
scripts/devnet-e2e/                   # + --feature f003
```

**Structure Decision**: the same workspace as F001/F002, plus one program, `dvp_settlement`,
planned in shared-model §2. The settlement formulas live in `mmf-common` so the programs and the
SDK share one implementation; the test model stays separate. No `services/` or `app/` until F008.

## Complexity Tracking

No constitution violations to justify. The Constitution IV exception for `settle` is named above,
as the principle requires.
