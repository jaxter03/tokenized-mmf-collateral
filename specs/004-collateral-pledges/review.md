# F004 review checklist (Constitution review gate, task T077)

**For**: the human reviewer who signs off F004 before merge. The constitution requires a person,
not the implementing agent, to check these:
- money math and rounding;
- hook check order;
- signer checks;
- cross-program calls;
- invariants.

(Development Workflow → Review.) Tick each box after checking it in the code. Links point at the
exact lines as of commit `47a00b9`. Record the sign-off in the pull request.

Automated evidence already in place (re-run before signing):
- **`cargo test -p tests`**: every F001–F004 suite. That includes:
  - `collateral_prop`: random collateral sequences against an independent model, with I1, I2,
    I15 and I16 checked on chain after every step;
  - the F001 invariant runs, now asserting I1/I2.
- **`cargo test -p tests --release -- --ignored`**: the scale tests (incl. F004's SC-007) and the
  long property runs (256 collateral cases).
- **`cargo test -p mmf-common -p collateral -p compliance_hook`**: the formulas', observe's and
  `set_lien` arithmetic's unit tests.
- **`scripts/check-error-coverage.sh`**: 160 of 160 errors asserted.

What changed and why is in [plan.md](plan.md) and [research.md](research.md); earlier-feature
test edits and other PR notes are in [pr-notes.md](pr-notes.md).

## 1. Money math against `docs/shared-model.md` §8

All of it is in [collateral.rs](../../crates/mmf-common/src/collateral.rs). The tests' model
([collateral_model.rs](../../tests/src/collateral_model.rs)) has its own copy.

| Formula | Where | Expected (§8) | ✔ |
|---|---|---|---|
| Collateral value `floor(units × nav × (10_000 − h) / (10^8 × 10_000))` | [collateral.rs:26](../../crates/mmf-common/src/collateral.rs#L26) | rounds **down**, once on the agreement's total pledged units; `None` above `u64` | [ ] |
| Units for a delivery call `ceil(v × 10^8 × 10_000 / (nav × (10_000 − h)))` | [collateral.rs:37](../../crates/mmf-common/src/collateral.rs#L37) | rounds **up** (the giver always posts enough); `None` for h ≥ 100 % or nav 0 | [ ] |
| Units for a return call (same, floor) | [collateral.rs:44](../../crates/mmf-common/src/collateral.rs#L44) | rounds **down** (the taker keeps enough); new §8 row | [ ] |
| Price age `nav_seq > 0 ∧ now − nav_ts ≤ max_age` | [collateral.rs:83](../../crates/mmf-common/src/collateral.rs#L83) | exactly the max age is fresh; checked subtraction | [ ] |
| Observe: `D = min(D, S)`; at S = 0, all calls Met | [collateral.rs:104](../../crates/mmf-common/src/collateral.rs#L104) | research R5; never raises D | [ ] |

Questions to answer:
- [ ] No floating point anywhere (`grep -rn "f64\|f32" programs crates`).
- [ ] Worked figures: $1M at $1.00 and 2 % needs 1 020 408.163266 units, worth exactly
  $1 000 000.000000. A $600 000 excess returns 612 244.897959 units. (Unit tests in
  collateral.rs, and the on-chain US2–US4 tests.)
- [ ] SC-005: `rounding_never_under_covers_the_taker` (collateral_prop) shows, for random values,
  prices and haircuts, that a call's units always cover it and a return never under-covers.
- [ ] A delivery call's value is the uncalled shortfall (`S − D`), computed after observing
  ([exposure.rs:83](../../programs/collateral/src/instructions/exposure.rs#L83),
  [issue_call.rs](../../programs/collateral/src/instructions/issue_call.rs)), so calls never
  double-count.

## 2. The lock (research R2, R3; Constitution II)

| Check | Line | Expected | ✔ |
|---|---|---|---|
| Hook step 5 | [execute.rs:171](../../programs/compliance_hook/src/hook/execute.rs#L171) | `post_src ≥ pledged_amount`, after step 4 (sender) and before step 6 (receiver); no bypass until F005's step 2 | [ ] |
| `set_lien` caller | [set_lien.rs:29](../../programs/compliance_hook/src/instructions/set_lien.rs#L29) | `registry.collateral_program` set, and `hook_caller` is its `["hook-caller"]` PDA and signed, else `NotCollateralCaller` | [ ] |
| `set_lien` accounts | [set_lien.rs:66](../../programs/compliance_hook/src/instructions/set_lien.rs#L66) | the owner's ATA for the Registry's mint and that owner's HolderState (`owner_accounts`, shared with `accrue`) | [ ] |
| `set_lien` I2 | [set_lien.rs:71](../../programs/compliance_hook/src/instructions/set_lien.rs#L71) | a growing lock stays ≤ the holding balance, else `InsufficientUnlockedUnits` | [ ] |
| `set_lien` totals | [set_lien.rs:44](../../programs/compliance_hook/src/instructions/set_lien.rs#L44) | `u128`, `LienUnderflow` / `Overflow`, no wrap | [ ] |
| `collateral_program` set at creation | [init_registry.rs:83](../../programs/compliance_hook/src/instructions/init_registry.rs#L83) | the compiled collateral id; no setter exists (a setter would be a rule-change power) | [ ] |

- [ ] Every way a locked unit could leave is refused. `f004_lock.rs` covers a hand-built
  `transfer_checked`, an approved delegate, redemption and secondary funding, and a pledge
  elsewhere.
- [ ] Locked units earn the same interest (`f004_interest.rs`): `set_lien` never touches the
  balance or the accrual fields.

## 3. Signer checks against §5.1

| Instruction | Expected signer | Check | ✔ |
|---|---|---|---|
| `propose_agreement` | the giver or the taker; both eligible; Fund Open | [propose_agreement.rs:83](../../programs/collateral/src/instructions/propose_agreement.rs#L83) | [ ] |
| `sign_agreement` | the other party, once | [sign_agreement.rs:45](../../programs/collateral/src/instructions/sign_agreement.rs#L45) | [ ] |
| `update_exposure` / `confirm_exposure` | taker / giver (exact pending value and reference) | [exposure.rs:70](../../programs/collateral/src/instructions/exposure.rs#L70), [exposure.rs:211](../../programs/collateral/src/instructions/exposure.rs#L211) | [ ] |
| `issue_call` | taker (Delivery) / giver (Return) | [issue_call.rs:55](../../programs/collateral/src/instructions/issue_call.rs#L55) | [ ] |
| `check_coverage` | anyone (permissionless) | only observes; the trigger is recorded | [ ] |
| `pledge`, `substitute` | giver, eligible | [pledge.rs:71](../../programs/collateral/src/instructions/pledge.rs#L71), [substitute.rs:78](../../programs/collateral/src/instructions/substitute.rs#L78) | [ ] |
| `release` | taker; keeper only against the open return call, within it | [release.rs:71](../../programs/collateral/src/instructions/release.rs#L71), [release.rs:89](../../programs/collateral/src/instructions/release.rs#L89), [release.rs:101](../../programs/collateral/src/instructions/release.rs#L101) | [ ] |
| `cancel_call` | taker (Delivery) / giver (Return) | [cancel_call.rs:58](../../programs/collateral/src/instructions/cancel_call.rs#L58) | [ ] |
| `dispute_call`, `withdraw_dispute` | giver | [dispute.rs:59](../../programs/collateral/src/instructions/dispute.rs#L59), [dispute.rs:100](../../programs/collateral/src/instructions/dispute.rs#L100) | [ ] |
| `governance::resolve_call_dispute` | compliance; Open fund; F001 reasons | [resolve_call_dispute.rs:50](../../programs/governance/src/instructions/resolve_call_dispute.rs#L50) | [ ] |
| `collateral::resolve_dispute` | governance `["collateral-caller"]` only | [dispute.rs:135](../../programs/collateral/src/instructions/dispute.rs#L135) | [ ] |

Questions to answer:
- [ ] Steps that add an obligation require an eligible actor; lowering exposure, releasing,
  cancelling and disputing don't (FR-034, `f004_status.rs`).
- [ ] `["collateral-caller"]` is shared by every fund: one fund's compliance can't resolve
  another's dispute (`a_call_of_another_funds_agreement_is_refused`).
- [ ] A Squads 2-of-3 compliance resolves only at its threshold, at call depth 4
  (`a_multisig_compliance_resolves_only_at_its_threshold`).

## 4. Cross-program calls against §5.2

| Edge | Where | Expected | ✔ |
|---|---|---|---|
| `collateral → compliance_hook::set_lien` | [lien.rs:22](../../programs/collateral/src/lien.rs#L22) | built by hand so `["hook-caller"]` is passed as signer (Anchor's CPI passes an unchecked account unsigned); signed with its seeds only | [ ] |
| `governance → collateral::resolve_dispute` | [resolve_call_dispute.rs](../../programs/governance/src/instructions/resolve_call_dispute.rs) | signed by `["collateral-caller"]`; `actor` = the compliance key | [ ] |

- [ ] No other new edge (`grep -rn "invoke\|cpi::" programs/collateral programs/governance/src/instructions/resolve_call_dispute.rs`).

## 5. Invariants (§7)

| ID | Statement | Evidence | ✔ |
|---|---|---|---|
| I1 | `pledged_amount == Σ remaining` over the holder's Active pledges; `active_liens ==` their count | `Tracker::check` after every step of every F004 test; `collateral_prop`; F001 sequences (no pledges ⇒ 0) | [ ] |
| I2 | balance ≥ locked units, always | same; plus hook step 5 and `set_lien` check 4 | [ ] |
| I15 | agreement `pledged_units == Σ remaining`, ≤ the pledge limit | `Tracker::check`, `collateral_prop` | [ ] |
| I16 | open-call count, demanded part ≤ Σ open values, 0 when none | `Tracker::check`, `collateral_prop` | [ ] |

## 6. Design points to judge

- [ ] **Calls met together** (research R5, a spec adjustment made in planning):
  - every open delivery call is Met when a step at a fresh price finds coverage ≥ 0, and a call's
    Met status is derived from `met_below_seq`;
  - oldest-first meeting was dropped because it would make every pledge depend on the list of
    open calls, which breaks prepared (slow-signer) pledges;
  - fairness across several calls moves to F005: enforcement on a call must be limited to the
    shortfall and to that call's own value.
- [ ] **Constitution IV exceptions**, both named in the plan: a raise opening its call in the same
  instruction (R7), and substitution's new pledge with the old release (FR-030).
- [ ] **Return calls** are not cancelled when a delivery call opens (R8). A stale return call can
  never under-cover the taker, because every release checks coverage.

## 7. Layout and earlier features

- [ ] Account sizes 432 / 196 / 268 B (`layouts.rs`); HolderState and Registry unchanged in size.
- [ ] Earlier-feature edits are only those listed in [pr-notes.md](pr-notes.md): the harness, the
  fixtures (a fund-terms parameter, same behaviour), `layouts.rs`, `hook_order.rs` (a new test),
  `invariants_prop.rs` (I1/I2 checks), `scale.rs` (a new test), `accrue`'s check split into
  `owner_accounts`, and the devnet script's `send_prepared` visibility.

## 8. Things the reviewer should know

- **Trusted parties**: the taker's exposure (the system records it, it doesn't compute it); the
  legal agreement (the on-chain terms mirror it); the keeper (timing only); custodians. These are
  stated in the spec's Assumptions.
- **Registries created before F004** have no collateral program and can't hold liens (R16). The
  devnet run creates a new fund.
- **`set_lien` compute** varies with the owner's ATA address: 13 141–23 641 CU over 40 runs
  (R14, revised). A whole pledge stays ≤ 80 000 (51 084 at most measured).
- **Test-only state edits** (each reaches a state no instruction can produce, and says so):
  - a Registry with an empty `collateral_program` (`registry_without_a_collateral_program_refuses_liens`);
  - a HolderState lock one unit below its pledge (`an_unlock_beyond_the_lock_is_refused`).
- **Not yet done**: the devnet run (T078) needs your go-ahead, devnet SOL and deployed programs.
  The script was rehearsed on a local validator with Token-2022 v11, where 37 of 37 steps matched.

## Sign-off

- Reviewer:
- Date:
- Commit reviewed:
- Outcome: ☐ approved ☐ changes requested
