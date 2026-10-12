# Contract: `governance` program (F001 instructions)

Checks run in the order listed; the first failure aborts the transaction. In F001 the record of a
compliance action is the event emitted by `compliance_hook` (shared-model I8). F007 adds an
`ActionRecord` to these same instructions without changing their arguments.

## `init_roles()`

Creates the role list before the Fund exists, and fixes who the issuer is (FR-002).

| Account | Kind | Constraint |
|---|---|---|
| `issuer` | signer, writable (payer) | becomes `role_registry.issuer` |
| `mint` | signer | the keypair that `fund::create_fund` will use. Its signature proves the caller controls that mint address, so nobody else can claim the issuer seat first (research R10) |
| `fund_state` | read (unchecked) | address = PDA `["fund", mint]` of fund; must not exist yet |
| `role_registry` | writable | new PDA `["roles", fund_state]` |
| `system_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | `fund_state` has the expected address and no data | `FundAlreadyExists` |
| 2 | `role_registry` does not exist | Anchor `init` failure (account in use) |

**Effect**: `RoleRegistry { issuer, fund_state, other roles = default }`. **Event**: `RolesInitialized`.

## `set_roles(fund_admin, transfer_agent, compliance, keeper, auditor: Pubkey)`

| Account | Kind | Constraint |
|---|---|---|
| `issuer` | signer | `== role_registry.issuer` |
| `role_registry` | writable | |
| `fund_state` | read | owner = fund; `== role_registry.fund_state` |

| # | Check | Error |
|---|---|---|
| 1 | signer is the issuer | `Unauthorized` |
| 2 | `fund_state.status == Setup` | `FundNotInSetup` |
| 3 | no role is the default pubkey | `RoleUnassigned` |
| 4 | keeper differs from every other role, including the issuer | `KeeperNotSeparate` |
| 5 | compliance differs from the issuer | `ComplianceIsIssuer` |

**Effect**: all five roles replaced. **Event**: `RolesAssigned`. May be called again while in Setup.

## Compliance actions

`suspend_holder`, `reinstate_holder`, `revoke_holder` take `(reason: u8, case_ref_hash: [u8; 32])`
and a target `holder_state`; `pause_transfers` and `unpause_transfers` take the same two arguments.

| Account | Kind | Constraint |
|---|---|---|
| `compliance` | signer | `== role_registry.compliance` (a Squads vault signs through Squads' execute) |
| `role_registry` | read | |
| `fund_state` | read | `== role_registry.fund_state` |
| `registry` | writable | compliance_hook `Registry` of this fund's mint |
| `holder_state` | writable | holder actions only |
| `hook_admin` | read | PDA `["hook-admin"]` of governance; signs the CPI |
| `compliance_hook_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | signer is the compliance role | `Unauthorized` |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | `reason ∈ {CourtOrder, SanctionsHit, LostKey, OperationalError, Other}` | `InvalidReason` |
| 4 | CPI `compliance_hook::set_status(action, reason, case_ref_hash, actor = compliance)` or `set_paused(paused, reason, case_ref_hash, actor)`, signed by `hook_admin`. The callee checks the transition | callee errors (see compliance_hook contract) |

Effects take hold from the next transaction, with no delay (FR-032; Constitution VIII:
protective and reversible).

## Not in F001

`propose_action` / `execute_action` / `cancel_action`, `commit_register_root` (F007),
`execute_enforcement` (F005), `record_action` (F007). The `["delegate", mint]` and
`["token-admin", mint]` PDAs sign nothing in F001.
