# Contract: `fund` program (F002 instructions)

Checks run in the order listed. The first failing check aborts the transaction with that error
(Constitution VII), and every instruction emits exactly one event (shared-model §1). F001's
`create_fund` and `launch` are unchanged. New error variants are **appended** to `FundError`, so
F001's error codes keep their numbers. The message of `Unauthorized` becomes "Signer does not hold
the required role"; the code is unchanged.

## `publish_daily(nav: u64, daily_rate_e12: i64, days_covered: u16)`

Publishes the day's price and rate, creates the next `NavRecord` and moves the interest counter
(FR-001–FR-011). One business transition: "publish a price". Clearing a pending override is
bookkeeping within it (Constitution IV).

| Account | Kind | Constraint |
|---|---|---|
| `fund_admin` | signer | `== role_registry.fund_admin` (may be a Squads vault) |
| `payer` | signer, writable | pays the NavRecord rent; any key |
| `fund_state` | writable | PDA `["fund", mint]` of fund |
| `role_registry` | read | owner = governance; address `== fund_state.role_registry` |
| `nav_record` | writable | new PDA `["nav", fund_state, (fund_state.nav_seq + 1).to_le_bytes()]`; a wrong address fails Anchor's seeds check |
| `system_program` | program | |
| `event_authority`, `program` | `#[event_cpi]` | |

| # | Check | Error |
|---|---|---|
| 1 | `role_registry` is owned by governance, at `fund_state.role_registry` | `RoleRegistryMismatch` |
| 2 | `fund_admin == role_registry.fund_admin` | `Unauthorized` |
| 3 | `fund_state.status == Open` | `FundNotOpen` **(new)** |
| 4 | `nav > 0` | `ZeroPrice` **(new)** |
| 5 | `daily_rate_e12 ≥ 0` | `NegativeRate` **(new)** |
| 6 | `days_covered ≥ 1` | `ZeroDaysCovered` **(new)** |
| 7 | `start ≤ utc_day(now)`, where `start = utc_day(now)` if `nav_seq == 0`, else `nav_period_end_day + 1` | `PeriodNotStarted` **(new)** |
| 8 | `price_move_ok(prev, nav, max_nav_move_bps)` or `pending_override_nav == nav`, where `prev = nav` of the fund if `nav_seq > 0`, else `terms.target_nav` | `PriceMoveTooLarge` **(new)** |
| 9 | `start + days_covered − 1`, `nav_seq + 1` and `accrual_index + rate × 10^6 × days` fit their types | `Overflow` **(new)** |

**Effect** (see [data-model.md](../data-model.md#fundstate-fund--fields-f002-fills)):
- `NavRecord { seq, nav, rate, days_covered, period_start_day: start, index_after, as_of_ts: now,
  published_by: fund_admin, override_used }` is created.
- FundState's `nav`, `nav_ts`, `nav_seq`, `daily_rate_e12`, `accrual_index` and
  `nav_period_end_day` are updated.
- `pending_override_nav` is set to 0.
- No `HolderState` is read or written (FR-010). The instruction takes the same accounts whatever
  the number of holders (FR-011).

**Event**: `NavPublished`.

**Rejection tests**: one per error. Also:
- the move limit is applied to the target price on the first publish;
- a move exactly at the limit is accepted;
- an override at a different price doesn't help, and is still cleared;
- a second publish on a covered day is rejected;
- a Friday 3-day publish, then Monday's;
- catch-up after a 2-day outage;
- a 0 rate leaves the index unchanged;
- a hand-built instruction carrying `−1` (i.e. `u64::MAX` bytes) gets `NegativeRate`;
- a publish from the issuer, compliance, keeper and transfer agent each gets `Unauthorized`.

## `set_nav_override(nav: u64, reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)`

Called only by governance (`approve_nav_override`). It records the one price the next publish may
use beyond the move limit (FR-008).

| Account | Kind | Constraint |
|---|---|---|
| `fund_caller` | signer | PDA `["fund-caller"]` of **governance** **[SM change: new PDA]** |
| `fund_state` | writable | |
| `event_authority`, `program` | `#[event_cpi]` | |

| # | Check | Error |
|---|---|---|
| 1 | signer is governance's `["fund-caller"]` PDA | `UnauthorizedCaller` **(new)** |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | `nav > 0` | `ZeroPrice` |
| 4 | `pending_override_nav == 0` | `OverrideAlreadyPending` **(new)** |

**Effect**: `pending_override_nav = nav`. **Event**: `NavOverrideApproved { nav, reason,
case_ref_hash, actor }`. Governance has already checked the role, the fund binding and the reason;
`fund` checks only its own state (defence in depth for status).

## `clear_nav_override(reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)`

Called only by governance (`withdraw_nav_override`).

| # | Check | Error |
|---|---|---|
| 1 | signer is governance's `["fund-caller"]` PDA | `UnauthorizedCaller` |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | `pending_override_nav != 0` | `NoOverridePending` **(new)** |

**Effect**: `pending_override_nav = 0`. **Event**: `NavOverrideWithdrawn { nav: withdrawn value,
reason, case_ref_hash, actor }`.

**Rejection tests**: calling either instruction directly with any other signer gets
`UnauthorizedCaller` (Constitution II: submitted directly, no SDK path).

## Not in F002

`compliance_hook::accrue` and the `fund → accrue` CPI before mint and burn move to F003
(research R8). `fund` still has no instruction that mints or burns.
