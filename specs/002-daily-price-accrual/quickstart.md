# Quickstart: validating F002

How to prove F002 works: local program tests first, then a run on devnet. Instruction details are
in [contracts/](contracts/), layouts and formulas in [data-model.md](data-model.md), and design
decisions in [research.md](research.md). Prerequisites and fixtures are unchanged from
[F001's quickstart](../001-fund-launch-onboarding/quickstart.md#prerequisites).

## Build and test

```bash
anchor build                       # fund, compliance_hook, governance
cargo test -p tests                # full suite: F001 and F002 (Constitution XII)
cargo test -p tests --release --test scale -- --ignored --nocapture   # SC-003, SC-004 (and F001 SC-006)
cargo test -p mmf-common           # accrual formulas, utc_day, price_move_ok, reason helper
scripts/check-error-coverage.sh    # SC-006: every error variant, incl. F002's, is asserted
```

**Expected**: everything green, including every F001 test unchanged. The only F001 test edits
allowed are the `TransferValidated` payload decoder, the `FundState` layout check and the
`seed_units` accrual bookkeeping (research R12). The pull request lists them (Constitution XII).

## Requirement → test map

Every row has at least one rejection test asserting the exact error (Constitution VI). Files are
under `tests/tests/`.

| Requirement | Test file | What it shows / rejections asserted |
|---|---|---|
| FR-001 | `f002_publish.rs` | `Unauthorized` from the issuer, compliance, keeper, transfer agent and a stranger; `FundNotOpen` in Setup; `RoleRegistryMismatch` |
| FR-002, FR-003, FR-004, SC-005 | `f002_publish.rs` | three publishes read back by seq with every field; FundState mirrors the latest; records unchanged after 90 more publishes; no instruction can write or close a NavRecord (create at an existing seq fails) |
| FR-005 | `mmf-common` unit tests | $1.00 = `100_000_000`; 4.5 %/yr = `123_287_671` round-trips with no change |
| FR-006 | `f002_publish.rs` | `ZeroPrice`; `NegativeRate` (−1, `i64::MIN`, and raw `u64::MAX` bytes) |
| FR-007 | `f002_publish.rs` | `PriceMoveTooLarge` just beyond the limit (up and down); exactly at the limit accepted; the first publish measured from the target price |
| FR-008 | `f002_override.rs` | override → publish at that price accepted, `override_used = true`, override cleared; publish at another price → `PriceMoveTooLarge`, and the override is still cleared; `OverrideAlreadyPending`; `NoOverridePending`; withdraw then publish → rejected; `InvalidReason` for codes 0, 1, 2, 4, 5, 7; `Unauthorized` from the fund admin, issuer and keeper; `UnauthorizedCaller` calling `set_nav_override` / `clear_nav_override` directly; Squads 2-of-3 compliance; cross-fund attempt rejected |
| FR-009, FR-009a | `f002_publish.rs` | `ZeroDaysCovered`; `PeriodNotStarted` (second publish on a covered day, the day after a 3-day publish); Friday 3-day then Monday; catch-up after an outage of 2 days; periods contiguous (I12) |
| FR-009b | `f002_publish.rs` | rate `i64::MAX` and `days_covered = u16::MAX` accepted (no cap). `Overflow` is unreachable through instructions (it would take about 5.6 × 10^8 maximum publishes), so it is reached by patching `FundState.accrual_index` near `u128::MAX` and, separately, `nav_seq = u64::MAX`, with state checked unchanged afterwards |
| FR-010, FR-011, SC-003 | `scale.rs` | publish compute units and account list identical with 10 and 10 000 holders; no HolderState changes after a publish |
| FR-012–FR-014, US2 1/2/4 | `f002_accrual.rs` | the spec's independent test: A holds 1 000 000 units for 10 publishes, sends half to B, then 10 more publishes with a different rate each; A and B equal a hand-computed table exactly |
| FR-013 ordering | `f002_accrual.rs` | transfer then publish → the receiver earns it; publish then transfer → the sender earns it; same slot both ways |
| FR-015 | `f002_accrual.rs` | suspended, KYC-expired and paused periods all accrue; fund treasury accrues |
| FR-016, US2 5 | `f002_accrual.rs` | a holder registered after 5 publishes earns nothing for them |
| FR-017 | `f002_accrual.rs` | SDK `earned_e18`, read without any transaction, equals the expected amount. It also equals the stored `accrued_e18` after a 0-unit transfer from that holder to another approved holder, which runs the hook and settles both |
| FR-018, SC-004 | `scale.rs` | hook CU identical for a holder last settled 1 vs 90 publishes ago, and with 10 vs 10 000 holders; ≤ 60 000 CU |
| FR-019 | `f002_accrual.rs`, `f002_publish.rs` | hook `Overflow` (patched `index_at_last_touch`, see [contracts/compliance_hook.md](contracts/compliance_hook.md)); publish `Overflow` (patched FundState); state unchanged after each. These and the forged-FundState test are F002's only `set_account` patches, each for a condition no instruction can reach |
| FR-020, FR-021, SC-007 | `f002_accrual.rs`, `accrual_prop.rs` | `NavPublished`, `NavOverrideApproved`, `NavOverrideWithdrawn` emitted with full payloads; every holder's earned interest rebuilt from NavRecords + `TransferValidated` events alone equals the stored figure |
| Hook step 8 | `hook_order.rs` | a failing transfer settles nothing; `WrongMint` for a foreign or forged FundState |
| SC-001, SC-002 (I11, I12) | `accrual_prop.rs` | see below |
| I3, I10 | `invariants_prop.rs` | F001's randomized suite, now with `Publish` operations interleaved |

## Randomized accrual check (`accrual_prop.rs`)

`proptest`, 64 cases by default (`PROPTEST_CASES` to raise). Each case:
- covers 90 days with 6 investors and the fund treasury;
- has 0–4 random transfers between publishes, plus suspend/reinstate and pause/unpause;
- uses a random rate each publish, including 0, and random `days_covered` (mostly 1, with Fridays
  at 3 and occasional 2-day outages caught up);
- seeds units only through `seed_units` (research R12).

After every step it checks:
- **SC-001 / I11**: each holder's `earned_e18` (stored + pending) equals the reference model's
  day-by-day sum exactly.
- **SC-002**: the sum over holders equals Σ over publishes (units held × rate × 10^6 × days).
- **I12**: index and period continuity against the NavRecords.
- **I3, I10**: still hold.

The reference model lives in `tests/src/model.rs` and doesn't use `mmf-common::accrual`
(research R11).

## Devnet run (research R13)

`scripts/devnet-e2e`, extended. It runs after the F001 steps on a new fund (`max_nav_move_bps` =
50):

1. `publish_daily(0.99, 4.5 %/yr, 1)` by the fund admin → **fails `PriceMoveTooLarge`** (1 % from
   the $1.00 target).
2. `publish_daily` by holder A's key → **fails `Unauthorized`**.
3. Compliance (Squads) `approve_nav_override(0.99, Other, case)` → `NavOverrideApproved`.
4. `publish_daily(0.99, 4.5 %/yr, 1)` → **succeeds**, record 1 with `override_used = true`.
5. `publish_daily(0.99, 4.5 %/yr, 1)` again → **fails `PeriodNotStarted`**.
6. Approve then withdraw an override → `NavOverrideWithdrawn`.
7. Read NavRecord 1 and print it.

`cargo run -p devnet-e2e -- --continue <fund>` on each of the next two days publishes the next
record (days covered = 1, or 3 on a Friday) and prints the full history: three records with
contiguous periods. Units, and so interest, can't exist on devnet before F003. Accrual with units
is proved in LiteSVM.

**Rehearse locally first**: same as F001 (local validator with the three programs and Token-2022
v11). The local validator's clock follows wall time, so `--continue` behaves the same as on devnet.
