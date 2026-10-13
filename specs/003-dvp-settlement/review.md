# F003 review checklist (Constitution review gate, task T079)

**For**: the human reviewer who signs off F003 before merge. The constitution requires a person,
not the implementing agent, to check money math and rounding, hook check order, signer checks,
cross-program calls and invariants (Development Workflow → Review). Tick each box after checking
it in the code. Links point at the exact lines as of commit `ba12f4a`. Record the sign-off in the
pull request.

Automated evidence already in place (re-run before signing):
- `cargo test -p tests`: every F001, F002 and F003 suite, including `dvp_prop` (random settlement
  sequences against an independent model) and both F001 invariant runs;
- `cargo test -p tests --release -- --ignored`: the scale tests (incl. SC-007) and the long
  property runs;
- `cargo test -p mmf-common -p mmf-sdk`: the formulas', views' and prepared-step unit tests;
- `scripts/check-error-coverage.sh`: 113 of 113 errors asserted (one retired variant skipped).

What changed and why is in [plan.md](plan.md) and [research.md](research.md). F001/F002 test
edits and other PR notes are in [pr-notes.md](pr-notes.md).

## 1. Money math against `docs/shared-model.md` §8

All of it is in [settlement.rs](../../crates/mmf-common/src/settlement.rs); the tests' model
([dvp_model.rs](../../tests/src/dvp_model.rs)) has its own copy.

| Formula | Where | Expected (§8) | ✔ |
|---|---|---|---|
| Subscription cash `ceil(units × nav / 10^8)` | [settlement.rs:24](../../crates/mmf-common/src/settlement.rs#L24) | rounds up (the Fund is never under-paid); checked `u128`; `None` if it doesn't fit `u64` | [ ] |
| Redemption cash `floor(units × nav / 10^8)` | [settlement.rs:32](../../crates/mmf-common/src/settlement.rs#L32) | rounds down (the Fund never over-pays) | [ ] |
| Daily limit `(redeemed_today + units) × 10_000 ≤ at_day_start × gate_bps` | [settlement.rs:40](../../crates/mmf-common/src/settlement.rs#L40) | exact, cross-multiplied; equal passes (new §8 row) | [ ] |
| Price freshness `nav_seq > 0 ∧ period_end + 1 ≥ today` | [settlement.rs:54](../../crates/mmf-common/src/settlement.rs#L54) | widened to `u64`, no overflow at `u32::MAX` | [ ] |
| Day start `utc_day(ts) × 86 400` | [settlement.rs:60](../../crates/mmf-common/src/settlement.rs#L60) | the redemption day is a UTC day (research R9) | [ ] |

Questions to answer:
- [ ] No floating point anywhere (`grep -rn "f64\|f32" programs crates`).
- [ ] A secondary transfer's cash is the agreed amount, not computed (FR-008); the price record is
  stored for reference only.
- [ ] Interest across mint and burn uses F002's exact settle (`accrue`, below); no rounding.

## 2. Mint and burn guard (§5.2, Constitution VIII)

[mint_burn.rs](../../programs/fund/src/instructions/mint_burn.rs):

| Check | Line | Expected | ✔ |
|---|---|---|---|
| caller is this fund's escrow PDA | [229](../../programs/fund/src/instructions/mint_burn.rs#L229) | `["escrow", fund_state]` of `DVP_SETTLEMENT_PROGRAM_ID`, else `UnauthorizedCaller` | [ ] |
| the settlement account itself | [198](../../programs/fund/src/instructions/mint_burn.rs#L198) | owned by dvp at its own PDA, same fund, right kind, Affirmed, same units, else `SettlementMismatch` | [ ] |
| the programs `fund` signs into | [242](../../programs/fund/src/instructions/mint_burn.rs#L242) | hook and Token-2022 program IDs checked: the FundState PDA (mint authority) is never handed to another program | [ ] |
| I5: cash in escrow before minting | [309](../../programs/fund/src/instructions/mint_burn.rs#L309) | escrow-owned account of the cash mint holding ≥ `cash_amount`, else `CashNotEscrowed` | [ ] |
| accrue before mint and burn | [326](../../programs/fund/src/instructions/mint_burn.rs#L326), [135](../../programs/fund/src/instructions/mint_burn.rs#L135) | F002 FR-013 | [ ] |
| limit after the day rolls over, without storing a refused rollover | [114](../../programs/fund/src/instructions/mint_burn.rs#L114), [218](../../programs/fund/src/instructions/mint_burn.rs#L218) | a copy is rolled and checked; stored only on success | [ ] |
| permissioned burn | [156](../../programs/fund/src/instructions/mint_burn.rs#L156) | FundState signs as burn authority; the escrow PDA's signature carries through as owner | [ ] |

- [ ] Only `mint_for_subscription` mints, and only inside `settle` together with the payment to
  the treasury ([settle.rs:193](../../programs/dvp_settlement/src/instructions/settle.rs#L193)).
  Evidence: `f003_mint_burn_guard.rs`, `f003_subscribe.rs` (I5 in one transaction), `dvp_prop`
  (supply = units outstanding after every step).

## 3. Signer checks against §5.1

| Instruction | Expected signer | Check | ✔ |
|---|---|---|---|
| `fund::set_cash_treasury` | issuer, once | [set_cash_treasury.rs:28](../../programs/fund/src/instructions/set_cash_treasury.rs#L28) | [ ] |
| `dvp_settlement::init_dvp` | issuer | [init_dvp.rs:81](../../programs/dvp_settlement/src/instructions/init_dvp.rs#L81) | [ ] |
| `create_instruction` | investor or transfer agent (Sub/Red); seller or buyer (Secondary) | [create_instruction.rs:90](../../programs/dvp_settlement/src/instructions/create_instruction.rs#L90) | [ ] |
| `affirm_and_fund`, `cancel`, `withdraw_cancel` | the side's party (transfer agent for the Fund side) | [affirm_and_fund.rs:94](../../programs/dvp_settlement/src/instructions/affirm_and_fund.rs#L94), [cancel.rs:44](../../programs/dvp_settlement/src/instructions/cancel.rs#L44); `side_party` at [state.rs:117](../../programs/dvp_settlement/src/state.rs#L117) | [ ] |
| `settle`, `expire`, `return_leg` | anyone (permissionless) | outcome depends only on state and time (FR-015); the trigger is only recorded | [ ] |
| `governance::reject_settlement` | compliance | [reject_settlement.rs:43](../../programs/governance/src/instructions/reject_settlement.rs#L43); Open fund; F001 reasons | [ ] |
| `dvp_settlement::reject` | governance `["dvp-caller"]` only | [reject.rs:36](../../programs/dvp_settlement/src/instructions/reject.rs#L36) | [ ] |
| `compliance_hook::accrue` | the FundState PDA only | [accrue.rs:96](../../programs/compliance_hook/src/instructions/accrue.rs#L96) | [ ] |

Questions to answer:
- [ ] Redemption cash can come only from the treasury cash account
  ([affirm_and_fund.rs:142](../../programs/dvp_settlement/src/instructions/affirm_and_fund.rs#L142),
  `WrongCashSource`).
- [ ] Every payment goes to a party's own account: the buyer's holding account, the seller's cash
  ATA, the treasury cash account (`WrongAccount` checks in settle.rs and return_leg.rs). Evidence:
  `units_and_cash_go_only_to_the_parties_accounts`, `a_return_goes_only_to_the_funder`.
- [ ] `["dvp-caller"]` is shared by every fund: one fund's compliance can't reject another's
  instruction (`one_funds_compliance_cannot_reject_another_funds_instruction`).
- [ ] A Squads 2-of-3 compliance rejects only at its threshold, at call depth 4
  (`a_multisig_compliance_rejects_only_at_its_threshold`).

## 4. Cross-program calls against §5.2

| Caller → callee | Where | Allowed by | ✔ |
|---|---|---|---|
| dvp → `fund::mint_for_subscription` / `burn_for_redemption`, escrow PDA signs | [settle.rs:193](../../programs/dvp_settlement/src/instructions/settle.rs#L193), [:258](../../programs/dvp_settlement/src/instructions/settle.rs#L258) | §5.2 | [ ] |
| fund → `compliance_hook::accrue`, FundState signs | [mint_burn.rs:135](../../programs/fund/src/instructions/mint_burn.rs#L135), [:326](../../programs/fund/src/instructions/mint_burn.rs#L326) | §5.2 (`accrue(post_balance)`, escrow class only) | [ ] |
| governance → `dvp_settlement::reject`, `["dvp-caller"]` signs | [reject_settlement.rs](../../programs/governance/src/instructions/reject_settlement.rs) | §5.2 row added in F003 | [ ] |
| dvp → Token-2022 `transfer_checked` with the hook's extra accounts | [token.rs](../../programs/dvp_settlement/src/token.rs) | shared-model §12 (writable Registry passed) | [ ] |
| dvp → cash token program `transfer_checked`; dvp → ATA program (escrow accounts, idempotent) | token.rs, [init_dvp.rs](../../programs/dvp_settlement/src/instructions/init_dvp.rs) | token movements; account creation | [ ] |

- [ ] No other new CPI edge exists.
- [ ] The hook's check order (§6) is unchanged; transfers into and out of escrow are ordinary
  hook-checked transfers.

## 5. Invariants (§7)

- [ ] **I4** (`units_outstanding == supply`): `assert_i4` in every F003 suite, after every step
  of `dvp_prop` and of both F001 invariant runs; `seed_units` keeps it.
- [ ] **I5** (minted units have their cash in escrow, paid to the treasury in the same
  transaction): guard above; `spec_independent_test_two_million_at_one_dollar`,
  `no_units_without_the_cash_in_escrow`.
- [ ] **I6** (both legs moved, or every party restored once returns are delivered): `dvp_prop`
  (balances equal the model after every step, cash conserved), the US4 spec test.
- [ ] **I13** (escrow ≥ Σ held; equal without stray deposits): `dvp_prop` (equality),
  `f003_escrow.rs` (stray deposits never released).
- [ ] **I14** (daily limit): `dvp_prop`, `f003_redeem.rs` (boundary, midnight rollover, first day).
- [ ] **I10, I11, I12** (F001/F002) still hold: `invariants_prop` (incl. the settlement variant),
  `f003_accrual.rs`, `f003_secondary.rs` (holder count after a full sale).
- [ ] **SC-008**: the rebuild from settlement records alone equals on-chain state
  (`f003_rebuild.rs`).

## 6. Atomicity and unwinds

- [ ] `settle` is the named Constitution IV exception: both legs (and the mint) in one
  instruction ([settle.rs:97](../../programs/dvp_settlement/src/instructions/settle.rs#L97)).
  Any failing part aborts the whole instruction (`a_failed_delivery_creates_nothing`).
- [ ] Cancel, expire and reject never move tokens; they earmark exactly what the instruction holds
  ([escrow.rs:21](../../programs/dvp_settlement/src/escrow.rs#L21); `units_held` / `cash_held` at
  [state.rs:88](../../programs/dvp_settlement/src/state.rs#L88)), so an unwind can't be blocked
  by an undeliverable return (research R3).
- [ ] A return that can't be delivered yet (suspended seller, pause, closed cash account) fails
  and keeps its earmark; it succeeds later (`an_undeliverable_return_keeps_its_earmark…`).

## 7. Layout and earlier features

- [ ] FundState size and every F001/F002 offset are unchanged; `units_at_day_start` comes from
  reserved bytes after `pending_override_nav` (`tests/tests/layouts.rs`).
- [ ] DvpConfig is 119 bytes and SettlementInstruction 333, as in the data model.
- [ ] New error variants were appended; `HookError::ClassNotAvailable` is retired, not removed, so
  codes kept their numbers.
- [ ] Every F001/F002 test edit is listed with its reason in [pr-notes.md](pr-notes.md)
  (Constitution XII).

## 8. Things the reviewer should know

- **Trusted parties**: the cash token's issuer (it can freeze accounts), the transfer agent (it
  funds redemptions it accepted) and custodians (they sign only what was approved); stated in the
  spec's Assumptions.
- **First-day rule**: no redemption can settle on the day a fund first has units (research R9).
  The devnet script shows it refused and settles it the next day.
- **Escrow interest**: units in escrow earn for the escrow's record, not the parties; F006
  decides what happens to it (research R16).
- **Not supported**: cash tokens with a transfer fee or hook (refused at setup); a seller whose
  holding account has the CPI guard on can't fund units (R5).
- **Test-only state edits** (each reaches a state no instruction can produce, and says so):
  - the cash escrow's balance lowered (`no_units_without_the_cash_in_escrow`);
  - FundState `cash_treasury` planted for a fee-bearing mint (`init_dvp_refuses_a_fee_bearing_cash_mint`).
- **Checks called directly** (unreachable through any transaction, because the caller has already
  checked the same account): `fund::…::check_settlement` and
  `compliance_hook::…::check_holder_accounts`, called with forged accounts.
- **Not yet done**: the devnet run (T080), which needs the go-ahead, devnet SOL and deployed
  programs. The script was rehearsed on a local validator, where 31 of 31 steps matched (the
  next-day `--continue` step is still to run).

## Sign-off

- Reviewer:
- Date:
- Commit reviewed:
- Outcome: ☐ approved ☐ changes requested
