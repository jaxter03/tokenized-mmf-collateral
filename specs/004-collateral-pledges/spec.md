# Feature Specification: Collateral Agreements, Margin Calls and Pledges

**Feature Branch**: `004-collateral-pledges`

**Created**: 2026-10-14

**Status**: Draft

**Input**: User description: "Let a holder pledge fund units to a counterparty as margin, without
moving them, and keep the pledge in line with the exposure." (full input: `docs/speckit-inputs.md`,
F004). Builds on 001-fund-launch-onboarding (Fund, Holder record, roles, transfer rules),
002-daily-price-accrual (Price record, interest accrual) and 003-dvp-settlement (escrow funding
under the transfer rules). Those entities are reused, not redefined. A holder's locked units live
on its existing Holder record.

## User Scenarios & Testing *(mandatory)*

Institutions owe margin on trades such as FX forwards, swaps and repos. Meeting a margin call from
a money market fund today means selling units and waiting days for the cash. This feature lets the
institution pledge the units instead: they stay in its own account and keep earning interest, but
the fund's own transfer rules stop them from leaving. The counterparty gets protection within
minutes and the certainty that the pledged units cannot be moved away. Both sides first record the
enforceable terms of their legal collateral agreement, and every later step is checked against
those terms.

Terms used below:

- **Giver**: the holder that owes margin and pledges units. **Taker**: the holder that demands
  margin and is protected by the pledge. Each acts through its custodian.
- **Agreement**: the recorded terms between one giver and one taker under one Fund.
- **Pledge**: a number of the giver's units locked for the taker under one agreement.
- **Locked units**: the units of a holder held under all its active pledges, across every
  agreement. **Unlocked units**: the holder's balance minus its locked units.
- **Required value**: the exposure the taker currently needs covered, in the Fund's cash asset.
- **Collateral value**: the agreement's pledged units × the latest price × (1 − haircut), rounded
  down to the smallest cash unit.
- **Coverage**: collateral value − required value. A negative coverage is a **shortfall**; a
  positive coverage is an **excess**.
- **Fresh price**: the Fund's latest price record, published no longer ago than the agreement's
  maximum price age.
- **Call**: a demand for more collateral (**delivery call**, from the taker) or for the return of
  excess collateral (**return call**, from the giver).
- **Unresolved**: a delivery call that is Open or Disputed (from F005, also Defaulted).
- **Demanded part**: the part of the current shortfall that unresolved delivery calls already
  demand (FR-015). **Uncalled shortfall**: the shortfall minus the demanded part.

Worked figures used in the tests below (unless a test says otherwise): price $1.00, haircut 2%,
minimum transfer amount $10,000, time to meet a call 4 hours, grace period 2 hours, pledge limit
5,000,000 units, maximum price age 26 hours, the taker may raise exposure alone, and the giver holds
2,000,000 units. Units and cash both have 6 decimal places, so "smallest unit" means 0.000001.

### User Story 1 - Agree the rules (Priority: P1)

Giver and taker record the enforceable terms of their legal collateral agreement: haircut, time to
meet a call, grace period, minimum transfer amount, pledge limit, maximum price age, whether the
taker may raise the exposure alone, and the fingerprint of the signed legal document. The agreement
becomes Active only once both have signed.

**Why this priority**: every later step checks these terms. Without an Active agreement no call,
pledge or release is possible.

**Independent Test**: the taker proposes an agreement with the worked terms; it is Proposed and
records exactly those terms and the taker's signature. The giver signs; it is Active. A third
holder's signature, or a second signature from the taker, is rejected.

**Acceptance Scenarios**:

1. **Given** two Approved holders with valid KYC, **When** one of them proposes an agreement naming
   both and the terms, **Then** it is Proposed, records exactly those terms, and counts the
   proposer's signature.
2. **Given** a Proposed agreement, **When** the other party signs, **Then** it is Active.
3. **Given** a Proposed agreement, **When** the proposer signs again or any third party signs,
   **Then** the signature is rejected and the agreement stays Proposed.
4. **Given** either party is not Approved with valid KYC (pending, suspended, revoked, KYC expired
   or not registered), **When** the agreement is proposed or signed, **Then** it is rejected.
5. **Given** a proposal naming the same holder as giver and taker, a haircut of 100% or more, a
   zero time to meet a call, a zero pledge limit, a zero maximum price age, an empty legal document
   fingerprint, or a pledge type other than lien, **Then** it is rejected.
6. **Given** a Proposed agreement, **When** anyone tries to set exposure, open a call or pledge
   under it, **Then** it is rejected.
7. **Given** a Proposed or Active agreement, **When** anyone tries to change its terms, **Then**
   it is rejected; no step in this feature changes terms once proposed.

---

### User Story 2 - Margin call from exposure (Priority: P1)

The taker records the protection it currently needs. If the collateral value falls short of it by
more than the minimum transfer amount, a delivery call opens automatically in the same step, with
a deadline to meet it and a later time from which enforcement is allowed. When the price falls
after pledging, the taker can open a further call for the part of the shortfall not yet called.

**Why this priority**: the call is what the giver must answer and what enforcement (F005) relies
on. Its value and deadlines must come from the agreed terms, not from either party's discretion.

**Independent Test**: with nothing pledged, the taker sets exposure to $1,000,000; a delivery call
opens for $1,000,000 requiring 1,020,408.163266 units, with a deadline 4 hours after it opened and
enforcement allowed from 6 hours after it opened.

**Acceptance Scenarios**:

1. **Given** an Active agreement whose uncalled shortfall, at a fresh price, would be more than the
   minimum transfer amount, **When** the taker's new exposure takes effect, **Then** in the same
   step a delivery call opens for the uncalled shortfall, with deadline = now + time to meet a call
   and enforce-after = deadline + grace period, and records the price record used and the units
   required (rounded up).
2. **Given** an uncalled shortfall at or below the minimum transfer amount, **When** the exposure
   takes effect, **Then** the exposure is recorded and no call opens.
3. **Given** the latest price is older than the agreement's maximum price age, **When** a raise in
   exposure would take effect, or the taker opens a delivery call, **Then** it is rejected and
   nothing changes.
4. **Given** an agreement on which the taker may not raise exposure alone, **When** the taker
   raises it, **Then** the raise is held as pending and nothing else changes; **When** the giver
   confirms that exact value, **Then** it takes effect as in scenario 1, with the call's deadlines
   counted from the confirmation.
5. **Given** units pledged that exactly cover $1,000,000, **When** the price falls to $0.99,
   **Then** the shortfall is $10,000 (equal to the minimum transfer amount) and the taker's attempt
   to open a call is rejected; **When** the price instead falls to $0.985, **Then** the taker can
   open a delivery call for $15,000 requiring 15,539.210609 units.
6. **Given** an unresolved delivery call for $1,000,000, **When** the taker opens another call
   while the shortfall is still $1,000,000, **Then** it is rejected, because nothing is uncalled.
7. **Given** an Active agreement, **When** anyone other than the taker sets its exposure or opens a
   delivery call, **Then** it is rejected.

---

### User Story 3 - Pledge without moving units (Priority: P1)

The giver locks some of its units for the taker. The call is met within minutes, while the units
stay in the giver's account and keep earning interest. From then on, the fund's transfer rules
refuse any transfer that would take the giver's balance below its locked units, whoever builds the
transfer and wherever it is sent.

**Why this priority**: this is the product. A pledge that moves no units and cannot be undone by
the giver is what makes fund units usable as margin.

**Independent Test**: after the US2 call opens, the giver pledges 1,020,408.163266 units; in one
step those units are locked, the giver's balance stays 2,000,000, no units move anywhere, and the
call is Met. A transfer of 979,591.836735 units, built by hand and submitted directly, is
rejected; a transfer of 979,591.836734 units succeeds.

**Acceptance Scenarios**:

1. **Given** an Active agreement and enough unlocked units, **When** the giver pledges N units,
   **Then** N units are locked, no units move, the pledge records the price record in use, and,
   if coverage is now zero or more, every unresolved delivery call is Met in the same step.
2. **Given** N is more than the giver's unlocked units, or would take the agreement's pledged units
   above its pledge limit, **When** the giver pledges N, **Then** it is rejected and nothing
   changes.
3. **Given** N of a giver's M units are locked, **When** the giver sends, funds a redemption or
   secondary sale (F003), or pledges under another agreement more than M − N units, **Then** it is
   rejected by the fund's transfer rules (or, for the pledge, by the unlocked-units check), and no
   units move.
4. **Given** N of a giver's M units are locked, **When** the giver sends exactly M − N units,
   **Then** it succeeds and the remaining N units stay locked.
5. **Given** a giver whose units are all locked and a holder with the same balance and nothing
   locked, **When** prices are published, **Then** both earn exactly the same interest.
6. **Given** anyone other than the giver, zero units, an agreement that is not Active, or a giver
   that is not Approved with valid KYC, **When** a pledge is attempted, **Then** it is rejected.
7. **Given** the latest price is older than the maximum price age, **When** the giver pledges,
   **Then** the pledge succeeds and locks the units, and open calls stay open until coverage is
   next checked at a fresh price.

---

### User Story 4 - Release excess (Priority: P2)

When exposure falls, the giver asks for excess units back by opening a return call. The taker, or
the keeper while the return call is open, then unlocks that excess. No release may leave the taker
under-covered.

**Why this priority**: without it, units stay locked after the trade shrinks. P2 because US1 to
US3 already protect the taker.

**Independent Test**: after the US3 pledge, the taker lowers exposure to $400,000; the giver opens
a return call for $600,000 covering 612,244.897959 units; the keeper releases those units; the
giver's locked units fall to 408,163.265307, no units move, coverage is exactly zero and the return
call is Met.

**Acceptance Scenarios**:

1. **Given** an excess of more than the minimum transfer amount at a fresh price, **When** the
   giver opens a return call, **Then** it opens for the whole excess and the units it covers,
   rounded down.
2. **Given** an excess at or below the minimum transfer amount, or another return call already
   open on the agreement, **When** the giver opens a return call, **Then** it is rejected.
3. **Given** an open return call, **When** the taker or the keeper releases units from an active
   pledge against it, **Then** those units are unlocked, no units move, and the call is Met once
   the units released against it reach its units.
4. **Given** a release that would make coverage negative at the current price, **When** the taker
   or the keeper attempts it, **Then** it is rejected.
5. **Given** no open return call, **When** the keeper releases units, or releases more than the
   open return call's remaining units, **Then** it is rejected.
6. **Given** an active pledge, **When** the giver or anyone other than the taker or keeper tries to
   release it, or anyone tries to release more than the pledge still holds, **Then** it is
   rejected.

---

### User Story 5 - Substitute (Priority: P2)

The giver replaces one active pledge with a new one in a single step, so there is no moment at
which the taker holds less protection.

**Why this priority**: it lets the giver restructure its pledges (for example, restate a pledge
against the current price record or enlarge it) without a release in between. P2 because pledge and
release (US3, US4) already cover the core need.

**Independent Test**: the giver replaces its 1,020,408.163266-unit pledge with a new pledge of
1,100,000 units; in one step the old pledge is Substituted, the new pledge is Active, the giver's
locked units are 1,100,000 and no units move. Replacing it with 1,000,000 units instead is
rejected.

**Acceptance Scenarios**:

1. **Given** an active pledge and enough unlocked units for the difference, **When** the giver
   substitutes it with a new pledge of at least as many units as the old pledge still holds,
   **Then** in one step the new pledge is Active, the old pledge is Substituted with its remaining
   units released, the giver's locked units rise by exactly the difference, and no units move.
2. **Given** a new pledge with fewer units than the old pledge still holds, **When** the giver
   substitutes, **Then** it is rejected (reducing a pledge is a release, US4).
3. **Given** a difference larger than the giver's unlocked units or above the pledge limit,
   **When** the giver substitutes, **Then** it is rejected and both pledges are unchanged.
4. **Given** anyone other than the giver, a pledge that is not active, an agreement that is not
   Active, or a giver that is not Approved with valid KYC, **When** a substitution is attempted,
   **Then** it is rejected.

---

### User Story 6 - Dispute (Priority: P3)

The giver disputes a delivery call it believes is wrong. While disputed, the call cannot be
enforced. The dispute ends when the giver withdraws it, the taker cancels the call, or compliance
resolves it.

**Why this priority**: disputes are part of every collateral agreement, but they matter only once
enforcement (F005) exists. P3 because US1 to US5 work without them.

**Independent Test**: the taker's $1,000,000 call opens; the giver disputes it with a reference;
it is Disputed with its deadlines unchanged. The giver withdraws the dispute; it is Open. The giver
disputes again and compliance resolves it as cancelled, with the reason "operational error"; it is
Cancelled, and the taker can open a new call for the shortfall.

**Acceptance Scenarios**:

1. **Given** an Open delivery call, **When** the giver disputes it with a non-empty reference,
   **Then** it is Disputed, keeps its deadline and enforce-after time, and the dispute is recorded
   permanently.
2. **Given** a return call, a call that is not Open, or a dispute by anyone other than the giver,
   **When** a dispute is attempted, **Then** it is rejected.
3. **Given** a Disputed call, **When** the giver withdraws its dispute, **Then** it is Open again;
   **When** the taker cancels the call, **Then** it is Cancelled.
4. **Given** a Disputed call, **When** compliance resolves it as Open or as Cancelled with a reason
   from the fixed list, **Then** it takes that status and the resolution is recorded with its
   reason; **When** anyone else tries to resolve it, **Then** it is rejected.
5. **Given** a Disputed call, **When** coverage at a fresh price comes to cover it, **Then** it is
   Met like any unresolved call.
6. **Given** a Disputed call, **When** enforcement is attempted, **Then** it is refused. (Verified
   once enforcement (F005) exists; in this feature nothing enforces.)

---

### Edge Cases

- **The price drops after pledging**: no call opens by itself when a price is published, because a
  publish must not grow in cost with the number of agreements. The taker opens a delivery call for
  the uncalled shortfall once it exceeds the minimum transfer amount (US2 scenario 5).
- **The price rises, or exposure falls, while a call is open**: anyone can ask for a re-check at a
  fresh price; if coverage is zero or more, every unresolved delivery call is Met. The same check
  runs inside every exposure update, call, pledge, release and substitution made at a fresh price.
  A price move counts from the first step that values the agreement at the new price.
- **Several pledges and calls on one agreement at once**: pledges add up; collateral value is
  taken on the agreement's total pledged units, rounded once. The agreement tracks the demanded
  part of the shortfall, and a new delivery call covers only the rest, so calls never
  double-count. When a pledge, a lower exposure or a higher price shrinks the shortfall, the
  demanded part shrinks with it, so a later rise opens a new call with its own deadline. All
  unresolved delivery calls are Met together once a step at a fresh price finds coverage zero or
  more; a call is met by covering the current shortfall, not by delivering its original value.
  Enforcement (F005) is limited by the shortfall and by the defaulted call's own value, so an
  earlier call never lets the taker enforce a later call's amount early. At most one return call
  is open per agreement, and it stays open alongside any delivery call until it is Met or the
  giver cancels it.
- **One giver, several agreements**: locked units are the sum over all the giver's active pledges
  in every agreement. A pledge limit applies per agreement. A release under one agreement never
  changes another agreement's pledges.
- **The giver is suspended (or revoked, or its KYC expires) while units are pledged**: its locks
  stay in place, its pledges and calls are unchanged, and call deadlines keep running. It cannot
  pledge, substitute or confirm an exposure raise until it is Approved with valid KYC again. The
  taker can still set exposure, open calls and release; the giver can still dispute and cancel its
  return call. Unlocking units does not let a suspended holder send them (F001). If a giver cannot
  cure because of a suspension, compliance can stop enforcement with a hold (F005).
- **The taker is suspended (or revoked, or its KYC expires)**: it cannot raise exposure or open
  calls; it can still lower exposure, release and cancel, since those only reduce the giver's
  burden.
- **Rounding**: units required by a delivery call are rounded up to the smallest unit (the giver
  always pledges enough); collateral value and the units released for a return call are rounded
  down (the taker is never left under-covered). Pledging exactly a call's units required at its
  price always meets that call.
- **Shortfall or excess exactly equal to the minimum transfer amount**: no call opens and no return
  call can be opened. A minimum transfer amount of zero means any shortfall or excess counts.
- **Stale price**: raising exposure, opening any call, releasing, and marking calls Met all need a
  fresh price. Pledging, lowering exposure, disputing, withdrawing a dispute and cancelling do not.
  A release when the required value is zero needs no price, so a missing publish never traps units
  that protect nothing.
- **Maximum price age shorter than the time between publishes**: calls can then be opened only in
  the window after each publish. This is the agreement's choice.
- **Exposure set to zero**: every unresolved delivery call is Met, and the taker can release every
  pledge.
- **Pledge limit reached**: further pledges under that agreement are rejected, even while a call is
  open; meeting the call is then a matter for the legal agreement.
- **Receiving while units are locked**: locks never block receiving. Units received raise the
  unlocked units.
- **Units in settlement escrow** (F003): units already funded into escrow are no longer in the
  giver's account and cannot be pledged. Funding a redemption or secondary sale with locked units
  is rejected (F003 US2 scenario 3, now verifiable).
- **Transfers paused** (F001): pledging, releasing and substituting move no units, so they are
  still allowed while transfers are paused. Meeting a margin call must not wait for an unpause.
- **Same external reference twice on one agreement**: the second call is rejected, so a call step
  submitted twice opens only one call.
- **Slow signers**: a step prepared hours before it is signed (for example a pledge waiting for
  custodian approval) is judged on the agreement's state when submitted, and takes effect at most
  once.
- **Fund not Open**: no agreement can be proposed for a Fund in Setup.

## Requirements *(mandatory)*

### Functional Requirements

**Agreement**

- **FR-001**: System MUST let an Approved holder with valid KYC propose an agreement under an Open
  Fund naming a giver and a taker (two different investor holders, one of them the proposer) and
  these terms: haircut, time to meet a call, grace period, minimum transfer amount,
  pledge limit in units, maximum price age, fingerprint of the legal document, and whether the
  taker may raise exposure alone. The pledge type is lien and interest goes to the giver; no other
  value is accepted in this feature. The proposal counts as the proposer's signature.
- **FR-002**: System MUST reject a proposal whose haircut is 100% or more, whose time to meet a
  call, pledge limit or maximum price age is zero, or whose legal document fingerprint is empty.
  A zero grace period and a zero minimum transfer amount are allowed.
- **FR-003**: System MUST make an agreement Active only when the party that did not propose it
  signs it. Any other signature MUST be rejected.
- **FR-004**: System MUST require both parties to be Approved with valid KYC when the agreement is
  proposed and when it is signed.
- **FR-005**: System MUST NOT let any step change an agreement's parties or terms once proposed.
- **FR-006**: System MUST identify each agreement by its Fund, giver, taker and a number chosen by
  the proposer, reject a second agreement with the same identity, and let anyone read any
  agreement.
- **FR-007**: System MUST reject every exposure, call, pledge, release and substitution step on an
  agreement that is not Active.

**Valuation**

- **FR-008**: System MUST compute collateral value as the agreement's total pledged units × the
  price in the Fund's latest price record × (1 − haircut), rounded down to the smallest cash unit,
  and coverage as collateral value − required value, each time they are used. Neither is stored.
- **FR-009**: System MUST treat the price as fresh only when the latest price record was published
  no longer ago than the agreement's maximum price age, and MUST reject any step that needs a
  valuation (raising exposure, opening a call, releasing while the required value is above zero)
  when it is not.
- **FR-010**: System MUST convert a value into units as value ÷ (price × (1 − haircut)), rounded up
  to the smallest unit for a delivery call and rounded down for a return call.

**Exposure**

- **FR-011**: System MUST let only the taker record a required value (zero or more, in the Fund's
  cash asset) with a non-empty external reference. A value at or below the current one MUST take
  effect at once, at any price age, and never opens a call.
- **FR-012**: A higher value MUST take effect at once only if the agreement lets the taker raise
  exposure alone. Otherwise it MUST be held as the agreement's pending raise until the giver
  confirms that exact value; a newer raise or any lower value replaces it, and confirming a
  replaced value MUST be rejected. A raise takes effect only at a fresh price.
- **FR-013**: When a raise takes effect and the uncalled shortfall is more than the minimum
  transfer amount, System MUST open a delivery call for it (FR-014) in the same step. This is one
  indivisible step so that the time to meet the call starts the moment the shortfall is recorded.
  Any exposure taking effect at a fresh price also applies FR-018.

**Calls**

- **FR-014**: System MUST open a delivery call, automatically (FR-013) or when the taker asks while
  the uncalled shortfall at a fresh price is more than the minimum transfer amount, recording: the
  uncalled shortfall as its value, the units required (FR-010), the price record used, issued
  time, deadline = issued time + time to meet a call, enforce-after time = deadline + grace period,
  the external reference, and status Open. Any other request MUST be rejected.
- **FR-015**: System MUST keep, per agreement, the demanded part of the shortfall: opening a
  delivery call raises it to the whole shortfall; every step that values the agreement at a fresh
  price lowers it to the shortfall when the shortfall has fallen below it; cancelling an
  unresolved delivery call lowers it by that call's value (not below zero); and it is zero
  whenever no delivery call is unresolved.
- **FR-016**: System MUST let the giver open a return call when the excess at a fresh price is more
  than the minimum transfer amount and no other return call is open, recording the excess as its
  value, the units it covers (FR-010), the price record used, issued time, deadline = issued time
  + time to meet a call, the external reference and status Open. A return call has no
  enforce-after time.
- **FR-017**: System MUST number calls per agreement from 1, reject an empty external reference or
  one already used by a call on the same agreement, and let anyone read any call.
- **FR-018**: In every step that values an agreement at a fresh price, System MUST check coverage:
  if it is zero or more, every unresolved delivery call on the agreement is Met in that step. Met,
  like Cancelled, is final. Marking calls Met is part of the step that changed coverage, so a
  call's status never disagrees with the coverage that step produced.
- **FR-019**: System MUST let anyone ask for a re-check of an agreement at a fresh price, which
  only applies FR-018. Its outcome MUST depend only on recorded state and the current time, never
  on who asked.
- **FR-020**: System MUST let the taker cancel an Open or Disputed delivery call, and the giver
  cancel its Open return call. Cancelled is final.

**Pledges and locks**

- **FR-021**: System MUST let only the giver pledge N > 0 units under an Active agreement, and MUST
  reject the pledge if N is more than the giver's unlocked units, if it would take the agreement's
  pledged units above the pledge limit, or if the giver is not Approved with valid KYC.
- **FR-022**: A pledge MUST record its agreement, its number within the agreement, the giver's
  reference for it (not empty, and unique within the agreement), units, time, the price record in
  use, released units (0), enforced units (0) and status Active; MUST add N to
  the giver's locked units and the agreement's pledged units; MUST move no units; and MUST apply
  FR-018 in the same step when the price is fresh.
- **FR-023**: The fund's transfer rules MUST reject every transfer of units that would leave the
  sender's balance below its locked units, whichever software builds it and wherever it goes,
  including funding a settlement escrow (F003). The only exception is a transfer made by the
  system's enforcement authority, which nothing in this feature can use (enforcement arrives in
  F005, court-ordered transfers in F007).
- **FR-024**: Only this feature's pledge, release and substitution steps (and, from F005,
  enforcement) MUST be able to change a holder's locked units. No holder, role or other part of the
  system can change them directly.
- **FR-025**: After every step, each holder's locked units MUST equal the sum over its active
  pledges of (units − released units − enforced units), and each agreement's pledged units the
  same sum over that agreement's pledges.
- **FR-026**: Locked units MUST earn interest exactly as unlocked units do (F002 FR-015). A lock
  MUST NOT block receiving units.

**Release**

- **FR-027**: System MUST let the taker release R > 0 units from an active pledge of its agreement
  at any time, and the keeper only while a return call is open and for at most that call's
  remaining units. Every release MUST be rejected if R is more than the pledge still holds, or if
  coverage after it would be negative at a fresh price (no price is needed while the required
  value is zero).
- **FR-028**: A release MUST add R to the pledge's released units (status Released once nothing
  remains), subtract R from the giver's locked units and the agreement's pledged units, and move no
  units. A release made against the open return call counts towards it, and the call is Met once
  such releases reach its units. The keeper's releases are always made against it.
- **FR-029**: System MUST reject a release by the giver or by anyone other than the taker or the
  keeper.

**Substitution**

- **FR-030**: System MUST let the giver replace an active pledge with a new pledge of at least as
  many units as the old pledge still holds, in one indivisible step: the new pledge is created
  (FR-022), the old pledge's remaining units are released and its status becomes Substituted, and
  the giver's locked units rise by exactly the difference. The unlocked-units, pledge-limit and
  giver-status checks of FR-021 apply to the difference. It is one step so that the taker's
  protection never dips between the two pledges.

**Dispute**

- **FR-031**: System MUST let the giver dispute an Open delivery call with a non-empty reference,
  making it Disputed without changing its deadline or enforce-after time.
- **FR-032**: System MUST end a dispute only when the giver withdraws it (the call returns to
  Open), the taker cancels the call (FR-020), compliance resolves it as Open or Cancelled with a
  reason from the fixed list used for F001's compliance actions, or the call is Met (FR-018).
- **FR-033**: A Disputed call MUST stay unresolved for the uncalled-shortfall and Met rules, and
  MUST be recorded so that enforcement (F005) refuses it.

**Parties' status and timing**

- **FR-034**: Steps that add a lock or an obligation (proposing, signing, raising or confirming
  exposure, opening a call, pledging, substituting) MUST require the acting party to be Approved
  with valid KYC. Lowering exposure, releasing, cancelling, disputing, withdrawing a dispute and
  re-checking MUST be allowed whatever either party's status. A party's status MUST NOT remove a
  lock or move a deadline.
- **FR-035**: Pledging, releasing and substituting MUST be allowed while transfers are paused.
- **FR-036**: System MUST judge every step on the agreement's state when it is submitted, however
  long ago it was prepared, and MUST let a signed step take effect at most once.

**Records**

- **FR-037**: System MUST leave a permanent record of every proposal, signature, activation,
  exposure update and pending raise, confirmation, call opened, Met or Cancelled, dispute,
  withdrawal and resolution, pledge, release, substitution and change of locked units. Each record
  MUST show the Fund, agreement, call or pledge number, units and values, the price record used,
  the signer (or, for re-checks, the triggering account), the time and the external reference; a
  compliance resolution also shows its reason.

### Key Entities *(include if feature involves data)*

- **Fund** (from F001 to F003): unchanged. Agreements exist only under an Open Fund.
- **Holder record** (from F001, F002): now also holds the holder's locked units and its number of
  active pledges. Its status and KYC decide who may propose, sign, pledge and call.
- **Price record** (from F002): unchanged. Collateral is valued at the latest one; each pledge and
  call names the one it used.
- **Agreement**: one per giver, taker and proposer-chosen number under a Fund. Giver, taker, pledge
  type (lien), interest to (giver), terms (haircut, time to meet a call, grace period, minimum
  transfer amount, pledge limit, maximum price age, legal document fingerprint, taker may raise
  exposure alone), status (Proposed, Active), each party's signature, total pledged units,
  required value, any pending raise, the demanded part of the shortfall, the open return call (if
  any), next pledge and call numbers, created time.
- **Pledge**: agreement, number, giver's reference, units, posted time, price record used, released units, enforced
  units (F005), status (Active, Released, Substituted; Enforced from F005).
- **Call**: agreement, number, kind (delivery, return), value, units required or to be released,
  price record used, issued time, deadline, enforce-after time (delivery only), units released
  against it (return only), status (Open, Met, Disputed, Cancelled; Defaulted and Enforced from
  F005), external reference, dispute reference.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A $1,000,000 call is met by a single pledge step in which zero units move: every
  holder's balance is the same before and after, and the call is Met in that step.
- **SC-002**: In randomized sequences of proposals, signatures, exposure changes, price publishes
  (up and down), calls, pledges, releases, substitutions, disputes, transfers, settlement funding,
  suspensions and pauses, after every operation each holder's locked units equal the sum of its
  active pledges and no holder's balance is below its locked units.
- **SC-003**: No test in this feature can move a locked unit: every direct transfer, settlement
  funding or pledge elsewhere that would use locked units is rejected, including steps built by
  hand and submitted without the project's own software.
- **SC-004**: Two holders with equal balances, one with every unit locked and one with none, earn
  exactly the same interest, to the smallest fraction, over any sequence of publishes.
- **SC-005**: In randomized tests, pledging a delivery call's units required at its price always
  meets it, and releasing a return call's units at its price never leaves coverage negative.
- **SC-006**: Every rule in this specification has at least one test showing it rejecting a
  violation with the specific reason.
- **SC-007**: A pledge, release or call takes the same effort whether the Fund has 10 or 10,000
  holders and 1 or several hundred agreements, and a price publish takes the same effort however
  many agreements exist.
- **SC-008**: A pledge step signed 6 hours after it was prepared succeeds while the agreement is
  Active and the giver still has the unlocked units.
- **SC-009**: An auditor can rebuild every agreement's history (exposures, calls and outcomes,
  pledges, releases, substitutions) and every holder's locked units from the permanent records
  alone.

## Assumptions

- **Exposure**: computed off-chain by the taker's risk system. The system records it; it does not
  compute or check it. If the taker records a wrong exposure, the giver's remedy is a dispute
  (US6) and the legal agreement.
- **Legal agreement**: exists off-chain and is the governing document. The on-chain terms mirror it
  and the fingerprint links the two, but the system cannot check that they match. Where they
  differ, the system enforces the on-chain terms.
- **Price**: collateral is valued at the fund admin's latest published price, trusted as in F002.
  Interest earned since the last publish, and interest earned but not yet paid, is not counted as
  collateral (a conservative default).
- **Input questions** (`docs/speckit-inputs.md` §3, F004), answered from the input and the shared
  model, for `/speckit-clarify` to confirm: (1) several pledges may exist under one agreement at
  once (the input's edge cases require it); (2) whether the taker may raise exposure alone is a
  term of each agreement, and lowering exposure never needs the giver; (3) the maximum price age
  is set per agreement (the input lists it among the agreement's terms).
- **Threshold**: "more than the minimum transfer amount" is read literally, so a shortfall or
  excess exactly equal to it triggers nothing. `docs/design.md` §6.5.4 uses "at least" for
  delivery; this spec follows the feature input.
- **Substitution**: with one Fund and lien pledges only, every unit is interchangeable, so a
  substitution here replaces one pledge record with a new, equal or larger one. Swapping into a
  different asset needs title transfer or several funds (backlog).
- **Keeper**: the keeper role from F001, trusted only for timing. It can release only units a
  return call already asks for and only within coverage, and it adds no discretion.
- **Compliance resolution of disputes**: until governance (F007) adds dedicated action records, the
  permanent record is the system's public activity log, as for F001's compliance actions.
- **Proposal withdrawal and termination**: not in this feature. A Proposed agreement has no effect
  until the other party signs, and an Active agreement with zero exposure and no pledges has no
  effect. Ending agreements is left to a later feature.
- **Enforcement**: Defaulted, Enforced, holds and enforcement itself belong to F005. In this
  feature no call becomes Defaulted; deadlines and enforce-after times are recorded for F005 to
  use.
- **Custodian behaviour**: custodians sign only what their institution approved. The system never
  holds an institution's signing key; it can only check that the right party signed.
- **Scale**: one Fund, up to about 10,000 holders and hundreds of agreements. The number of
  unresolved calls on an agreement does not change the effort of any step.
- **Out of scope**: title-transfer pledges, manufactured income, multilateral netting (backlog);
  terminating agreements or withdrawing proposals; changing terms after proposal; enforcement,
  defaults and holds (F005); and the collateral desk screens (F008).
