# Contract: `governance` changes (F003)

## `reject_settlement(reason: u8, case_ref_hash: [u8; 32])`: compliance

Compliance rejects an open settlement instruction (FR-025, R11). It follows the pattern of F001's
compliance actions and F002's `approve_nav_override`: governance checks the role, the fund
binding and the reason, then CPIs into the owning program with a governance PDA. Rejection is
protective and reversible in effect (every funded leg is returned), so it takes effect
immediately (Constitution VIII). Its permanent record is dvp's `SettlementRejected` event until
F007 adds the `ActionRecord` (kind `RejectSettlement`, already in shared-model §4.5).

| Account | Kind | Constraint |
|---|---|---|
| `compliance` | signer | `== role_registry.compliance` (Squads vault) |
| `role_registry` | read | `RoleRegistry` |
| `fund_state` | read | `address = role_registry.fund_state` |
| `settlement` | writable | passed through; dvp checks it |
| `dvp_caller` | read | PDA `["dvp-caller"]` of governance **[SM change: new PDA]**; signs the CPI |
| `dvp_event_authority`, `dvp_settlement_program` | | |

| # | Check | Error |
|---|---|---|
| 1 | `compliance == role_registry.compliance` | `Unauthorized` |
| 2 | fund (via `FundStateView`) is Open | `FundNotOpen` |
| 3 | `ReasonCode::try_from(reason)` is allowed (`allowed_in_f001`: CourtOrder, SanctionsHit, LostKey, OperationalError, Other) | `InvalidReason` |
| — | dvp `reject` checks (`InstructionFinal`, `FundMismatch`) | callee errors |

**Effect**: CPI `dvp_settlement::reject(reason, case_ref_hash, actor = compliance)` signed by
`["dvp-caller"]`. Governance emits no event of its own. The record is dvp's `SettlementRejected`,
the same arrangement as F002's override events.

**New dependency**: `governance` depends on the `dvp_settlement` crate (`cpi`, `no-entrypoint`).

**Rejection tests**:
- signer is the issuer, fund admin, transfer agent, keeper, auditor or an investor;
- reasons 4 and 5, and 7 (out of range);
- the settlement belongs to another fund;
- the instruction is already Settled, Cancelled, Expired or Rejected;
- `dvp_settlement::reject` called directly by compliance (not through governance) gets
  `UnauthorizedCaller`.
