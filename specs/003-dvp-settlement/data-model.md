# Data Model: Subscription, Redemption and Transfer Settlement (F003)

This file maps the spec's entities to on-chain accounts in `docs/shared-model.md`. It lists only
what F003 creates, fills or changes. Everything else is as in
[F001's](../001-fund-launch-onboarding/data-model.md) and
[F002's](../002-daily-price-accrual/data-model.md) data models. Changes to the shared model are
marked **[SM change]** and listed in [plan.md](plan.md#shared-model-changes).

| Spec entity | Account | Program | PDA seeds |
|---|---|---|---|
| Fund (extended) | `FundState` | `fund` | `["fund", mint]` |
| Holder record (escrow) | `HolderState`, class `Escrow` | `compliance_hook` | `["holder", mint, escrow_authority]` |
| Price record (read) | `FundState.nav`, `nav_seq` (= `NavRecord(nav_seq)`, I12) | `fund` | — |
| Settlement settings | `DvpConfig` | `dvp_settlement` | `["dvp", fund_state]` |
| Escrow | escrow authority PDA + its two ATAs | `dvp_settlement` | `["escrow", fund_state]` |
| Settlement instruction | `SettlementInstruction` | `dvp_settlement` | `["settlement", fund_state, created_by, external_ref]` **[SM change: seeds, R2]** |

## Units and formulas

From shared-model §1 and §8. The formulas live in a new `mmf_common::settlement` module, shared
by `dvp_settlement`, `fund` and `mmf-sdk`. The test model keeps its own copy (R18).

```text
subscription_cash(units, nav) = ceil(units × nav / 10^8)        rounds up: the Fund is never under-paid       (§8, existing row)
redemption_cash(units, nav)   = floor(units × nav / 10^8)       rounds down: the Fund never over-pays         (§8, existing row)
redemption_ok(redeemed_today, units, at_day_start, gate_bps)
    = (redeemed_today + units) × 10_000 ≤ at_day_start × gate_bps     exact, cross-multiplied              [SM change: new §8 row]
eligible(h, now) = h.class == Investor ∧ h.status == Approved ∧ h.kyc_expiry_ts > now
price_fresh(fund, now) = fund.nav_seq > 0 ∧ fund.nav_period_end_day + 1 ≥ utc_day(now)
```

All products are in checked `u128`. A result that doesn't fit `u64` is `Overflow`.

Worked checks (tests):
- 2 000 000.000000 units (`2_000_000_000_000`) at $1.00 → `2_000_000_000_000` cash base ($2M).
- 1 base unit at $0.99999999 → subscription 1, redemption 0 (`ZeroCash`).
- 1_000_001 base units at $0.99999999 (`99_999_999`): product `100_000_098_999_999`, ÷ 10^8 =
  1_000_000.98999999 → subscription `1_000_001`, redemption `1_000_000`.

## `FundState` (fund): fields F003 fills

```text
units_outstanding      u64      [F003]  == mint supply (I4); += on mint_for_subscription, −= on burn_for_redemption
redeemed_today         u64      [F003]  units burned by redemptions since day_start_ts
day_start_ts           i64      [F003]  00:00 UTC of the current redemption day; 0 = never rolled
cash_treasury          Pubkey   [F003]  token account of terms.cash_mint; set once by set_cash_treasury
…
pending_override_nav   u64      [F002]
units_at_day_start     u64      [F003]  units_outstanding when the current day rolled over      [SM change: from _reserved]
_reserved              [u8; 44]          was [u8; 52]                                            [SM change]
```

`units_at_day_start` comes after `pending_override_nav`, so no F001 or F002 offset moves
(`tests/tests/layouts.rs` checks this).

**Day rollover** (in `fund`, before every mint and burn, R9):

```text
today = utc_day(now)
if day_start_ts == 0 ∨ utc_day(day_start_ts) < today:
    day_start_ts = today × 86_400; redeemed_today = 0; units_at_day_start = units_outstanding
```

## `DvpConfig` (dvp_settlement): new

PDA `["dvp", fund_state]`. Created by `init_dvp`. Never closed.

```text
version              u8       1
fund_state           Pubkey
next_seq             u64      seq of the next instruction; starts at 1
default_expiry_secs  u32      86_400 unless given at init (R20)
bump                 u8
escrow_bump          u8       bump of ["escrow", fund_state]          [SM change: new field]
_reserved            [u8; 64]
```

Size: 8 + 1 + 32 + 8 + 4 + 1 + 1 + 64 = **119 bytes**.

## Escrow

- **Escrow authority**: PDA `["escrow", fund_state]` of `dvp_settlement`. Has no data. Signs every
  movement out of escrow and the mint/burn CPIs.
- **Unit escrow**: the escrow authority's ATA for the fund mint (Token-2022). Created by
  `init_dvp`, frozen by default. Becomes usable after compliance registers and approves the
  escrow's HolderState (class `Escrow`, owner = escrow authority) and anyone runs
  `thaw_holding_account` (F001).
- **Cash escrow**: the escrow authority's ATA for `terms.cash_mint`, under the cash mint's token
  program. Created by `init_dvp`.

`compliance_hook::register_holder` and `approve_fund_account` accept class `Escrow` when
`owner == PDA(["escrow", registry.fund_state], DVP_SETTLEMENT_PROGRAM_ID)`. F001's
`ClassNotAvailable` for Escrow goes away; `NotSystemControlled` covers any other owner.

## `SettlementInstruction` (dvp_settlement): new

PDA `["settlement", fund_state, created_by, external_ref]` **[SM change: seeds]**. Created by
`create_instruction` with rent from a separate `payer` signer. **Never closed**, so every
instruction stays readable (FR-006, SC-008).

```text
version           u8       1
fund_state        Pubkey                                                    [SM change: new field]
seq               u64      1, 2, 3, … per fund (DvpConfig.next_seq at creation)
kind              u8       0 Subscription, 1 Redemption, 2 SecondaryTransfer
seller            Pubkey   Subscription: fund_state; otherwise the unit owner
buyer             Pubkey   Redemption: fund_state; otherwise the unit receiver
units             u64      > 0, base units
cash_amount       u64      > 0, cash base units, fixed at creation (§8)
nav_seq           u64      FundState.nav_seq at creation
not_before_ts     i64      valid-from; settlement only at or after it
expires_at_ts     i64      > max(created_at, not_before_ts)
status            u8       0 Instructed, 1 SellerFunded, 2 BuyerFunded, 3 Affirmed,
                           4 Settled, 5 Cancelled, 6 Expired, 7 Rejected
seller_funded_at  i64      0 = not funded (subscription: when the transfer agent affirmed)
buyer_funded_at   i64      0 = not funded
external_ref      [u8; 32] ≠ all zero; unique per creator (it is in the seeds)
created_by        Pubkey
created_at        i64                                                       [SM change: new field]
closed_at         i64      time of the final status; 0 while open           [SM change: new field]
cancel_requests   u8       bit 0 seller side, bit 1 buyer side (Affirmed only, R17)   [SM change: new field]
units_to_return   u64      earmarked units owed back to the seller (R3)     [SM change: new field]
cash_to_return    u64      earmarked cash owed back to the buyer side (R3)  [SM change: new field]
bump              u8
_reserved         [u8; 64]
```

Size: 8 + 1 + 32 + 8 + 1 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1 + 8 + 8 + 32 + 32 + 8 + 8 + 1 + 8 + 8
+ 1 + 64 = **333 bytes**.

**Parties and sides**:

| Kind | Seller side signs as | Seller leg | Buyer side signs as | Buyer leg |
|---|---|---|---|---|
| Subscription | transfer agent | affirm only (units minted at settle) | investor (`buyer`) | `cash_amount` → cash escrow |
| Redemption | investor (`seller`) | `units` → unit escrow | transfer agent | `cash_amount` from `cash_treasury` → cash escrow |
| SecondaryTransfer | holder (`seller`) | `units` → unit escrow | holder (`buyer`) | `cash_amount` → cash escrow |

Who may create: Subscription and Redemption, the investor or the transfer agent. Secondary, the
seller or the buyer (Clarification Q1).

**Where each leg goes**:

| Outcome | Units | Cash |
|---|---|---|
| Settled: Subscription | minted to unit escrow, then sent to the buyer's holding account | cash escrow → `cash_treasury` |
| Settled: Redemption | burned from unit escrow | cash escrow → seller's cash ATA |
| Settled: Secondary | unit escrow → buyer's holding account | cash escrow → seller's cash ATA |
| Returned (any final non-settled) | unit escrow → seller's holding account | cash escrow → buyer's cash ATA, or `cash_treasury` for a redemption |

**Escrow held by an instruction** (FR-030, I13):

```text
open (status < Settled):
    units_held = units        if seller_funded_at ≠ 0 ∧ kind ≠ Subscription, else 0
    cash_held  = cash_amount  if buyer_funded_at ≠ 0, else 0
final:
    units_held = units_to_return; cash_held = cash_to_return
```

### State transitions

```text
create_instruction [creator]                       → Instructed
affirm_and_fund(Seller) / (Buyer) [that side]      Instructed → SellerFunded / BuyerFunded
                                                    SellerFunded --Buyer--> Affirmed; BuyerFunded --Seller--> Affirmed
settle [anyone]                                     Affirmed → Settled            (both legs move, one instruction)
cancel(side) [that side]                            Instructed | SellerFunded | BuyerFunded → Cancelled
                                                    Affirmed: set side bit; both bits → Cancelled
withdraw_cancel(side) [that side]                   Affirmed: clear own bit (other bit unset)
expire [anyone, now ≥ expires_at_ts]                any open status → Expired
reject [governance "dvp-caller", for compliance]    any open status → Rejected
return_leg(leg) [anyone]                            final, leg earmarked → leg delivered, earmark cleared
```

On Cancelled, Expired or Rejected: `units_to_return = units_held`, `cash_to_return = cash_held`
(open formulas above), and `closed_at = now`. Settled sets `closed_at` and leaves both earmarks at
0.

**Validation** (full ordered lists in [contracts/dvp_settlement.md](contracts/dvp_settlement.md)):

| Rule | Error |
|---|---|
| fund Open | `FundNotOpen` |
| settlement set up and escrow HolderState Approved | `EscrowNotReady` |
| creator allowed for kind | `Unauthorized` |
| `units > 0` | `ZeroUnits` |
| `external_ref ≠ 0` | `ZeroExternalRef` |
| secondary: `seller ≠ buyer`, `cash_amount > 0` | `SameParty`, `ZeroCash` |
| price exists and is fresh | `NoPrice`, `StalePrice` |
| sub/red cash > 0 | `ZeroCash` |
| `expires_at > max(now, not_before)` | `InvalidExpiry` |
| every non-Fund party eligible | `PartyNotEligible` |
| arithmetic fits | `Overflow` |
| step on a final instruction (except `return_leg`) | `InstructionFinal` |
| funding at or after expiry | `InstructionExpired` |
| leg already funded | `LegAlreadyFunded` |
| signer not the side's party | `Unauthorized` |
| settle not Affirmed / before valid-from / at or after expiry / paused | `NotAffirmed`, `NotYetValid`, `InstructionExpired`, `TransfersPaused` |
| expire before expiry | `NotExpired` |
| return with nothing earmarked | `NothingToReturn` |
| withdraw with no own request, or after both | `NoCancelRequest` |

## `HolderState` (compliance_hook): changes

No layout change. The escrow's HolderState (class `Escrow`) is now possible. `accrue` (R8) writes
`index_at_last_touch`, `accrued_e18`, `last_balance` and `last_slot`, for the escrow holder only
(class `Escrow`; F006 widens it to investors together with the `holder_count` update).

```text
accrue(post_balance) [FundState PDA]:
    pre = holding_account.amount
    require h.class == Escrow                       (ClassNotAccrued)
    settle(h, pre, fund.accrual_index)              F002 formula, exact
    h.last_balance = post_balance; h.last_slot = slot
```

## Events

| Event | Emitted by | Payload (besides `fund`, `slot`, `unix_ts`) |
|---|---|---|
| `DvpInitialized` **[SM change: new]** | dvp `init_dvp` | default_expiry_secs, escrow_authority, unit_escrow, cash_escrow, signer |
| `SettlementCreated` | dvp `create_instruction` | seq, address, kind, seller, buyer, units, cash_amount, nav_seq, not_before_ts, expires_at_ts, external_ref, created_by |
| `LegFunded` | dvp `affirm_and_fund` | seq, side, amount (0 for subscription affirm), status_after, signer, external_ref |
| `Settled` | dvp `settle` | seq, kind, units, cash_amount, seller, buyer, nav_seq, triggered_by, external_ref |
| `CancelRequested` **[SM change: new]** | dvp `cancel` (first side, Affirmed) | seq, side, signer |
| `CancelRequestWithdrawn` **[SM change: new]** | dvp `withdraw_cancel` | seq, side, signer |
| `SettlementCancelled` | dvp `cancel` | seq, units_to_return, cash_to_return, signer |
| `SettlementExpired` | dvp `expire` | seq, units_to_return, cash_to_return, triggered_by |
| `SettlementRejected` | dvp `reject` | seq, reason, case_ref_hash, actor (compliance), units_to_return, cash_to_return |
| `LegReturned` **[SM change: new]** | dvp `return_leg` | seq, leg, amount, recipient, triggered_by |
| `CashTreasurySet` **[SM change: new]** | fund `set_cash_treasury` | cash_treasury, signer |
| `SubscriptionMinted` **[SM change: new]** | fund `mint_for_subscription` | settlement, units, units_outstanding |
| `RedemptionBurned` **[SM change: new]** | fund `burn_for_redemption` | settlement, units, units_outstanding, redeemed_today, units_at_day_start |
| `Accrued` | hook `accrue` (`emit!`) | owner, pre_balance, post_balance, accrual_index, accrued_e18 |

Every record carries the instruction's `seq` and `external_ref` where relevant (FR-031). SC-008
(rebuild from records alone) is tested by replaying these events in `tests/src/rebuild.rs`.

## Invariants

| ID | Invariant | Where tested |
|---|---|---|
| I3, I10, I11, I12 | from F001/F002, still hold with settlements interleaved | `dvp_prop.rs`, existing suites |
| I4 | `FundState.units_outstanding == mint supply` | every F003 test helper; `dvp_prop.rs` |
| I5 | when `mint_for_subscription` runs, the cash escrow holds ≥ that instruction's `cash_amount`, and the same transaction pays it to `cash_treasury` | `f003_subscribe.rs`, `dvp_prop.rs` |
| I6 | a final instruction has both legs moved, or (after its returns) every party at its starting units and cash | `dvp_prop.rs` (SC-001) against the reference model |
| I13 **[SM change: new]** | unit escrow balance ≥ Σ units_held and cash escrow balance ≥ Σ cash_held over all instructions; equal when nothing was sent to escrow outside an instruction | `dvp_prop.rs` (SC-003), `f003_escrow.rs` |
| I14 **[SM change: new]** | for the current day, `redeemed_today × 10_000 ≤ units_at_day_start × redemption_gate_bps` | `dvp_prop.rs` (SC-005), `f003_redeem.rs` |
