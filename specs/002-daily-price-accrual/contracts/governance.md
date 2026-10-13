# Contract: `governance` program (F002 instructions)

Same pattern as F001's compliance actions: compliance signs, governance checks, then governance
CPIs into the target program with one of its own PDAs. In F002 the record of an override is the
event emitted by `fund` (shared-model I8). F007 adds an `ActionRecord` of kind `NavOverride` (14)
to these same instructions without changing their arguments.

## `approve_nav_override(nav: u64, reason: u8, case_ref_hash: [u8; 32])`

## `withdraw_nav_override(reason: u8, case_ref_hash: [u8; 32])`

| Account | Kind | Constraint |
|---|---|---|
| `compliance` | signer | `== role_registry.compliance` (a Squads vault signs through Squads' execute) |
| `role_registry` | read | governance `RoleRegistry` |
| `fund_state` | writable | `== role_registry.fund_state`; decoded with `FundStateView` |
| `fund_caller` | read | PDA `["fund-caller"]` of governance; signs the CPI **[SM change: new PDA]** |
| `fund_event_authority` | read | fund's `["__event_authority"]` PDA |
| `fund_program` | program | fund |

| # | Check | Error |
|---|---|---|
| 1 | signer is the compliance role | `Unauthorized` |
| 2 | fund exists and `status == Open` | `FundNotOpen` |
| 3 | `reason ∈ {OperationalError (3), Other (6)}` | `InvalidReason` |

Then CPI `fund::set_nav_override(nav, reason, case_ref_hash, compliance)` or
`fund::clear_nav_override(reason, case_ref_hash, compliance)`, signed by `["fund-caller"]`, which
runs its own checks ([fund.md](fund.md)).

**Why the fund binding matters**: the `["fund-caller"]` PDA is shared by every fund.
`fund_state == role_registry.fund_state`, together with the role registry being governance-owned
at `["roles", fund_state]` (Anchor account check), ensures that one fund's compliance can't set
another fund's override. A test covers this with two funds.

**Call depth**: Squads execute → governance → fund → `emit_cpi` = 4 (F001 research R7).

**Reasons**: a new helper `ReasonCode::allowed_for_nav_override()` in `mmf-common`. CourtOrder,
SanctionsHit, LostKey, CollateralDefault and IssuerWindDown are rejected; there is one test per
code.

**Rejection tests**:
- each error;
- the fund admin, issuer and keeper calling the instruction get `Unauthorized`;
- under a Squads 2-of-3 compliance vault, one approval leaves no override and two approvals set
  it;
- compliance of fund B targeting fund A gets `Unauthorized` or an Anchor constraint error,
  asserted exactly.

## New cross-program edge **[SM change: §5.2]**

| Caller | Callee instruction | Callee checks |
|---|---|---|
| `governance` | `fund::set_nav_override` / `clear_nav_override` | signer = governance PDA `["fund-caller"]` |

`governance` gains a dependency on the `fund` crate (`features = ["cpi", "no-entrypoint"]`). `fund`
doesn't depend on `governance`, so there is no cycle (F001 research R12).
