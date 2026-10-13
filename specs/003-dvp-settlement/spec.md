# Feature Specification: Subscription, Redemption and Transfer Settlement

**Feature Branch**: `003-dvp-settlement`

**Created**: 2026-10-13

**Status**: Draft

**Input**: User description: "Exchange cash for fund units (and back) so that either both sides
move or neither does." (full input: `docs/speckit-inputs.md`, F003). Builds on
001-fund-launch-onboarding (Fund, Holder record, Register, roles, transfer rules) and
002-daily-price-accrual (Price record, interest settlement). Those entities are reused, not
redefined.

## Clarifications

### Session 2026-10-13

- Q: Who may create a subscription or redemption instruction, and must the Fund side accept each
  one? → A: Either side creates it (the investor, or the transfer agent for the Fund); the other
  side must affirm or fund before it can settle.
- Q: What is the default expiry of a settlement instruction? → A: 24 hours after creation.

## User Scenarios & Testing *(mandatory)*

Buying or redeeming fund units today takes one to two days and relies on trust between the
parties. On-chain, the risk is paying and not receiving, or delivering and not being paid.
Institutions also sign through custodians whose approval can take hours. This feature lets two
parties agree a settlement instruction, place their sides in escrow at their own pace, and then
exchange both sides in a single step. If the exchange cannot happen, every party gets back exactly
what it put in.

Terms used below:

- **Leg**: one side of an instruction. The unit leg is the units; the cash leg is the cash.
- **Seller**: the party delivering units. In a subscription, the seller is the Fund itself (the
  units are created at settlement). **Buyer**: the party receiving units. In a redemption, the
  buyer is the Fund itself (the units are destroyed at settlement).
- **Fund side**: the Fund as a party to a subscription or redemption. The transfer agent acts for
  it (see Assumptions).
- **Funding**: a party placing its leg in escrow. Funding a leg is also that party's affirmation
  of the instruction.
- **Final status**: Settled, Cancelled, Expired or Rejected. Nothing moves an instruction out of a
  final status.

### User Story 1 - Subscribe (Priority: P1)

An approved investor pays cash and receives newly created units at the price in a specific price
record. The cash waits in escrow, and units come into existence only at the moment the cash goes
to the fund, so the investor cannot pay without receiving and no unit is ever created unpaid.

**Why this priority**: this is how units come into existence. Every later feature needs units.

**Independent Test**: with a published price of $1.00, an approved investor's subscription for
2,000,000 units is created, the transfer agent affirms it for the Fund, and the investor funds
$2,000,000; settlement runs;
the investor holds 2,000,000 units, the fund's treasury cash account has received $2,000,000,
escrow holds nothing for that instruction, and units outstanding have risen by 2,000,000.

**Acceptance Scenarios**:

1. **Given** an Open fund with a current price record, **When** a subscription instruction for N
   units is created, **Then** it records that price record, and its cash amount is N × that price,
   rounded up to the smallest cash unit, and it starts as Instructed.
2. **Given** a subscription instruction, **When** the investor funds the cash amount, **Then** the
   cash leaves the investor and is held in escrow for that instruction, and no units are created.
3. **Given** a subscription with both legs funded (Affirmed), **When** settlement runs, **Then** in
   one step N units are created and delivered to the investor and the cash is paid to the fund's
   treasury cash account, and the instruction is Settled.
4. **Given** an Affirmed subscription whose investor is not Approved (or whose KYC has expired),
   **When** settlement runs, **Then** it is refused, no units are created, no cash moves and the
   instruction stays Affirmed.
5. **Given** a subscription whose cash has not been funded, **When** settlement runs, **Then** it is
   refused and no units are created.
6. **Given** a subscription, **When** anyone other than the investor tries to fund its cash leg,
   or the investor tries to fund an amount other than the instruction's cash amount, **Then** the
   funding is rejected and nothing moves.

---

### User Story 2 - Redeem (Priority: P1)

An approved investor returns units and receives cash at the price in a specific price record,
within the fund's daily redemption limit. The units wait in escrow and are destroyed at the same
moment the cash is paid.

**Why this priority**: an investor must be able to get its cash back. A fund that only takes cash
in is not a money market fund.

**Independent Test**: with a published price of $1.00, an investor holding 500,000 units creates
a redemption for 500,000 units, funds the units, the transfer agent funds $500,000 from the
treasury cash account, and settlement runs; the investor has $500,000 more cash and 500,000 fewer
units, those units no longer exist, and units outstanding have fallen by 500,000.

**Acceptance Scenarios**:

1. **Given** a redemption instruction for N units, **Then** its cash amount is N × the price in its
   price record, rounded down to the smallest cash unit.
2. **Given** a redemption instruction, **When** the investor funds its units, **Then** the units
   move from the investor's holding account into escrow under the transfer rules of F001.
3. **Given** units locked as collateral, **When** the investor tries to fund a redemption with
   them, **Then** funding is rejected and the units stay where they are. (Verified once the
   collateral feature (F004) exists; in this feature no lock exists.)
4. **Given** an Affirmed redemption, **When** settlement runs, **Then** in one step the units in
   escrow are destroyed and the cash in escrow is paid to the investor, and the instruction is
   Settled.
5. **Given** redemptions already settled today, **When** settling this redemption would make
   today's redeemed units exceed the fund's daily redemption limit, **Then** settlement is
   refused, nothing moves and the instruction stays Affirmed (it may settle on a later day before
   it expires).
6. **Given** a redemption, **When** anyone other than the transfer agent tries to fund its cash
   leg, **Then** it is rejected.

---

### User Story 3 - Secondary transfer against cash (Priority: P2)

Two approved holders exchange existing units for cash with each other, with the same
both-or-neither guarantee. No units are created or destroyed.

**Why this priority**: holders already can move units (F001), but without a cash leg one side
carries delivery risk. Primary issuance and redemption (P1) come first.

**Independent Test**: holder A (seller, 1,000,000 units) and holder B (buyer) agree a transfer of
1,000,000 units for $999,500; both fund; settlement runs; B holds the units, A has the cash, and
units outstanding are unchanged.

**Acceptance Scenarios**:

1. **Given** two Approved holders, **When** one creates a secondary transfer instruction naming
   both, the units and the cash amount, **Then** it is Instructed with exactly those terms.
2. **Given** a secondary transfer where only the seller has funded, **When** settlement runs,
   **Then** it is refused and nothing moves.
3. **Given** an Affirmed secondary transfer, **When** settlement runs, **Then** in one step the
   units go to the buyer and the cash goes to the seller, and the instruction is Settled.
4. **Given** an Affirmed secondary transfer whose buyer has been suspended, **When** settlement
   runs, **Then** it is refused and nothing moves.
5. **Given** a secondary transfer, **When** someone tries to create it with the same holder as
   seller and buyer, or with zero cash, **Then** creation is rejected.

---

### User Story 4 - Cancel, expire, reject (Priority: P2)

A party can cancel an instruction that has not yet been fully funded. An instruction that is not
settled by its expiry can be expired by anyone. Compliance can reject any instruction that is not
yet final. In every case, each funded leg goes back to the party that funded it.

**Why this priority**: without this, a leg funded against a counterparty that never funds is
stuck. It is the safety net for US1 to US3.

**Independent Test**: create three instructions; fund one side of each. Cancel the first, let the
second pass its expiry and expire it, have compliance reject the third. Every funding party ends
with exactly the units and cash it started with, and escrow holds nothing for any of them.

**Acceptance Scenarios**:

1. **Given** an Instructed, Seller funded or Buyer funded instruction, **When** either party
   cancels it, **Then** it is Cancelled and any funded leg is returned to the party that funded
   it.
2. **Given** an Affirmed instruction, **When** only one party asks to cancel, **Then** it stays
   Affirmed; **When** the other party also asks, **Then** it is Cancelled and both legs are
   returned.
3. **Given** an instruction past its expiry that is not final, **When** anyone expires it, **Then**
   it is Expired and each funded leg is returned to the party that funded it.
4. **Given** an instruction before its expiry, **When** anyone tries to expire it, **Then** it is
   rejected and nothing changes.
5. **Given** an instruction that is not final, **When** compliance rejects it with a reason,
   **Then** it is Rejected, every funded leg is returned, and the rejection leaves a permanent
   record with its reason.
6. **Given** an instruction that is not final, **When** anyone other than compliance tries to
   reject it, or anyone other than its parties tries to cancel it, **Then** it is rejected and
   nothing changes.
7. **Given** a final instruction, **When** anyone tries to fund, settle, cancel, expire or reject
   it, **Then** it is rejected and nothing moves.

---

### User Story 5 - Slow signers (Priority: P2)

A custodian prepares a step (funding, cancelling) and its approvers sign it hours later. The step
still succeeds as long as the instruction is not expired and still in a state that allows it.

**Why this priority**: institutions sign through custodians with multi-person approval. If a
prepared step lapses within minutes, no institution can use the feature. P2 because US1 and US2
can be demonstrated with fast signing first.

**Independent Test**: prepare an investor's funding step, wait 6 hours (simulated), then sign and
submit it; it succeeds. Prepare a second funding step for an instruction that is cancelled before
it is signed; when submitted, it is rejected and nothing moves.

**Acceptance Scenarios**:

1. **Given** a funding step prepared 6 hours ago for an instruction that is still open and not
   expired, **When** the custodian signs and submits it, **Then** it succeeds.
2. **Given** a prepared step for an instruction that has since been cancelled, expired, rejected
   or settled, **When** it is submitted, **Then** it is rejected and nothing moves.
3. **Given** a step that has already taken effect, **When** the same signed step is submitted
   again, **Then** it is rejected and nothing moves a second time.

---

### Edge Cases

- **One side funds, the other never does**: the instruction stays in Seller funded or Buyer
  funded until a party cancels it, compliance rejects it, or it passes its expiry and anyone
  expires it. In each case the funded leg is returned in full.
- **The price changes between instruction and settlement**: the instruction keeps the price
  record and cash amount fixed at creation. A later publish, override or price move does not
  change what either party pays or receives.
- **Settlement attempted twice**: the first settlement makes the instruction Settled; any later
  settlement attempt is rejected and nothing moves.
- **A party is suspended (or its KYC expires, or it is revoked) after funding but before
  settlement**: settlement is refused while that party is not Approved with valid KYC, and the
  instruction stays where it is. The party is reinstated before expiry (then settlement can
  succeed), compliance rejects the instruction, or it expires; in the last two cases funded legs
  are returned.
- **A refund cannot be delivered right now** (for example, units going back to a suspended
  holder, which the transfer rules forbid, or units while transfers are paused): the instruction
  still reaches its final status, and that leg stays in escrow, earmarked for that party only.
  Anyone can retry the return later; it succeeds as soon as the party can receive. Nothing else
  can move earmarked amounts.
- **Transfers paused**: funding a unit leg and settling any instruction are refused while
  transfers are paused, including redemptions (which destroy units rather than transfer them).
  Creating, cancelling, expiring and rejecting instructions are still allowed, and cash legs are
  still returned.
- **Settlement before valid-from**: refused. Funding is allowed before valid-from, so slow signers
  can fund early.
- **Settlement at or after expiry**: refused. From the moment of expiry, the instruction can only
  be expired, cancelled under the rules above, or rejected.
- **Daily redemption limit**: a day is a UTC calendar day, as in F002. The limit is the Fund's
  daily redemption limit (a share of units outstanding, F001 FR-001), measured against units
  outstanding at the start of that day. Only settled redemptions count. A redemption larger than
  one day's limit can never settle and will expire. Subscriptions settled during the day do not
  raise that day's limit.
- **Holder cap reached** (F001): a subscription or secondary transfer that would give units to an
  investor holding none is refused at settlement once the cap is reached, and stays where it is.
- **Investor not yet holding units**: allowed. Its first units arrive by subscription, under the
  same transfer rules as any receiver.
- **Treasury cash account short of cash for a redemption**: the transfer agent's funding fails and
  the instruction stays where it is.
- **Price record too old**: an instruction cannot be created from a price record that is not the
  Fund's latest, or whose covered period ended before yesterday (UTC). After a missed publish,
  subscriptions and redemptions wait for the next publish.
- **Zero units, zero cash or wrong cash asset**: an instruction for zero units is rejected; a
  secondary transfer with zero cash is rejected (a free transfer is an ordinary F001 transfer);
  only the Fund's cash asset (F001 FR-001) is accepted as cash.
- **Interest while units sit in escrow**: under F002's record-date rule, a publish credits the
  holder of the units at that moment. Units in escrow earn interest for the escrow's Holder record,
  not for the seller or buyer. Neither party is credited for that time, including when the units
  are returned. A unit created at settlement is delivered in the same step, so it earns nothing in
  escrow.
- **Fund not Open**: no instruction can be created, funded or settled for a Fund in Setup.

## Requirements *(mandatory)*

### Functional Requirements

**Setup**

- **FR-001**: System MUST let the issuer set up settlement for a Fund once: the fund's treasury
  cash account (which receives subscription cash and from which the transfer agent funds
  redemption cash) and the default expiry for new instructions. In this feature, neither can be
  changed afterwards.
- **FR-002**: System MUST provide one escrow for each Fund, controlled by the system itself and not
  by any person or institution, holding the units and cash of every open instruction of that Fund.
  The escrow's units account is a fund-operated Escrow holder (F001 FR-019) and is subject to the
  transfer rules like any holder.
- **FR-003**: System MUST reject the creation of any instruction until settlement is set up for the
  Fund and its escrow holder is Approved.

**Instructions**

- **FR-004**: System MUST let a settlement instruction be created only by the parties set out in
  FR-005, only while the Fund is Open, and MUST record on it: kind (subscription, redemption or
  secondary transfer), seller, buyer, number of units, cash amount, the price record it is priced
  from, valid-from, expiry, status, an external reference supplied by the creator, and who created
  it.
- **FR-005**: System MUST let a subscription or redemption instruction be created by the investor
  or by the transfer agent for the Fund, and a secondary transfer instruction by either of its two
  holders. Creating an instruction does not fund or affirm the creator's leg. An instruction
  settles only after both sides have funded or, for the Fund side of a subscription, affirmed
  (FR-014). The Fund side affirms a subscription only through the transfer agent.
- **FR-006**: System MUST give each instruction a sequence number within its Fund (1 for the first,
  then increasing by exactly one) and let anyone read any instruction by that number.
- **FR-007**: System MUST price every subscription and redemption from the Fund's latest price
  record, and MUST reject creation if that record's covered period ended before yesterday (UTC).
  The instruction keeps that price record for its whole life.
- **FR-008**: System MUST fix the cash amount at creation: for a subscription, units × price
  rounded up to the smallest cash unit (the Fund is never under-paid); for a redemption, units ×
  price rounded down (the Fund never over-pays); for a secondary transfer, the amount the creator
  states, which must be above zero. The price record is recorded on a secondary transfer for
  reference only.
- **FR-009**: System MUST reject an instruction with zero units, with the same holder as seller and
  buyer, with an expiry at or before its creation time or at or before its valid-from, or whose
  parties other than the Fund are not Approved with valid KYC at creation.
- **FR-010**: System MUST set expiry to the Fund's default expiry after creation when the creator
  does not give one. The default expiry is 24 hours, set at settlement setup (FR-001).

**Funding and affirmation**

- **FR-011**: System MUST let each party fund only its own leg: the seller funds the units, the
  buyer funds the cash. The Fund side's unit leg in a subscription needs no deposit (the units are
  created at settlement); in a redemption the transfer agent funds the cash leg from the fund's
  treasury cash account.
- **FR-012**: System MUST accept funding only of the exact amount of the leg, in full, in one
  step, only before expiry, and only while the instruction is not final and that leg is not yet
  funded.
- **FR-013**: System MUST move funded units into escrow under the transfer rules of F001 (and, from
  F004, collateral locks), so units that the seller could not transfer cannot be funded.
- **FR-014**: System MUST move the status to Seller funded or Buyer funded when the first leg is
  funded, and to Affirmed when both legs are funded, recording when each leg was funded. In a
  subscription, the Fund side's unit leg counts as funded once the Fund side has affirmed the
  instruction (FR-005).

**Settlement**

- **FR-015**: System MUST let the keeper, or anyone, trigger settlement of an instruction. Its
  outcome MUST depend only on the instruction, the parties' records, the Fund and the current
  time, never on who triggered it.
- **FR-016**: System MUST refuse settlement unless the instruction is Affirmed, the current time is
  at or after valid-from and before expiry, the Fund is Open, transfers are not paused, and every
  party other than the Fund is Approved with valid KYC.
- **FR-017**: System MUST perform each settlement as one indivisible step. Subscription: create the
  units, deliver them to the buyer, pay the cash to the treasury cash account. Redemption: destroy
  the units, pay the cash to the seller. Secondary transfer: deliver the units to the buyer, pay
  the cash to the seller. If any part fails, nothing moves and the instruction stays Affirmed.
- **FR-018**: System MUST apply the transfer rules of F001 (including the holder cap) to every
  delivery of units out of escrow, and MUST settle interest (F002 FR-013) before units are created
  or destroyed.
- **FR-019**: System MUST create units only in a subscription settlement, in the same step as that
  subscription's cash, already held in escrow, is paid to the treasury cash account. No other
  operation in this feature creates units.
- **FR-020**: System MUST destroy units only in a redemption settlement, only units held in escrow
  for that redemption, and in the same step as its cash is paid to the investor.
- **FR-021**: System MUST refuse a redemption settlement that would make the units redeemed on that
  UTC day exceed the Fund's daily redemption limit applied to units outstanding at the start of
  that day.
- **FR-022**: System MUST keep a count of units outstanding that equals all units of the Fund in
  existence, updated in the same step as every creation and destruction.

**Cancel, expire, reject, return**

- **FR-023**: System MUST let either party cancel an instruction that is Instructed, Seller funded
  or Buyer funded. An Affirmed instruction MUST be cancelled only once both parties have asked to
  cancel it; a party may withdraw its request before the other party asks.
- **FR-024**: System MUST let anyone expire an instruction that is not final, at or after its
  expiry, and MUST reject an attempt before its expiry.
- **FR-025**: System MUST let compliance reject an instruction that is not final, with a reason
  from the fixed list used for F001's compliance actions.
- **FR-026**: On cancellation, expiry or rejection, System MUST return every funded leg in full to
  the party that funded it, in the same step. A leg that cannot be returned at that moment MUST
  stay in escrow earmarked for that party; anyone MUST be able to retry its return, and nothing
  else MUST be able to move it.
- **FR-027**: System MUST reject every step on an instruction in a final status, other than
  retrying an earmarked return.
- **FR-028**: System MUST reject any step whose effect has already been applied, so a step signed
  once takes effect at most once.

**Slow signers**

- **FR-029**: A party's funding or cancellation step MUST remain acceptable from the moment it is
  prepared until the instruction's expiry, however long its signers take, and MUST be judged on
  the instruction's state when it is submitted, not when it was prepared.

**Escrow and records**

- **FR-030**: System MUST track, per instruction, the units and cash it holds in escrow, and MUST
  NOT release more of either than that instruction holds. The escrow's total units and cash MUST
  always equal the sum held for all instructions, including earmarked returns.
- **FR-031**: System MUST leave a permanent record of every instruction creation, funding,
  settlement, cancellation request, cancellation, expiry, rejection and return. Each record MUST
  show the Fund, the instruction number, the amounts moved, the signer (or, for steps anyone may
  trigger, the triggering account), the time and the external reference; a rejection also shows
  its reason.

### Key Entities *(include if feature involves data)*

- **Fund** (from F001, F002): now also holds units outstanding, the units redeemed on the current
  UTC day and when that day started, and the treasury cash account.
- **Holder record** (from F001): unchanged. The escrow's units account has a Holder record of class
  Escrow. Investors' records govern who may fund, receive and settle.
- **Price record** (from F002): unchanged. Each subscription and redemption names the one it was
  priced from.
- **Settlement settings**: one per Fund. Default expiry, the next instruction number, and the link
  to the Fund's escrow.
- **Settlement instruction**: one per exchange. Sequence number, kind (subscription, redemption,
  secondary transfer), seller, buyer, units, cash amount, price record used, valid-from, expiry,
  status (Instructed, Seller funded, Buyer funded, Affirmed, Settled, Cancelled, Expired,
  Rejected), when each leg was funded, cancellation requests, any earmarked return, external
  reference, creator.
- **Escrow**: one per Fund, controlled by the system. Holds the units and cash of every open
  instruction and every earmarked return; the amount belonging to each instruction is tracked on
  that instruction.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In every test, including randomized sequences of create, fund, settle, cancel,
  expire, reject, suspend, pause and publish, every instruction that reaches a final status leaves
  either both legs moved in full, or every party with exactly the units and cash it held before
  the instruction (once any earmarked return has been delivered).
- **SC-002**: After every operation in those sequences, total units in existence equal the sum of
  units created by settled subscriptions minus units destroyed by settled redemptions (plus units
  placed during test setup), and every created unit has a matching cash amount received by the
  treasury cash account in the same step. Zero units exist without matching cash received.
- **SC-003**: After every operation, the escrow's units and cash equal exactly the sum held for all
  open instructions and earmarked returns.
- **SC-004**: A funding step signed and submitted 6 hours, and 23 hours, after it was prepared
  succeeds when the instruction has not expired.
- **SC-005**: On no UTC day in the randomized sequences do settled redemptions exceed the daily
  redemption limit.
- **SC-006**: Every rule in this specification has at least one test showing it rejecting a
  violation with the specific reason, including tests that submit steps directly rather than
  through the project's own software.
- **SC-007**: Settling one instruction takes the same effort whether the Fund has 10 or 10,000
  holders and 1 or 1,000 open instructions.
- **SC-008**: An auditor can rebuild every instruction's history, and every party's movements into
  and out of escrow, from the permanent records alone.

## Assumptions

- **Cash asset**: the Fund's cash asset (F001 FR-001) is a stablecoin or tokenized deposit already
  on-chain. Its issuer is trusted: if it freezes a party's cash account or the escrow's, cash legs
  to or from that account cannot move. The system then refuses settlement or earmarks the return;
  it cannot recover the cash itself.
- **Fund side**: the transfer agent (F001 roles) acts for the Fund in subscriptions and
  redemptions: affirming the Fund side where required, funding redemption cash from the treasury
  cash account, and cancelling. The treasury cash account is controlled by the fund's operators;
  the transfer agent is trusted to fund redemptions it has accepted. If it does not, the
  redemption expires and the investor's units are returned.
- **Treasury cash account**: designated by the issuer at settlement setup. Moving the fund's cash
  into or out of investments is outside this feature.
- **Price at creation**: an instruction is priced from the latest published price, which may be up
  to about a day old (the window before the day's publish). The fund admin's price is trusted as
  in F002. Pricing at the next publish after the order ("forward pricing") is not offered.
- **Units, not cash, are stated**: a subscription states the number of units; the cash follows
  from the price. At $1.00 the two figures are equal.
- **Secondary price**: the two holders agree the cash amount between themselves. The system does
  not check it against the published price.
- **Redemption limit**: input question 3 ("percentage or fixed amount?") is settled by F001: the
  limit is a share of units outstanding (F001 FR-001). This feature only enforces it.
- **Escrow interest**: interest earned while units sit in escrow is credited to the escrow's Holder
  record and paid to no party in this feature; the distributions feature (F006) decides what
  happens to it. Parties can keep it small by funding units close to settlement.
- **Rejection records**: until the governance feature (F007) adds dedicated action records, the
  permanent record of a rejection is the system's public activity log for the Fund, as for F001's
  compliance actions.
- **Custodian behaviour**: custodians sign only what their institution approved. The system never
  holds an institution's signing key; it can only check that the right party signed.
- **Scale**: one Fund, up to about 10,000 holders, up to about 1,000 open instructions at a time.
- **Out of scope**: order books, price discovery, fiat payment rails, partial fills or partial
  funding, netting several instructions together, forward pricing, changing settlement settings
  after setup, and the investor-facing screens and signing service (F008).
