# Data Model: Daily Price and Interest Accrual (F002)

This file maps the spec's entities to on-chain accounts in `docs/shared-model.md`. It lists only
what F002 creates, fills or changes; everything else is as in
[F001's data model](../001-fund-launch-onboarding/data-model.md). Changes to the shared model are
marked **[SM change]** and listed in [plan.md](plan.md#shared-model-changes).

| Spec entity | Account | Program | PDA seeds |
|---|---|---|---|
| Fund (extended) | `FundState` | `fund` | `["fund", mint]` |
| Holder record (extended) | `HolderState` | `compliance_hook` | `["holder", mint, owner]` |
| Price record | `NavRecord` (new) | `fund` | `["nav", fund_state, seq.to_le_bytes()]` |
| Global interest counter | `FundState.accrual_index` | `fund` | — |
| Price override | `FundState.pending_override_nav` | `fund` | set through governance PDA `["fund-caller"]` **[SM change]** |

## Units and formulas

From shared-model §1, unchanged. The formulas live in `mmf-common::accrual` (research R11).

| Quantity | Encoding | Example |
|---|---|---|
| Price | `nav: u64`, cash per whole unit × 10^8 | $1.00 = `100_000_000` |
| Daily rate | `daily_rate_e12: u64`, units earned per unit per day × 10^12 | 4.5 %/yr ≈ `123_287_671` |
| Interest counter | `accrual_index: u128`, interest per base unit × 10^18 since the first publish | — |
| Earned interest | `accrued_e18: u128`, base units × 10^18 | 1 base unit = `10^18` |
| Day | `u32`, `unix_timestamp.div_euclid(86_400)` (UTC day number, R2) | 2026-10-13 = `20_739` |

```text
index_increment(rate, days) = rate × 10^6 × days                                  exact, checked u128
settle(h, pre_balance, index):
    h.accrued_e18        += pre_balance × (index − h.index_at_last_touch)        exact, checked u128
    h.index_at_last_touch = index
earned_now(h, fund)      = h.accrued_e18 + h.last_balance × (fund.accrual_index − h.index_at_last_touch)
price_move_ok(prev, nav, bps) = |nav − prev| × 10_000 ≤ prev × bps               exact (cross-multiplied)
utc_day(ts)              = ts.div_euclid(86_400)
```

New shared-model §8 rows **[SM change]**: index increment, exact; price-move check, exact with no
division. The existing row "Interest accrued" is unchanged.

## `FundState` (fund) — fields F002 fills

```text
version, mint, role_registry, terms, status         [F001] unchanged
nav                    u64      [F002]  latest published price (8 dp); 0 before the first publish
nav_ts                 i64      [F002]  unix time of the latest publish
nav_seq                u64      [F002]  seq of the latest NavRecord; 0 = none
accrual_index          u128     [F002]  += index_increment on every publish; never decreases
daily_rate_e12         u64      [F002]  latest published rate
units_outstanding … current_cycle                   [F003, F006] zero
bump                   u8
nav_period_end_day     u32      [F002]  last UTC day covered by any publish; 0 = none   [SM change: from _reserved]
pending_override_nav   u64      [F002]  price allowed by an approved override; 0 = none   [SM change: from _reserved]
_reserved              [u8; 52]          was [u8; 64]                                   [SM change]
```

The two new fields come **after** `bump`, so every F001 offset is unchanged and existing funds
decode as "no publish, no override" (research R10). `FundStateView` (mmf-common) additionally
decodes `accrual_index`, which the hook reads. `tests/tests/layouts.rs` checks the offset.

**Validation** (`publish_daily`, in contract order, see [contracts/fund.md](contracts/fund.md)):

| Rule | Error |
|---|---|
| `role_registry` is governance's registry bound to this fund | `RoleRegistryMismatch` |
| signer == `role_registry.fund_admin` | `Unauthorized` |
| `status == Open` | `FundNotOpen` |
| `nav > 0` | `ZeroPrice` |
| `daily_rate_e12 ≥ 0` (parameter is `i64`, R6) | `NegativeRate` |
| `days_covered ≥ 1` | `ZeroDaysCovered` |
| `start ≤ today`, where `start = today` if `nav_seq == 0`, else `nav_period_end_day + 1` | `PeriodNotStarted` |
| `price_move_ok(prev, nav, terms.max_nav_move_bps) ∨ pending_override_nav == nav`, where `prev = nav_seq > 0 ? nav : terms.target_nav` | `PriceMoveTooLarge` |
| period end, `nav_seq + 1` and the new index fit their types | `Overflow` |

**Transitions**: F002 adds no status. Publishing and overrides require `Open`; Setup rejects them.

```text
publish_daily [fund_admin]:
  nav_seq += 1; nav, nav_ts = now, daily_rate_e12 = rate
  accrual_index += rate × 10^6 × days
  nav_period_end_day = start + days − 1
  override_used = (pending_override_nav == nav && !price_move_ok(...)); pending_override_nav = 0
set_nav_override   [governance PDA "fund-caller"]: 0 → nav     (OverrideAlreadyPending if ≠ 0)
clear_nav_override [governance PDA "fund-caller"]: nav → 0     (NoOverridePending if == 0)
```

`override_used` is true only when the override was needed. A price within the limit that happens
to equal the override is recorded as an ordinary publish. The override is still cleared.

## `NavRecord` (fund) — new **[SM change: fields added]**

PDA `["nav", fund_state, seq.to_le_bytes()]`. Created by `publish_daily`, paid by a separate
`payer` signer, **never modified or closed** (FR-003). No `_reserved`, since it is immutable
(research R9).

```text
version           u8       1
seq               u64      1, 2, 3, … (FundState.nav_seq after this publish)
nav               u64      price (8 dp)
daily_rate_e12    u64      rate as published
days_covered      u16      ≥ 1                                       [SM change: new]
period_start_day  u32      first UTC day this record pays for        [SM change: new]
index_after       u128     FundState.accrual_index after this publish
as_of_ts          i64      Clock unix_timestamp at publish
published_by      Pubkey   the fund admin that signed
override_used     bool     the price passed only by an approved override   [SM change: new]
bump              u8
```

Size: 8 (discriminator) + 1 + 8 + 8 + 8 + 2 + 4 + 16 + 8 + 32 + 1 + 1 = **97 bytes**.

Any past price can be looked up from `seq` alone (SC-005). The days a record pays for are
`period_start_day ..= period_start_day + days_covered − 1`. Records with consecutive `seq` have
contiguous, non-overlapping periods (FR-009).

## `HolderState` (compliance_hook) — fields F002 fills

No layout change. F002 starts writing two existing fields:

```text
index_at_last_touch   u128   [F002]  FundState.accrual_index at this holder's last settlement
accrued_e18           u128   [F002]  interest earned, not yet paid, base units × 10^18 (exact)
last_balance          u64    [F001]  balance at the last hook touch; the balance settled on
```

Records registered after a publish start with `index_at_last_touch = 0` and `last_balance = 0`.
Their first transfer in settles on a pre-balance of 0, then sets `index_at_last_touch` to the
current index, so they never earn for earlier publishes (FR-016).

## Hook step 8 (shared-model §6)

Runs after steps 0–7 pass and before step 9 (see
[contracts/compliance_hook.md](contracts/compliance_hook.md)):

```text
index   = FundStateView::load(fund_state, registry.fund_state).accrual_index   (failure → WrongMint)
pre_src = post_src + amount;  pre_dst = post_dst − amount                       (checked → Overflow)
if same owner:  settle(src, post_src + post_dst, index)                         once
else:           settle(src, pre_src, index); settle(dst, pre_dst, index)
```

## Events

| Event | Emitted by | Payload (besides `fund`, `slot`, `unix_ts`) |
|---|---|---|
| `NavPublished` | fund `publish_daily` (`emit_cpi!`) | seq, nav, daily_rate_e12, days_covered, period_start_day, index_after, publisher, override_used, override_cleared_nav |
| `NavOverrideApproved` **[SM change: new]** | fund `set_nav_override` (`emit_cpi!`) | nav, reason, case_ref_hash, actor (compliance key) |
| `NavOverrideWithdrawn` **[SM change: new]** | fund `clear_nav_override` (`emit_cpi!`) | nav (the one withdrawn), reason, case_ref_hash, actor |
| `TransferValidated` **[SM change: payload]** | hook `execute` (`emit!`) | F001 fields + `accrual_index`, `source_accrued_e18`, `destination_accrued_e18` |

`Accrued` stays reserved for `compliance_hook::accrue`, which moves to F003 (research R8).
`override_cleared_nav` is the pending override that this publish cleared (0 if none), so an
unused override's end is recorded too.

## Invariants

| ID | Invariant | Where tested |
|---|---|---|
| I3, I10 | from F001, unchanged; still checked with publishes interleaved | `invariants_prop.rs`, `accrual_prop.rs` |
| I11 **[SM change: new]** | For every holder, `earned_now(h, fund)` equals the reference sum over publishes of (units held at that publish × rate × 10^6 × days covered) | `accrual_prop.rs` (SC-001, SC-007) |
| I12 **[SM change: new]** | `FundState.accrual_index == NavRecord(nav_seq).index_after`, and each record's `index_after − previous.index_after == rate × 10^6 × days`. `nav_period_end_day == latest.period_start_day + latest.days_covered − 1`, and `period_start_day(seq + 1) == period_end(seq) + 1`. For every holder, `index_at_last_touch ≤ accrual_index` | `accrual_prop.rs`, `f002_publish.rs` |

SC-002 (no interest lost or created) follows from I11 summed over holders. The test asserts the
sum explicitly as well.
