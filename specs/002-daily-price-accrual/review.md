# F002 review checklist (Constitution review gate, task T038)

**For**: the human reviewer who signs off F002 before merge. The constitution requires a person,
not the implementing agent, to check money math and rounding, hook check order, signer checks,
cross-program calls and invariants (Development Workflow → Review). Tick each box after checking
it in the code. Links point at the exact lines as of commit `2c990fc` (program code unchanged
since `690940f`). Record the sign-off in the pull request.

Automated evidence already in place (re-run before signing):
- `cargo test -p tests`: 157 tests, incl. 64 random 90-day accrual simulations and 128 random
  F001 sequences with publishes;
- `cargo test -p tests --release --test scale -- --ignored`;
- `cargo test -p mmf-common`: the formulas' unit tests;
- `scripts/check-error-coverage.sh`: 73 of 73 errors asserted.

What changed and why is in [plan.md](plan.md) and [research.md](research.md). The F001 test
edits are listed in [pr-notes.md](pr-notes.md).

## 1. Money math against `docs/shared-model.md` §8

All of it is in [accrual.rs](../../crates/mmf-common/src/accrual.rs). The hook, `fund` and the SDK
share this one implementation. The tests' reference model
([model.rs](../../tests/src/model.rs), `AccrualModel`) is written independently.

| Formula | Where | Expected (§8) | ✔ |
|---|---|---|---|
| Index increment `rate × 10^6 × days` | [accrual.rs:22-26](../../crates/mmf-common/src/accrual.rs#L22-L26) | exact; checked `u128`; no division | [ ] |
| Settle `accrued += pre × (index − last)`; `last = index` | [accrual.rs:32-41](../../crates/mmf-common/src/accrual.rs#L32-L41) | exact; checked; `None` if the index is behind the checkpoint | [ ] |
| Earned on read | [accrual.rs:45](../../crates/mmf-common/src/accrual.rs#L45) | same formula, writes nothing (FR-017) | [ ] |
| Price-move check `abs(nav − prev) × 10_000 ≤ prev × bps` | [accrual.rs:62](../../crates/mmf-common/src/accrual.rs#L62) | exact, cross-multiplied; equal to the limit passes; products can't overflow `u128` | [ ] |
| UTC day `ts.div_euclid(86_400)` | [accrual.rs:17](../../crates/mmf-common/src/accrual.rs#L17) | the only division; `None` before 1970 | [ ] |

Questions to answer:
- [ ] No floating point anywhere (`grep -rn "f64\|f32" programs crates` finds nothing new).
- [ ] Rounding to whole units happens nowhere in F002. Payout (F006) is the first place, per §8.
- [ ] The rate parameter is `i64` and negatives are rejected before the `as u64` cast
  ([publish_daily.rs:67-68](../../programs/fund/src/instructions/publish_daily.rs#L67-L68);
  research R6).

## 2. Hook step 8 against §6

[execute.rs](../../programs/compliance_hook/src/hook/execute.rs):

| Check | Line | Expected | ✔ |
|---|---|---|---|
| step 8 comes after steps 3, 4, 6, 7 | 183 | after the cap check (176), before step 9 (205) | [ ] |
| the counter comes only from a verified FundState | 186 | `FundStateView::load` checks owner, address `registry.fund_state` and discriminator → `WrongMint` | [ ] |
| pre-balances | 195-202 | `pre_src = post_src + amount`, `pre_dst = post_dst − amount`, both checked | [ ] |
| same owner settles once | 193 | single settle on the combined balance | [ ] |
| each HolderState written once, with step 9's changes | 212-217 | one `store_holder` per record | [ ] |
| the event carries the settled figures | 220+ | `accrual_index`, `source_accrued_e18`, `destination_accrued_e18` | [ ] |

- [ ] Evidence: `a_transfer_rejected_at_any_check_settles_nothing`,
  `a_fund_state_that_cannot_be_verified_is_rejected` and `accrual_arithmetic_fails_closed` in
  `tests/tests/hook_order.rs`. Note that the first is also guaranteed by transaction atomicity.
- [ ] Hook compute stays constant and ≤ 60 000 CU (`scale.rs`: about 16 000–22 000 depending on the
  fund's PDA bumps, equal at 10 and 10 000 holders).

## 3. Signer checks against §5.1

| Instruction | Expected signer | Check | ✔ |
|---|---|---|---|
| `fund::publish_daily` | fund admin (+ any payer) | role registry verified at `fund_state.role_registry` ([publish_daily.rs:47-56](../../programs/fund/src/instructions/publish_daily.rs#L47-L56)); Open (58) | [ ] |
| `fund::set_nav_override`, `clear_nav_override` | governance PDA `["fund-caller"]` only | [nav_override.rs:25-37](../../programs/fund/src/instructions/nav_override.rs#L25-L37) (`UnauthorizedCaller`) | [ ] |
| `governance::approve_nav_override`, `withdraw_nav_override` | compliance | [nav_override.rs:33-55](../../programs/governance/src/instructions/nav_override.rs#L33-L55): compliance key, Open fund, reason ∈ {3, 6} | [ ] |

Questions to answer:
- [ ] The fund admin has no way to set an override. Evidence: `only_compliance_approves_or_withdraws`
  and `fund_override_instructions_accept_only_governance` in `tests/tests/f002_override.rs`.
- [ ] `["fund-caller"]` is shared by every fund, so the fund binding matters. Fund B's compliance
  can't reach fund A: Anchor's `address = role_registry.fund_state`
  ([governance nav_override.rs:20](../../programs/governance/src/instructions/nav_override.rs#L20)).
  Evidence: `one_funds_compliance_cannot_override_another_fund`.
- [ ] A Squads 2-of-3 compliance vault acts only at its threshold. Evidence:
  `a_multisig_compliance_approves_only_at_its_threshold`, which also runs at call depth 4.

## 4. Cross-program calls against §5.2

| Caller → callee | Where | Allowed by | ✔ |
|---|---|---|---|
| governance → `fund::set_nav_override` / `clear_nav_override`, signed by `["fund-caller"]` | [governance nav_override.rs:79](../../programs/governance/src/instructions/nav_override.rs#L79), [:97](../../programs/governance/src/instructions/nav_override.rs#L97) | §5.2 row added in F002 | [ ] |
| fund → System (NavRecord creation, Anchor `init`) | [publish_daily.rs](../../programs/fund/src/instructions/publish_daily.rs) accounts | account creation | [ ] |
| fund → fund (`emit_cpi!` events) | publish and override handlers | events (§1) | [ ] |

- [ ] No other new CPI edge exists. `compliance_hook::accrue` (fund → hook) is **not** built; it
  moved to F003 (research R8).

## 5. Invariants (§7)

- [ ] **I11** (per-holder interest equals the day-by-day sum): `tests/tests/accrual_prop.rs`,
  checked after every step, exact. Mutation-checked: crediting the receiver on its post-balance
  fails at the first transfer into a holder.
- [ ] **I12** (index and period continuity; checkpoints never ahead of the counter):
  `fixtures::check_i12` after every publish in `f002_publish.rs`, and after every step in both
  property tests.
- [ ] **I3, I10** (F001) still hold with publishes interleaved (`invariants_prop.rs`,
  `accrual_prop.rs`).
- [ ] **SC-007**: the auditor's rebuild from NavRecords and `TransferValidated` alone agrees with
  every holder (`tests/src/rebuild.rs`, used by `f002_accrual.rs` and `accrual_prop.rs`).

## 6. Layout and earlier features

- [ ] FundState size and every F001 offset are unchanged. The two new fields come after `bump`
  ([state.rs](../../programs/fund/src/state.rs); asserted in `tests/tests/layouts.rs`). Existing
  funds decode as "no publish, no override".
- [ ] `FundError` variants were appended, so F001 error codes kept their numbers. Only the
  message of `Unauthorized` changed.
- [ ] Every F001 test edit is listed with its reason in [pr-notes.md](pr-notes.md)
  (Constitution XII).

## 7. Things the reviewer should know

- **Trusted figures**: the rate and days covered have no cap (Clarification Q2). A wrong rate pays
  every holder wrongly and can't be reversed in F002. An oversized `days_covered` blocks publishing
  until that period passes. Both are stated in the spec's Assumptions.
- **Days are UTC days** (research R2). A publish after 00:00 UTC lands on the next day.
- **Test-only state edits** (each reaches a state no instruction can produce, and says so in a
  comment):
  - FundState `accrual_index` / `nav_seq` (publish overflow);
  - FundState owner (forged account);
  - HolderState `index_at_last_touch` (hook overflow);
  - the scale test's FundState restore for a like-for-like publish comparison.
- **Not yet done**: the devnet run (T039) and devnet deployment of the programs in general (F001
  T079 is also open). The script was rehearsed on a local validator, where 34 of 34 steps matched.

## Sign-off

- Reviewer:
- Date:
- Commit reviewed:
- Outcome: ☐ approved ☐ changes requested
