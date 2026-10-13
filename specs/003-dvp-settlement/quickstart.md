# Quickstart: validating F003 (settlement)

How to show that F003 works, and where each requirement is tested. Accounts and checks are in
[data-model.md](data-model.md) and [contracts/](contracts/); this file doesn't repeat them.

## Prerequisites

- The F001/F002 toolchain: Rust 1.90 (pinned), Anchor 1.1.2, Solana CLI, and `cargo test -p tests`
  passing on `main`.
- A new program keypair for `dvp_settlement` (`anchor keys sync`). Its ID goes into
  `mmf_common::ids::DVP_SETTLEMENT_PROGRAM_ID` and `Anchor.toml`. The keypair stays in
  `target/deploy/` and is never committed (as for F001).

## Run

```bash
anchor build                      # four programs: fund, compliance_hook, governance, dvp_settlement
cargo test -p tests               # all F001, F002 and F003 suites
cargo test -p mmf-common          # settlement formula vectors (data-model worked checks)
scripts/check-error-coverage.sh   # every DvpError and new Fund/Hook error has a test asserting it (SC-006)
cargo run -p devnet-e2e -- --feature f003   # one devnet run (below)
```

## Setting up a fund for settlement (test fixture `settlement_ready_fund`)

1. F001/F002: create, launch, onboard investors, publish a price of $1.00.
2. Issuer: `fund::set_cash_treasury(treasury_cash_ata)`, then `dvp_settlement::init_dvp(None)`.
3. Compliance: `register_holder(Escrow, escrow_authority)`, then `approve_fund_account`.
4. Anyone: `thaw_holding_account(unit_escrow)`.
5. Mint mock cash to investors (the test cash mint is a classic SPL Token mint, 6 decimals).

## Independent tests per user story

| Story | Scenario | Expected | Test file |
|---|---|---|---|
| US1 | investor creates subscription for 2 000 000 units at $1.00; transfer agent affirms; investor funds $2M; keeper settles | investor +2 000 000 units, −$2M; treasury +$2M; escrow empty for it; `units_outstanding` +2 000 000; supply equal | `f003_subscribe.rs` |
| US2 | investor with 500 000 units creates redemption; funds units; transfer agent funds $500 000 from treasury; settle | investor −500 000 units, +$500 000; units burned; `redeemed_today` = 500 000 | `f003_redeem.rs` |
| US3 | A sells 1 000 000 units to B for $999 500 | B +units, A +cash, supply unchanged | `f003_secondary.rs` |
| US4 | three one-sided instructions: cancelled, expired, rejected; then `return_leg` | every funder restored exactly; escrow empty | `f003_cancel_expire_reject.rs` |
| US5 | funding message prepared with a durable nonce, submitted after 6 h and 23 h of clock | succeeds; ordinary-blockhash control fails `BlockhashNotFound`; one for a since-cancelled instruction fails `InstructionFinal` | `f003_slow_signers.rs` (LiteSVM `with_blockhash_check(true)`, R13) |

## Requirement → test map

| Requirement | Test(s) |
|---|---|
| FR-001 setup once; FR-010 default 24 h | `f003_setup.rs`: second `set_cash_treasury` → `CashTreasuryAlreadySet`; non-issuer → `Unauthorized`; wrong mint → `InvalidCashTreasury`; fee mint → `UnsupportedCashMint`; `init_dvp(Some(0))` → `InvalidExpiry`; default expiry = created_at + 86 400 |
| FR-002 escrow is system-controlled, Escrow holder | `f003_setup.rs`, `us2_onboarding.rs` (escrow class with wrong owner / other fund → `NotSystemControlled`) |
| FR-003 not before setup | `f003_setup.rs`: create before `init_dvp`, before approval, before thaw → `EscrowNotReady` |
| FR-004–FR-006 creation, creators, numbering | `f003_create.rs`: each creator per kind; outsiders → `Unauthorized`; seq 1, 2, 3; same `external_ref` twice → already in use; zero ref → `ZeroExternalRef` |
| FR-007 latest, fresh price | `f003_create.rs`: no publish → `NoPrice`; after a missed day → `StalePrice`; Monday before publish with Friday's 3-day record → accepted |
| FR-008 cash amounts and rounding | `f003_create.rs` + `mmf-common` vectors (ceil / floor, `ZeroCash`, `CashAmountNotAllowed`) |
| FR-009 invalid instructions | `f003_create.rs`: `ZeroUnits`, `SameParty`, `InvalidExpiry` (≤ now, ≤ valid-from), `PartyNotEligible` (pending, suspended, KYC-expired, fund-operated) |
| FR-011–FR-014 funding | `f003_fund.rs`: wrong side → `Unauthorized`; twice → `LegAlreadyFunded`; after expiry → `InstructionExpired`; suspended funder → `PartyNotEligible`; redemption cash not from treasury → `WrongCashSource`; paused unit funding → `TransfersPaused`; statuses in both orders |
| FR-013 units under transfer rules (and F004 locks) | `f003_fund.rs` (paused, suspended sender); F004 adds the lock test (US2 scenario 3) |
| FR-015 anyone settles, outcome independent of trigger | `f003_settle.rs`: keeper, a random key and an investor each settle identical instructions with identical results |
| FR-016 settle preconditions | `f003_settle.rs`: `NotAffirmed`, `NotYetValid`, `InstructionExpired`, `FundNotOpen`, `TransfersPaused` (incl. redemption), `PartyNotEligible` (suspended after funding; edge case) |
| FR-017 one indivisible step | `f003_settle.rs`: buyer hits holder cap → whole settle fails, cash still in escrow, no units minted; missing seller cash ATA → fails, nothing moved |
| FR-018 hook on delivery; accrual before mint/burn | `f003_settle.rs` (`ReceiverNotApproved`, `HolderCapReached`); `f003_accrual.rs` (escrow I11 across publishes) |
| FR-019 mint only on subscription settle | `f003_mint_burn_guard.rs`: direct `mint_for_subscription` by any key / other fund's escrow / forged settlement → `UnauthorizedCaller` / `SettlementMismatch`; `CashNotEscrowed` |
| FR-020 burn only on redemption settle | `f003_mint_burn_guard.rs` (same pattern for burn) |
| FR-021 daily limit | `f003_redeem.rs`: exactly at limit accepted; one unit over → `RedemptionLimitExceeded`, stays Affirmed, settles after UTC midnight; first-day `units_at_day_start = 0` |
| FR-022 units outstanding | I4 checked after every step in all F003 tests and `dvp_prop.rs` |
| FR-023 cancel rules | `f003_cancel_expire_reject.rs`: one-sided cancel by either side; Affirmed needs both; `CancelAlreadyRequested`; `withdraw_cancel` / `NoCancelRequest`; settle still works with one request |
| FR-024 expire | same file: before expiry → `NotExpired`; at expiry → Expired; Instructed → Expired with nothing to return |
| FR-025 reject | `f003_reject.rs` + governance contract tests (roles, reasons, other fund, direct call → `UnauthorizedCaller`) |
| FR-026 earmarked returns | `f003_returns.rs`: return to suspended seller fails, earmark kept, succeeds after reinstatement; paused → fails then succeeds; `NothingToReturn`; `NotReturnable` on open/settled |
| FR-027, FR-028 final and replay | `f003_settle.rs` (settle twice); `f003_slow_signers.rs` (resubmitted signed step); every step on each final status → `InstructionFinal` |
| FR-029 slow signers | `f003_slow_signers.rs` |
| FR-030 escrow accounting | `f003_escrow.rs` (stray deposits never released) and I13 in `dvp_prop.rs` |
| FR-031 records | `tests/src/rebuild.rs` extended: instruction history and escrow movements rebuilt from events equal on-chain state (SC-008) |

## Success criteria

| SC | How |
|---|---|
| SC-001 | `dvp_prop.rs`: random op sequences against the independent model (R18); after each final status and its returns, both legs moved or funders restored |
| SC-002 | `dvp_prop.rs`: supply = seeded + Σ settled sub units − Σ settled red units; each `SubscriptionMinted` pairs with treasury +cash in the same transaction |
| SC-003 | `dvp_prop.rs`: I13 equality (no stray deposits in the generator); `f003_escrow.rs` for the inequality case |
| SC-004 | `f003_slow_signers.rs` (6 h, 23 h) |
| SC-005 | `dvp_prop.rs`: I14 on every simulated day |
| SC-006 | `check-error-coverage.sh` over `DvpError` and the new Fund/Hook/Governance errors |
| SC-007 | `scale.rs`: `settle` CU equal (±0) with 10 vs 10 000 holders and 1 vs 1 000 open instructions |
| SC-008 | `rebuild.rs` |

## Devnet run (`scripts/devnet-e2e`, `--feature f003`)

On a fund launched by the F001/F002 run, with a mock 6-decimal stablecoin minted by the script:
1. Set the cash treasury, run `init_dvp`, approve and thaw the escrow.
2. Subscribe 1 000 units. The funding is prepared with a durable nonce, the clock is moved on by
   waiting at least 2 minutes (past blockhash expiry), and then it is signed and submitted.
3. Settle as the keeper.
4. Redeem 400 units.
5. Create a one-sided secondary transfer, let it expire (short expiry), expire it, and return its
   leg.

Print every signature and the final balances; the run fails on any mismatch.

## Known behaviours (documented, not handled)

- A seller whose holding account has Token-2022's CPI guard enabled can't fund units (R5).
- A party without an associated token account for the cash mint can't be paid until one exists.
  Anyone can create it, and the keeper does so before `settle` or `return_leg` (R14).
- No redemption can settle on the first day a fund has units (`units_at_day_start = 0`, R9).
- A revoked seller's earmarked units can't be returned in F003. They stay earmarked in escrow until
  F007's recovery action.
- Interest earned while units sit in escrow accrues to the escrow holder and is left for F006
  (R16).
