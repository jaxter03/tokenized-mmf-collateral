# Shared Model: accounts, authorities and cross-program rules

**Status:** v1. The single source of truth for anything more than one feature touches.
**Rule:** every `/speckit-plan` must conform to this file. If a feature needs a change here, the plan says so explicitly and this file is updated in the same pull request, before code.
**Precedence:** if this file and `docs/design.md` disagree, this file wins (see section 10).

---

## 1. Conventions

| Topic | Rule |
|---|---|
| Account versioning | Every program account starts with `version: u8` (= 1) and ends with `_reserved: [u8; 64]`. Small additions use reserved bytes instead of resizing. |
| Full layout from day one | Accounts are created with their **final** layout. Fields used by later features exist from the start and stay zero until that feature ships. |
| Ownership of an entity | The feature that first creates an account type owns its definition. Later features extend fields listed here; they never define a second account for the same concept. |
| Integer math | No floats anywhere on-chain. All multiplications in `u128`, checked. Every division states its rounding direction (section 8). |
| Unit decimals | Fund unit: 6 decimals. Cash (USDC or mock): 6 decimals. |
| Price (NAV) | Price of one whole unit, in cash, with 8 decimals: $1.00 = `100_000_000`. |
| Value of units | `value_cash_base = units_base × nav / 10^8` |
| Daily rate | Fraction of value earned that day × 10^12, stored as `u64`. 4.5 % a year ≈ 0.000123288/day = `123_287_671`. |
| Accrual index | `u128`, scale 10^18 (interest per unit since launch). `index += daily_rate_e12 × 10^6` on each publish. |
| Time | `i64` Unix seconds from the Clock sysvar. |
| Seeds | Integers in seeds are little-endian `u64` (`seq.to_le_bytes()`). |
| Events | Every state-changing instruction emits one event (section 9) with `fund`, `slot`, `unix_ts`. |

---

## 2. Programs and what each owns

| Program | Owns accounts | Introduced by |
|---|---|---|
| `compliance_hook` | `Registry`, `HolderState`, `ExtraAccountMetaList` | F001 (extended by F002, F004) |
| `fund` | `FundState`, `NavRecord`, `DistributionCycle`, `DistributionReceipt` | F001 (extended by F002, F003, F006) |
| `dvp_settlement` | `DvpConfig`, `SettlementInstruction`, escrow vaults | F003 |
| `collateral` | `PledgeAgreement`, `Pledge`, `CollateralCall`, `EnforcementHold` | F004 (extended by F005) |
| `governance` | `RoleRegistry`, `ActionRecord`, `RegisterRoot` | F001 roles (extended by F005, F007) |

---

## 3. Fund mint (Token-2022)

| Setting | Value | Holder of the authority |
|---|---|---|
| Transfer hook | `compliance_hook` program | n/a |
| Default account state | `Frozen` | n/a |
| Permanent delegate | PDA `["delegate", mint]` of **governance** | governance |
| Metadata pointer + metadata | name, symbol, `fund_id`, `terms_hash`, `terms_uri` | fund PDA |
| Mint close authority | `FundState` PDA | fund |
| Mint authority | `FundState` PDA | fund |
| Freeze authority | `Registry` PDA | compliance_hook |
| Not used | ConfidentialTransfer, InterestBearing, TransferFee, NonTransferable | |

Created once in F001 by `fund::create_fund`, with all extensions initialised in the same transaction.

---

## 4. Account layouts

Field tags show which feature first **uses** the field: `[F001]` … `[F008]`. Fields tagged `[backlog]` are reserved and must stay zero.

### 4.1 `compliance_hook`

**Registry**: PDA `["registry", mint]`

```text
version                u8
mint                   Pubkey   [F001]
governance             Pubkey   [F001]  governance program id (role checks read RoleRegistry)
role_registry          Pubkey   [F001]
fund_state             Pubkey   [F001]
collateral_program     Pubkey   [F004]  only caller allowed to change locked units
kyc_providers          [Pubkey; 4] [F001]
allowed_jurisdictions  u64      [F001]  bitmask
max_holders            u32      [F001]  0 = no cap
holder_count           u32      [F001]
transfers_paused       bool     [F001]
bump                   u8
_reserved              [u8; 64]
```

**HolderState**: PDA `["holder", mint, owner]` (`owner` = owner of the token account)

```text
version                u8
mint                   Pubkey   [F001]
owner                  Pubkey   [F001]
status                 u8       [F001]  0 Pending, 1 Approved, 2 Suspended, 3 Revoked
class                  u8       [F001]  0 Investor, 1 FundTreasury, 2 Escrow, 3 CollateralVault [backlog]
jurisdiction           u8       [F001]
investor_type          u8       [F001]
kyc_expiry_ts          i64      [F001]
attestation_hash       [u8; 32] [F001]
attested_by            Pubkey   [F001]
pledged_amount         u64      [F004]  units locked; = Σ active pledges (invariant I1)
active_liens           u16      [F004]
index_at_last_touch    u128     [F002]
accrued_e18            u128     [F002]  interest owed in units × 10^18, not yet paid
last_balance           u64      [F002]
last_slot              u64      [F002]
last_paid_cycle        u32      [F006]
bump                   u8
_reserved              [u8; 64]
```

**ExtraAccountMetaList**: PDA `["extra-account-metas", mint]`, created in F001. It resolves, in this fixed order:

1. `Registry` (read-only)
2. `FundState` (read-only)
3. `HolderState` of source owner (writable; seed from source token account `owner` field)
4. `HolderState` of destination owner (writable; seed from destination token account `owner` field)
5. Governance delegate PDA `["delegate", mint]` (read-only), used to recognise enforcement transfers

Adding entries later changes every client's transfer, so all five exist from F001.

### 4.2 `fund`

**FundState**: PDA `["fund", mint]` (also the mint authority)

```text
version                u8
mint                   Pubkey   [F001]
role_registry          Pubkey   [F001]
terms                  FundTerms[F001]
status                 u8       [F001]  0 Setup, 1 Open, 2 Gated, 3 Suspended, 4 WindingDown, 5 Closed
nav                    u64      [F002]  latest price (8 dp)
nav_ts                 i64      [F002]
nav_seq                u64      [F002]  last published NavRecord seq (0 = none)
accrual_index          u128     [F002]
daily_rate_e12         u64      [F002]  last published rate
units_outstanding      u64      [F003]
redeemed_today         u64      [F003]
day_start_ts           i64      [F003]
cash_treasury          Pubkey   [F003]  token account for cash
current_cycle          u32      [F006]
bump                   u8
_reserved              [u8; 64]
```

**FundTerms** (embedded)

```text
fund_id                [u8; 12]
cash_mint              Pubkey
target_nav             u64      100_000_000
max_nav_move_bps       u16      typo guard for publish_daily
redemption_gate_bps    u16      max daily redemptions as % of units_outstanding
distribution_mode      u8       0 Reinvest (units), 1 Cash
distribution_cadence_days u16
terms_hash             [u8; 32]
terms_uri              [u8; 128] fixed-size, zero-padded
```

**NavRecord**: PDA `["nav", fund_state, seq]`, immutable once written

```text
version u8 · seq u64 · nav u64 · daily_rate_e12 u64 · index_after u128 · as_of_ts i64 · published_by Pubkey · bump u8
```

**DistributionCycle**: PDA `["dist", fund_state, cycle_u32]` [F006]

```text
version u8 · cycle u32 · mode u8 · index_at_close u128 · total_due_e18 u128 · total_paid u64 · holders_paid u32 · status u8 (0 Open, 1 Closed, 2 Finalised) · bump u8 · _reserved
```

**DistributionReceipt**: PDA `["dreceipt", cycle_account, owner]` [F006]. Its existence makes payouts idempotent.

```text
version u8 · cycle u32 · owner Pubkey · amount u64 · paid_ts i64 · bump u8
```

### 4.3 `dvp_settlement` [F003]

**DvpConfig**: PDA `["dvp", fund_state]`

```text
version u8 · fund_state Pubkey · next_seq u64 · default_expiry_secs u32 · bump u8 · _reserved
```

**Escrow authority**: PDA `["escrow", fund_state]`. One per fund. It owns:
- the unit escrow token account (fund mint), with its own `HolderState` of class `Escrow`, approved at F003 setup;
- the cash escrow token account (cash mint).

Per-instruction amounts are tracked in `SettlementInstruction`, not in separate vaults.

**SettlementInstruction**: PDA `["settlement", fund_state, seq]`

```text
version          u8
seq              u64
kind             u8      0 Subscription, 1 Redemption, 2 SecondaryTransfer
seller           Pubkey  Subscription: fund_state; others: unit owner
buyer            Pubkey  Redemption: fund_state; others: unit receiver
units            u64
cash_amount      u64     fixed at creation from NavRecord(nav_seq), rounding per section 8
nav_seq          u64
not_before_ts    i64
expires_at_ts    i64
status           u8      0 Instructed, 1 SellerFunded, 2 BuyerFunded, 3 Affirmed, 4 Settled, 5 Cancelled, 6 Expired, 7 Rejected
seller_funded_at i64
buyer_funded_at  i64
external_ref     [u8; 32]
created_by       Pubkey
bump             u8
_reserved        [u8; 64]
```

### 4.4 `collateral` [F004, F005]

**PledgeAgreement**: PDA `["agreement", fund_state, giver, taker, nonce]`

```text
version                     u8
fund_state                  Pubkey
giver                       Pubkey
taker                       Pubkey
nonce                       u64
mode                        u8      0 Lien (only mode until backlog), 1 TitleTransfer [backlog]
income_to                   u8      0 Giver [backlog for others]
haircut_bps                 u16
max_nav_age_secs            u32
call_cure_secs              u32
enforcement_grace_secs      u32
min_transfer_value          u64     cash base units
concentration_cap_units     u64
taker_may_call_unilaterally bool
agreement_hash              [u8; 32]
status                      u8      0 Proposed, 1 Active, 2 Terminating, 3 Terminated
giver_signed                bool
taker_signed                bool
pledged_units               u64     Σ active pledge units on this agreement
required_value              u64     current exposure, cash base units
pledge_count                u64     next Pledge seq
call_count                  u64     next CollateralCall seq
created_ts                  i64
bump                        u8
_reserved                   [u8; 64]
```

**Pledge**: PDA `["pledge", agreement, seq]`

```text
version u8 · agreement Pubkey · seq u64 · units u64 · mode u8 · posted_at i64 · nav_seq_at_post u64
released_units u64 · enforced_units u64 · status u8 (0 Active, 1 Released, 2 Enforced, 3 Substituted) · bump u8 · _reserved
```

**CollateralCall**: PDA `["call", agreement, seq]`

```text
version          u8
agreement        Pubkey
seq              u64
kind             u8      0 Delivery, 1 Return
value_delta      u64     cash base units
units_required   u64     at issuance, section 8 rounding
nav_seq          u64
issued_at        i64
cure_deadline_ts i64     issued_at + call_cure_secs
enforce_after_ts i64     cure_deadline_ts + enforcement_grace_secs
status           u8      0 Open, 1 Met, 2 Disputed, 3 Defaulted, 4 Enforced, 5 Cancelled
enforced_units   u64     [F005] running total for partial enforcement
enforced_value   u64     [F005]
external_ref     [u8; 32]
bump             u8
_reserved        [u8; 64]
```

**EnforcementHold**: PDA `["hold", call]` [F005]

```text
version u8 · call Pubkey · placed_by Pubkey · reason u8 · case_ref_hash [u8; 32] · placed_at i64 · lifted_at i64 · active bool · bump u8
```

### 4.5 `governance`

**RoleRegistry**: PDA `["roles", fund_state]` [F001]

```text
version         u8
fund_state      Pubkey
issuer          Pubkey   Squads vault
fund_admin      Pubkey
transfer_agent  Pubkey
compliance      Pubkey   Squads vault
keeper          Pubkey
auditor         Pubkey
timelock_secs   u32      [F007] e.g. 86_400
action_count    u64      next ActionRecord seq
bump            u8
_reserved       [u8; 64]
```

**Delegate authority**: PDA `["delegate", mint]`. This is the mint's permanent delegate. It signs only inside `execute_enforcement` [F005] and `execute_action` for Seize or ForcedTransfer [F007].

**ActionRecord**: PDA `["action", fund_state, seq]`

```text
version          u8
seq              u64
kind             u8   0 FreezeHolder, 1 ThawHolder, 2 SuspendHolder, 3 RevokeHolder, 4 PauseTransfers,
                      5 UnpauseTransfers, 6 Seize, 7 ForcedTransfer, 8 Enforcement, 9 EnforcementHold,
                      10 LiftHold, 11 RejectSettlement, 12 SetRules, 13 CommitRegisterRoot, 14 NavOverride
reason           u8   0 CourtOrder, 1 SanctionsHit, 2 LostKey, 3 OperationalError, 4 CollateralDefault,
                      5 IssuerWindDown, 6 Other
case_ref_hash    [u8; 32]
actor            Pubkey
target           Pubkey   holder, call, settlement or zero
amount           u64
proposed_ts      i64
executable_ts    i64      = proposed_ts for immediate actions
executed_ts      i64
status           u8       0 Proposed, 1 Executed, 2 Cancelled
bump             u8
_reserved        [u8; 64]
```

**RegisterRoot**: PDA `["root", fund_state, epoch]` [F007]

```text
version u8 · epoch u64 · slot u64 · register_root [u8; 32] · pledge_root [u8; 32] · holder_count u32 · total_units u64 · committed_by Pubkey · committed_ts i64 · bump u8
```

---

## 5. Who may call what

### 5.1 Signer roles per instruction (summary)

| Instruction | Program | Signer | Feature |
|---|---|---|---|
| `create_fund`, `launch` | fund | issuer | F001 |
| `init_roles` | governance | issuer | F001 |
| `init_registry` (+ ExtraAccountMetaList) | compliance_hook | issuer | F001 |
| `register_holder` | compliance_hook | the institution or compliance | F001 |
| `approve_holder` | compliance_hook | compliance + KYC statement (ed25519) | F001 |
| `suspend/revoke_holder`, `pause/unpause` | governance → compliance_hook | compliance | F001 (record from F007) |
| `publish_daily` | fund | fund_admin | F002 |
| `create_instruction`, `affirm_and_fund`, `cancel` | dvp_settlement | parties | F003 |
| `settle`, `expire` | dvp_settlement | keeper or anyone (permissionless, state-checked) | F003 |
| `propose/sign_agreement`, `pledge`, `release`, `substitute`, `dispute_call` | collateral | giver / taker per section 4.4 | F004 |
| `update_exposure`, `issue_call` | collateral | taker (giver affirms unless unilateral) | F004 |
| `mark_defaulted` | collateral | keeper (state-checked) | F005 |
| `enforce` | collateral | taker | F005 |
| `place_hold`, `lift_hold` | collateral | compliance | F005 |
| `close_cycle`, `finalize_cycle` | fund | transfer_agent | F006 |
| `distribute` | fund | keeper | F006 |
| `propose_action`, `execute_action`, `cancel_action` | governance | compliance / issuer | F007 |
| `commit_register_root` | governance | auditor | F007 |
| `wind_down`, `close_mint` | fund | issuer | F007 |

### 5.2 Cross-program calls (CPI): the only allowed edges

| Caller | Callee instruction | Callee checks |
|---|---|---|
| Token-2022 | `compliance_hook::execute` | standard transfer-hook checks (section 6) |
| `fund` | `compliance_hook::accrue(owner, pre_balance)` | signer = `FundState` PDA |
| `dvp_settlement` | `fund::mint_for_subscription`, `fund::burn_for_redemption` | signer = escrow authority PDA; instruction Affirmed |
| `collateral` | `compliance_hook::set_lien(owner, ±units)` | signer = PDA `["hook-caller"]` of `registry.collateral_program` |
| `collateral` | `governance::execute_enforcement(call)` | signer = collateral PDA `["gov-caller"]`; call account owned by collateral and status Defaulted |
| `governance` | `compliance_hook::set_status` / `set_paused` | signer = governance PDA `["hook-admin"]` |
| `fund`, `dvp_settlement`, `collateral`, `compliance_hook` | `governance::record_action(kind, reason, ref, target, amount)` | signer = the caller's registered PDA; kind allowed for that caller |

No other CPI edge is allowed without updating this table.

---

## 6. Hook `execute`: fixed order of checks

```text
0. Called by Token-2022 during transfer_checked (source and destination have transferring flag)
1. Load Registry, FundState, HolderState(src owner), HolderState(dst owner), delegate PDA
2. is_enforcement = (transfer authority == delegate PDA)
3. if registry.transfers_paused && !is_enforcement          → TransfersPaused
4. if !is_enforcement: src.status == Approved && kyc valid  → SenderNotApproved
5. if !is_enforcement: post_src_balance >= src.pledged_amount → PledgedUnitsLocked
6. dst.status == Approved && kyc valid && class allowed &&
   jurisdiction allowed (ALWAYS, also for enforcement)       → ReceiverNotApproved
7. if dst becomes a new holder and max_holders reached       → HolderCapReached
8. accrue(src, pre_src_balance); accrue(dst, pre_dst_balance)   [F002]
9. src.last_balance, dst.last_balance, last_slot updated
10. emit TransferValidated
```

`pre_src = post_src + amount`, `pre_dst = post_dst − amount`. When source and destination are the same owner, accrue once.

Features insert their logic only at their step. F001 builds steps 0–7 and 9–10 with step 8 as a no-op. F002 fills step 8. F004 activates step 5 (it reads `pledged_amount`, which is zero until then).

Mint and burn do not trigger the hook, so `fund` calls `compliance_hook::accrue` before every mint or burn.

---

## 7. Invariants (tested in every feature from the one that introduces them)

| ID | Invariant | From |
|---|---|---|
| I1 | `HolderState.pledged_amount == Σ (units − released − enforced)` over that holder's active pledges | F004 |
| I2 | `token_balance(holder) >= pledged_amount` at all times | F004 |
| I3 | Only approved holders (or escrow/treasury classes) hold a non-zero balance | F001 |
| I4 | `FundState.units_outstanding == mint supply` | F003 |
| I5 | Every unit minted by subscription has its cash in escrow in the same transaction | F003 |
| I6 | A settlement ends with both legs moved or every party at its starting balance | F003 |
| I7 | Enforced value ≤ shortfall at the price record used | F005 |
| I8 | Every discretionary admin action has an `ActionRecord` | F007 (F001–F006 call `record_action` once F007 lands; before that, events only) |
| I9 | Σ distribution receipts for a cycle == total due, within rounding (section 8) | F006 |

---

## 8. Rounding rules (always in favour of the protected party)

| Formula | Rounding | Why |
|---|---|---|
| Collateral value of units: `units × nav × (10_000 − haircut) / (10^8 × 10_000)` | down | Taker never over-counts collateral |
| Units required for a value: `value × 10^8 × 10_000 / (nav × (10_000 − haircut))` | **up** | Giver always posts enough |
| Units taken on enforcement for a shortfall | **down**, then min(available) | Taker never takes more than owed |
| Subscription cash for N units: `N × nav / 10^8` | up | Fund never under-paid |
| Redemption cash for N units | down | Fund never over-pays |
| Interest accrued: `balance × Δindex` (exact in e18) | exact; paid amount = floor(accrued_e18 / 10^18) | Remainder stays in `accrued_e18` for the next cycle |

Worked check (used in tests): NAV 1.00, haircut 2 %:
- $1,000,000 needs ceil(1,020,408.16…) = **1,020,409** units.
- A $500,000 shortfall enforces floor(510,204.08…) = **510,204** units.

---

## 9. Events (names are shared; payloads include `fund`, `slot`, `unix_ts`)

`FundCreated`, `FundLaunched`, `HolderRegistered`, `HolderApproved`, `HolderStatusChanged`, `TransfersPaused`, `TransferValidated`, `Accrued`, `NavPublished`, `SettlementCreated`, `LegFunded`, `Settled`, `SettlementCancelled`, `SettlementExpired`, `SettlementRejected`, `AgreementProposed`, `AgreementActive`, `ExposureUpdated`, `CallIssued`, `CallMet`, `CallDisputed`, `CallDefaulted`, `Pledged`, `Released`, `Substituted`, `LienSet`, `HoldPlaced`, `HoldLifted`, `Enforced`, `CycleClosed`, `Distributed`, `CycleFinalised`, `ActionProposed`, `ActionExecuted`, `ActionCancelled`, `RegisterRootCommitted`, `FundWindingDown`, `FundClosed`.

---

## 10. Changes from `docs/design.md`

| Area | Design doc | This file | Reason |
|---|---|---|---|
| Daily yield | `daily_yield_bps: u32` | `daily_rate_e12: u64` | 1 bp = 0.01 % per day is too coarse: 4.5 % a year is ~1.23 bp/day, so bps would round it to 1 bp (~3.65 % a year). |
| Escrow vaults | Per-instruction vaults | One escrow authority per fund; amounts tracked per instruction | Fewer accounts and one approved `Escrow` holder; same safety because amounts are checked per instruction. |
| Holds | Governance action | `EnforcementHold` in `collateral`, recorded via `governance::record_action` | Enforcement reads the hold in its own program; governance still keeps the audit record. |
| Distribution tracking | Receipt PDA only | Receipt PDA + `HolderState.last_paid_cycle` | Cheap check before deriving the receipt. |
| Title transfer, netting, manufactured income | In scope (stretch) | Fields reserved, `[backlog]` | Not in F001–F008. |

---

## 11. Field-by-feature checklist

| Feature | Must create | Must fill or activate |
|---|---|---|
| F001 | mint + extensions, `FundState` (Setup/Open), `Registry`, `ExtraAccountMetaList` (all 5 entries), `HolderState`, `RoleRegistry` | hook steps 0–7, 9–10 |
| F002 | `NavRecord` | `FundState.nav*`, `accrual_index`, `daily_rate_e12`; `HolderState.index_at_last_touch`, `accrued_e18`, `last_balance`; hook step 8; `fund::accrue` CPI |
| F003 | `DvpConfig`, escrow authority + its `HolderState`, `SettlementInstruction` | `FundState.units_outstanding`, `redeemed_today`, `cash_treasury` |
| F004 | `PledgeAgreement`, `Pledge`, `CollateralCall` | `Registry.collateral_program`; `HolderState.pledged_amount`, `active_liens`; hook step 5 |
| F005 | `EnforcementHold` | `CollateralCall.enforced_*`; delegate PDA signing in `execute_enforcement` |
| F006 | `DistributionCycle`, `DistributionReceipt` | `FundState.current_cycle`; `HolderState.last_paid_cycle` |
| F007 | `ActionRecord` (full), `RegisterRoot` | `RoleRegistry.timelock_secs`; `record_action` wired into F001–F006 admin paths |
| F008 | none on-chain | reads everything; must not require new on-chain fields |
