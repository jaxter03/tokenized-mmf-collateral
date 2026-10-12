# Contract: `compliance_hook` program (F001 instructions)

Checks run in the order listed; the first failure aborts the transaction (Constitution VII).
`compliance_hook` reads `FundState` (fund) and `RoleRegistry` (governance) through read-only views
that check the owning program, the address and the account discriminator (research R12).

## Setup (issuer, fund in Setup)

### `init_registry()`

Accounts: `issuer` (signer, payer), `mint`, `fund_state`, `role_registry`, `registry` (new PDA
`["registry", mint]`), `extra_account_meta_list` (new PDA `["extra-account-metas", mint]`),
`system_program`.

| # | Check | Error |
|---|---|---|
| 1 | signer `== role_registry.issuer` | `Unauthorized` |
| 2 | `fund_state.status == Setup` | `FundNotInSetup` |

**Effect**: `Registry` with mint, governance, role_registry, fund_state; empty rules;
`ExtraAccountMetaList` with the five entries in [data-model.md](../data-model.md#extraaccountmetalist-compliance_hook).
**Event**: `RegistryInitialized`.

### `set_rules(kyc_providers: [Pubkey; 4], allowed_jurisdictions: [u64; 11], max_holders: u32)`

| # | Check | Error |
|---|---|---|
| 1 | signer `== role_registry.issuer` | `Unauthorized` |
| 2 | `fund_state.status == Setup` | `FundNotInSetup` (FR-009) |
| 3 | non-default providers are distinct | `DuplicateProvider` |
| 4 | every set jurisdiction bit is an assigned ISO 3166-1 code; bits ≥ 676 are zero | `UnknownJurisdiction` |

**Effect**: rules replaced. **Event**: `RegisterRulesSet`.

## Onboarding (fund Open)

### `register_holder(owner: Pubkey, class: u8)`

Accounts: `authority` (signer), `payer` (signer, writable), `registry`, `fund_state`,
`role_registry`, `holder_state` (writable, PDA `["holder", mint, owner]`), `system_program`.

| # | Check | Error |
|---|---|---|
| 1 | `fund_state.status == Open` | `FundNotOpen` |
| 2 | `class ∈ {Investor, FundTreasury, Escrow}` | `InvalidClass` |
| 3 | authority is compliance, **or** authority `== owner` | `Unauthorized` |
| 4 | if authority is not compliance: `class == Investor` | `SelfRegistrationClassNotAllowed` |
| 5 | `class == Escrow` → not available until the settlement feature (F003) | `ClassNotAvailable` |
| 6 | `class == FundTreasury` → `owner == PDA(fund, ["treasury", fund_state])` | `NotSystemControlled` |
| 7 | `holder_state` has no data | `HolderAlreadyRegistered` (FR-014; also covers Revoked records) |

**Effect**: `HolderState { status: Pending, class, owner, mint }`; everything else zero.
**Event**: `HolderRegistered`.

### `approve_investor(attestation_hash: [u8; 32])` / `renew_investor(attestation_hash: [u8; 32])`

Accounts: `compliance` (signer), `registry`, `fund_state`, `role_registry`, `holder_state`
(writable), `instructions` sysvar.

| # | Check | Error |
|---|---|---|
| 1 | signer is compliance | `Unauthorized` |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | holder not Revoked | `HolderRevoked` |
| 4 | approve: status Pending · renew: status Approved | `InvalidHolderStatus` |
| 5 | `class == Investor` | `NotInvestor` |
| 6–17 | statement checks 1–12 in [kyc-statement.md](kyc-statement.md) | as listed there |

**Effect**: status Approved; stores jurisdiction, investor_type, kyc_expiry_ts, attestation_hash,
attested_by. **Event**: `HolderApproved { renewal }`.

### `approve_fund_account()`

Accounts: `compliance` (signer), `registry`, `fund_state`, `role_registry`, `holder_state` (writable).

| # | Check | Error |
|---|---|---|
| 1 | signer is compliance | `Unauthorized` |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | holder not Revoked | `HolderRevoked` |
| 4 | status Pending | `InvalidHolderStatus` |
| 5 | class is FundTreasury (Escrow from F003) | `NotFundAccount` |
| 6 | owner is still the expected system PDA | `NotSystemControlled` |

**Effect**: status Approved; `kyc_expiry_ts = i64::MAX`; jurisdiction, investor_type, attestation
fields zero. **Event**: `HolderApproved { renewal: false }`.

### `thaw_holding_account()` — permissionless **[SM change: new, listed in §5.1]**

Thaws the owner's one holding account. The caller adds no discretion: the outcome is fixed by the
owner's Approved status, which compliance already signed (Constitution III).

Accounts: `payer` (any signer), `registry` (PDA, freeze authority, signs the CPI), `holder_state`,
`token_account` (writable), `mint`, `token_2022_program`.

| # | Check | Error |
|---|---|---|
| 1 | `approved(holder_state, now)` | `HolderNotApproved` |
| 2 | `token_account == ATA(holder_state.owner, mint, Token-2022)` | `NotHoldingAccount` |
| 3 | `token_account` is frozen | `AlreadyThawed` |

**Effect**: Token-2022 `ThawAccount` signed by the `Registry` PDA. **Event**: `HoldingAccountThawed`.
Compliance's onboarding transaction normally carries `create ATA (idempotent) → approve_investor →
thaw_holding_account`, so onboarding stays one registration plus one compliance approval (SC-005).

## Called only by governance (signer = governance PDA `["hook-admin"]`)

### `set_status(action: u8, reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)`

| # | Check | Error |
|---|---|---|
| 1 | `hook_admin` is the governance `["hook-admin"]` PDA and signed | `UnauthorizedCaller` |
| 2 | holder not Revoked | `HolderRevoked` |
| 3 | Suspend: status Approved · Reinstate: status Suspended · Revoke: status Pending, Approved or Suspended | `InvalidHolderStatus` |

**Effect**: status changes per the transition table. Suspension and revocation do **not** freeze the
token account; the hook's status check is the rule (research R13). **Event**: `HolderStatusChanged`.

### `set_paused(paused: bool, reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)`

| # | Check | Error |
|---|---|---|
| 1 | caller is the `["hook-admin"]` PDA | `UnauthorizedCaller` |
| 2 | `paused != registry.transfers_paused` | `AlreadyPaused` / `NotPaused` |

**Effect**: `registry.transfers_paused = paused`. **Event**: `TransfersPaused { paused }`.

## Transfer hook `execute(amount)` — fixed check order (FR-020–FR-026, shared-model §6)

Called by Token-2022 during `transfer_checked`. Token-2022 checks some things **before** calling
the hook, and these come first in the documented order:

| Order | Check | Error |
|---|---|---|
| T1 | (Token-2022) source and destination not frozen. A never-approved owner's account is still frozen. | Token-2022 `AccountFrozen` |
| T2 | (Token-2022) mint, authority, balance | Token-2022 errors |
| 0 | source and destination carry the `transferring` flag (rejects direct calls) | `NotTransferring` |
| 1 | `registry.mint == mint`; accounts at the expected PDAs | `WrongMint` |
| 2 | enforcement bypass — **lands with F005** (it can't be exercised before) **[SM change]** | — |
| 3 | `registry.transfers_paused` | `TransfersPaused` |
| 4 | sender `approved(src, now)`; a missing or foreign `HolderState` counts as not approved | `SenderNotApproved` |
| 5 | lien check — **lands with F004** (`pledged_amount` is always zero before then) **[SM change]** | — |
| 6 | receiver `approved(dst, now) && class_allowed(dst) && jurisdiction_ok(dst)`; a missing record counts as not approved | `ReceiverNotApproved` |
| 7 | receiver is an Investor with `last_balance == 0`, `amount > 0`, `max_holders > 0` and `holder_count >= max_holders` | `HolderCapReached` |
| 8 | accrual — no-op until F002 | — |
| 9 | holder count and `last_balance` / `last_slot` update ([data-model.md](../data-model.md#holder-count)); same owner → updated once | `Overflow` (checked math) |
| 10 | `emit!(TransferValidated)` | — |

Notes:
- Zero-unit transfers run every check (spec Edge Cases); this is what the devnet run uses (R16).
- A transfer from an account to itself never reaches the hook: Token-2022 checks only that the
  account isn't frozen and the authority signed, then returns before moving anything (R4). Nothing
  changes, so no rule is at stake. Because each owner has one usable account (R9), "same sender and
  receiver" always means this case.
- The base accounts (source, mint, destination, authority) reach the hook read-only. `HolderState`
  entries are loaded as unchecked accounts and decoded by hand: this maps a missing record to "not
  approved", and when both entries are the same owner it loads and writes the record once (Anchor 1.x
  rejects a duplicate writable typed account with error 2040 — R4).
- The hook does constant work with no loops over holders (SC-006). A compute-unit ceiling is asserted
  in tests (research R8).
