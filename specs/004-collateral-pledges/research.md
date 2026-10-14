# Research: Collateral Agreements, Margin Calls and Pledges (F004)

Each entry gives the decision, why, and what else was considered. **[SM change]** marks a change to
`docs/shared-model.md`, listed in [plan.md](plan.md#shared-model-changes). The Technical Context had
no open unknowns: the stack is unchanged from F003. These entries settle the design questions the
spec raised.

## R1. A new `collateral` program that calls `compliance_hook::set_lien`

- **Decision**: F004 adds the program planned in shared-model §2, `collateral`. It owns
  `PledgeAgreement`, `Pledge` and `CollateralCall` (`EnforcementHold` waits for F005). It reaches
  the hook with an Anchor CPI to one new instruction, `compliance_hook::set_lien`, signed by a
  `collateral` PDA `["hook-caller"]`. That is the only new edge `collateral → compliance_hook`
  (already in §5.2). `collateral` reads FundState, RoleRegistry and HolderState through
  `mmf_common::views`, as `dvp_settlement` does. It has no per-fund config account: agreements name
  their FundState and everything else hangs off them.
- **Rationale**: the user input fixes it ("Lien = locked amount in the holder record, set only by
  CPI from collateral"). One owner per account type (shared-model §1): the lock is a HolderState
  field, so only `compliance_hook` writes it, and `collateral` asks for each change. A config
  account would hold nothing that FundState and the RoleRegistry don't already hold.
- **Alternatives**: writing `pledged_amount` from `collateral` directly is impossible (the program
  doesn't own the account). Storing the lock in a `collateral` account read by the hook would add a
  sixth extra account to every transfer, which shared-model §4.1 freezes at five.

## R2. The lock: `HolderState.pledged_amount`, changed only by `set_lien`

- **Decision**: `set_lien(change: LienChange { lock: u64, unlock: u64, opened: u16, closed: u16 })`
  (contract in [compliance_hook.md](contracts/compliance_hook.md)). It checks that:
  - the signer is `["hook-caller"]` of `registry.collateral_program`, and that field isn't empty;
  - the holder record and holding account belong to the same owner and mint (as `accrue` does).
  The new lock is `pledged_amount + lock − unlock`. When the lock grows (`lock > unlock`), it
  requires `holding_account.amount ≥ new pledged_amount` (invariant I2), otherwise
  `InsufficientUnlockedUnits`. It updates `active_liens` by `opened − closed` and emits `LienSet`.
  One instruction serves pledge (lock N, opened 1), release (unlock R, closed 1 when the pledge
  empties) and substitution (lock N, unlock R, opened 1, closed 1), so a substitution changes the
  lock in one CPI.
- **`Registry.collateral_program`** is filled by `init_registry` with
  `mmf_common::ids::COLLATERAL_PROGRAM_ID`, as `governance` already is. There is no setter.
  Registries created before F004 keep it empty, so `set_lien` fails closed for them
  (`NotCollateralCaller`). Devnet funds from F001 to F003 are test funds; the F004 devnet run
  creates a new one (R16).
- **Rationale**: `compliance_hook` owns both the lock and the transfer check, so it is the right
  place to guarantee I2 against the real token balance. Then `collateral` can never lock units the
  holder doesn't have, even with a bug. A setter for `collateral_program` would be a rule change,
  which Constitution VIII time-locks (F007). Filling it at creation avoids adding that power.
- **Alternatives**:
  - `collateral` checks the balance and the hook trusts it: two places for one rule.
  - Separate `lock` / `unlock` instructions: substitution would need two CPIs, with an
    intermediate lock nobody reviews.
  - Verifying the signer against the compiled id rather than the Registry field: works, but
    leaves the shared-model field meaningless.

## R3. Hook step 5, and why the enforcement exception waits for F005

- **Decision**: `execute` gains step 5 between steps 4 and 6:
  `post_src_balance ≥ src.pledged_amount`, otherwise `PledgedUnitsLocked`. It applies to every
  transfer, including units funded into F003's escrow, so the F003 redemption and secondary-sale
  funding paths are covered with no change to `dvp_settlement`. When source and destination
  belong to the same owner, it checks the source account's post-balance (fail closed). This case
  can't arise in practice, because an owner has only one usable account (shared-model §1). The F004
  plan note says the check is skipped "unless the authority is the governance PDA". That bypass is
  step 2, which shared-model §6 schedules for F005: nothing in F004 can sign as the delegate, so a
  bypass test can't be written yet (Constitution VI).
- **Rationale**: the check sits where the shared model fixes it. It comes after the sender check,
  so a suspended sender still reports `SenderNotApproved` (FR-026 of F001: the first failed check
  in a fixed order). It costs one comparison.
- **Alternatives**: checking the lock in `collateral` or in the client. Constitution II forbids it:
  the transfer itself must refuse.

## R4. Valuation: total pledged units at FundState's price, rounded once

- **Decision**: a new module `mmf_common::collateral`, shared by the program and the SDK; the test
  model keeps its own copy (R15).
  - `collateral_value(units, nav, haircut_bps) = floor(units × nav × (10_000 − h) / (10^8 × 10_000))`
  - `units_for_value_up(v, nav, h) = ceil(v × 10^8 × 10_000 / (nav × (10_000 − h)))` (delivery
    calls)
  - `units_for_value_down(v, nav, h) = floor(…)` (return calls)
  - `coverage = collateral_value(agreement.pledged_units) − required_value` as `i128`
  The price is `FundState.nav` and the record is `FundState.nav_seq` (= `NavRecord(nav_seq)` by
  I12), as F003 R10 does. It is fresh when `nav_seq > 0 ∧ now − nav_ts ≤ max_nav_age_secs`,
  otherwise `NoPrice` or `StalePrice`. `FundStateView` gains `nav_ts`. All arithmetic is checked
  `u128`. **[SM change: §8 rows for return units and single rounding; §8 worked check restated at
  the smallest unit]**
- **Rationale**: valuing the total once is never below the sum of per-pledge values, and it needs
  no pledge accounts (SC-007). The agreement's `pledged_units` is the stored total that I15 proves.
  The age test uses the publish time. The agreement's maximum price age is in seconds, so it is
  measured from when the price arrived. F003's covered-period test answers a different question
  (is this the price for today).
- **Worked figures** (spec, tests): $1,000,000 at $1.00 and 2 % needs `1_020_408_163_266` base
  units, i.e. 1,020,408.163266 units (§8's 1,020,409 is the same rule applied to whole units). That
  pledge is worth exactly $1,000,000.000000. An excess of $600,000 returns 612,244.897959 units.
  At $0.985 the original pledge is worth $985,000, a $15,000 shortfall, which needs 15,539.210609
  more units.
- **Alternatives**:
  - Summing per-pledge rounded values: needs every pledge account in every step.
  - Reading the `NavRecord` account: one more account for a value FundState already holds.
  - Counting interest accrued since the publish: design.md §6.5.3 excludes it; spec Assumptions
    keep that.

## R5. Calls: a stored demanded part, and all open calls met together

- **Decision**: the agreement stores:
  - `called_outstanding` (D): the demanded part of the shortfall;
  - `open_delivery_calls`: the count of unresolved delivery calls;
  - `met_below_seq`.
  Every step that values the agreement at a fresh price (exposure, calls, pledge, release,
  substitution, `check_coverage`) first **observes** coverage:
  - `S = max(0, −coverage)`, then `D = min(D, S)`;
  - if `S == 0`: `met_below_seq = call_count`, `D = 0`, `open_delivery_calls = 0`.

  A delivery call opens only for the uncalled shortfall `S − D` (more than the MTA), and sets
  `D = S`. Cancelling an unresolved delivery call subtracts its value from D, but not below zero.
  A delivery call's status is **derived**: a stored Open or Disputed status with
  `seq < met_below_seq` means Met. Any instruction that touches a call first applies this,
  storing Met if so; nothing else needs to. Each step's event carries `met_below_seq`, so the
  history shows exactly when each call was met (SC-009). **[SM change: new agreement fields; I16]**
- **Rationale**:
  - **Slow signers**: a pledge, an exposure update and a call each need only the agreement, never
    the list of open calls. A pledge prepared hours earlier stays valid however many calls open or
    close meanwhile (SC-008).
  - **Determinism**: the coverage test happens inside the step that changes coverage, so whether a
    call is met never depends on when the keeper runs.
  - **Fairness to the giver**: D shrinks when a pledge, a lower exposure or a higher price reduces
    the shortfall. So a later rise opens a new call with its own deadline instead of being folded
    into an older call.
  - **Constitution V**: the derived status is computed on read, and D is the one stored running
    total (it depends on history, so it can't be recomputed), proved by I16.
  - **One rule for "met"**: the user input says "the call is Met if coverage is now zero or more",
    which is all-together.
- **For F005**: `mark_defaulted` must use the derived status, and enforcement on a call must be
  limited to both the shortfall and that call's own value. Then an earlier call can't be used to
  enforce a later call's amount before the later call's own enforce-after time.
- **Alternatives**:
  - Oldest-first meeting (the draft spec): each step must load every open call, so a prepared
    pledge fails whenever a call opens or closes before it lands. Fairness is better kept in
    F005's enforcement cap.
  - Writing Met on each call inside the step: same problem.
  - One open call at a time: a rise while it is open would either wait or be folded into its
    deadline.

## R6. Addresses known at preparation: agreement nonce, pledge reference, call reference

- **Decision**:
  - Agreement: `["agreement", fund_state, giver, taker, nonce]` (unchanged), with the nonce
    chosen by the proposer.
  - Pledge: `["pledge", agreement, pledge_ref]` **[SM change]**, with a non-zero reference
    chosen by the giver.
  - Call: `["call", agreement, external_ref]` **[SM change]**, with a non-zero reference chosen
    by the party opening it.
  `seq` is still assigned from `pledge_count` / `call_count`, stored and carried in events.
- **Rationale**: the F003 R2 pattern. With `seq` seeds, a taker's call and a giver's return call
  race for the same next number, and two pledges in custodian approval at once collide. With
  reference seeds, every prepared step names a fixed address, and a resubmitted step can't create
  a second account (FR-017, FR-036).
- **Alternatives**: `seq` seeds (shared model as written) fail under concurrent preparation.
  Random nonces would work, but the spec already asks for external references.

## R7. Exposure: lower at once, raise alone or with the giver's confirmation

- **Decision**: `update_exposure(required_value, external_ref)`, signed by the taker:
  - **Lower or equal value**: applies at once at any price age and clears any pending raise. If
    the price is fresh, or the new value is zero, it observes coverage (R5).
  - **Higher value, `taker_may_call_unilaterally`**: needs a fresh price and an eligible taker.
    It observes coverage first, then applies the raise. If `S − D > MTA`, it creates the call
    account at `["call", agreement, external_ref]` in the same instruction, by a system-program
    CPI signed with the PDA seeds, only when the call is needed.
  - **Higher value, otherwise**: stores `pending_raise_{value, ref, ts}`, with no price needed.
    `confirm_exposure(required_value, external_ref)`, signed by an eligible giver, must match
    both, needs a fresh price, and then does what the unilateral raise does. A newer raise or any
    lower value replaces the pending one (`PendingRaiseMismatch` for a stale confirmation).
- **Constitution IV**: applying a raise and opening the call it creates is a **named exception**.
  The call's value and deadline must come from the same valuation and moment as the raise.
  Otherwise a publish or pledge landing between two instructions would make the call disagree
  with the exposure that caused it. And the confirming giver can't open delivery calls itself.
- **Alternatives**:
  - Two instructions in one transaction: the giver's confirmation couldn't include the taker's
    call.
  - Always requiring the giver's confirmation (design.md "two-step"): the shared model already
    makes it a term (`taker_may_call_unilaterally`), and lowering never needs it.

## R8. Return calls and releases

- **Decision**:
  - **Opening**: `issue_call(kind = Return)` (giver, eligible) needs a fresh price, an excess
    above the MTA and no open return call (`agreement.open_return_call == default`). It records
    `units_required = units_for_value_down(excess)` (`ReturnTooSmall` if that is 0).
  - **`release(units)`** on a pledge, signed by the taker or the keeper (RoleRegistry):
    - Optionally names the open return call; the keeper must name it.
    - The keeper may release at most `units_required − units_released` of that call.
    - Every release needs coverage after it ≥ 0 at a fresh price. With `required_value == 0`, no
      price is needed.
    - Calls `set_lien(unlock)` and observes coverage, which is ≥ 0 by construction.
    - A named return call accumulates `units_released` and is Met when it reaches
      `units_required`. Its key is then cleared from the agreement.
  - **Giver's cancel**: `cancel_call` lets the giver cancel its Open return call.
  - Opening a delivery call leaves an open return call alone.
- **Rationale**:
  - Releases that don't name the return call don't depend on it, so a taker's release prepared
    before the giver opened one still lands.
  - The coverage check bounds every release, so a stale return call can never under-cover the
    taker.
  - The keeper adds no discretion beyond timing: the amount comes from the call and the cap from
    coverage.
- **Alternatives**: cancelling an open return call when a delivery call opens (the draft spec): the
  delivery call's step would need the return call account, the same staleness problem as R5, for
  no safety gain. Counting every release towards the return call: same problem.

## R9. Substitution: equal or larger, one lien change

- **Decision**: `substitute(new_units, new_pledge_ref)`, signed by an eligible giver on an Active
  pledge. It requires `new_units ≥ remaining(old)`. It creates the new pledge, marks the old one
  Substituted with its remaining units released, and makes one CPI:
  `set_lien { lock: new, unlock: remaining, opened: 1, closed: 1 }`. The hook checks the balance
  against the net increase. The pledge limit applies to `pledged_units + new − remaining`.
- **Rationale**: spec FR-030. The units are interchangeable (one fund, lien only), so coverage can
  only stay or grow. One instruction makes "no dip" structural (Constitution IV exception, named in
  the spec).
- **Alternatives**: a substitution to fewer units with the taker's affirmation (design.md): that is
  a release, which already has its own rules.

## R10. Disputes, and compliance resolution through governance

- **Decision**:
  - **Giver**: `dispute_call(dispute_ref)` (Open delivery call, not derived Met) → Disputed,
    recording `dispute_ref` and `disputed_at`. `withdraw_dispute` → Open.
  - **Taker**: `cancel_call` cancels Open or Disputed delivery calls.
  - **Compliance**: `governance::resolve_call_dispute(outcome, reason, case_ref_hash)`. It checks
    the role registry is bound to the fund and the reason is on the F001 list, then CPIs
    `collateral::resolve_dispute(outcome)` signed by a new governance PDA `["collateral-caller"]`.
    Outcome is Open or Cancelled. **[SM change: PDA, §5.1 rows, §5.2 edge, ActionKind 15
    ResolveDispute reserved for F007]**
- **Rationale**: F003 R11. Every compliance action enters through governance, so F007 adds the
  ActionRecord in one place. Call depth: Squads → governance → collateral → `emit_cpi` = 4, as in
  F003.
- **Alternatives**: `collateral` checking the compliance key itself: one fewer edge, but a second
  pattern for compliance actions. F005's holds can decide the same way when F005 is planned.

## R11. Who must be eligible, and when the Fund's status is checked

- **Decision**:
  - `eligible(h) = class == Investor ∧ status == Approved ∧ kyc_expiry_ts > now` (as in F003).
  - **Both parties** at `propose_agreement` and `sign_agreement`.
  - **Taker** for raising exposure and for delivery calls.
  - **Giver** for confirming a raise, return calls, pledges and substitutions.
  - **No status check** for lowering exposure, releases, cancels, disputes, withdrawals and
    `check_coverage` (FR-034).
  - **Fund**: only `propose_agreement` requires it to be Open.
- **Rationale**:
  - Steps that add an obligation fail closed on an ineligible actor. Steps that only reduce the
    giver's burden stay available, so a suspension can't trap units the taker no longer needs.
  - A Fund can't leave Open before F007 (wind-down), so a status check on later steps could never
    be shown failing (Constitution VI). F007 decides what wind-down does to agreements.

## R12. Paused transfers

- **Decision**: no step in `collateral`, and not `set_lien`, reads `transfers_paused` (FR-035).
- **Rationale**: a lock moves no units, and a margin call must be answerable during a pause. The
  pause still stops every transfer (hook step 3), so nothing locked or unlocked during a pause can
  leave.

## R13. Interest on locked units

- **Decision**: no change to accrual. `set_lien` doesn't touch the balance, so no `accrue` call is
  needed. Interest follows `last_balance` (F002 FR-015). Test: two investors with equal balances,
  one fully locked, across several publishes and a transfer each, end with identical `accrued_e18`
  (SC-004). This also completes F002 US2 scenario 3.
- **Rationale**: the lien design chosen in design.md §9.2 exists for this.

## R14. Compute, call depth and transaction size

- **Decision**:
  - **Compute**: each `collateral` instruction stays at or below 80 000 CU, and `set_lien` at or
    below 15 000 CU. Hook step 5 adds under 100 CU to `execute`.
  - **Call depth**: at most 3 for party steps (collateral → hook → none; `emit_cpi` is the
    third), and 4 for compliance resolution (Squads → governance → collateral → `emit_cpi`).
  - **Accounts**: the largest instruction, `substitute`, uses 12 accounts. A durable-nonce pledge
    signed by a Squads vault fits a v0 message without a lookup table.
  - **Scale**: every instruction's account set is fixed whatever the number of holders, agreements,
    pledges or calls (SC-007). `publish_daily` is untouched.
- **Rationale**: measured in tests (`scale.rs`) like F003's limits.

## R15. Tests: a reference model and the new invariants

- **Decision**:
  - **Model**: `tests/src/collateral_model.rs`, an independent model of agreements, pledges,
    calls, D, derived status and locks, with its own copy of the §8 formulas.
  - **Property test**: `collateral_prop.rs` runs random sequences (propose, sign, exposure up and
    down, publish up and down, calls, pledge, release, substitute, dispute, cancel, check,
    transfer, settlement funding, suspend, pause) against the model. After every step it checks I1,
    I2 and the new I15 (agreement total) and I16 (demanded part), plus SC-002, SC-003 and SC-005.
  - **Rejection tests**: one per rule, asserting the exact error. `check-error-coverage.sh` gains
    `CollateralError` and the new hook errors (SC-006).
  - **Slow signers**: durable-nonce tests with the blockhash check on (pledge after 6 h; a pledge
    prepared before a call opened; a stale confirmation).
  - **Lock bypasses**: a "no path moves locked units" suite covering direct `transfer_checked`, a
    delegate approved by the owner, F003 redemption and secondary funding (F003 US2 scenario 3),
    pledges under a second agreement, and direct `set_lien` from any key.
- **Rationale**: the same structure as F003 R18. A model that shares code with the program would
  repeat its bugs.

## R16. Deployment and existing funds

- **Decision**:
  - **`collateral`**: a new program, whose upgrade authority is the issuer's Squads vault with
    its time lock.
  - **Upgraded in place**: `compliance_hook` (step 5, `set_lien`, `init_registry` filling
    `collateral_program`) and `governance` (resolution).
  - **No account resizes**: the HolderState and Registry fields already exist.
  - **Existing funds**: registries created earlier keep an empty `collateral_program`, so liens
    are unavailable for them (fail closed). Step 5 is harmless for them, since every lock is 0.
    The F004 devnet run creates a fresh fund.
- **Rationale**: devnet funds from F001 to F003 hold test units only. A migration instruction would
  be a rule-change power (Constitution VIII) for no real holder.

## R17. Spec adjustments found while planning

Recorded in [plan.md](plan.md#spec-adjustments-made-during-planning): the all-together Met rule
with the demanded part (R5); no automatic cancel of return calls, and return-call progress from
named releases only (R8); a giver reference on each pledge (R6); no limit on unresolved calls.
