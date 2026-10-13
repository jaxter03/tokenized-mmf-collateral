# Contract: `dvp_settlement` program (new in F003)

Checks run in the order listed. The first failing check aborts the transaction with that error
(Constitution VII), and every instruction emits exactly one event with `emit_cpi!`
(shared-model §1). `DvpError` is a new enum. Account reads of other programs go through
`mmf_common::views`, which check the owner, the address and the discriminator. A view that fails
to load is reported as the error named in its row.

Common definitions:
- `now = Clock.unix_timestamp`.
- `eligible(h)`: `h.class == Investor ∧ h.status == Approved ∧ h.kyc_expiry_ts > now` (R12).
- `open(s)`: `s.status ∈ {Instructed, SellerFunded, BuyerFunded, Affirmed}`.
- **Side party**:
  - Seller side: the transfer agent for a Subscription, otherwise `seller`.
  - Buyer side: the transfer agent for a Redemption, otherwise `buyer`.

Common accounts are written once here and referred to by name below:

| Account | Kind | Constraint |
|---|---|---|
| `fund_state` | read | FundState, via `FundStateView` |
| `role_registry` | read | RoleRegistry at `fund_state.role_registry`, via `RoleRegistryView` |
| `registry` | read (writable where the hook runs) | Registry `["registry", mint]` of compliance_hook, via `RegistryView` |
| `dvp_config` | read / writable | PDA `["dvp", fund_state]` |
| `escrow_authority` | read | PDA `["escrow", fund_state]`, bump `dvp_config.escrow_bump` |
| `unit_escrow` | writable | ATA(escrow_authority, mint, Token-2022) |
| `cash_escrow` | writable | ATA(escrow_authority, cash_mint, cash token program) |
| `settlement` | writable | PDA `["settlement", fund_state, s.created_by, s.external_ref]` |
| `hook accounts` | per shared-model §4.1 | `extra-account-metas`, Registry (writable), FundState, source and destination HolderState (writable), delegate PDA, hook program |

## `init_dvp(default_expiry_secs: Option<u32>)`: issuer

Sets up settlement for a Fund (FR-001, FR-002).

| Account | Kind | Constraint |
|---|---|---|
| `issuer` | signer | `== role_registry.issuer` |
| `payer` | signer, writable | rent for the config and both ATAs |
| `fund_state`, `role_registry` | read | |
| `mint` | read | `fund_state.mint` |
| `cash_mint` | read | `fund_state.terms.cash_mint` |
| `dvp_config` | writable, init | PDA `["dvp", fund_state]` |
| `escrow_authority` | read | PDA `["escrow", fund_state]` |
| `unit_escrow`, `cash_escrow` | writable, init | ATAs as above |
| `token_2022_program`, `cash_token_program`, `associated_token_program`, `system_program` | programs | `cash_token_program == cash_mint.owner` |

| # | Check | Error |
|---|---|---|
| 1 | FundState and RoleRegistry load and are bound to each other | `FundMismatch` |
| 2 | `issuer == role_registry.issuer` | `Unauthorized` |
| 3 | `fund_state.status ∈ {Setup, Open}` | `FundNotOpen` |
| 4 | `fund_state.cash_treasury ≠ default` | `CashTreasuryNotSet` |
| 5 | `cash_mint` has no transfer-fee or transfer-hook extension | `UnsupportedCashMint` |
| 6 | `default_expiry_secs ≠ Some(0)` | `InvalidExpiry` |
| 7 | `dvp_config` doesn't exist yet | (Anchor `init`: already in use) |

**Effect**: `DvpConfig { next_seq: 1, default_expiry_secs: d.unwrap_or(86_400), escrow_bump }`.
The unit escrow (frozen by default) and the cash escrow are created. **Event**: `DvpInitialized`.

After `init_dvp`, compliance registers and approves the escrow holder and anyone thaws the unit
escrow (F001 instructions; see [compliance_hook.md](compliance_hook.md)).

## `create_instruction(args)`: investor, transfer agent, or a secondary party

```text
args: kind u8, seller Pubkey, buyer Pubkey, units u64, cash_amount u64 (secondary only; else 0),
      not_before_ts i64 (0 = now), expires_at_ts i64 (0 = now + default), external_ref [u8; 32]
```

For a Subscription `seller` must be `fund_state`; for a Redemption `buyer` must be `fund_state`.

| Account | Kind | Constraint |
|---|---|---|
| `creator` | signer | |
| `payer` | signer, writable | rent for the instruction |
| `fund_state`, `role_registry`, `dvp_config` (writable) | | |
| `escrow_holder` | read | HolderState of `escrow_authority` |
| `seller_holder` | read, optional | HolderState of `seller`; absent for a Subscription (Anchor optional account) |
| `buyer_holder` | read, optional | HolderState of `buyer`; absent for a Redemption |
| `settlement` | writable, init | PDA `["settlement", fund_state, creator, external_ref]` |
| `system_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | FundState and RoleRegistry load and are bound | `FundMismatch` |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | `dvp_config` loads; escrow HolderState is `Escrow`, Approved | `EscrowNotReady` |
| 4 | `kind` is 0, 1 or 2; the Fund appears exactly where the kind requires | `InvalidKind` |
| 5 | creator allowed: Sub/Red: investor or `transfer_agent`; Secondary: `seller` or `buyer` | `Unauthorized` |
| 6 | `units > 0` | `ZeroUnits` |
| 7 | `external_ref ≠ [0; 32]` | `ZeroExternalRef` |
| 8 | Secondary: `seller ≠ buyer` | `SameParty` |
| 9 | Sub/Red: `fund.nav_seq > 0` | `NoPrice` |
| 10 | Sub/Red: `fund.nav_period_end_day + 1 ≥ utc_day(now)` | `StalePrice` |
| 11 | cash: Sub `ceil`, Red `floor` of `units × nav / 10^8` (cash_amount arg must be 0); Secondary: arg. Result > 0 | `ZeroCash` (`CashAmountNotAllowed` if a Sub/Red arg ≠ 0) |
| 12 | `nb = not_before_ts or now`; `exp = expires_at_ts or now + default`; `exp > now ∧ exp > nb` | `InvalidExpiry` |
| 13 | every non-Fund party's HolderState (correct PDA for this mint) is `eligible` | `PartyNotEligible` |
| 14 | arithmetic and `next_seq + 1` fit | `Overflow` |
| 15 | the PDA is free (same creator and external_ref used before) | (Anchor `init`: already in use) |

**Effect**: a new SettlementInstruction with `seq = next_seq`, `status = Instructed`,
`created_at = now`, `nav_seq` (Sub/Red: `fund.nav_seq`; Secondary: `fund.nav_seq`, which may be
0). `next_seq += 1`. **Event**: `SettlementCreated`.

## `affirm_and_fund(side: u8)`: the side's party

Funds one leg (FR-011 to FR-014). `side`: 0 Seller, 1 Buyer.

| Account | Kind | Constraint |
|---|---|---|
| `party` | signer | the side's party |
| `fund_state`, `role_registry`, `registry`, `dvp_config`, `settlement` (writable) | | |
| `party_holder` | read | HolderState of the investor party (not for a transfer-agent side) |
| `source` | writable | units: party's holding account; cash: any account the party can authorise; Redemption buyer side: must be `fund_state.cash_treasury` |
| `unit_escrow` or `cash_escrow` | writable | destination |
| `mint` / `cash_mint`, token program | | |
| hook accounts | | only for a unit leg |

| # | Check | Error |
|---|---|---|
| 1 | FundState, RoleRegistry, Registry, DvpConfig load and are bound; `settlement.fund_state == fund_state` | `FundMismatch` |
| 2 | `side ∈ {0, 1}` | `InvalidSide` |
| 3 | `open(settlement)` | `InstructionFinal` |
| 4 | `now < expires_at_ts` | `InstructionExpired` |
| 5 | signer is the side's party | `Unauthorized` |
| 6 | the side's leg isn't funded yet (`seller_funded_at` / `buyer_funded_at == 0`) | `LegAlreadyFunded` |
| 7 | investor party is `eligible` | `PartyNotEligible` |
| 8 | Redemption buyer side: `source == cash_treasury` | `WrongCashSource` |
| 9 | token accounts have the right mint and destination | `WrongAccount` |
| — | unit leg: Token-2022 and the hook run in full (`TransfersPaused`, `SenderNotApproved`, F004 `PledgedUnitsLocked`, …) | hook errors |

**Effect**:
- Seller side: units are moved to the unit escrow by `transfer_checked` CPI (party is the
  authority, R5). For a Subscription nothing moves; this is the Fund's affirmation.
  `seller_funded_at = now`.
- Buyer side: `cash_amount` is moved to the cash escrow. `buyer_funded_at = now`.
- `status` becomes SellerFunded or BuyerFunded if the other leg is unfunded, else Affirmed.

**Event**: `LegFunded`. Funding before `not_before_ts` is allowed.

## `settle()`: anyone (keeper normally)

Both legs in one instruction. **Constitution IV exception**: atomicity is the point. Splitting it
would reintroduce the risk of paying without receiving.

| Account | Kind | Constraint |
|---|---|---|
| `triggered_by` | signer | any key; recorded only |
| `fund_state` (writable for Sub/Red), `role_registry`, `registry` (writable), `dvp_config`, `settlement` (writable) | | |
| `seller_holder`, `buyer_holder` | read / writable | HolderStates of non-Fund parties |
| `escrow_authority`, `unit_escrow`, `cash_escrow` | | |
| `buyer_units` | writable | buyer's holding account (Sub, Secondary) |
| `seller_cash` | writable | ATA(seller, cash_mint) (Red, Secondary) |
| `cash_treasury` | writable | `fund_state.cash_treasury` (Sub) |
| `mint`, `cash_mint`, token programs, hook accounts | | |
| `fund_program`, `fund_event_authority`, `escrow_holder` (writable), `compliance_hook_program` | | Sub/Red only (CPI) |

| # | Check | Error |
|---|---|---|
| 1 | accounts load and are bound | `FundMismatch` |
| 2 | `settlement.status == Affirmed` (final → `InstructionFinal`) | `NotAffirmed` |
| 3 | `now ≥ not_before_ts` | `NotYetValid` |
| 4 | `now < expires_at_ts` | `InstructionExpired` |
| 5 | `fund_state.status == Open` | `FundNotOpen` |
| 6 | `!registry.transfers_paused` | `TransfersPaused` |
| 7 | every non-Fund party `eligible` | `PartyNotEligible` |
| 8 | destination accounts are the parties' own (holding account / cash ATA / `cash_treasury`) | `WrongAccount` |
| — | Sub: fund `mint_for_subscription` checks ([fund.md](fund.md)); Red: `burn_for_redemption` incl. `RedemptionLimitExceeded`; unit delivery: hook incl. `ReceiverNotApproved`, `HolderCapReached` | callee errors |

**Effect**:
- **Subscription**: CPI `fund::mint_for_subscription(units)` (escrow PDA signs), then
  `transfer_checked` unit escrow → buyer, then cash escrow → `cash_treasury`.
- **Redemption**: CPI `fund::burn_for_redemption(units)` (escrow PDA signs), then cash escrow →
  seller's cash ATA.
- **Secondary**: unit escrow → buyer (hook), then cash escrow → seller's cash ATA.

Then `status = Settled`, `closed_at = now`. **Event**: `Settled`.

## `cancel(side: u8)`: the side's party

| # | Check | Error |
|---|---|---|
| 1 | accounts bound | `FundMismatch` |
| 2 | `side ∈ {0, 1}` | `InvalidSide` |
| 3 | `open(settlement)` | `InstructionFinal` |
| 4 | signer is the side's party | `Unauthorized` |
| 5 | Affirmed: the side's bit isn't already set | `CancelAlreadyRequested` |

**Effect**:
- Instructed, SellerFunded or BuyerFunded: earmark the funded legs (`units_to_return`,
  `cash_to_return`, data-model formulas), then `status = Cancelled`, `closed_at = now`.
  **Event**: `SettlementCancelled`.
- Affirmed: set the side's bit. If both bits are now set, earmark and cancel as above
  (`SettlementCancelled`); otherwise emit `CancelRequested`.

Cancelling after expiry is allowed (spec Edge Cases). No tokens move (R3).

## `withdraw_cancel(side: u8)`: the side's party

| # | Check | Error |
|---|---|---|
| 1–4 | as `cancel` | |
| 5 | `status == Affirmed`, own bit set, other bit not set | `NoCancelRequest` |

**Effect**: clears own bit. **Event**: `CancelRequestWithdrawn`.

## `expire()`: anyone

| # | Check | Error |
|---|---|---|
| 1 | accounts bound | `FundMismatch` |
| 2 | `open(settlement)` | `InstructionFinal` |
| 3 | `now ≥ expires_at_ts` | `NotExpired` |

**Effect**: earmark the funded legs, `status = Expired`, `closed_at = now`. **Event**:
`SettlementExpired`. Works whatever the Fund's status or the pause flag.

## `reject(reason: u8, case_ref_hash: [u8; 32], actor: Pubkey)`: governance only

Called by `governance::reject_settlement` ([governance.md](governance.md)).

| Account | Kind | Constraint |
|---|---|---|
| `dvp_caller` | signer | PDA `["dvp-caller"]` of **governance** **[SM change: new PDA]** |
| `fund_state`, `settlement` (writable) | | |

| # | Check | Error |
|---|---|---|
| 1 | signer is governance's `["dvp-caller"]` | `UnauthorizedCaller` |
| 2 | `settlement.fund_state == fund_state` | `FundMismatch` |
| 3 | `open(settlement)` | `InstructionFinal` |

**Effect**: earmark the funded legs, `status = Rejected`, `closed_at = now`. **Event**:
`SettlementRejected { reason, case_ref_hash, actor, … }`. Governance has already checked the role,
the fund binding and the reason.

## `return_leg(leg: u8)`: anyone

Delivers one earmarked leg (FR-026, R3). `leg`: 0 units, 1 cash.

| Account | Kind | Constraint |
|---|---|---|
| `triggered_by` | signer | any key |
| `settlement` (writable), `fund_state`, `dvp_config`, `escrow_authority` | | |
| units: `unit_escrow`, `seller_units` (seller's holding account), hook accounts, Registry (writable) | | |
| cash: `cash_escrow`, `recipient_cash` | | ATA(buyer, cash_mint), or `cash_treasury` for a Redemption |

| # | Check | Error |
|---|---|---|
| 1 | accounts bound | `FundMismatch` |
| 2 | `leg ∈ {0, 1}` | `InvalidLeg` |
| 3 | `status ∈ {Cancelled, Expired, Rejected}` | `NotReturnable` |
| 4 | the leg's earmark > 0 | `NothingToReturn` |
| 5 | recipient is the funder's own account | `WrongAccount` |
| — | units: hook (`TransfersPaused`, `ReceiverNotApproved` …); cash: token program | callee errors |

**Effect**: transfer the earmarked amount from escrow (escrow PDA signs), set the earmark to 0.
**Event**: `LegReturned`. A failed return leaves the earmark in place, so it can be retried.

## Errors (`DvpError`, new)

`FundMismatch`, `Unauthorized`, `UnauthorizedCaller`, `FundNotOpen`, `CashTreasuryNotSet`,
`UnsupportedCashMint`, `EscrowNotReady`, `InvalidKind`, `InvalidSide`, `InvalidLeg`, `ZeroUnits`,
`ZeroCash`, `CashAmountNotAllowed`, `ZeroExternalRef`, `SameParty`, `NoPrice`, `StalePrice`,
`InvalidExpiry`, `PartyNotEligible`, `InstructionFinal`, `InstructionExpired`, `LegAlreadyFunded`,
`WrongCashSource`, `WrongAccount`, `NotAffirmed`, `NotYetValid`, `TransfersPaused`,
`CancelAlreadyRequested`, `NoCancelRequest`, `NotExpired`, `NotReturnable`, `NothingToReturn`,
`Overflow`.

**Rejection tests**: one per error (SC-006). Also:
- a prepared funding step submitted 6 h and 23 h later succeeds; one for a since-cancelled
  instruction fails (US5);
- the same signed step resubmitted fails;
- settle twice; settle at exactly `expires_at_ts` fails and at `not_before_ts` succeeds;
- expire at exactly `expires_at_ts` succeeds;
- a stray deposit into either escrow is never released (I13);
- a suspended seller's unit return fails, then succeeds after reinstatement;
- every instruction is sent directly, with no SDK checks in the path (Constitution II).
