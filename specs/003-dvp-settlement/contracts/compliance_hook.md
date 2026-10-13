# Contract: `compliance_hook` changes (F003)

The hook's check order (shared-model §6) is **unchanged**. Transfers into and out of escrow are
ordinary transfers: the escrow is an Approved holder of class `Escrow`, never counted toward the
holder cap and exempt from the jurisdiction check (F001). New `HookError` variants are appended.

## `register_holder` / `approve_fund_account`: class `Escrow` enabled

F001 rejected `Escrow` with `ClassNotAvailable` (F001 research R17). From F003:

| Class | Owner must be | Else |
|---|---|---|
| `FundTreasury` | PDA `["treasury", fund_state]` of `fund` | `NotSystemControlled` |
| `Escrow` | PDA `["escrow", fund_state]` of `dvp_settlement` (`DVP_SETTLEMENT_PROGRAM_ID`) | `NotSystemControlled` |
| `CollateralVault` | — | `ClassNotAvailable` (backlog) |

Only compliance may register either class (F001 FR-013). Approval sets `kyc_expiry_ts = i64::MAX`
and zero jurisdiction, as for the treasury. Tests: escrow registration by a non-compliance signer,
with a wrong owner, and with another fund's escrow PDA, each rejected; F001's
`ClassNotAvailable` test for Escrow is replaced by these (listed in the PR, Principle XII).

## `accrue(post_balance: u64)`: FundState PDA only (CPI from `fund`)

Settles a holder's interest before a mint or burn changes its balance (F002 FR-013, R8)
**[SM change: signature]**.

| Account | Kind | Constraint |
|---|---|---|
| `fund_authority` | signer | `== registry.fund_state` |
| `registry` | writable | PDA `["registry", mint]` |
| `fund_state` | read | `== registry.fund_state`, via `FundStateView` |
| `holder_state` | writable | PDA `["holder", registry.mint, holding_account.owner]` |
| `holding_account` | read | Token-2022 account of `registry.mint`; the owner's ATA |

| # | Check | Error |
|---|---|---|
| 1 | signer is `registry.fund_state` | `UnauthorizedCaller` (exists) |
| 2 | FundState loads at `registry.fund_state` | `WrongMint` |
| 3 | `holder_state` is the PDA for `holding_account.owner`; `holding_account` is that owner's ATA for this mint | `WrongHolder` **(new)** |
| 4 | `holder_state.class == Escrow` (the only class F003 mints to or burns from) | `ClassNotAccrued` **(new)** |
| 5 | interest product fits | `Overflow` |

**Effect**:
- `pre = holding_account.amount`
- `settle(h, pre, fund.accrual_index)`
- `last_balance = post_balance`; `last_slot = slot`

No `holder_count` change: only the escrow (never counted) is accepted. F006, which mints
reinvested units to investors, widens the class check and adds the `holder_count` update together
with its tests (Constitution I: no untestable branch ships early).

**Event**: `Accrued { owner, pre_balance, post_balance, accrual_index, accrued_e18 }` via `emit!`
(not `emit_cpi!`, to stay within the call-depth limit, R15).

**Tests**:
- called directly by any key: `UnauthorizedCaller`;
- the escrow's interest, `last_balance` and `earned_now` after a mint and after a burn match the
  reference model (I11), including a publish between funding and a redemption burn;
- `WrongHolder` and `ClassNotAccrued` can only be reached through `fund`. They are covered by
  unit tests of the account checks and by a `fund` test that passes an investor's HolderState as
  `escrow_holder` (rejected by `fund`'s own escrow address check first, so the hook check is
  defence in depth and unit-tested).
