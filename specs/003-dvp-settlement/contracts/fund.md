# Contract: `fund` program (F003 instructions)

Same conventions as F002's [fund contract](../../002-daily-price-accrual/contracts/fund.md): checks
run in order, the first failure aborts, there is one event per instruction (`emit_cpi!`), and new
`FundError` variants are **appended** so earlier codes keep their numbers. F001's and F002's
instructions are unchanged.

`escrow_pda(fund_state) = find_program_address(["escrow", fund_state], DVP_SETTLEMENT_PROGRAM_ID)`.

## `set_cash_treasury()`: issuer

Designates the fund's treasury cash account, once (FR-001, R6).

| Account | Kind | Constraint |
|---|---|---|
| `issuer` | signer | `== role_registry.issuer` |
| `fund_state` | writable | |
| `role_registry` | read | at `fund_state.role_registry` |
| `cash_treasury` | read | token account |
| `cash_mint` | read | `== fund_state.terms.cash_mint` |
| `event_authority`, `program` | `#[event_cpi]` | |

| # | Check | Error |
|---|---|---|
| 1 | role registry owned by governance, at `fund_state.role_registry` | `RoleRegistryMismatch` |
| 2 | `issuer == role_registry.issuer` | `Unauthorized` |
| 3 | `status ∈ {Setup, Open}` | `FundNotOpen` |
| 4 | `fund_state.cash_treasury == default` | `CashTreasuryAlreadySet` **(new)** |
| 5 | `cash_treasury` is an initialized token account of `terms.cash_mint`, owned by the cash mint's token program | `InvalidCashTreasury` **(new)** |
| 6 | the cash mint has no transfer-fee or transfer-hook extension | `UnsupportedCashMint` **(new)** |

**Effect**: `fund_state.cash_treasury = cash_treasury`. **Event**: `CashTreasurySet`
**[SM change: new]**.

## `mint_for_subscription(units: u64)`: dvp escrow PDA only (CPI)

Creates a subscription's units in escrow (FR-019, I5). Shared-model §5.2 edge.

| Account | Kind | Constraint |
|---|---|---|
| `escrow_authority` | signer | `== escrow_pda(fund_state)` |
| `fund_state` | writable | PDA `["fund", mint]`; mint authority |
| `mint` | writable | `fund_state.mint` |
| `unit_escrow` | writable | ATA(escrow_authority, mint, Token-2022) |
| `cash_escrow` | read | ATA(escrow_authority, cash_mint, cash token program) |
| `settlement` | read | owned by `dvp_settlement` |
| `registry` | writable | compliance_hook Registry for `mint` (for `accrue`) |
| `escrow_holder` | writable | HolderState of `escrow_authority` |
| `compliance_hook_program`, `token_2022_program` | programs | |
| `event_authority`, `program` | `#[event_cpi]` | |

| # | Check | Error |
|---|---|---|
| 1 | signer is the dvp escrow PDA for this fund | `UnauthorizedCaller` |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | `settlement` is owned by dvp, has the right discriminator, sits at the PDA of its own `fund_state, created_by, external_ref`, and `settlement.fund_state == fund_state` | `SettlementMismatch` **(new)** |
| 4 | `kind == Subscription`, `status == Affirmed`, `units == settlement.units` | `SettlementMismatch` |
| 5 | `cash_escrow.amount ≥ settlement.cash_amount` | `CashNotEscrowed` **(new)** |
| 6 | `units_outstanding + units` fits | `Overflow` |

**Effect**, in order:
1. Day rollover ([data-model](../data-model.md#fundstate-fund--fields-f003-fills)).
2. CPI `compliance_hook::accrue(post_balance = unit_escrow.amount + units)`, signed by FundState.
3. `mint_to` unit escrow (FundState PDA signs as mint authority).
4. `units_outstanding += units`.

**Event**: `SubscriptionMinted { settlement, units, units_outstanding }`.

## `burn_for_redemption(units: u64)`: dvp escrow PDA only (CPI)

Destroys a redemption's units in escrow, within the daily limit (FR-020, FR-021).

Accounts: as `mint_for_subscription`, minus `cash_escrow`.

| # | Check | Error |
|---|---|---|
| 1 | signer is the dvp escrow PDA for this fund | `UnauthorizedCaller` |
| 2 | `fund_state.status == Open` | `FundNotOpen` |
| 3 | `settlement` checks as above, `kind == Redemption`, `status == Affirmed`, `units == settlement.units` | `SettlementMismatch` |
| 4 | after day rollover: `(redeemed_today + units) × 10_000 ≤ units_at_day_start × redemption_gate_bps` | `RedemptionLimitExceeded` **(new)** |
| 5 | `units ≤ units_outstanding`, `units ≤ unit_escrow.amount`, sums fit | `Overflow` |

**Effect**, in order:
1. Day rollover.
2. CPI `accrue(post_balance = unit_escrow.amount − units)`.
3. Permissioned `burn_checked` from the unit escrow (FundState PDA signs as burn authority; the
   escrow PDA's signature carries through as account owner).
4. `units_outstanding −= units`; `redeemed_today += units`.

**Event**: `RedemptionBurned { settlement, units, units_outstanding, redeemed_today,
units_at_day_start }`.

## Errors appended to `FundError`

`CashTreasuryAlreadySet`, `InvalidCashTreasury`, `UnsupportedCashMint`, `SettlementMismatch`,
`CashNotEscrowed`, `RedemptionLimitExceeded`. (`UnauthorizedCaller`, `FundNotOpen` and `Overflow`
already exist from F002.)

**Rejection tests**: one per error. Also:
- `mint_for_subscription` and `burn_for_redemption` called directly by any key, by the escrow PDA
  of another fund, and by a forged settlement account (wrong owner, wrong address, Instructed
  status, wrong units);
- the redemption limit at exactly the boundary (accepted) and one unit over;
- rollover across a UTC midnight;
- the first day with `units_at_day_start = 0`;
- I4 after every mint and burn.
