# Contract: `governance` changes (F004)

## `resolve_call_dispute(outcome: u8, reason: u8, case_ref_hash: [u8; 32])`: compliance

Compliance ends a dispute on a delivery call, either reopening the call or cancelling it (spec
FR-032, R10). It follows F003's `reject_settlement`: governance checks the role, the fund binding
and the reason, then CPIs into the owning program with a governance PDA. Its permanent record is
collateral's `DisputeResolved` event until F007 adds the `ActionRecord` (kind 15
`ResolveDispute`, reserved now in shared-model §4.5).

Is it a destructive power (Constitution VIII)? No. It moves no units and changes no rules: it
restores or withdraws a demand that the parties' own agreement created. So it takes effect
immediately, like a reject.

| Account | Kind | Constraint |
|---|---|---|
| `compliance` | signer | `== role_registry.compliance` (Squads vault) |
| `role_registry` | read | `RoleRegistry` |
| `fund_state` | read | `address = role_registry.fund_state` |
| `agreement`, `call` | writable | passed through; collateral checks them |
| `collateral_caller` | read | PDA `["collateral-caller"]` of governance **[SM change: new PDA]**; signs the CPI |
| `collateral_event_authority`, `collateral_program` | | |

| # | Check | Error |
|---|---|---|
| 1 | `compliance == role_registry.compliance` | `Unauthorized` |
| 2 | fund (via `FundStateView`) is Open | `FundNotOpen` |
| 3 | `ReasonCode::try_from(reason)` is allowed (`allowed_in_f001`: CourtOrder, SanctionsHit, LostKey, OperationalError, Other) | `InvalidReason` |
| — | collateral `resolve_dispute` checks (`FundMismatch`, `InvalidOutcome`, `NotDisputed`) | callee errors |

**Effect**: CPI `collateral::resolve_dispute(outcome, reason, case_ref_hash, actor = compliance)`
signed by `["collateral-caller"]`. Governance emits no event of its own.

**New dependency**: `governance` depends on the `collateral` crate (`cpi`, `no-entrypoint`).
`collateral` doesn't depend on `governance`; it checks the caller PDA from
`mmf_common::ids::GOVERNANCE_PROGRAM_ID`.

**Rejection tests**:
- signer is the issuer, fund admin, transfer agent, keeper, auditor, the giver or the taker;
- reasons 4 and 5, and 7 (out of range);
- outcome 1 (Met) or 3 (Defaulted);
- the call is Open, Met (stored or derived) or Cancelled;
- the call belongs to an agreement of another fund;
- `collateral::resolve_dispute` called directly by compliance gets `UnauthorizedCaller`.
