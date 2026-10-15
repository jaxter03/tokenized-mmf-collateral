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
| Day | UTC calendar day number `unix_timestamp.div_euclid(86_400)`, stored as `u32`. Price records cover whole days; coverage is contiguous (§4.2 NavRecord). [F002] |
| Jurisdiction | ISO 3166-1 alpha-2 country code, stored as `[u8; 2]` ASCII. Bit index in a jurisdiction set: `(c1 − 'A') × 26 + (c2 − 'A')` (0..675). A compile-time constant lists the assigned codes; any other code is rejected. [F001] |
| Holding account | Each owner holds units in exactly one token account: its associated token account (ATA) for the fund mint under Token-2022. Only `compliance_hook::thaw_holding_account` thaws accounts, and only that ATA; any other account stays frozen. [F001] |
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
| Transfer hook authority | PDA `["token-admin", mint]` of **governance**; signs nothing until a time-locked F007 action | governance |
| Default account state | `Frozen` | n/a |
| Permanent delegate | PDA `["delegate", mint]` of **governance** | governance |
| Metadata pointer + metadata | name, symbol, `fund_id`, `terms_hash`, `terms_uri`; pointer authority and metadata update authority = `FundState` PDA (no F001 instruction updates them) | fund PDA |
| Mint close authority | `FundState` PDA | fund |
| Permissioned burn | enabled; burn authority = `FundState` PDA. Every burn (owner, delegate or permanent delegate) needs the fund PDA's co-signature, so holders cannot burn their own units and only `fund` burns (F003 redemption) | fund |
| Mint authority | `FundState` PDA | fund |
| Freeze authority | `Registry` PDA | compliance_hook |
| Not used | ConfidentialTransfer, InterestBearing, TransferFee, NonTransferable, Pausable (transfers are paused in the hook; mint and burn are gated by `fund`) | |

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
collateral_program     Pubkey   [F004]  only caller allowed to change locked units; set by `init_registry` to the collateral program id, no setter
kyc_providers          [Pubkey; 4] [F001]
allowed_jurisdictions  [u64; 11] [F001] 676-bit set indexed per §1 Jurisdiction; bits ≥ 676 zero
max_holders            u32      [F001]  0 = no cap
holder_count           u32      [F001]  Investor HolderStates with last_balance > 0 (invariant I10)
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
jurisdiction           [u8; 2]  [F001]  ISO 3166-1 alpha-2; zero for FundTreasury / Escrow
investor_type          u8       [F001]
kyc_expiry_ts          i64      [F001]
attestation_hash       [u8; 32] [F001]
attested_by            Pubkey   [F001]
pledged_amount         u64      [F004]  units locked; = Σ active pledges (invariant I1)
active_liens           u16      [F004]
index_at_last_touch    u128     [F002]
accrued_e18            u128     [F002]  interest owed in units × 10^18, not yet paid
last_balance           u64      [F001]  owner's ATA balance after the last hook touch (holder count from F001; accrual from F002)
last_slot              u64      [F001]
last_paid_cycle        u32      [F006]
bump                   u8
_reserved              [u8; 64]
```

**ExtraAccountMetaList**: PDA `["extra-account-metas", mint]`, created in F001. It resolves, in this fixed order:

1. `Registry` (**writable**: the hook updates `holder_count`; every transfer of the fund write-locks the Registry, so the fund's transfers are processed one after another within a block)
2. `FundState` (read-only)
3. `HolderState` of source owner (writable; seed from source token account `owner` field)
4. `HolderState` of destination owner (writable; seed from destination token account `owner` field)
5. Governance delegate PDA `["delegate", mint]` (read-only), used to recognise enforcement transfers

Adding entries later changes every client's transfer, so all five exist from F001.

### 4.2 `fund`

**FundState**: PDA `["fund", mint]` (also the mint authority)

**Fund treasury authority**: PDA `["treasury", fund_state]` of `fund`. Owns the fund's own unit holding account (`HolderState` class `FundTreasury`). [F001]

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
redeemed_today         u64      [F003]  units burned by redemptions since day_start_ts
day_start_ts           i64      [F003]  00:00 UTC of the current redemption day (0 = never); rolled over before each mint/burn
cash_treasury          Pubkey   [F003]  token account for cash
current_cycle          u32      [F006]
bump                   u8
nav_period_end_day     u32      [F002]  last UTC day covered by a NavRecord (0 = none); after bump so F001 offsets don't move
pending_override_nav   u64      [F002]  price the next publish may use beyond max_nav_move_bps (0 = none); set only via governance ["fund-caller"]; cleared by every publish
units_at_day_start     u64      [F003]  units_outstanding when the redemption day rolled over (the daily limit's base)
_reserved              [u8; 44]
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

**NavRecord**: PDA `["nav", fund_state, seq]`, immutable once written, never closed. No `_reserved` (immutable). Rent paid by a separate payer signer. [F002]

```text
version u8 · seq u64 · nav u64 · daily_rate_e12 u64 · days_covered u16 · period_start_day u32 · index_after u128 · as_of_ts i64 · published_by Pubkey · override_used bool · bump u8
```

`period_start_day` = the UTC day of the publish for seq 1, else `nav_period_end_day + 1` of the fund before the publish; it must not be after today. `days_covered ≥ 1`, no upper cap. `index_after − previous index_after = daily_rate_e12 × 10^6 × days_covered`.

**DistributionCycle**: PDA `["dist", fund_state, cycle_u32]` [F006]

```text
version u8 · cycle u32 · mode u8 · index_at_close u128 · total_due_e18 u128 · total_paid u64 · holders_paid u32 · status u8 (0 Open, 1 Closed, 2 Finalised) · bump u8 · _reserved
```

**DistributionReceipt**: PDA `["dreceipt", cycle_account, owner]` [F006]. Its existence makes payouts idempotent.

```text
version u8 · cycle u32 · owner Pubkey · amount u64 · paid_ts i64 · bump u8
```

### 4.3 `dvp_settlement` [F003]

**DvpConfig**: PDA `["dvp", fund_state]`. Created by `init_dvp` (issuer) once `FundState.cash_treasury` is set.

```text
version u8 · fund_state Pubkey · next_seq u64 · default_expiry_secs u32 (86_400 unless given) · bump u8 · escrow_bump u8 · _reserved
```

**Escrow authority**: PDA `["escrow", fund_state]`. One per fund. It owns:
- the unit escrow: its ATA for the fund mint (Token-2022), with its own `HolderState` of class `Escrow` (owner = this PDA), registered and approved by compliance and thawed after `init_dvp`;
- the cash escrow: its ATA for `terms.cash_mint` under the cash mint's token program.

Per-instruction amounts are tracked in `SettlementInstruction`, not in separate vaults. The escrow balances are ≥ the sum held by all instructions (I13); anything sent outside an instruction is never released by one.

**SettlementInstruction**: PDA `["settlement", fund_state, created_by, external_ref]`. Never closed. The address is fixed by the creator before submission, so prepared (durable-nonce) steps stay valid; `seq` numbers instructions per fund.

```text
version          u8
fund_state       Pubkey
seq              u64     DvpConfig.next_seq at creation
kind             u8      0 Subscription, 1 Redemption, 2 SecondaryTransfer
seller           Pubkey  Subscription: fund_state; others: unit owner
buyer            Pubkey  Redemption: fund_state; others: unit receiver
units            u64
cash_amount      u64     fixed at creation from FundState.nav (= NavRecord(nav_seq).nav), rounding per section 8; secondary: agreed amount
nav_seq          u64
not_before_ts    i64
expires_at_ts    i64
status           u8      0 Instructed, 1 SellerFunded, 2 BuyerFunded, 3 Affirmed, 4 Settled, 5 Cancelled, 6 Expired, 7 Rejected
seller_funded_at i64     subscription: when the transfer agent affirmed for the Fund
buyer_funded_at  i64
external_ref     [u8; 32] non-zero; unique per creator (in the seeds)
created_by       Pubkey
created_at       i64
closed_at        i64     time of the final status; 0 while open
cancel_requests  u8      bit 0 seller side, bit 1 buyer side (Affirmed only)
units_to_return  u64     earmarked for the seller once Cancelled / Expired / Rejected
cash_to_return   u64     earmarked for the buyer side (redemption: cash_treasury)
bump             u8
_reserved        [u8; 64]
```

Sides: the transfer agent acts for the Fund (seller side of a subscription, buyer side of a redemption). Cancel, expire and reject only earmark; `return_leg` (anyone) delivers each earmarked leg and can be retried.

### 4.4 `collateral` [F004, F005]

**PledgeAgreement**: PDA `["agreement", fund_state, giver, taker, nonce]`. Never closed. [F004]

```text
version                     u8
fund_state                  Pubkey
giver                       Pubkey
taker                       Pubkey
nonce                       u64     chosen by the proposer
mode                        u8      0 Lien (only mode until backlog), 1 TitleTransfer [backlog]
income_to                   u8      0 Giver [backlog for others]
haircut_bps                 u16     < 10_000
max_nav_age_secs            u32     > 0; price fresh iff nav_seq > 0 and now − FundState.nav_ts ≤ this
call_cure_secs              u32     > 0
enforcement_grace_secs      u32
min_transfer_value          u64     cash base units; a shortfall or excess must be MORE than this
concentration_cap_units     u64     pledge limit; > 0
taker_may_call_unilaterally bool    else a raise waits for the giver's confirmation
agreement_hash              [u8; 32] non-zero
status                      u8      0 Proposed, 1 Active, 2 Terminating, 3 Terminated (2, 3 reserved)
giver_signed                bool
taker_signed                bool
pledged_units               u64     Σ active pledge units on this agreement (I15)
required_value              u64     current exposure, cash base units
pledge_count                u64     next Pledge seq (starts 1)
call_count                  u64     next CollateralCall seq (starts 1)
created_ts                  i64
activated_ts                i64     [F004] 0 while Proposed
pending_raise_value         u64     [F004] raise awaiting the giver (0 = none)
pending_raise_ref           [u8; 32] [F004]
pending_raise_ts            i64     [F004]
exposure_ref                [u8; 32] [F004] reference of the exposure in force
exposure_updated_ts         i64     [F004]
called_outstanding          u64     [F004] part of the shortfall already demanded by unresolved delivery calls (I16)
open_delivery_calls         u16     [F004] unresolved delivery calls with seq ≥ met_below_seq (I16)
met_below_seq               u64     [F004] delivery calls with seq below this are Met (derived status)
open_return_call            Pubkey  [F004] default = none; at most one
bump                        u8
_reserved                   [u8; 64]
```

Every step that values an agreement at a fresh price first **observes** coverage:
`S = max(0, required_value − collateral_value(pledged_units))`; `called_outstanding = min(called_outstanding, S)`;
if `S == 0`: `met_below_seq = call_count`, `called_outstanding = 0`, `open_delivery_calls = 0`.
A delivery call opens only for `S − called_outstanding` (> `min_transfer_value`) and sets `called_outstanding = S`. [F004]

**Pledge**: PDA `["pledge", agreement, pledge_ref]` (giver-chosen, non-zero). Never closed. [F004]

```text
version u8 · agreement Pubkey · seq u64 · pledge_ref [u8; 32] · units u64 · mode u8 · posted_at i64 · nav_seq_at_post u64
released_units u64 · enforced_units u64 · status u8 (0 Active, 1 Released, 2 Enforced, 3 Substituted) · closed_at i64 · bump u8 · _reserved
```

**CollateralCall**: PDA `["call", agreement, external_ref]` (non-zero; unique per agreement). Never closed. [F004]

```text
version          u8
agreement        Pubkey
seq              u64     agreement.call_count at creation
kind             u8      0 Delivery, 1 Return
value_delta      u64     cash base units: delivery = uncalled shortfall; return = excess
units_required   u64     at issuance, section 8 rounding (delivery up, return down)
nav_seq          u64
issued_at        i64
cure_deadline_ts i64     issued_at + call_cure_secs
enforce_after_ts i64     cure_deadline_ts + enforcement_grace_secs (delivery); 0 (return)
status           u8      0 Open, 1 Met, 2 Disputed, 3 Defaulted, 4 Enforced, 5 Cancelled
enforced_units   u64     [F005] running total for partial enforcement
enforced_value   u64     [F005]
external_ref     [u8; 32]
units_released   u64     [F004] return calls: units released against it
dispute_ref      [u8; 32] [F004]
disputed_at      i64     [F004]
closed_at        i64     [F004] Met (stored) or Cancelled time; 0 while unresolved
bump             u8
_reserved        [u8; 64]
```

**Derived status** [F004]: a Delivery call with stored status Open or Disputed and `seq < agreement.met_below_seq` **is Met**. Every instruction that touches a call (F004 cancel/dispute, F005 `mark_defaulted`, `enforce`) applies this first; readers apply the same rule. F005 enforcement on a call is limited to both the shortfall and that call's `value_delta`.

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

`init_roles` is signed by the issuer **and the mint keypair**, and runs before `fund::create_fund`, which requires `role_registry.issuer == signer`. The issuer is stored only here. `set_roles` enforces: keeper differs from every other role; compliance differs from issuer. [F001]

**Hook admin**: PDA `["hook-admin"]` of governance. Signs `compliance_hook::set_status` / `set_paused` CPIs. [F001]

**Fund caller**: PDA `["fund-caller"]` of governance. Signs `fund::set_nav_override` / `clear_nav_override` CPIs. Shared by every fund, so governance checks that the role registry is bound to the target fund. [F002]

**DVP caller**: PDA `["dvp-caller"]` of governance. Signs `dvp_settlement::reject` CPIs for `reject_settlement` (compliance). Shared by every fund; governance checks the role registry is bound to the target fund. RejectSettlement accepts the F001 reasons (0, 1, 2, 3, 6). [F003]

**Collateral caller**: PDA `["collateral-caller"]` of governance. Signs `collateral::resolve_dispute` CPIs for `resolve_call_dispute` (compliance). Shared by every fund; governance checks the role registry is bound to the target fund. Accepts the F001 reasons (0, 1, 2, 3, 6). [F004]

**Token admin**: PDA `["token-admin", mint]` of governance. Holds the mint's transfer hook authority. Signs nothing until F007. [F001]

**Delegate authority**: PDA `["delegate", mint]`. This is the mint's permanent delegate. It signs only inside `execute_enforcement` [F005] and `execute_action` for Seize or ForcedTransfer [F007].

**ActionRecord**: PDA `["action", fund_state, seq]`

```text
version          u8
seq              u64
kind             u8   0 FreezeHolder, 1 ThawHolder, 2 SuspendHolder, 3 RevokeHolder, 4 PauseTransfers,
                      5 UnpauseTransfers, 6 Seize, 7 ForcedTransfer, 8 Enforcement, 9 EnforcementHold,
                      10 LiftHold, 11 RejectSettlement, 12 SetRules, 13 CommitRegisterRoot, 14 NavOverride,
                      15 ResolveDispute [F004; record from F007]
reason           u8   0 CourtOrder, 1 SanctionsHit, 2 LostKey, 3 OperationalError, 4 CollateralDefault,
                      5 IssuerWindDown, 6 Other   (NavOverride accepts only 3 and 6 [F002])
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
| `init_roles` | governance | issuer + mint keypair | F001 |
| `set_roles` | governance | issuer (fund in Setup) | F001 |
| `init_registry` (+ ExtraAccountMetaList), `set_rules` | compliance_hook | issuer (fund in Setup) | F001 |
| `register_holder` | compliance_hook | the owner itself (Investor class only) or compliance (any class) | F001 |
| `approve_investor`, `renew_investor` | compliance_hook | compliance + KYC statement (ed25519) | F001 |
| `approve_fund_account` | compliance_hook | compliance (owner must be the system PDA for its class) | F001 |
| `thaw_holding_account` | compliance_hook | anyone (permissionless, state-checked: owner Approved, target is the owner's ATA) | F001 |
| `suspend/reinstate/revoke_holder`, `pause/unpause_transfers` | governance → compliance_hook | compliance | F001 (record from F007) |
| `publish_daily` | fund | fund_admin (+ separate payer) | F002 |
| `approve_nav_override`, `withdraw_nav_override` | governance → fund | compliance | F002 (record from F007) |
| `set_nav_override`, `clear_nav_override` | fund | governance PDA `["fund-caller"]` only | F002 |
| `set_cash_treasury` | fund | issuer (once; Setup or Open) | F003 |
| `init_dvp` | dvp_settlement | issuer (+ separate payer) | F003 |
| `create_instruction` | dvp_settlement | investor or transfer_agent (subscription, redemption); seller or buyer (secondary) (+ separate payer) | F003 |
| `affirm_and_fund`, `cancel`, `withdraw_cancel` | dvp_settlement | the side's party: transfer_agent for the Fund side, else the seller / buyer | F003 |
| `settle`, `expire`, `return_leg` | dvp_settlement | keeper or anyone (permissionless, state-checked) | F003 |
| `reject_settlement` | governance → dvp_settlement | compliance | F003 (record from F007) |
| `reject` | dvp_settlement | governance PDA `["dvp-caller"]` only | F003 |
| `mint_for_subscription`, `burn_for_redemption` | fund | dvp escrow PDA `["escrow", fund_state]` only (CPI) | F003 |
| `accrue` | compliance_hook | `FundState` PDA only (CPI) | F003 |
| `propose_agreement`, `sign_agreement` | collateral | giver or taker (proposer), then the other party; both eligible | F004 |
| `update_exposure` | collateral | taker (a raise waits for `confirm_exposure` unless `taker_may_call_unilaterally`) | F004 |
| `confirm_exposure` | collateral | giver (exact pending value and reference) | F004 |
| `issue_call` | collateral | taker (Delivery) / giver (Return) | F004 |
| `check_coverage` | collateral | anyone (permissionless, state-checked: only observes coverage) | F004 |
| `pledge`, `substitute`, `dispute_call`, `withdraw_dispute` | collateral | giver | F004 |
| `release` | collateral | taker; or keeper, only against the open Return call, within coverage | F004 |
| `cancel_call` | collateral | taker (Delivery) / giver (Return) | F004 |
| `resolve_call_dispute` | governance → collateral | compliance | F004 (record from F007) |
| `resolve_dispute` | collateral | governance PDA `["collateral-caller"]` only | F004 |
| `set_lien` | compliance_hook | collateral PDA `["hook-caller"]` only (CPI) | F004 |
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
| `fund` | `compliance_hook::accrue(post_balance)` | signer = `FundState` PDA (`registry.fund_state`); pre-balance read from the holding account; F003 accepts the `Escrow` class only (F006 widens to investors) |
| `dvp_settlement` | `fund::mint_for_subscription`, `fund::burn_for_redemption` | signer = escrow authority PDA of this fund; the `SettlementInstruction` passed is owned by dvp at its own PDA, same fund, right kind, Affirmed, same units; mint also needs the cash escrow ≥ `cash_amount` (I5); burn enforces the daily limit |
| `governance` | `dvp_settlement::reject` | signer = governance PDA `["dvp-caller"]` [F003] |
| `collateral` | `compliance_hook::set_lien(LienChange { lock, unlock, opened, closed })` | signer = PDA `["hook-caller"]` of `registry.collateral_program` (non-default); holder record and holding account of one owner and this mint; if the lock grows, holding balance ≥ new `pledged_amount` (I2) |
| `governance` | `collateral::resolve_dispute` | signer = governance PDA `["collateral-caller"]` [F004] |
| `collateral` | `governance::execute_enforcement(call)` | signer = collateral PDA `["gov-caller"]`; call account owned by collateral and status Defaulted |
| `governance` | `compliance_hook::set_status` / `set_paused` | signer = governance PDA `["hook-admin"]` |
| `governance` | `fund::set_nav_override` / `clear_nav_override` | signer = governance PDA `["fund-caller"]` [F002] |
| `fund`, `dvp_settlement`, `collateral`, `compliance_hook` | `governance::record_action(kind, reason, ref, target, amount)` | signer = the caller's registered PDA; kind allowed for that caller |

No other CPI edge is allowed without updating this table.

---

## 6. Hook `execute`: fixed order of checks

```text
0. Called by Token-2022 during transfer_checked (source and destination have transferring flag)
1. Load Registry, FundState, HolderState(src owner), HolderState(dst owner), delegate PDA
2. is_enforcement = (transfer authority == delegate PDA)     [F005; false until then]
3. if registry.transfers_paused && !is_enforcement          → TransfersPaused
4. if !is_enforcement: src.status == Approved && kyc valid  → SenderNotApproved
5. if !is_enforcement: post_src_balance >= src.pledged_amount → PledgedUnitsLocked   [F004]
6. dst.status == Approved && kyc valid && class allowed &&
   (dst.class != Investor || jurisdiction allowed)
   (ALWAYS, also for enforcement)                            → ReceiverNotApproved
7. if dst.class == Investor && dst.last_balance == 0 && amount > 0 &&
   max_holders > 0 && holder_count >= max_holders            → HolderCapReached
8. accrue(src, pre_src_balance); accrue(dst, pre_dst_balance)   [F002]
   index = FundState.accrual_index (FundState verified: owner, address, discriminator → WrongMint)
   accrue(h, pre): h.accrued_e18 += pre × (index − h.index_at_last_touch); h.index_at_last_touch = index
9. for each Investor touched (once if src owner == dst owner):
   holder_count += (post_balance > 0) − (last_balance > 0); then
   src.last_balance, dst.last_balance, last_slot updated
10. emit TransferValidated
```

`pre_src = post_src + amount`, `pre_dst = post_dst − amount`. When source and destination are the same owner, accrue once.

Features insert their logic only at their step. F001 builds steps 0, 1, 3, 4, 6, 7, 9 and 10. F002 builds step 8. Step 5 lands with F004 and step 2 with F005, because neither can be exercised by a test before then (Constitution VI). In F004 step 5 therefore applies to every transfer; the governance-delegate bypass arrives with step 2. A missing or foreign `HolderState` counts as not approved (steps 4 and 6). Token-2022 rejects frozen source or destination accounts (`AccountFrozen`) before the hook runs, so that check comes first in the documented order.

Mint and burn do not trigger the hook, so `fund` calls `compliance_hook::accrue` before every mint or burn.

---

## 7. Invariants (tested in every feature from the one that introduces them)

| ID | Invariant | From |
|---|---|---|
| I1 | `HolderState.pledged_amount == Σ (units − released − enforced)` over that holder's active pledges | F004 |
| I2 | `token_balance(holder) >= pledged_amount` at all times | F004 |
| I3 | Only approved holders (or escrow/treasury classes) hold a non-zero balance | F001 |
| I4 | `FundState.units_outstanding == mint supply` | F003 |
| I5 | Every unit minted by subscription has its cash in escrow when minted, and that cash is paid to `cash_treasury` in the same transaction | F003 |
| I6 | A settlement ends with both legs moved or, once its earmarked returns are delivered, every party at its starting units and cash | F003 |
| I13 | Unit escrow balance ≥ Σ units held and cash escrow balance ≥ Σ cash held over all `SettlementInstruction`s (held = funded amounts while open, earmarks once final); equal when nothing was sent to the escrow outside an instruction | F003 |
| I14 | For the current redemption day, `redeemed_today × 10_000 ≤ units_at_day_start × redemption_gate_bps` | F003 |
| I7 | Enforced value ≤ shortfall at the price record used | F005 |
| I8 | Every discretionary admin action has an `ActionRecord` | F007 (F001–F006 call `record_action` once F007 lands; before that, events only) |
| I9 | Σ distribution receipts for a cycle == total due, within rounding (section 8) | F006 |
| I10 | `Registry.holder_count == |{Investor HolderState with last_balance > 0}|`, and never below the number of Investors with an actual balance > 0 | F001 |
| I11 | For every holder, `accrued_e18 + last_balance × (accrual_index − index_at_last_touch)` == Σ over publishes of (units held at that publish × daily_rate_e12 × 10^6 × days_covered) | F002 |
| I12 | `FundState.accrual_index == NavRecord(nav_seq).index_after`; consecutive records' index difference == rate × 10^6 × days; periods contiguous (`period_start_day(n+1) == period_end(n) + 1`) and `nav_period_end_day == period_end(nav_seq)`; every `index_at_last_touch ≤ accrual_index` | F002 |
| I15 | `PledgeAgreement.pledged_units == Σ (units − released − enforced)` over that agreement's Active pledges, and `≤ concentration_cap_units` | F004 |
| I16 | `open_delivery_calls ==` number of Delivery calls with stored status Open/Disputed and `seq ≥ met_below_seq`; `called_outstanding ≤ Σ value_delta` of those calls; `called_outstanding == 0` when that number is 0 | F004 |

---

## 8. Rounding rules (always in favour of the protected party)

| Formula | Rounding | Why |
|---|---|---|
| Collateral value of units: `units × nav × (10_000 − haircut) / (10^8 × 10_000)` | down, once on the agreement's total pledged units | Taker never over-counts collateral [F004: one rounding, no per-pledge sum] |
| Units required for a value: `value × 10^8 × 10_000 / (nav × (10_000 − haircut))` | **up** | Giver always posts enough |
| Units released for a return call's value (same formula) | down | Taker keeps enough [F004] |
| Units taken on enforcement for a shortfall | **down**, then min(available) | Taker never takes more than owed |
| Subscription cash for N units: `N × nav / 10^8` | up | Fund never under-paid |
| Redemption cash for N units | down | Fund never over-pays |
| Interest accrued: `balance × Δindex` (exact in e18) | exact; paid amount = floor(accrued_e18 / 10^18) | Remainder stays in `accrued_e18` for the next cycle |
| Index increment per publish: `daily_rate_e12 × 10^6 × days_covered` | exact | No division [F002] |
| Price-move check: `abs(nav − prev) × 10_000 ≤ prev × max_nav_move_bps` (prev = last nav, or target_nav before the first publish) | exact (cross-multiplied); equal to the limit passes | No division, so no boundary rounding [F002] |
| Redemption limit: `(redeemed_today + units) × 10_000 ≤ units_at_day_start × redemption_gate_bps` | exact (cross-multiplied); equal to the limit passes | No division [F003] |

Worked check (used in tests): NAV 1.00, haircut 2 %, at the smallest unit (6 decimals) [F004]:
- $1,000,000 needs ceil(1,020,408.163265…) = **1,020,408.163266** units (`1_020_408_163_266` base units; 1,020,409 if counted in whole units), worth exactly $1,000,000.000000.
- An excess of $600,000 returns floor(612,244.897959…) = **612,244.897959** units.
- A $500,000 shortfall enforces floor(510,204.081632…) = **510,204.081632** units [F005].

---

## 9. Events (names are shared; payloads include `fund`, `slot`, `unix_ts`)

`RolesInitialized`, `RolesAssigned`, `FundCreated`, `RegistryInitialized`, `RegisterRulesSet`, `FundLaunched`, `HolderRegistered`, `HolderApproved`, `HolderStatusChanged`, `TransfersPaused`, `HoldingAccountThawed`, `TransferValidated` (from F002 also carries `accrual_index`, `source_accrued_e18`, `destination_accrued_e18`), `Accrued` (compliance_hook::accrue, F003; `emit!`: owner, pre_balance, post_balance, accrual_index, accrued_e18), `NavPublished`, `NavOverrideApproved`, `NavOverrideWithdrawn`, `CashTreasurySet`, `SubscriptionMinted`, `RedemptionBurned`, `DvpInitialized`, `SettlementCreated`, `LegFunded`, `Settled`, `CancelRequested`, `CancelRequestWithdrawn`, `SettlementCancelled`, `SettlementExpired`, `SettlementRejected`, `LegReturned`, `AgreementProposed`, `AgreementActive`, `ExposureUpdated` (outcome Applied / Pending / Confirmed; the call it opened, if any), `CallIssued`, `CoverageChecked`, `CallCancelled`, `CallDisputed`, `DisputeWithdrawn`, `DisputeResolved`, `CallDefaulted`, `Pledged`, `Released` (carries `return_call_met`), `Substituted`, `LienSet` (compliance_hook::set_lien, `emit!`) [F004: no `CallMet` event; every valuing event carries `coverage`, `nav_seq`, `called_outstanding`, `met_below_seq`], `HoldPlaced`, `HoldLifted`, `Enforced`, `CycleClosed`, `Distributed`, `CycleFinalised`, `ActionProposed`, `ActionExecuted`, `ActionCancelled`, `RegisterRootCommitted`, `FundWindingDown`, `FundClosed`.

---

## 10. Changes from `docs/design.md`

| Area | Design doc | This file | Reason |
|---|---|---|---|
| Daily yield | `daily_yield_bps: u32` | `daily_rate_e12: u64` | 1 bp = 0.01 % per day is too coarse: 4.5 % a year is ~1.23 bp/day, so bps would round it to 1 bp (~3.65 % a year). |
| Escrow vaults | Per-instruction vaults | One escrow authority per fund; amounts tracked per instruction | Fewer accounts and one approved `Escrow` holder; same safety because amounts are checked per instruction. |
| Holds | Governance action | `EnforcementHold` in `collateral`, recorded via `governance::record_action` | Enforcement reads the hold in its own program; governance still keeps the audit record. |
| Distribution tracking | Receipt PDA only | Receipt PDA + `HolderState.last_paid_cycle` | Cheap check before deriving the receipt. |
| Title transfer, netting, manufactured income | In scope (stretch) | Fields reserved, `[backlog]` | Not in F001–F008. |
| Margin calls | `units_required` recomputed on cure; `meet_call`; substitution with taker affirm; Pledge/Call seeds by `seq` | Demanded part + derived Met (§4.4); pledge and release only; substitution to equal or larger units by the giver; seeds by party reference | A prepared pledge must not depend on the list of open calls or on a sequence number (F004 research R5, R6, R9). |

---

## 11. Field-by-feature checklist

| Feature | Must create | Must fill or activate |
|---|---|---|
| F001 | mint + extensions, `FundState` (Setup/Open), `Registry`, `ExtraAccountMetaList` (all 5 entries), `HolderState`, `RoleRegistry` | hook steps 0, 1, 3, 4, 6, 7, 9, 10; `holder_count`, `last_balance`, `last_slot` |
| F002 | `NavRecord` | `FundState.nav*`, `accrual_index`, `daily_rate_e12`, `nav_period_end_day`, `pending_override_nav`; `HolderState.index_at_last_touch`, `accrued_e18`; hook step 8; governance `["fund-caller"]` and the override instructions |
| F003 | `DvpConfig`, escrow authority + its two ATAs + its `HolderState`, `SettlementInstruction` | `FundState.units_outstanding`, `redeemed_today`, `day_start_ts`, `units_at_day_start`, `cash_treasury`; `compliance_hook::accrue` (escrow class only) and the `fund → accrue` CPI before every mint and burn; Escrow class in `register_holder` / `approve_fund_account`; governance `["dvp-caller"]` and `reject_settlement` |
| F004 | `PledgeAgreement`, `Pledge`, `CollateralCall` | `Registry.collateral_program` (set by `init_registry`); `HolderState.pledged_amount`, `active_liens` (only via `set_lien`); hook step 5 (no bypass until F005 step 2); governance `["collateral-caller"]` and `resolve_call_dispute` |
| F005 | `EnforcementHold` | `CollateralCall.enforced_*`; delegate PDA signing in `execute_enforcement`; hook step 2 |
| F006 | `DistributionCycle`, `DistributionReceipt` | `FundState.current_cycle`; `HolderState.last_paid_cycle`; `accrue` widened to Investor holders with the `holder_count` update |
| F007 | `ActionRecord` (full), `RegisterRoot` | `RoleRegistry.timelock_secs`; `record_action` wired into F001–F006 admin paths |
| F008 | none on-chain | reads everything; must not require new on-chain fields |

---

## 12. Token-2022 behaviours every feature must design for

Checked against Token-2022 program v11.0.0 (mainnet) and v11.1.0 (devnet) in F001 research.

| Behaviour | Consequence |
|---|---|
| Mint extensions can only be set before `InitializeMint`. | The F001 extension set (§3) is final. A new mint-level extension means a new mint. |
| Without permissioned burn, an owner can burn its own units with no hook call. | Permissioned burn is enabled (§3). Burns go only through `fund`, which settles accrual first (F002) and keeps `units_outstanding` (I4). |
| A transfer whose source and destination are the same token account returns before the hook runs. | Nothing moves; no rule applies. Each owner has one usable account (§1), so "send to yourself" is always this case. |
| The hook runs for zero-amount `transfer_checked`; unchecked `Transfer` fails for hooked mints. | Zero-unit transfers exercise every check (used by the F001 devnet run). |
| Frozen source or destination fails with `AccountFrozen` before the hook; that also applies to the permanent delegate. | Default-frozen accounts block unregistered owners. Suspension does not freeze (F001 research R13), so F005 enforcement and F007 recovery are not blocked by a freeze. |
| Transfers by the permanent delegate also go through the hook. | Step 2 (F005) recognises them; the receiver check still applies. |
| A program calling `transfer_checked` by CPI must pass the extra accounts with the flags in §4.1, including the **writable** `Registry`. | Applies to `dvp_settlement` (F003) and `governance` (F005, F007). |
| Holder-side account extensions (required memo on incoming transfers, CPI guard) are set by the account owner. | A receiver could make incoming transfers fail. F003 settlements must tolerate this through expiry and refund; F005 and F006 must decide how to handle it. |
| Anchor 1.x rejects the same writable typed account passed twice (error 2040). | The hook loads `HolderState` as unchecked accounts and writes once when source owner == destination owner. |
