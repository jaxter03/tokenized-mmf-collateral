# Research: Subscription, Redemption and Transfer Settlement (F003)

Each entry gives the decision, why it was made, and the alternatives considered. The Technical
Context left no item marked NEEDS CLARIFICATION: the toolchain, test stack and accounts are
unchanged from F001/F002, and the shared model fixes most of the settlement layout. The decisions
below cover what the shared model leaves open, or where it had to change.

## R1. A new `dvp_settlement` program, reached by Anchor CPI

- **Decision**: add the fifth program `dvp_settlement` (shared-model §2), with a new program ID in
  `mmf_common::ids` (`DVP_SETTLEMENT_PROGRAM_ID`). It depends on the `fund` crate (`cpi`,
  `no-entrypoint`) for `mint_for_subscription` and `burn_for_redemption`. It reads FundState,
  RoleRegistry, Registry and HolderState through `mmf_common::views`, adding a new
  `HolderStateView`. It does not link against `compliance_hook` or `governance`. `governance`
  depends on `dvp_settlement` (`cpi`) for the reject edge (R11).
- **Rationale**: shared-model §2 and the constitution's program list give settlement its own
  program. Reading other programs' accounts through views (checked owner, address and
  discriminator) is the pattern the hook already uses for FundState (F002 R7). It avoids circular
  crate dependencies (fund ↔ dvp).
- **Alternatives**: put settlement in `fund` (one program would own both the mint and the escrow,
  against "one responsibility each"); link every program crate (cycles: `fund` must recognise the
  dvp escrow PDA, so it would need dvp, which needs fund).

## R2. Instruction address: `["settlement", fund_state, created_by, external_ref]`

- **Decision**: a SettlementInstruction's PDA is derived from its creator and the creator's
  32-byte external reference, not from its sequence number. The sequence number is still assigned
  at creation from `DvpConfig.next_seq`, stored on the account, carried in every event, and used to
  read an instruction by number (an account filter on `seq`, or the indexer in F008). An
  all-zero external reference is rejected. **[SM change]**
- **Rationale**: US5. A custodian prepares the creation transaction hours before it is signed. With
  `seq` seeds, any other creation in the meantime moves `next_seq` and invalidates every prepared
  creation (wrong PDA). With creator + reference seeds, the address is fixed when the step is
  prepared. As a bonus, a resubmitted creation can't create a second instruction, because the
  account already exists (FR-028). Every later step addresses the instruction by the same key.
- **Alternatives**: `seq` seeds (shared model as written): fails US5 under concurrent use. A random
  nonce in the seeds: same effect, but the external reference is already required by the spec and
  is meaningful to both parties. Keeping `seq` seeds but using a lookup account: an extra account
  for no gain.

## R3. Returns are a separate step: `return_leg`

- **Decision**: cancel, expire and reject change only the instruction's state. They move the
  status to its final value and earmark every funded leg in `units_to_return` / `cash_to_return`.
  A separate permissionless instruction, `return_leg(leg)`, delivers one earmarked leg to the
  party that funded it and clears it. The keeper normally sends `expire` (or `cancel`) and the
  `return_leg` calls in one transaction. If a return fails, the keeper resends the final-status
  step alone and retries the return later.
- **Rationale**: on Solana a failed CPI aborts the whole transaction, so "try the return and
  earmark it if it fails" can't be done in one instruction. Some failures can't be predicted
  on-chain: a suspended or revoked receiver (hook), paused transfers, a receiver's CPI guard or
  required-memo extension (shared-model §12), a stablecoin issuer's freeze. If the return were
  part of `expire`, any of these would stop the instruction from ever expiring. Splitting keeps
  every final-status step certain to succeed (Constitution VII: no partial effects; IV: one
  transition each). The spec's FR-026 and refund edge case were reworded to match; the user-facing
  promise (every funded leg goes back to its funder) is unchanged.
- **Alternatives**: check every predictable failure (receiver status, pause) before the return
  and earmark only those: still blocked by the unpredictable ones. Let the receiver claim it:
  needs the party's signature, so a custodian's slow approval would delay a refund anyone could
  trigger.

## R4. One escrow per fund, per-instruction accounting, stray deposits

- **Decision**: one escrow authority PDA `["escrow", fund_state]` of `dvp_settlement`. It owns two
  associated token accounts: the unit escrow (fund mint, Token-2022), with a HolderState of class
  `Escrow`, and the cash escrow (cash mint, under the cash mint's own token program). Each
  instruction tracks what it holds. Units held = `units` if the seller funded units (redemption,
  secondary) and the instruction isn't final, or `units_to_return` once final. Cash held =
  `cash_amount` if the buyer funded and the instruction isn't final, or `cash_to_return`. Every
  release is bounded by those numbers. The escrow's balances are at least the sum held by all
  instructions. Anything above that sum (someone sending units or cash to the escrow outside an
  instruction) belongs to no instruction, and no F003 instruction releases it. **[spec FR-030 and
  SC-003 adjusted]**
- **Rationale**: shared-model §10 already chose one escrow per fund. Anyone can send cash to any
  token account, and any approved holder can send units to the approved escrow holder, so strict
  equality can't be enforced on-chain. "Never releases more than an instruction holds" is the
  property that protects parties. The test suite checks equality wherever no stray deposit was
  made, and has a separate test showing a stray deposit can't be taken by any instruction.
- **Alternatives**: one escrow per instruction (design.md): one more HolderState per instruction,
  approved by compliance every time, which isn't workable. Blocking direct transfers into the
  escrow in the hook: the hook can't tell a funding CPI signed by the seller from a direct
  transfer signed by the same seller.

## R5. Funding units: `transfer_checked` by CPI, signed by the seller

- **Decision**: `affirm_and_fund` moves units with a Token-2022 `transfer_checked` CPI from the
  seller's holding account (its ATA) to the unit escrow. The seller is the transfer authority; its
  signature on the outer transaction carries into the CPI. The five hook extra accounts
  (shared-model §4.1, Registry writable) are passed through. The hook runs in full: pause, sender
  Approved, (F004) locked units, escrow receiver Approved, accrual settled for both sides.
- **Rationale**: the hook enforces every rule on units, so funding can't bypass a check. In
  particular, collateral locks (F004) will reject funding a redemption with pledged units at hook
  step 5, with no F003 code involved (US2 scenario 3).
- **Limitation**: a seller that turns on Token-2022's CPI guard on its holding account can't fund
  through a program. Its funding fails. This is the seller's own choice and affects only its own
  step. It is recorded in quickstart as a known behaviour, not handled.
- **Alternatives**: the seller signs a plain `transfer_checked` in the same transaction and dvp
  checks the escrow balance delta via instruction introspection: more fragile, and harder for a
  custodian to review.

## R6. The Fund side: transfer agent, and the treasury cash account

- **Decision**: the transfer agent (RoleRegistry) acts for the Fund. It may create subscriptions
  and redemptions, affirm the Fund side of a subscription (no deposit; FR-014), fund the cash leg
  of a redemption, and ask to cancel. Redemption cash comes from `FundState.cash_treasury`, a
  token account of the Fund's cash mint designated once by the issuer through the new
  `fund::set_cash_treasury`. The transfer agent must be able to move that account's cash (as its
  owner or approved delegate). The token program enforces this; dvp only checks that the source
  is `cash_treasury`. Subscription cash is paid to `cash_treasury`, and returned redemption cash
  goes back there.
- **Rationale**: the transfer agent is the role that processes subscriptions and redemptions
  (F001 Assumptions). Keeping the treasury cash account outside program control matches the spec
  (the fund's operators control it, and investing cash is out of scope) and adds no new authority.
  `cash_treasury` already lives on FundState in the shared model, so it is set by `fund`.
- **Alternatives**: a program-owned treasury cash vault: it would need a withdrawal path for
  investing cash, which is a new discretionary power outside scope. The fund admin as the Fund
  side: the fund admin publishes prices, which is a separate duty.

## R7. Units are created and destroyed only by `fund`, called by dvp with the escrow PDA

- **Decision**: two new `fund` instructions, callable only by CPI signed by the dvp escrow
  authority PDA (derived from `DVP_SETTLEMENT_PROGRAM_ID`):
  - `mint_for_subscription(units)`: mints into the unit escrow.
  - `burn_for_redemption(units)`: burns from the unit escrow through the permissioned-burn
    extension. The FundState PDA is the burn authority, and the escrow PDA, as the account owner,
    co-signs.

  Each also reads the SettlementInstruction account and checks:
  - it is owned by dvp and its address is the PDA for its own stored seeds;
  - its fund is this fund;
  - the kind is right, its status is Affirmed and its units equal the amount.

  `mint_for_subscription` also checks that the cash escrow holds at least the instruction's
  `cash_amount` (I5). Each settles the escrow holder's interest first (R8), keeps
  `units_outstanding` (I4), and rolls the redemption day over (R9).
- **Rationale**: Constitution VIII (units minted only by subscription settlement, with its cash in
  escrow). Shared-model §5.2 lists this edge and its callee checks. `fund` checks the instruction
  account itself as well as trusting the signer, which costs a few hundred CU and means a bug in
  dvp can't mint for an unfunded subscription.
- **Alternatives**: dvp holds a mint delegate: Token-2022 has no mint delegate, and the mint
  authority must stay with the FundState PDA (shared-model §3).

## R8. `compliance_hook::accrue(post_balance)` before every mint and burn

- **Decision**: build `accrue`, moved from F002 (F002 R8). It is signed by the FundState PDA
  recorded in the Registry. Accounts: Registry, FundState, the holder's HolderState and its
  holding account. It reads `pre_balance` from the holding account (the balance before the mint or
  burn) and settles interest on it at the current index. It then sets `last_balance =
  post_balance` and `last_slot`. In F003 it accepts only the escrow's HolderState (class `Escrow`;
  otherwise `ClassNotAccrued`). Event: `Accrued`, emitted with `emit!` (log), not `emit_cpi!`, to
  stay inside the call-depth limit (R15). **[SM change: signature]**
- **Rationale**: mint and burn don't run the hook (shared-model §6). Without `accrue`, the escrow's
  `last_balance` would be wrong after a burn, and `earned_now` (F002 FR-017, I11) would overstate
  it. Reading the pre-balance from the account rather than trusting the caller keeps the settlement
  exact. F003 only mints to and burns from the escrow, which isn't counted toward the holder cap,
  so an investor branch (updating `holder_count` when a balance crosses zero) couldn't be tested
  here. F006, the first feature to mint to investors, adds it with its tests (Constitution I and
  VI, the same reasoning as F002 R8).
- **Alternatives**: `accrue(owner, pre_balance)` as written in the shared model: leaves
  `last_balance` stale after a burn. Re-reading balances on every hook touch: the hook already
  does that for transfers, but nothing touches the escrow between a burn and the next funding.

## R9. Daily redemption limit: lazy UTC-day rollover and a day-start snapshot

- **Decision**: new FundState field `units_at_day_start: u64`, taken from `_reserved` (52 → 44
  bytes) after `pending_override_nav`. **[SM change]** Before every mint or burn, if
  `utc_day(now) > utc_day(day_start_ts)` (or `day_start_ts == 0`), `fund` sets:
  - `day_start_ts = utc_day(now) × 86_400`
  - `redeemed_today = 0`
  - `units_at_day_start = units_outstanding`

  `units_outstanding` changes only through mint and burn, so this equals units outstanding at the
  start of the UTC day. `burn_for_redemption` then requires
  `(redeemed_today + units) × 10_000 ≤ units_at_day_start × redemption_gate_bps` (exact,
  cross-multiplied, so equal to the limit passes) and adds `units` to `redeemed_today`.
- **Rationale**: spec FR-021 and Edge Cases (a UTC day, measured at the start of the day,
  subscriptions don't raise it). Rolling over at the first mint or burn costs nothing on publish
  and needs no keeper. Cross-multiplying avoids rounding at the boundary (new shared-model §8
  row).
- **Consequence**: on the day a fund has its first subscription, `units_at_day_start` is 0, so no
  redemption can settle that day. This follows from the rule and is stated in quickstart.
- **Alternatives**: reset the counter in `publish_daily` (design.md §6.3): ties the limit to publish
  times, not days, and a missed publish would carry one day's redemptions into the next. Measure
  against current units outstanding: the limit would shrink with each redemption and grow with
  subscriptions, against the spec.

## R10. Price used at creation: read from FundState, not from a NavRecord account

- **Decision**: `create_instruction` prices from FundState's latest price: `nav`, `nav_seq` and
  `nav_period_end_day`. It requires `nav_seq > 0` (`NoPrice`) and
  `nav_period_end_day + 1 ≥ utc_day(now)` (`StalePrice`). `nav_seq` is stored on the instruction.
  Cash amounts:
  - subscription: `ceil(units × nav / 10^8)`
  - redemption: `floor(units × nav / 10^8)`

  Both are computed in checked `u128` (shared-model §8, existing rows). A zero result is rejected
  (`ZeroCash`). A secondary transfer takes `cash_amount` from the creator (> 0) and records
  `nav_seq` for reference.
- **Rationale**: FR-007 and FR-008. I12 guarantees `FundState.nav == NavRecord(nav_seq).nav`, so
  passing the NavRecord account would add an account and a PDA check for no new information.
  "Ended before yesterday" is exactly `nav_period_end_day + 1 < today`.
- **Alternatives**: let the creator name any NavRecord: lets a party pick a favourable old price.
  Require today's period to be covered: blocks every instruction between midnight UTC and the
  day's publish.

## R11. Compliance rejects through governance

- **Decision**: `governance::reject_settlement(reason, case_ref_hash)`, signed by compliance,
  checks:
  - the role registry is bound to the fund;
  - the fund is Open;
  - the reason is allowed (`allowed_in_f001` list: CourtOrder, SanctionsHit, LostKey,
    OperationalError, Other).

  It then CPIs `dvp_settlement::reject`, signed by a new governance PDA `["dvp-caller"]`.
  `dvp_settlement::reject` accepts only that signer. **[SM change: new PDA, new §5.1 rows, new
  §5.2 edge]**
- **Rationale**: F001's compliance actions and F002's override both enter through governance. The
  F002 clarification states that compliance acts "through governance as it does for suspensions
  and pauses". Keeping one entry point means F007 adds the `ActionRecord` (kind
  `RejectSettlement`, already in §4.5) in one place. Call depth: Squads → governance → dvp →
  `emit_cpi` = 4.
- **Alternatives**: `dvp_settlement::reject` signed by compliance directly: one fewer edge, but a
  second pattern for compliance actions.

## R12. Party checks: who is checked, and when

- **Decision**: a party other than the Fund must be an **Investor** HolderState with status
  Approved and `kyc_expiry_ts > now` ("eligible"). This is checked:
  - at creation, for every such party (FR-009);
  - at funding, for the funding party, for both cash and units (FR-012; units are checked again
    by the hook);
  - at settlement, for every such party (FR-016).

  Delivery of units at settlement also runs the hook, so pause, receiver jurisdiction and the
  holder cap apply as for any transfer (FR-018). Settlement checks `Registry.transfers_paused`
  explicitly, because a redemption burns units and runs no hook. Fund-operated classes (treasury,
  escrow) can't be parties.
- **Rationale**: spec FR-009, FR-012, FR-016, and the edge case of suspension between funding and
  settlement. Checking at funding stops a suspended party from placing cash in escrow (sanctions).
  Checking at settlement is what makes settlement fail closed.
- **Alternatives**: rely only on the hook: a redemption pays cash and burns units without running
  it, so a suspended redeemer would be paid.

## R13. Slow signers: durable nonces and deterministic accounts

- **Decision**: no on-chain logic depends on a recent blockhash or slot. Every step is judged only
  on account state and `Clock.unix_timestamp`, so a step stays valid until the instruction's
  `expires_at_ts` (FR-029). Custodians prepare transactions with `AdvanceNonceAccount` first, on
  their own durable nonce account. `mmf-sdk` gains `prepare::with_durable_nonce(ixs, nonce,
  authority, payer)`, which returns an unsigned message. Every account a prepared step needs can be
  derived when it is prepared: the instruction PDA (R2), escrow ATAs, holding accounts, HolderState
  PDAs and the hook's extra accounts. Replay: the nonce advances once per use, and each step
  checks state (`LegAlreadyFunded`, `InstructionFinal`), so a step takes effect at most once
  (FR-028).
- **Tests**: LiteSVM 0.12 supports durable nonces when the blockhash check is on
  (`with_blockhash_check(true)`; the nonce path is in `check_transaction_age`). The US5 test turns
  it on, prepares a funding message with a nonce, advances the clock 6 h and 23 h, expires recent
  blockhashes, and submits it successfully. A control transaction with an ordinary blockhash fails
  with `BlockhashNotFound`. The devnet run repeats this once with a real nonce account.
- **Alternatives**: long-lived signatures over a custom message checked with ed25519: more code,
  and custodians already support durable nonces.

## R14. Cash token program, cash accounts and missing accounts

- **Decision**: cash moves with `transfer_checked` through `anchor_spl::token_interface`, under
  whichever program owns `terms.cash_mint` (SPL Token or Token-2022, F001 FR-001). Cash is paid to
  each party's associated token account for the cash mint, which is deterministic and can't be
  redirected. Cash may be funded from any token account the party can authorise. If a
  destination ATA doesn't exist, the step fails. Anyone can create the ATA (the keeper bundles an
  idempotent ATA creation before `settle` or `return_leg`). Cash-mint Token-2022 extensions on the
  stablecoin itself (fees, hooks) aren't supported. `set_cash_treasury` and `init_dvp` reject a
  cash mint with a transfer fee or transfer hook extension (`UnsupportedCashMint`).
- **Rationale**: FR-011 and FR-017 need fixed destinations. Supporting both token programs costs
  nothing with the interface types. A fee-bearing cash token would break the exact cash amounts
  in FR-008.
- **Alternatives**: store a cash destination per party on the instruction: another field for each
  party to get wrong, with no gain.

## R15. Compute, call depth and transaction size

- **Decision**: budgets, checked by tests (SC-007):

  | Step | Est. CU | Max call depth |
  |---|---|---|
  | `affirm_and_fund` (units) | ≤ 90 000 | Squads → dvp → Token-2022 → hook = 4 |
  | `settle` subscription | ≤ 200 000 | dvp → fund → hook `accrue` = 3; dvp → Token-2022 → hook = 3 |
  | `settle` redemption | ≤ 120 000 | dvp → fund → Token-2022 (burn) = 3 |
  | `reject` | ≤ 30 000 | Squads → governance → dvp → `emit_cpi` = 4 |

  `accrue` and the hook use `emit!` (logs). dvp and fund use `emit_cpi!` where it fits within
  depth 4. `settle` for a subscription takes about 24 accounts, which fits a legacy transaction.
  Prepared funding transactions add the nonce, its authority and the recent-blockhashes sysvar,
  and still fit. No address lookup table is required in F003 (F008 may add one).
- **Rationale**: Solana's invoke stack height is 5 (depth 4 below the top-level call). The keeper
  signs `settle` directly, not through Squads, which leaves room for fund → hook.
- **Alternatives**: `emit_cpi!` in `accrue`: would put a Squads-initiated path at depth 5.

## R16. Interest on units in escrow

- **Decision**: no special handling. Units in the unit escrow accrue interest to the escrow's
  HolderState under F002's record-date rule. Returned units come back without it, and a subscription
  mints and delivers in the same step, so its units earn nothing in escrow. The escrow's
  `accrued_e18` is left for F006 to dispose of.
- **Rationale**: spec Edge Cases and Assumptions; F002 FR-015 (interest depends only on units held).
- **Alternatives**: attribute escrow interest to the seller: needs a per-instruction index
  checkpoint and a payout path, which is F006's job.

## R17. Mutual cancellation of an Affirmed instruction

- **Decision**: `cancel(side)`:
  - Instructed, SellerFunded or BuyerFunded: cancels immediately.
  - Affirmed: sets the caller's bit in `cancel_requests` (bit 0 seller side, bit 1 buyer side).
    When both bits are set, the instruction is cancelled. `CancelRequested` is emitted for the
    first request and `SettlementCancelled` for the cancel.
  - `withdraw_cancel(side)` clears the caller's bit while the other bit is unset.

  Settlement ignores `cancel_requests`: a one-sided request doesn't block settlement (FR-023).
- **Rationale**: spec US4 scenario 2 and FR-023. Two custodians can't co-sign one transaction in
  practice, so mutual cancel must be two steps. One bitfield in reserved space is enough.
- **Alternatives**: no cancel once Affirmed (wait for expiry): the spec asks for mutual cancel.

## R18. Tests: an independent settlement model and the I4 fix to `seed_units`

- **Decision**:
  - `tests/src/dvp_model.rs` is a reference model of instruction states and party balances,
    written from the spec and sharing no code with the programs.
  - `dvp_prop.rs` (proptest) runs random sequences of create, fund, settle, cancel,
    withdraw-cancel, expire, reject, return, suspend, reinstate, pause, unpause, publish and
    clock moves.
  - After every step it checks: SC-001 (every final instruction has both legs moved, or the
    funders are restored once returns are done), SC-002 / I4 / I5, SC-003 / I13, SC-005 / I14,
    and the F001/F002 invariants (I3, I10, I11).
  - `seed_units` must also add to `FundState.units_outstanding`, otherwise I4 fails in every test
    that seeds units. This is a test-helper change under Principle XII and will be listed in the
    PR.
- **Rationale**: Constitution VI. The SC-001 wording "every party has exactly what it started
  with" is checked against the model's balances, not against the program's own bookkeeping.

## R19. Deployment and existing funds

- **Decision**:
  - Deploy `dvp_settlement` as a new program with the issuer's Squads vault as upgrade authority.
    `fund`, `compliance_hook` and `governance` are upgraded in place.
  - FundState keeps its size. `units_at_day_start` comes from reserved bytes after `bump`, so no
    F001/F002 offset moves.
  - Existing devnet funds have supply 0 (no mint path existed), so `units_outstanding = 0`
    already satisfies I4.
  - Settlement on an existing fund needs three setup steps: `set_cash_treasury` and `init_dvp`
    (issuer), then escrow registration and approval (compliance) and the escrow thaw (anyone).
- **Rationale**: Principle XII (F001/F002 stay green, no migration).

## R20. Default expiry

- **Decision**: `init_dvp(default_expiry_secs: Option<u32>)`. `None` stores 86 400 (24 h,
  Clarification); `Some(0)` is rejected (`InvalidExpiry`). `create_instruction(expires_at_ts = 0)`
  uses `now + default_expiry_secs`. An explicit expiry must be after both now and valid-from. There
  is no upper bound.
- **Rationale**: spec FR-001 and FR-010 (24 h default, set at setup). No maximum was asked for, and
  a long expiry only ties up the parties' own assets.
