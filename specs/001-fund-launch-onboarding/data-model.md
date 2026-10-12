# Data Model: Fund Launch, Roles and Holder Onboarding (F001)

This file maps the spec's entities to the on-chain accounts in `docs/shared-model.md`. It lists only
what F001 creates or uses. Every account is created with its final layout (shared-model §1), and
fields tagged for later features stay zero in F001. Changes to the shared model that this feature
makes are marked **[SM change]** and listed in [plan.md](plan.md#shared-model-changes).

| Spec entity | Account | Program | PDA seeds |
|---|---|---|---|
| Fund | `FundState` + the Token-2022 fund mint | `fund` | `["fund", mint]` |
| Register | `Registry` (+ `ExtraAccountMetaList`) | `compliance_hook` | `["registry", mint]`, `["extra-account-metas", mint]` |
| Holder record | `HolderState` | `compliance_hook` | `["holder", mint, owner]` |
| Role assignment | `RoleRegistry` | `governance` | `["roles", fund_state]` |
| KYC statement | not stored; fingerprint and signer only | off-chain object | n/a |

## Fund mint (Token-2022)

Created by `fund::create_fund` in the same instruction as `FundState` (FR-004). Decimals: 6.

| Setting | Value | Authority holder |
|---|---|---|
| Transfer hook program | `compliance_hook` | n/a |
| Transfer hook authority | PDA `["token-admin", mint]` of `governance`. No F001 instruction uses it. A hook change is a time-locked governance action (F007). **[SM change]** | governance |
| Default account state | `Frozen` | can be changed only by the freeze authority; `compliance_hook` exposes no instruction for it |
| Permanent delegate | PDA `["delegate", mint]` of `governance`. Signs nothing in F001. | governance |
| Metadata pointer | points to the mint itself; authority = `FundState` PDA **[SM change: authority named]** | fund |
| Token metadata | name, symbol, uri = `terms_uri`; extra fields `fund_id` (hex) and `terms_hash` (hex); update authority = `FundState` PDA. No F001 instruction updates it. | fund |
| Mint close authority | `FundState` PDA | fund |
| Permissioned burn | enabled; burn authority = `FundState` PDA. Every burn, even by the owner or the permanent delegate, needs the fund PDA's co-signature, so a holder can't destroy its own units (research R3). No F001 instruction burns. **[SM change: new extension]** | fund |
| Mint authority | `FundState` PDA. No F001 instruction mints (FR-035). | fund |
| Freeze authority | `Registry` PDA | compliance_hook |

**Holding account rule [SM change]**: an owner holds units in exactly one account, its associated
token account (ATA) for the fund mint under Token-2022. New accounts start frozen (default state).
Only `compliance_hook::thaw_holding_account` thaws an account, and only the owner's ATA, only while
the owner's `HolderState` is Approved. Any other account the owner opens stays frozen, so it can
never send or receive. This keeps "one Holder record per owner" (FR-015) consistent with the
per-owner balance that the holder count (FR-024) and F002 accrual rely on.

## `FundState` (fund) — fields used in F001

```text
version        u8        = 1
mint           Pubkey
role_registry  Pubkey    = PDA ["roles", fund_state] (governance)
terms          FundTerms
status         u8        0 Setup → 1 Open (F001 uses only these two)
...            zero      F002–F006 fields (nav*, accrual_index, units_outstanding, …)
bump           u8
_reserved      [u8; 64]
```

**FundTerms** (set once at creation; no instruction changes them — FR-009):

| Field | Validation (FR-003) | Error |
|---|---|---|
| `fund_id [u8;12]` | not all zero | `InvalidFundId` |
| `cash_mint Pubkey` | an initialized SPL Token or Token-2022 mint with 6 decimals | `InvalidCashMint` |
| `target_nav u64` | > 0 | `ZeroTargetNav` |
| `max_nav_move_bps u16` | > 0 | `ZeroMaxNavMove` |
| `redemption_gate_bps u16` | 1 ..= 10 000 | `InvalidRedemptionGate` |
| `distribution_mode u8` | 0 Reinvest or 1 Cash | `InvalidDistributionMode` |
| `distribution_cadence_days u16` | > 0 | `ZeroDistributionCadence` |
| `terms_hash [u8;32]` | not all zero | `MissingTermsHash` |
| `terms_uri [u8;128]` | at least one non-zero byte, zero-padded, valid UTF-8 | `MissingTermsUri` |
| name, symbol (metadata only) | non-empty; name ≤ 32 bytes, symbol ≤ 10 bytes | `InvalidName`, `InvalidSymbol` |

**State transitions**

```text
(none) --create_fund [issuer]--> Setup --launch [issuer]--> Open
```

`launch` requires: status Setup; every role in `RoleRegistry` assigned (non-default); `Registry` exists
with ≥ 1 KYC provider and ≥ 1 allowed jurisdiction (FR-006). Gated, Suspended, WindingDown and Closed
belong to later features.

## `RoleRegistry` (governance)

```text
version         u8
fund_state      Pubkey   PDA ["fund", mint] of fund (derived at init)
issuer          Pubkey   creator of the fund (FR-002); a Squads vault or a key
fund_admin      Pubkey   default until set_roles
transfer_agent  Pubkey
compliance      Pubkey   typically a Squads vault (FR-012)
keeper          Pubkey
auditor         Pubkey
timelock_secs   u32      zero [F007]
action_count    u64      zero [F007]
bump            u8
_reserved       [u8; 64]
```

- Created by `governance::init_roles`, signed by the issuer **and the mint keypair**, before
  `fund::create_fund`. `create_fund` then requires `role_registry.issuer == signer`. The issuer is
  stored once, here, and never copied (Constitution V).
- `set_roles` (issuer, fund in Setup) sets all five operating roles at once. Separation rules
  (Clarification Q4, FR-010):
  - `keeper` differs from `issuer`, `fund_admin`, `transfer_agent`, `compliance` and `auditor` →
    else `KeeperNotSeparate`.
  - `compliance` differs from `issuer` → else `ComplianceIsIssuer`.
  - No role may be the default pubkey → else `RoleUnassigned`.
  - Other overlaps (e.g. `fund_admin == transfer_agent`) are allowed.
- Fixed once the fund is Open → `FundNotInSetup`.

## `Registry` (compliance_hook)

```text
version                u8
mint                   Pubkey
governance             Pubkey   governance program id
role_registry          Pubkey
fund_state             Pubkey
collateral_program     Pubkey   zero [F004]
kyc_providers          [Pubkey; 4]       unused slots = default pubkey
allowed_jurisdictions  [u64; 11]         676-bit set, see "Jurisdictions" [SM change: was u64]
max_holders            u32      0 = no cap
holder_count           u32      investors whose last recorded balance > 0
transfers_paused       bool
bump                   u8
_reserved              [u8; 64]
```

- Created by `init_registry` (issuer, fund in Setup), together with the `ExtraAccountMetaList`.
- `set_rules(kyc_providers, allowed_jurisdictions, max_holders)` (issuer, Setup only — FR-008/009):
  - providers: non-default entries must be distinct → `DuplicateProvider`.
  - jurisdictions: every set bit must be an assigned ISO 3166-1 alpha-2 code →
    `UnknownJurisdiction`.
- **`holder_count`** is a stored running total (Constitution V allows it: it can't be recomputed
  over 10 000 holders inside one transaction). It changes only inside the hook, in the same
  transfer that changes the balances it summarizes. See "Holder count" below.
- `transfers_paused` changes only via `set_paused`, called by governance with a reason.

## `HolderState` (compliance_hook) — fields used in F001

```text
version           u8
mint              Pubkey
owner             Pubkey   the owner account (the institution's identity — Clarification Q2)
status            u8       0 Pending, 1 Approved, 2 Suspended, 3 Revoked
class             u8       0 Investor, 1 FundTreasury, 2 Escrow, 3 CollateralVault [backlog]
jurisdiction      [u8; 2]  ISO 3166-1 alpha-2, ASCII; zero for fund-operated accounts [SM change: was u8]
investor_type     u8       provider's code, non-zero for investors; zero for fund-operated
kyc_expiry_ts     i64      investors: from the statement; fund-operated: i64::MAX (never expires)
attestation_hash  [u8; 32] SHA-256 of the statement message; zero for fund-operated
attested_by       Pubkey   provider key; default for fund-operated
pledged_amount    u64      zero [F004]
active_liens      u16      zero [F004]
index_at_last_touch u128   zero [F002]
accrued_e18       u128     zero [F002]
last_balance      u64      [F001] balance of the owner's ATA after the last hook touch [SM change: retagged from F002]
last_slot         u64      [F001] slot of the last hook touch [SM change: retagged from F002]
last_paid_cycle   u32      zero [F006]
bump              u8
_reserved         [u8; 64]
```

**Validity predicates** (used by the hook and by `thaw_holding_account`):

- `kyc_valid(h, now) := now < h.kyc_expiry_ts` (expired from the exact second the expiry is reached — FR-025).
- `approved(h, now) := h.status == Approved && kyc_valid(h, now)`.
- `jurisdiction_ok(h) := h.class != Investor || allowed(registry, h.jurisdiction)`.
- `class_allowed(h) := h.class ∈ {Investor, FundTreasury, Escrow}`.

**State transitions** (all others are rejected with the named error):

| From | Action (signer) | To | Conditions |
|---|---|---|---|
| none | `register_holder` (owner, Investor only) | Pending | fund Open; no record yet (`HolderAlreadyRegistered`); class Investor (`SelfRegistrationClassNotAllowed`) |
| none | `register_holder` (compliance) | Pending | fund Open; no record yet; fund-operated owner is the expected system PDA (`NotSystemControlled`); Escrow → `ClassNotAvailable` until F003 |
| Pending (Investor) | `approve_investor` (compliance + statement) | Approved | statement checks, see contract; stores jurisdiction, investor_type, kyc_expiry_ts, attestation_hash, attested_by |
| Pending (fund-operated) | `approve_fund_account` (compliance) | Approved | owner still the expected system PDA |
| Approved (Investor) | `renew_investor` (compliance + statement) | Approved | same checks as approval and `new expiry > current expiry` (`RenewalNotNewer`) |
| Approved | `suspend` (compliance via governance, reason) | Suspended | |
| Suspended | `reinstate` (compliance via governance, reason) | Approved | KYC expiry still applies (FR-028) |
| Pending / Approved / Suspended | `revoke` (compliance via governance, reason) | Revoked | final (FR-029) |
| Revoked | any | — | `HolderRevoked`; re-registration fails with `HolderAlreadyRegistered` because the record persists |

Status checks for suspend, reinstate and renew fail with `InvalidHolderStatus` (the error carries
no data; each test asserts the code).

## `ExtraAccountMetaList` (compliance_hook)

Created by `init_registry`. Resolves, in this fixed order (shared-model §4.1):

| # | Account | Resolution | Writable |
|---|---|---|---|
| 1 | `Registry` | literal PDA | **yes [SM change: was read-only]** — the hook updates `holder_count` |
| 2 | `FundState` | literal PDA | no |
| 3 | `HolderState(source owner)` | seeds `["holder", mint, data(source token account)[32..64]]` | yes |
| 4 | `HolderState(destination owner)` | seeds `["holder", mint, data(destination token account)[32..64]]` | yes |
| 5 | governance delegate PDA `["delegate", mint]` | literal pubkey | no |

Effect of #1 being writable: every transfer of the fund's units write-locks the `Registry`, so
transfers of this fund run one after another within a block, not in parallel. That's acceptable at
money-market-fund volumes (research R5).

## Holder count

For each Investor-class `HolderState` touched by a transfer (source and destination; once if they
are the same owner), at hook step 9:

```text
was     = h.last_balance > 0
now     = post_balance_of_owner_ATA > 0
holder_count += (now as i64) - (was as i64)      // checked; overflow/underflow → abort
h.last_balance = post_balance; h.last_slot = clock.slot
```

The cap check (hook step 7) rejects with `HolderCapReached` when the destination is an Investor,
`max_holders > 0`, `dst.last_balance == 0`, `amount > 0`, and `holder_count >= max_holders`. Fund
treasury and escrow accounts are never counted.

**Invariant I10 [SM change: new]**: `Registry.holder_count == |{ h : h.class == Investor ∧
h.last_balance > 0 }|`, and every Investor's `last_balance` equals its ATA balance. Investor balances
change only through hook-checked transfers: permissioned burn stops owners burning (R3), only the
fund PDA could mint and F001 has no mint path, and every other account an owner opens stays frozen
(R9). So the recorded and actual balances never diverge, and the count is exact.

## Jurisdictions [SM change]

- A jurisdiction is an ISO 3166-1 alpha-2 country code (two ASCII uppercase letters) — Clarification Q5.
- Bit index: `index(c1, c2) = (c1 − 'A') × 26 + (c2 − 'A')`, range 0..675. The index needs no lookup
  table and never shifts when a country is added.
- `Registry.allowed_jurisdictions` is a 676-bit set stored as `[u64; 11]`; bit `i` lives in word
  `i / 64`, bit `i % 64`. Bits 676..703 must be zero.
- `ASSIGNED_ISO3166` is a compile-time 676-bit constant of the officially assigned codes (shared
  `mmf-common` crate). A code outside it is rejected in `set_rules` and in statements
  (`UnknownJurisdiction`). Adding a newly assigned code needs a program upgrade.

## KYC statement (off-chain object; contract in [contracts/kyc-statement.md](contracts/kyc-statement.md))

Fixed 95-byte message signed with ed25519 by a provider listed in the `Registry`:

```text
domain         [u8; 20]  b"MMF-KYC-STATEMENT-V1"
mint           Pubkey    the fund mint (scopes the statement to one fund)
owner          Pubkey    the owner account it names (Clarification Q2)
jurisdiction   [u8; 2]   ISO 3166-1 alpha-2
investor_type  u8        non-zero provider code
expiry_ts      i64 LE    Unix seconds
```

Stored on approval or renewal: `attestation_hash = sha256(message)`, `attested_by = provider`.
The statement's contents are not stored beyond the fields above (spec Key Entities).

## Reason codes (F001 subset of shared-model `ActionRecord.reason`)

Allowed for suspend, reinstate, revoke, pause and unpause (FR-031): `0 CourtOrder`, `1 SanctionsHit`,
`2 LostKey`, `3 OperationalError`, `6 Other`. Codes 4 (CollateralDefault) and 5 (IssuerWindDown)
are rejected in F001 (`InvalidReason`). Each action also carries a `case_ref_hash [u8;32]` (may be
zero) so the instruction interface doesn't change when F007 adds `ActionRecord`.

## Events (records — FR-033)

Admin and onboarding events use Anchor `emit_cpi!` so they survive log truncation. The hook's
`TransferValidated` uses `emit!` (logs) to save a CPI level (research R7). Every payload carries
`fund`, `slot`, `unix_ts`.

| Event | Emitted by | Payload (besides fund, slot, unix_ts) |
|---|---|---|
| `RolesInitialized` **[SM change: new]** | governance `init_roles` | issuer |
| `RolesAssigned` **[SM change: new]** | governance `set_roles` | all six role keys, signer |
| `FundCreated` | fund `create_fund` | mint, issuer, terms |
| `RegistryInitialized` **[SM change: new]** | compliance_hook `init_registry` | signer |
| `RegisterRulesSet` **[SM change: new]** | compliance_hook `set_rules` | providers, jurisdictions, max_holders, signer |
| `FundLaunched` | fund `launch` | signer |
| `HolderRegistered` | compliance_hook `register_holder` | owner, class, signer |
| `HolderApproved` | `approve_investor`, `approve_fund_account`, `renew_investor` | owner, class, jurisdiction, investor_type, kyc_expiry_ts, attestation_hash, attested_by, `renewal: bool`, signer |
| `HolderStatusChanged` | compliance_hook `set_status` | owner, from, to, reason, case_ref_hash, actor (compliance key) |
| `TransfersPaused` | compliance_hook `set_paused` | `paused: bool`, reason, case_ref_hash, actor |
| `HoldingAccountThawed` **[SM change: new]** | compliance_hook `thaw_holding_account` | owner, token_account, signer |
| `TransferValidated` | compliance_hook `execute` | source owner, destination owner, amount, holder_count after |

A holder's full status history is rebuilt from `HolderRegistered`, `HolderApproved` and
`HolderStatusChanged` for that owner (FR-034, SC-003).

## Invariants tested in F001

| ID | Invariant | Where tested |
|---|---|---|
| I3 | Only approved holders (or escrow/treasury classes) hold a non-zero balance, and every successful transfer's receiver was `approved` at that moment | randomized sequences (SC-001) |
| I10 | `holder_count` equals Investor records with `last_balance > 0`, and each `last_balance` equals the ATA balance | randomized sequences (SC-007) |
