# Research: Daily Price and Interest Accrual (F002)

Phase 0 of [plan.md](plan.md). F002 adds no new dependencies. The toolchain, crate versions, test
harness and event conventions from F001 still apply (F001 research R1, R7, R8). Each item below
settles one design question the spec leaves to planning.

## R1. Index arithmetic and exactness

- **Decision**: use the shared-model encoding without change. The price is `nav: u64` with 8
  decimals. The rate is `daily_rate_e12`, a fraction of a unit per unit per day × 10^12. The
  accrual index is `u128` at scale 10^18. A publish adds `Δ = daily_rate_e12 × 10^6 × days_covered`.
  A holder settles with `accrued_e18 += pre_balance × (accrual_index − index_at_last_touch)`, in
  base units × 10^18. All operations are checked `u128` multiplications and additions, and none
  of them divides.
- **Rationale**: the exact interest for one day is `balance × rate_e12 / 10^12` base units. Times
  10^18 that is `balance × rate_e12 × 10^6`, an integer. Accrual therefore loses nothing at any
  step (FR-014, shared-model §8: "exact"), and SC-001 can demand **equality**, not "within one
  unit". Rounding happens only at payout (F006), as `floor(accrued_e18 / 10^18)`, and the
  remainder stays with the same holder.
- **Range**: overflow needs `pre_balance × Δindex > 3.4 × 10^38`. At 10^9 whole units (10^15 base
  units), Δindex would have to exceed 3.4 × 10^23, which is cumulative interest of 340 000 units
  per unit. Real rates never reach that. If an absurd rate ever did, the checked math aborts the
  transfer instead of wrapping (Constitution VII). The resulting risk is listed under R6.
- **Alternatives**: accrue in units with per-day rounding was rejected: it loses up to one base
  unit per holder per day and breaks SC-001. Storing the rate in bps was rejected by the shared
  model (§10): it is too coarse.

## R2. What a "day" is

- **Decision**: a day is a UTC calendar day: `day = unix_timestamp.div_euclid(86_400)`, stored as
  `u32` (days since 1970-01-01). The clock is the `Clock` sysvar.
- **Rationale**: it is deterministic, needs no per-fund setting (FundTerms are fixed at launch,
  F001 FR-009), and fits the shared-model time convention. A US fund that strikes its price at
  around 17:00 New York time publishes at 21:00–22:00 UTC, which is still the same UTC date.
- **Consequence**: a fund that publishes after 00:00 UTC for the previous local business day
  would be one day off. That is an operational rule for the fund admin and is recorded in the
  spec's Assumptions.
- **Alternatives**: a per-fund UTC offset would need a new FundTerm (F001 terms are final); an
  admin-supplied "as-of date" would add a field that is itself a typo risk, with no gain over the
  clock.

## R3. Days covered and the coverage rule (Clarification Q1)

- **Decision**: `publish_daily` takes `days_covered: u16`. The fund keeps the last covered day,
  `nav_period_end_day`. The new period's `start` is today for the first publish and
  `nav_period_end_day + 1` for every later one. Checks: `days_covered ≥ 1` (`ZeroDaysCovered`)
  and `start ≤ today` (`PeriodNotStarted`). On success the period end becomes
  `start + days_covered − 1`.
- **Rationale**: coverage is contiguous by construction, so no day is skipped or covered twice
  (FR-009). Each case works out as follows:
  - **Friday**: start = Friday, days = 3.
  - **Holiday**: covered in advance by the publish before it.
  - **Outage**: the late publish starts on the first uncovered day. It can cover through today,
    or the admin can publish several records in a row to catch up.
  - **Second publish on a day already covered**: its start lies in the future, so it gets
    `PeriodNotStarted`.
  - **Staleness guard**: figures can only be published for a period that has begun.
- **No cap** on `days_covered` beyond its type (Clarification Q2, FR-009b). An oversized period
  blocks the next publish until it has passed (spec Assumptions).
- **Alternatives**: the alternatives were rejected in clarify: one publish per calendar day, or
  the system counting days since the last publish.

## R4. Price-move check (FR-007)

- **Decision**: `prev = fund.nav` if `nav_seq > 0`, else `terms.target_nav`. Accept if
  `|nav − prev| × 10_000 ≤ prev × max_nav_move_bps`, computed in `u128` by cross-multiplication. A
  move exactly at the limit passes. Otherwise the publish is accepted only if
  `pending_override_nav == nav`; if not, it fails with `PriceMoveTooLarge`.
- **Rationale**: exact integer comparison involves no division and so no rounding direction to
  choose. A new row is added to shared-model §8 as "exact".
- **Alternatives**: a percentage computed with division was rejected because it would need a
  rounding rule at the boundary.

## R5. Price override: where it lives and who sets it (Clarification Q3)

- **Decision**:
  - Compliance calls `governance::approve_nav_override(nav, reason, case_ref_hash)` or
    `withdraw_nav_override(reason, case_ref_hash)`.
  - Governance checks the role, that the fund is Open, that the role registry belongs to this
    fund, and the reason. It then CPIs into `fund::set_nav_override` or
    `fund::clear_nav_override`, signed by a new governance PDA `["fund-caller"]`.
  - The pending override is stored in `FundState.pending_override_nav` (0 = none).
  - The **next publish always clears it**, whether or not it used it. The `NavPublished` event
    says whether it was used and which value was cleared.
  - Only one override can be pending at a time; approving a second gets `OverrideAlreadyPending`.
  - Allowed reasons are `OperationalError` (3) and `Other` (6). `case_ref_hash` names the
    valuation decision.
- **Rationale**:
  - This mirrors F001's suspend and pause path: compliance → governance → target program, signed
    by a governance PDA. In F007, governance gains `record_action(NavOverride)` in the same place
    (ActionRecord kind 14 already exists).
  - `fund` must read and clear the override inside `publish_daily`, so the override has to live
    in a fund-owned account. FundState's reserved bytes hold it, so no new account is needed.
  - Clearing on the next publish means an old override can never be used days later
    (spec: "only for the next publish").
  - The fund admin has no path to set it.
- **Not time-locked**: a price override is a discretionary power (Constitution VIII: role-gated,
  reason-coded, recorded), but not a destructive one. It moves no units and changes no rule.
- **Call depth**: Squads → governance → fund → `emit_cpi` is 4 levels, the same depth as F001's
  `set_status` (F001 R7).
- **Alternatives**:
  - An override account owned by governance was rejected: `fund` cannot clear another program's
    account without a second CPI edge.
  - `fund::approve_nav_override` signed by compliance directly was rejected because it
    contradicts Clarification Q3 ("through governance").
  - Replacing a pending override was rejected because every change should be an explicit,
    recorded withdrawal.
  - A new reason code `PriceEvent` was rejected as an unnecessary shared-model change; `Other`
    with a case reference covers it.

## R6. Rate input and the "negative rate" rule (FR-006, FR-009b)

- **Decision**: `publish_daily` takes `daily_rate_e12: i64` and rejects `< 0` (`NegativeRate`).
  It stores the value as `u64`. There is no upper bound (Clarification Q2).
- **Rationale**: with a `u64` parameter a negative rate is impossible to express, but a client
  that encodes −1 as `i64` would send `u64::MAX`. Without a rate cap that would be accepted as a
  huge rate. A signed parameter turns that mistake into a rejection, which gives FR-006 a real
  rejection test. `i64::MAX` × 10^6 × 65 535 is about 6 × 10^29, which fits `u128` for any single
  publish.
- **Accepted risk (spec Assumptions)**: the rate is trusted. A wildly wrong rate overpays every
  holder, and in the extreme (R1) could make a large holder's next transfer overflow and fail
  closed. Recovery is operational.

## R7. Hook step 8: settling on every transfer

- **Decision**: step 8 settles in this order:
  1. Load `FundState` through `FundStateView`, which now also decodes `accrual_index`. The view
     checks the owner program, the address `registry.fund_state` and the discriminator; any
     failure returns `WrongMint`.
  2. Settle the source on `pre_src = post_src + amount`, then the destination on
     `pre_dst = post_dst − amount`, both with checked math (`Overflow`).
  3. When the source and destination owner are the same, settle once.
  4. `index_at_last_touch = accrual_index` is set even when the pre-balance is 0, which is
     FR-016: no interest for publishes before the holder held units.

  Step 8 runs after every check (steps 3–7), so a rejected transfer settles nothing.
- **Rationale**: this is shared-model §6 as written. The pre-balance comes from the token
  account, which is the truth; invariant I10 already proves it equals `last_balance`. Constant
  work: two checked multiplications and one account decode, with no loop (FR-018, SC-004).
- **Records**: `TransferValidated` (logged with `emit!` to save a call level, F001 R7) gains
  `accrual_index`, `source_accrued_e18` and `destination_accrued_e18`. An auditor can rebuild
  every settlement from the NavRecords plus the transfer history (FR-021). The figures can also
  be recomputed from on-chain data, so a truncated log loses nothing that can't be rebuilt.
- **Alternatives**: a separate `Accrued` event per holder was rejected because it doubles the log
  cost on every transfer. The `Accrued` name stays reserved for `compliance_hook::accrue`
  (see R8).

## R8. Accrual before unit creation and destruction (`compliance_hook::accrue`)

- **Decision**: build `compliance_hook::accrue` and the `fund → accrue` CPI in **F003**, not F002.
  shared-model §11 moves that item from the F002 row to the F003 row.
- **Rationale**: the only caller is `fund` during mint and burn, and F002 has neither. A test of
  the success path needs the FundState PDA to sign, and that is only possible through a fund
  instruction that doesn't exist yet. Constitution VI requires a failing test before the code,
  and F001 deferred hook steps 2 and 5 for the same reason. FR-013 already scopes this to "every
  other way a balance can change that a later feature adds".
- **Alternatives**: building the instruction with only a rejection test was rejected because it
  would ship an unexercised success path.

## R9. NavRecord layout and cost

- **Decision**: NavRecord is PDA `["nav", fund_state, seq_le]` with these fields: `version`,
  `seq`, `nav`, `daily_rate_e12`, `days_covered: u16`, `period_start_day: u32`, `index_after`,
  `as_of_ts`, `published_by`, `override_used: bool` and `bump`. It has no `_reserved`, because it
  is immutable once written; `DistributionReceipt` follows the same precedent. A separate `payer`
  signer funds it, since the fund admin may be a Squads vault (F001: payers are separate from
  authorities). About 100 bytes, roughly 0.0016 SOL of rent per day.
- **Rationale**: `days_covered` and `period_start_day` let an auditor see which calendar days each
  record paid for (FR-002), and `override_used` marks records that passed only by override.
  Records are never closed, so the history is permanent (FR-003, SC-005).

## R10. FundState additions without resizing

- **Decision**: take 12 of FundState's 64 reserved bytes and place the new fields **after**
  `bump`: `nav_period_end_day: u32` and `pending_override_nav: u64`, leaving `_reserved: [u8; 52]`.
- **Rationale**: this follows the shared-model convention ("small additions use reserved bytes").
  Putting the fields after `bump` keeps every existing offset, so funds created by F001 decode
  unchanged, and zero means "none yet" for both fields. `nav_period_end_day` is a checkpoint
  updated in the same instruction as the NavRecord it summarizes. Constitution V allows that only
  with an invariant test: it must equal the latest record's `period_start_day + days_covered − 1`
  (I12).

## R11. One implementation of the math, one independent reference

- **Decision**: put the formulas in `mmf-common::accrual`: `utc_day`, `index_increment`,
  `settle` and `price_move_ok`. Both programs and the SDK's read helper `earned_e18` (FR-017) use
  them. The test reference model in `tests/src/model.rs` does **not** reuse them. It tracks each
  holder's balance at each publish and adds `balance × rate × 10^6` once per covered day.
- **Rationale**: a shared helper keeps the hook, `fund` and the client in agreement. An
  independent model is what makes SC-001 a real check rather than code compared with itself.

## R12. Test placement of units under accrual

- **Decision**: the F001 helper `seed_units` writes balances directly (F001 R8). It now also does
  the bookkeeping a hook-checked transfer would do first: it settles the holder at the current
  index (`accrued_e18`, `index_at_last_touch`) before raising `last_balance`. The reference model
  treats seeding as a transfer from outside the register.
- **Rationale**: without this step, units seeded after a publish would carry an old
  `index_at_last_touch` and earn interest for days they weren't held. That would be a test
  artefact that SC-001 must not hide.

## R13. Devnet run with real days

- **Decision**: the devnet script publishes for real UTC days, so one run can create only one
  record per day (R3). The run covers:
  - an over-limit first price rejected (`PriceMoveTooLarge`);
  - a compliance override approved through Squads;
  - a publish at that price accepted with `override_used`;
  - a second publish the same day rejected (`PeriodNotStarted`);
  - a publish by a non-admin rejected;
  - record 1 read back.

  A `--continue` flag publishes the next day on the same fund, so running it on three days gives
  the three-record history. Multi-day accrual with units is proved in LiteSVM, because devnet
  still can't create units before F003.

## Notes on documents

- `docs/design.md` §12 ("Runtime constraints and assumptions") mentions "intraday republish allowed". The spec (Assumptions) and this plan
  allow no republish, and the shared model, which takes precedence, never mentions one. The plan
  records the difference rather than editing design.md.
