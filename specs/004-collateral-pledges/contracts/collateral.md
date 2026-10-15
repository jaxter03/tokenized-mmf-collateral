# Contract: `collateral` program (new in F004)

Checks run in the order listed. The first failing check aborts the transaction with that error
(Constitution VII), and every instruction emits exactly one event with `emit_cpi!`
(shared-model §1). `CollateralError` is a new enum. Account reads of other programs go through
`mmf_common::views`, which check the owner, the address and the discriminator. A view that fails
to load is reported as the error named in its row. Formulas, `fresh`, `eligible`, `observe` and
the derived call status are defined in [data-model.md](../data-model.md#units-and-formulas).

Common definitions:
- `now = Clock.unix_timestamp`; `a` = the agreement; `fund` = `FundStateView` of `a.fund_state`.
- **Party bound**: the agreement's `fund_state` equals the FundState passed, and RoleRegistry is
  `fund.role_registry` (`FundMismatch`).
- **Holder record of X**: `HolderStateView` at `["holder", fund.mint, X]` of compliance_hook
  (`WrongHolder` if it doesn't load).
- **Valuing step**: requires `fresh` (`NoPrice` if `nav_seq == 0`, else `StalePrice`), then runs
  `observe` before its own effect.
- **Lien CPI**: `compliance_hook::set_lien` signed by `["hook-caller"]`
  ([compliance_hook.md](compliance_hook.md)). Its errors (`InsufficientUnlockedUnits`, …) surface
  unchanged.

Common accounts are written once here and referred to by name below:

| Account | Kind | Constraint |
|---|---|---|
| `fund_state` | read | FundState, via `FundStateView` |
| `role_registry` | read | RoleRegistry at `fund_state.role_registry`, via `RoleRegistryView` |
| `agreement` | writable | PDA `["agreement", fund_state, a.giver, a.taker, a.nonce]` |
| `lien accounts` | | `registry` (read, Registry of `fund.mint`), `giver_holder` (writable, HolderState of `a.giver`), `giver_holding` (read, `a.giver`'s Token-2022 ATA for the mint), `hook_caller` (PDA `["hook-caller"]` of collateral), `compliance_hook_program` |
| `payer` | signer, writable | rent for any account created; may differ from the signing party |

## `propose_agreement(args)`: giver or taker

```text
args: giver Pubkey, taker Pubkey, nonce u64, mode u8, income_to u8, haircut_bps u16,
      max_nav_age_secs u32, call_cure_secs u32, enforcement_grace_secs u32,
      min_transfer_value u64, concentration_cap_units u64, taker_may_call_unilaterally bool,
      agreement_hash [u8; 32]
```

| Account | Kind | Constraint |
|---|---|---|
| `proposer` | signer | `∈ {giver, taker}` |
| `payer`, `fund_state` | | |
| `giver_holder`, `taker_holder` | read | HolderState of `giver`, of `taker` |
| `agreement` | writable, init | PDA `["agreement", fund_state, giver, taker, nonce]` |
| `system_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | FundState loads | `FundMismatch` |
| 2 | `fund.status == Open` | `FundNotOpen` |
| 3 | `proposer ∈ {giver, taker}` | `Unauthorized` |
| 4 | `giver ≠ taker` | `SameParty` |
| 5 | `mode == Lien (0)` | `UnsupportedPledgeMode` |
| 6 | `income_to == Giver (0)` | `UnsupportedIncomeRouting` |
| 7 | `haircut_bps < 10_000` | `InvalidHaircut` |
| 8 | `call_cure_secs > 0` | `InvalidCureTime` |
| 9 | `concentration_cap_units > 0` | `InvalidPledgeLimit` |
| 10 | `max_nav_age_secs > 0` | `InvalidPriceAge` |
| 11 | `agreement_hash ≠ [0; 32]` | `EmptyAgreementHash` |
| 12 | both holder records load and both are `eligible` | `WrongHolder` / `PartyNotEligible` |
| 13 | the agreement doesn't exist yet | (Anchor `init`: already in use) |

**Effect**: the agreement is created with the args, `status = Proposed`, the proposer's
`*_signed = true`, `pledge_count = call_count = 1`, `created_ts = now`, and every other field
zero. **Event**: `AgreementProposed`.

## `sign_agreement()`: the party that didn't propose

| Account | Kind | Constraint |
|---|---|---|
| `signer` | signer | |
| `fund_state`, `agreement` | | |
| `giver_holder`, `taker_holder` | read | HolderState of `a.giver`, of `a.taker` |

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Proposed` | `AgreementNotProposed` |
| 3 | `signer ∈ {a.giver, a.taker}` | `Unauthorized` |
| 4 | the signer's flag is still false | `AlreadySigned` |
| 5 | both holder records are `eligible` | `WrongHolder` / `PartyNotEligible` |

**Effect**: sets the signer's flag; `status = Active`, `activated_ts = now`. **Event**:
`AgreementActive`.

## `update_exposure(required_value: u64, external_ref: [u8; 32])`: taker

| Account | Kind | Constraint |
|---|---|---|
| `taker` | signer | `== a.taker` |
| `payer`, `fund_state`, `agreement` | | |
| `taker_holder` | read | HolderState of `a.taker` |
| `call` | writable | PDA `["call", agreement, external_ref]`; created only if a call opens |
| `system_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Active` | `AgreementNotActive` |
| 3 | `taker == a.taker` | `Unauthorized` |
| 4 | `external_ref ≠ [0; 32]` | `ZeroExternalRef` |
| 5 | **lower or equal** (`required_value ≤ a.required_value`): no further checks | |
| 5a | **raise**: taker holder record is `eligible` | `PartyNotEligible` |
| 5b | **raise, unilateral**: `fresh` | `NoPrice` / `StalePrice` |
| 5c | **raise, unilateral**, call needed: `call` doesn't exist yet | `CallRefInUse` |

**Effect**:
- **Lower or equal**: set `required_value`, `exposure_ref`, `exposure_updated_ts`; clear the
  pending raise. If `fresh` or `required_value == 0`, run `observe`. Outcome Applied.
- **Raise, `a.taker_may_call_unilaterally`**: run `observe`, then set the exposure fields and clear
  the pending raise. Compute `U = shortfall − called_outstanding`. If `U > min_transfer_value`,
  open a delivery call at `call` (below, "Opening a delivery call"). Outcome Applied.
- **Raise, otherwise**: store `pending_raise_{value = required_value, ref = external_ref,
  ts = now}`. Outcome Pending.

**Event**: `ExposureUpdated`. The raise with the call it opens is the named Constitution IV
exception (R7).

**Opening a delivery call** (shared by `update_exposure`, `confirm_exposure`, `issue_call`):
`CollateralCall { seq: a.call_count, kind: Delivery, value_delta: U,
units_required: units_for_value_up(U), nav_seq: fund.nav_seq, issued_at: now,
cure_deadline_ts: now + call_cure_secs, enforce_after_ts: cure_deadline_ts +
enforcement_grace_secs, status: Open, external_ref }`. Then `a.call_count += 1`,
`a.open_delivery_calls += 1`, `a.called_outstanding += U` (it now equals the shortfall). The call
account is created by a system-program CPI with the call's PDA seeds and paid by `payer`.

## `confirm_exposure(required_value: u64, external_ref: [u8; 32])`: giver

Accounts as `update_exposure`, with `giver` (signer, `== a.giver`) and `giver_holder` instead of
the taker's. `call` is at `["call", agreement, external_ref]`.

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Active` | `AgreementNotActive` |
| 3 | `giver == a.giver` | `Unauthorized` |
| 4 | `a.pending_raise_value ≠ 0` | `NoPendingRaise` |
| 5 | `required_value == a.pending_raise_value ∧ external_ref == a.pending_raise_ref` | `PendingRaiseMismatch` |
| 6 | giver holder record is `eligible` | `PartyNotEligible` |
| 7 | `fresh` | `NoPrice` / `StalePrice` |
| 8 | call needed: `call` doesn't exist yet | `CallRefInUse` |

**Effect**: as the unilateral raise. The call's deadlines count from `now`, the confirmation.
Outcome Confirmed. **Event**: `ExposureUpdated`.

## `issue_call(kind: u8, external_ref: [u8; 32])`: taker (delivery) or giver (return)

| Account | Kind | Constraint |
|---|---|---|
| `signer` | signer | `a.taker` for Delivery, `a.giver` for Return |
| `payer`, `fund_state`, `agreement` | | |
| `signer_holder` | read | HolderState of the signer |
| `call` | writable, init | PDA `["call", agreement, external_ref]` |
| `system_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Active` | `AgreementNotActive` |
| 3 | `kind ∈ {0, 1}` | `InvalidCallKind` |
| 4 | signer matches the kind | `Unauthorized` |
| 5 | `external_ref ≠ [0; 32]` | `ZeroExternalRef` |
| 6 | signer holder record is `eligible` | `PartyNotEligible` |
| 7 | `fresh`; then `observe` | `NoPrice` / `StalePrice` |
| 8D | Delivery: `shortfall − called_outstanding > min_transfer_value` | `NothingToCall` |
| 8R | Return: `a.open_return_call == default` | `ReturnCallAlreadyOpen` |
| 9R | Return: `excess > min_transfer_value` | `NoExcess` |
| 10R | Return: `units_for_value_down(excess) > 0` | `ReturnTooSmall` |
| 11 | `call` doesn't exist yet | (Anchor `init`: already in use) |

**Effect**:
- **Delivery**: open a delivery call (as in `update_exposure`).
- **Return**: `CollateralCall { kind: Return, value_delta: excess,
  units_required: units_for_value_down(excess), cure_deadline_ts: now + call_cure_secs,
  enforce_after_ts: 0, status: Open, … }`; then `a.call_count += 1` and
  `a.open_return_call = call`.

**Event**: `CallIssued`.

## `check_coverage()`: anyone (permissionless, state-checked)

| Account | Kind | Constraint |
|---|---|---|
| `triggered_by` | signer | any key |
| `fund_state`, `agreement` | | |

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Active` | `AgreementNotActive` |
| 3 | `fresh` | `NoPrice` / `StalePrice` |

**Effect**: `observe` only (FR-019). The outcome depends only on state and time.
**Event**: `CoverageChecked`.

## `pledge(units: u64, pledge_ref: [u8; 32])`: giver

| Account | Kind | Constraint |
|---|---|---|
| `giver` | signer | `== a.giver` |
| `payer`, `fund_state`, `agreement` | | |
| `pledge` | writable, init | PDA `["pledge", agreement, pledge_ref]` |
| `lien accounts` | | |
| `system_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Active` | `AgreementNotActive` |
| 3 | `giver == a.giver` | `Unauthorized` |
| 4 | `units > 0` | `ZeroUnits` |
| 5 | `pledge_ref ≠ [0; 32]` | `ZeroPledgeRef` |
| 6 | giver holder record is `eligible` | `PartyNotEligible` |
| 7 | `a.pledged_units + units ≤ a.concentration_cap_units` | `PledgeLimitExceeded` |
| 8 | lien CPI `{lock: units, unlock: 0, opened: 1, closed: 0}` succeeds | `InsufficientUnlockedUnits`, … (hook) |
| 9 | `pledge` doesn't exist yet | (Anchor `init`: already in use) |

No price is needed (spec US3 scenario 7), and transfers may be paused (FR-035).

**Effect**:
- Creates `Pledge { seq: a.pledge_count, pledge_ref, units, mode: Lien, posted_at: now,
  nav_seq_at_post: fund.nav_seq, status: Active }`.
- `a.pledge_count += 1`; `a.pledged_units += units`.
- If `fresh`, runs `observe`, so the call is Met in this step when coverage is now ≥ 0.

**Event**: `Pledged`.

## `release(units: u64)`: taker, or keeper against the open return call

| Account | Kind | Constraint |
|---|---|---|
| `signer` | signer | `a.taker` or `role_registry.keeper` |
| `fund_state`, `role_registry`, `agreement` | | |
| `pledge` | writable | a Pledge of this agreement |
| `return_call` | writable, optional | `== a.open_return_call` when given |
| `lien accounts` | | |

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Active` | `AgreementNotActive` |
| 3 | `signer ∈ {a.taker, keeper}` | `Unauthorized` |
| 4 | `pledge.agreement == a ∧ pledge.status == Active` | `PledgeNotActive` |
| 5 | `units > 0` | `ZeroUnits` |
| 6 | `units ≤ remaining(pledge)` | `ExceedsPledgeRemaining` |
| 7 | keeper: `return_call` given | `KeeperNeedsReturnCall` |
| 8 | `return_call` given: it is `a.open_return_call` and Open | `WrongReturnCall` |
| 9 | keeper: `units ≤ return_call.units_required − return_call.units_released` | `ExceedsReturnCall` |
| 10 | if `a.required_value > 0`: `fresh` | `NoPrice` / `StalePrice` |
| 11 | `coverage` with `pledged_units − units` is ≥ 0 (skipped when `required_value == 0`) | `WouldUndercollateralise` |
| 12 | lien CPI `{lock: 0, unlock: units, opened: 0, closed: 1 if remaining becomes 0}` | (hook) |

No holder status check (FR-034); paused transfers are irrelevant (FR-035).

**Effect**:
- **Pledge**: `released_units += units`; if `remaining == 0`, then `status = Released` and
  `closed_at = now`.
- **Agreement**: `a.pledged_units −= units`, then `observe` (coverage ≥ 0, so every unresolved
  delivery call is Met).
- **Return call** (if named): `units_released += units`; when it reaches `units_required`, its
  `status = Met`, `closed_at = now` and `a.open_return_call = default`.

**Event**: `Released`.

## `substitute(new_units: u64, new_pledge_ref: [u8; 32])`: giver

| Account | Kind | Constraint |
|---|---|---|
| `giver` | signer | `== a.giver` |
| `payer`, `fund_state`, `agreement` | | |
| `old_pledge` | writable | a Pledge of this agreement |
| `new_pledge` | writable, init | PDA `["pledge", agreement, new_pledge_ref]` |
| `lien accounts`, `system_program` | | |

| # | Check | Error |
|---|---|---|
| 1 | party bound | `FundMismatch` |
| 2 | `a.status == Active` | `AgreementNotActive` |
| 3 | `giver == a.giver` | `Unauthorized` |
| 4 | old pledge belongs to `a` and is Active | `PledgeNotActive` |
| 5 | `new_pledge_ref ≠ [0; 32]` | `ZeroPledgeRef` |
| 6 | `new_units ≥ remaining(old)` | `SubstituteTooSmall` |
| 7 | giver holder record is `eligible` | `PartyNotEligible` |
| 8 | `a.pledged_units − remaining(old) + new_units ≤ cap` | `PledgeLimitExceeded` |
| 9 | lien CPI `{lock: new_units, unlock: remaining(old), opened: 1, closed: 1}` | `InsufficientUnlockedUnits`, … (hook) |

**Effect**:
- **Old pledge**: `released_units += remaining`, `status = Substituted`, `closed_at = now`.
- **New pledge**: created as in `pledge`.
- **Agreement**: `pledged_units` adjusted and `pledge_count += 1`. If `fresh`, runs `observe`.

The new pledge and the old pledge's release are the spec's named one-step case (FR-030).
**Event**: `Substituted`.

## `cancel_call()`: taker (delivery) or giver (return)

| Account | Kind | Constraint |
|---|---|---|
| `signer` | signer | |
| `fund_state`, `agreement` | | |
| `call` | writable | a CollateralCall of this agreement |

| # | Check | Error |
|---|---|---|
| 1 | party bound; `call.agreement == a` | `FundMismatch` |
| 2 | `settle_status(call)`: store derived Met | — |
| 3 | Delivery: `signer == a.taker`; Return: `signer == a.giver` | `Unauthorized` |
| 4 | Delivery: status ∈ {Open, Disputed}; Return: status == Open | `CallNotOpen` |

**Effect**: `status = Cancelled`, `closed_at = now`.
- **Delivery**: `open_delivery_calls −= 1`;
  `called_outstanding −= min(called_outstanding, value_delta)`, and it is set to 0 if the count
  reaches 0.
- **Return**: `a.open_return_call = default`.

If step 2 stored Met, the instruction still fails at step 4 and the whole transaction rolls back
(nothing is stored). **Event**: `CallCancelled`.

## `dispute_call(dispute_ref: [u8; 32])`: giver

| # | Check | Error |
|---|---|---|
| 1 | party bound; `call.agreement == a` | `FundMismatch` |
| 2 | `signer == a.giver` | `Unauthorized` |
| 3 | `call.kind == Delivery` | `WrongCallKind` |
| 4 | derived status is Open | `CallNotOpen` |
| 5 | `dispute_ref ≠ [0; 32]` | `ZeroExternalRef` |

**Effect**: `status = Disputed`, `dispute_ref`, `disputed_at = now`. The deadlines are unchanged.
**Event**: `CallDisputed`.

## `withdraw_dispute()`: giver

| # | Check | Error |
|---|---|---|
| 1 | party bound; `call.agreement == a` | `FundMismatch` |
| 2 | `signer == a.giver` | `Unauthorized` |
| 3 | derived status is Disputed | `NotDisputed` |

**Effect**: `status = Open`. **Event**: `DisputeWithdrawn`.

## `resolve_dispute(outcome: u8, reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)`: governance only

| Account | Kind | Constraint |
|---|---|---|
| `collateral_caller` | signer | PDA `["collateral-caller"]` of governance |
| `fund_state`, `agreement`, `call` (writable) | | |

| # | Check | Error |
|---|---|---|
| 1 | signer is governance's `["collateral-caller"]` | `UnauthorizedCaller` |
| 2 | party bound; `call.agreement == a` | `FundMismatch` |
| 3 | `outcome ∈ {Open (0), Cancelled (5)}` | `InvalidOutcome` |
| 4 | derived status is Disputed | `NotDisputed` |

**Effect**: Open → as `withdraw_dispute`; Cancelled → as `cancel_call` (delivery bookkeeping).
**Event**: `DisputeResolved` with reason, case reference and actor (the compliance key).
Governance checks the role and the reason ([governance.md](governance.md)).

## Errors (`CollateralError`, new)

`FundMismatch`, `FundNotOpen`, `Unauthorized`, `UnauthorizedCaller`, `SameParty`,
`UnsupportedPledgeMode`, `UnsupportedIncomeRouting`, `InvalidHaircut`, `InvalidCureTime`,
`InvalidPledgeLimit`, `InvalidPriceAge`, `EmptyAgreementHash`, `WrongHolder`,
`PartyNotEligible`, `AgreementNotProposed`, `AlreadySigned`, `AgreementNotActive`,
`ZeroExternalRef`, `CallRefInUse`, `NoPrice`, `StalePrice`, `NoPendingRaise`,
`PendingRaiseMismatch`, `InvalidCallKind`, `NothingToCall`, `ReturnCallAlreadyOpen`, `NoExcess`,
`ReturnTooSmall`, `ZeroUnits`, `ZeroPledgeRef`, `PledgeLimitExceeded`, `PledgeNotActive`,
`ExceedsPledgeRemaining`, `KeeperNeedsReturnCall`, `WrongReturnCall`, `ExceedsReturnCall`,
`WouldUndercollateralise`, `SubstituteTooSmall`, `CallNotOpen`, `WrongCallKind`, `NotDisputed`,
`InvalidOutcome`, `Overflow`.

Each has at least one test asserting it (SC-006, [quickstart.md](../quickstart.md)).
