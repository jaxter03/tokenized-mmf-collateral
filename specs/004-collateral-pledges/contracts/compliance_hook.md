# Contract: `compliance_hook` changes (F004)

Three changes: a new CPI-only instruction `set_lien`, step 5 of the transfer hook, and
`init_registry` filling `collateral_program`. New `HookError` variants are appended, so earlier
codes keep their numbers: `PledgedUnitsLocked`, `InsufficientUnlockedUnits`,
`NotCollateralCaller`, `LienUnderflow`.

## `set_lien(change: LienChange)`: `collateral` only (signer = its `["hook-caller"]` PDA)

```text
LienChange { lock: u64, unlock: u64, opened: u16, closed: u16 }
```

| Account | Kind | Constraint |
|---|---|---|
| `hook_caller` | signer | PDA `["hook-caller"]` of `registry.collateral_program` |
| `registry` | read | Registry `["registry", mint]` |
| `holder_state` | writable | the owner's HolderState; loaded and stored by hand |
| `holding_account` | read | the owner's Token-2022 ATA for `registry.mint` |

| # | Check | Error |
|---|---|---|
| 1 | `registry.collateral_program ≠ default` and `hook_caller` is its `["hook-caller"]` PDA and signed | `NotCollateralCaller` |
| 2 | `holding_account` is a Token-2022 account of `registry.mint` and is the ATA of its owner; `holder_state` is that owner's PDA, owned by this program | `WrongHolder` |
| 3 | `new = pledged_amount + lock − unlock` without underflow (checked `u128`, then fits `u64`) | `LienUnderflow` / `Overflow` |
| 4 | if `lock > unlock`: `holding_account.amount ≥ new` (I2) | `InsufficientUnlockedUnits` |
| 5 | `active_liens + opened − closed` without underflow or overflow | `LienUnderflow` / `Overflow` |

**Effect**: `pledged_amount = new`, `active_liens` updated. Nothing else changes: no balance moves
and interest is untouched (R13). Holder status and `transfers_paused` aren't read: `collateral`
checks eligibility where the spec requires it, and locks work while paused (R11, R12).
**Event**: `LienSet` (`emit!`, as `accrue` does).

**Rejection tests**:
- signer is any key, a governance PDA, the FundState PDA, or a `["hook-caller"]` of another
  program;
- the Registry was created with an empty `collateral_program`;
- the holder record or holding account of another owner, or of another fund's mint;
- the lock grows beyond the balance by 1 base unit (and exactly equal passes);
- the unlock exceeds the lock;
- the closed count exceeds `active_liens`.

## Transfer hook `execute`: step 5

Inserted between steps 4 and 6 (shared-model §6, R3):

```text
5. post_src_balance ≥ src.pledged_amount              → PledgedUnitsLocked
```

It applies to every transfer, because the enforcement bypass (step 2) arrives with F005. When
source and destination belong to the same owner, the source account's post-balance is used (fail
closed; an owner has one usable account).

| Case | Expected |
|---|---|
| sender with 2 000 000 units, 1 020 408.163266 locked, sends 979 591.836735 | `PledgedUnitsLocked` |
| same sender sends exactly 979 591.836734 | succeeds, lock unchanged, balance = lock |
| sender fully locked sends 0 units | succeeds (post-balance = lock; zero transfers exercise every check) |
| suspended sender with locked units | `SenderNotApproved` (step 4 first) |
| transfers paused, sender with locked units | `TransfersPaused` (step 3 first) |
| locked sender sends to an unapproved receiver, over the lock | `PledgedUnitsLocked` (step 5 before 6) |
| owner-approved delegate transfers over the lock | `PledgedUnitsLocked` |
| F003 redemption or secondary funding over the lock | `PledgedUnitsLocked` (F003 US2 scenario 3) |

`hook_order.rs` gains the step-5 rows. Compute: one comparison, no new account.

## `init_registry`: fills `collateral_program`

`reg.collateral_program = mmf_common::ids::COLLATERAL_PROGRAM_ID`, next to the existing
`reg.governance = GOVERNANCE_PROGRAM_ID`. No new check, no new account. A new assertion in
`f004_lien.rs` checks the value on a freshly created fund; no earlier test reads the field (R2,
R16).

## Views

`mmf_common::views::FundStateView` gains `nav_ts` (offset `FUND_NAV + 8`). `HolderStateView`
already exposes status, class and KYC expiry; `collateral` reads nothing else from the record.
