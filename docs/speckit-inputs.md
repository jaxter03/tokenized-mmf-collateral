# Spec Kit inputs: Tokenized MMF Collateral on Solana (full system)

Covers every phase: launch, onboarding, subscription, daily NAV and yield,
distributions, collateral, enforcement, redemption, governance, audit, and
custodian signing. Scope only. Spec Kit generates the tasks.

---

## 0. How to use this with Spec Kit

**Spec Kit rules this document follows**

- `/speckit-specify` produces **one feature per run**. The whole system is too big for one spec, so it is split into 8 features below. Run specify once per feature, in order.
- Specify input is **what and why only**: no languages, frameworks or chain names. Those go in `/speckit-plan`.
- The generated spec has these mandatory sections, and our inputs supply material for each:
  - **User Scenarios & Testing**: user stories with priority (P1, P2, P3), each with *Why this priority*, *Independent Test*, and *Acceptance Scenarios* (Given / When / Then)
  - **Edge Cases**
  - **Requirements**: Functional Requirements (FR-001 "System MUST …") and Key Entities
  - **Success Criteria**: Measurable Outcomes (SC-001), technology-agnostic
  - **Assumptions**
- At most 3 `[NEEDS CLARIFICATION]` markers per spec. Open questions are listed per feature below, so answer them in `/speckit-clarify`.
- `/speckit-plan` needs: Language/Version, Primary Dependencies, Storage, Testing, Target Platform, Project Type, Performance Goals, Constraints, Scale/Scope. It also runs a Constitution Check.

**So: yes, stories are expected in specify.** Each story must be independently testable and deliver value on its own.

**Order of commands**

```
/speckit-constitution          (once, section 1)
for each feature F001..F008 (start each from an up-to-date main):
    /speckit-specify  <F's specify input>
    (review spec.md; optionally compare it against earlier specs/)
    /speckit-clarify               (answer F's open questions)
    /speckit-plan     <shared plan input + F's plan notes>
    /speckit-tasks
    /speckit-analyze               (optional consistency check)
    /speckit-implement
    run ALL tests (this and earlier features) → merge to main
```

**Feature map**

| # | Feature | Lifecycle phases | Depends on |
|---|---|---|---|
| F001 | Fund launch, roles and holder onboarding | 0, 1, transfer rules of 3 | none |
| F002 | Daily price and yield accrual | 3 | F001 |
| F003 | Subscription, redemption and transfer settlement (DvP) | 2, 7 | F001, F002 |
| F004 | Collateral agreements, margin calls and pledges | 4 | F001, F002 |
| F005 | Default, compliance holds and enforcement | 5 | F004 |
| F006 | Monthly distributions | 6 | F002 |
| F007 | Governance, audit trail and reconciliation | 8, compliance powers, wind-down | F001 (touches all) |
| F008 | Custodian signing, operator API and collateral desk | all (off-chain) | F001 to F007 |
| Backlog | Title-transfer pledges, manufactured income, multilateral netting, confidential balances | | after F008 |

---

## 1. `/speckit-constitution`

```
Project: open-source, issuer-side infrastructure for a tokenized
money market fund used as collateral on Solana. The fund issuer controls
the token. The system covers the full lifecycle: launch, onboarding,
subscription and redemption, daily price and yield, distributions,
collateral pledges, margin calls, enforcement, governance and audit.

Principles:

1. Correctness over features. This code moves regulated assets. A smaller
   feature set that is provably right beats a larger one that is mostly right.

2. Rules live on-chain. Who may hold the token, what is locked, who earns
   yield, and when enforcement is allowed are enforced by the programs and
   the token itself, never only by a UI or backend.

3. Every state change has a named signer. No hidden backend keys. No
   off-chain service ever holds an institution's signing key.

4. One instruction, one state transition. If a description needs "and
   then", it is two instructions (except where atomicity is the point,
   e.g. both legs of a settlement).

5. On-chain state is the source of truth. Indexers and UIs mirror it and
   never decide anything. Values that can be computed (coverage, accrued
   yield) are computed, not stored separately.

6. Every rule has a failing test first. Each requirement gets at least one
   test that proves the rejection path, not just the happy path.

7. Fail closed. Any failed check reverts the whole transaction. No partial
   outcomes, ever.

8. No unpaid units, no silent powers. A unit is never created without its
   cash being locked in the same settlement. Every discretionary admin
   power (freeze, seize, forced transfer, rule change) is role-gated,
   reason-coded and permanently recorded; destructive ones are time-locked.

9. Money math is exact. Integer arithmetic only, rounding direction stated
   for every formula (always in favour of the protected party), and
   overflow checked.

10. Stated assumptions. Anything relied on but not enforced (legal
    agreement terms, KYC provider honesty, price accuracy) is written down.

11. Features ship in order. Each feature is complete, tested and merged
    before the next starts. Ideas outside the current feature go to the
    backlog, not into the code.

12. Earlier features stay green. No feature merges if any earlier
    feature's tests fail.

13. One shared model. Every design conforms to docs/shared-model.md
    (account layouts, seeds, authorities, allowed cross-program calls, hook
    check order, rounding). A change to it is made explicitly, in that
    file, in the same pull request, before code.
```

---

## 2. Feature specify inputs

### F001: Fund launch, roles and holder onboarding

```
Feature: Launch a tokenized money market fund and control who may hold it.

WHY
A regulated fund may only be held by approved (KYC-checked) institutions.
Today that is enforced by a transfer agent's database. We want the token
itself to refuse any holder who is not approved, so the rule cannot be
bypassed, while the issuer and compliance team keep full control.

USERS
- Fund issuer: creates and launches the fund, assigns roles.
- Compliance officer (a multi-person group): approves, suspends and revokes
  holders; can pause all transfers.
- Institution (via its custodian): applies to hold the fund.
- KYC provider: signs a statement that an institution passed checks.

USER STORIES

US1 (P1) Launch a fund
As the issuer, I want to create a fund with its terms (name, target price,
distribution style, daily redemption limit, link and fingerprint of the
legal documents) and open it, so investors can start onboarding.
Why this priority: nothing else works without a fund.
Independent test: create a fund, launch it, read back its terms and status.
Acceptance:
- Given no fund, when the issuer creates one, then it exists in Setup
  status with the given terms and nobody can hold units yet.
- Given a fund in Setup, when the issuer launches it, then it is Open.
- Given a fund, when anyone other than the issuer tries to launch it,
  then it is rejected.

US2 (P1) Approve holders
As a compliance officer, I want an institution to become able to hold units
only after a recognised KYC provider has signed for it.
Why this priority: the core regulatory promise.
Independent test: register an institution, approve it with a valid
statement, then send it units successfully.
Acceptance:
- Given a newly registered institution, then its account cannot send or
  receive units (Pending).
- Given a valid statement signed by a recognised provider, when compliance
  approves, then the institution can receive units.
- Given a statement from an unknown provider or with a past expiry, then
  approval is rejected.
- Given an unapproved destination, when anyone sends units to it, then the
  transfer is rejected.

US3 (P2) Suspend, revoke and pause
As a compliance officer, I want to suspend or revoke a holder, and pause all
transfers in an emergency.
Independent test: suspend a holder and show it can no longer send or
receive; pause and show no transfer succeeds; unpause and show it works.
Acceptance:
- Given a suspended holder, then it can neither send nor receive units.
- Given transfers are paused, then every ordinary transfer is rejected.
- Given a holder whose KYC has expired, then it is treated as not approved.

US4 (P2) Roles
As the issuer, I want to assign the roles (fund admin, compliance, keeper,
auditor) to specific accounts or groups, so every action has an
accountable owner.
Acceptance:
- Given a role assignment, then only that holder of the role can perform
  that role's actions.

EDGE CASES
- What happens when the holder cap (maximum number of holders) is reached?
- What happens when a holder's KYC expires while it has units?
- How does the system handle an institution registered twice?
- What happens when a transfer has the same sender and receiver?

KEY ENTITIES
- Fund: terms, status (Setup, Open, Gated, Suspended, Winding down,
  Closed), roles.
- Holder record: owner, status (Pending, Approved, Suspended, Revoked),
  class (investor, fund treasury, escrow), jurisdiction, KYC expiry,
  fingerprint of the KYC statement and who signed it.
- Register: list of recognised KYC providers, allowed jurisdictions,
  holder cap, pause flag.

SUCCESS CRITERIA
- No unapproved account ever holds units, across all tests.
- A compliance action (approve, suspend, pause) takes effect in the next
  transaction.
- Every holder status change leaves a permanent, queryable record.

ASSUMPTIONS
- KYC providers are trusted to sign honestly; the system only checks the
  signature and expiry. Real provider integration is mocked.
- The issuer controls the token; this is not a layer on someone else's
  fund token.

OUT OF SCOPE
- Real KYC provider APIs; investor-facing onboarding screens.
```

### F002: Daily price and yield accrual

```
Builds on 001-fund-launch-onboarding (Fund, Holder record, Register,
roles). Reuse those entities; do not redefine them.

Feature: Publish the fund's daily price and interest, and credit every
holder's share fairly without updating every account each day.

WHY
A money market fund earns interest daily and announces a price per unit
(target $1.00). Each holder must earn exactly for the days they held units,
even if they trade in between. Updating thousands of accounts every day is
slow and fragile; a missed update means someone is paid wrongly.

USERS
- Fund administrator: publishes price and daily interest rate.
- Holders: earn interest.
- Auditor: checks the history.

USER STORIES

US1 (P1) Publish daily price and interest
As the fund admin, I want to publish today's price and interest rate once a
day, kept permanently, so every later calculation uses an official figure.
Why this priority: collateral values and interest both depend on it.
Independent test: publish three days of figures and read the full history.
Acceptance:
- Given an Open fund, when the admin publishes, then a new permanent
  record with a sequence number and time is stored.
- Given someone who is not the fund admin, then publishing is rejected.
- Given a price that moves more than the allowed limit from yesterday,
  then it is rejected unless governance has approved an override.

US2 (P1) Fair interest for every holder
As a holder, I want interest for exactly the units I held on each day,
even if my balance changed, so I am never over- or under-paid.
Why this priority: getting this wrong is a financial error for real people.
Independent test: holder A holds for 10 days, sends half on day 10, holds
the rest for 10 more; compare earned interest to a day-by-day calculation.
Acceptance:
- Given a holder whose balance did not change for N days, then their earned
  interest equals balance × sum of the N daily rates.
- Given a balance change, then interest up to that moment is settled on the
  old balance before the change takes effect.
- Given units are locked as collateral (F004), then they still earn
  interest for their owner.
- Given rates that differ every day, then each day is credited at its own
  rate.

EDGE CASES
- What happens when no price is published for a day (holiday, outage)?
- How are weekends handled (one publish covering three days)?
- What happens with a zero or negative daily rate?
- How are rounding remainders handled over millions of holders?

KEY ENTITIES
- Price record: sequence, price, daily rate, timestamp, publisher.
- Global interest counter: cumulative interest per unit since launch.
- Holder accrual: counter value at last settlement, interest earned so far.

SUCCESS CRITERIA
- Earned interest matches an independent day-by-day calculation to the
  smallest unit, across randomised 90-day simulations.
- Publishing costs the same whether there are 10 or 10,000 holders.
- The price used for any past calculation can be looked up by sequence.

ASSUMPTIONS
- The administrator's figures are accurate; the system guards only against
  obvious typos and staleness.
- Interest accrues to whoever holds units at the moment of publishing.

OUT OF SCOPE
- Paying out interest (F006); computing the rate from the portfolio.
```

### F003: Subscription, redemption and transfer settlement (DvP)

```
Builds on 001 (Fund, Holder record, roles) and 002 (Price record).
Reuse those entities; do not redefine them.

Feature: Exchange cash for fund units (and back) so that either both sides
move or neither does.

WHY
Buying or redeeming fund units today takes one to two days and relies on
trust between the parties. On-chain, the risk is paying and not receiving
(or delivering and not being paid). Institutions also sign through
custodians who may take hours to approve, so a settlement must stay valid
for that long.

USERS
- Investor (via custodian): subscribes or redeems.
- Fund treasury: receives subscription cash, pays redemption cash.
- Keeper: triggers settlement once both sides are funded.
- Compliance: can reject a pending settlement.

USER STORIES

US1 (P1) Subscribe
As an investor, I want to pay cash and receive newly created units at the
published price, with no risk of paying without receiving.
Why this priority: this is how units come into existence.
Independent test: investor funds $2M; settle; investor has 2,000,000 units
at price $1.00 and treasury has $2M.
Acceptance:
- Given a subscription instruction priced from a specific price record,
  when the investor funds the cash, then the cash is held in escrow.
- Given the cash is funded, when settlement runs, then units are created
  and delivered to the investor and cash goes to the treasury, together.
- Given the investor is not approved, then settlement fails and nothing
  moves.
- Given the cash has not been funded, when settlement runs, then it is
  rejected and no units are created.

US2 (P1) Redeem
As an investor, I want to return units and receive cash at the published
price, within the fund's daily redemption limit.
Independent test: redeem 500,000 units and receive $500,000.
Acceptance:
- Given units locked as collateral, then they cannot be put into a
  redemption.
- Given today's redemptions would exceed the daily limit, then the
  redemption is rejected.
- Given settlement succeeds, then the units are destroyed and cash is paid
  together.

US3 (P2) Secondary transfer against cash
As two approved holders, we want to swap units for cash with each other
with the same both-or-neither guarantee.

US4 (P2) Cancel, expire, reject
As a party, I want unfunded or stuck settlements to be cancelled or to
expire with full refunds, and compliance to be able to reject one.
Acceptance:
- Given an instruction past its expiry, then anyone can expire it and
  each funded side is refunded.
- Given compliance rejects, then all funded sides are refunded.

US5 (P2) Slow signers
As a custodian, I want to approve a settlement step hours after it was
prepared, without it becoming invalid.

EDGE CASES
- One side funds, the other never does.
- The price changes between instruction and settlement (the instruction
  keeps its original price).
- Settlement is attempted twice.
- A party is suspended after funding but before settlement.

KEY ENTITIES
- Settlement instruction: kind (subscription, redemption, transfer),
  parties, units, cash amount, price record used, valid-from, expiry,
  status (Instructed, Seller funded, Buyer funded, Affirmed, Settled,
  Cancelled, Expired, Rejected), external reference.
- Escrow holdings for units and cash per instruction.

SUCCESS CRITERIA
- In every test, after any settlement outcome, either both legs moved or
  every party has exactly what it started with.
- A step signed hours after preparation still succeeds.
- No unit exists without matching cash received.

ASSUMPTIONS
- Cash is a stablecoin or tokenized deposit already available on-chain.

OUT OF SCOPE
- Order books, price discovery, fiat rails.
```

### F004: Collateral agreements, margin calls and pledges

```
Builds on 001 (Holder record, roles) and 002 (Price record, interest
accrual). Reuse those entities; do not redefine them. The holder's locked
units live on the existing Holder record.

Feature: Let a holder pledge fund units to a counterparty as margin,
without moving them, and keep the pledge in line with the exposure.

WHY
Institutions owe margin on trades (FX forwards, swaps, repos). Meeting a
call from a money market fund means selling units and waiting days for
cash. Pledging the units instead is instant, and they keep earning
interest. The counterparty needs certainty that pledged units cannot be
moved away.

USERS
- Collateral giver (via custodian): owes margin, pledges units.
- Collateral taker (via custodian): demands margin, receives protection.

USER STORIES

US1 (P1) Agree the rules
As giver and taker, we want to record the enforceable terms of our legal
collateral agreement (haircut, time to meet a call, grace period, minimum
transfer amount, pledge limit, maximum price age, fingerprint of the legal
document), active only once both of us have signed.
Why this priority: every later step checks these terms.
Acceptance:
- Given one signature, then the agreement is Proposed, not Active.
- Given both signatures, then it is Active.
- Given either party is not an approved holder, then it is rejected.

US2 (P1) Margin call from exposure
As the taker, I want to set the amount of protection I currently need, and
have a call with deadlines opened automatically if the pledged value falls
short.
Independent test: set exposure $1M with nothing pledged; a call opens for
$1M with a deadline and an enforcement-allowed time.
Acceptance:
- Given pledged value (after haircut, at the latest price) below the
  required amount by more than the minimum transfer amount, then a call
  opens with deadline = now + cure time and enforce-after = deadline +
  grace.
- Given a shortfall below the minimum transfer amount, then no call opens.
- Given the latest price is older than allowed, then no call can be issued.

US3 (P1) Pledge without moving units
As the giver, I want to lock N of my units for the taker, so the call is
met in minutes, while the units stay in my account and keep earning.
Acceptance:
- Given an active agreement and enough unlocked units, when I pledge N,
  then N units are locked, none move, and the call is Met if coverage is
  now zero or more.
- Given N exceeds my unlocked units or the pledge limit, then rejected.
- Given N of my M units are locked, when I try to send, redeem or pledge
  elsewhere more than M−N, then rejected by the token itself.

US4 (P2) Release excess
As the giver, when exposure falls, I want excess units unlocked.
Acceptance:
- Given coverage exceeds the requirement by more than the minimum transfer
  amount, then the giver can open a return call and the taker (or keeper)
  can release that excess.
- Given a release would make coverage negative, then rejected.

US5 (P2) Substitute
As the giver, I want to swap one pledge for another in one step, so
coverage never dips.

US6 (P3) Dispute
As the giver, I want to dispute a call I think is wrong, which blocks
enforcement until resolved.

EDGE CASES
- The price drops after pledging so coverage turns negative (new call).
- Several pledges and calls on one agreement at once.
- The giver is suspended while units are pledged.
- Rounding when converting a value to a unit count (always round up the
  units required from the giver).

KEY ENTITIES
- Agreement: giver, taker, pledge type (lien only in this feature), terms
  above, status, total pledged units, required value.
- Pledge: agreement, units, time, price record used, released units,
  enforced units, status.
- Call: agreement, kind (delivery, return), value, units required, price
  record, issued time, deadline, enforce-after time, status (Open, Met,
  Disputed, Defaulted, Enforced, Cancelled), external reference.
- Holder lock: total locked units per holder.

INVARIANT
- A holder's locked units always equal the sum of their active pledges
  (units − released − enforced).

SUCCESS CRITERIA
- A $1M call is met by a pledge in one transaction with zero units moving.
- No test can move locked units by any path other than enforcement (F005).
- Locked units earn the same interest as unlocked units.

ASSUMPTIONS
- Exposure is calculated off-chain by the taker's risk system; the system
  records it, it does not compute it.
- The legal agreement exists off-chain; on-chain terms mirror it.

OUT OF SCOPE
- Title-transfer pledges, manufactured income, multilateral netting
  (backlog).
```

### F005: Default, compliance holds and enforcement

```
Builds on 004 (Agreement, Pledge, Call, Holder lock) and 002 (Price
record). Reuse those entities; do not redefine them.

Feature: When a giver fails to meet a call, let the taker take exactly the
shortfall, under the agreed deadlines, unless compliance blocks it.

WHY
Collateral is only worth something if the taker can actually use it when
the giver defaults. But taking it must be limited to what is owed, follow
the agreed timing, and be stoppable for sanctions or court orders.

USERS
- Keeper: marks a call defaulted after the deadline.
- Collateral taker: enforces.
- Compliance officer: places or lifts a hold.
- Auditor: reviews the record.

USER STORIES

US1 (P1) Mark default
As the keeper, I want to mark a call Defaulted once its deadline passed
and coverage is still negative.
Acceptance:
- Given the deadline has not passed, or coverage is not negative, then
  rejected.

US2 (P1) Enforce the shortfall
As the taker, after the grace period, I want units worth the shortfall
moved from the giver to me, with a permanent record.
Why this priority: this is what makes the collateral real.
Independent test: $500k call defaults; after grace, enforce; taker gets
510,204 units at $1.00 and 2% haircut, giver's lock shrinks by the same.
Acceptance:
- Given Defaulted and enforce-after passed, no hold, not disputed, price
  fresh, then units worth at most the shortfall move, the lock and pledge
  shrink equally, and a reason-coded record is written.
- Given I ask for more than the shortfall, then rejected.
- Given I (the taker) am not an approved holder, then everything fails.
- Enforcement moves units only and never triggers a fund redemption.

US3 (P1) Compliance hold
As a compliance officer, I want to block enforcement on a specific call
immediately, with a reason and case reference, and lift it later.
Acceptance:
- Given an active hold, then enforcement is rejected.
- Given the hold is lifted, then enforcement may proceed.

US4 (P2) Late cure
As the giver, I want to cure a defaulted call before enforcement happens.

EDGE CASES
- Hold placed in the same block as enforcement.
- Partial enforcement repeated until the shortfall is covered.
- The giver's units are fewer than the shortfall (take all pledged, record
  the remaining unsecured amount).
- Price stale at enforcement time.

KEY ENTITIES
- Enforcement hold: call, placed by, reason, case reference, placed and
  lifted times.
- Action record: kind (enforcement), reason (collateral default), call
  reference, units, time.

SUCCESS CRITERIA
- Every rejection path above has a passing test.
- Enforcement never transfers more value than the shortfall at the price
  used.
- Every enforcement is traceable to its call, agreement and price record.

ASSUMPTIONS
- The pre-agreed cure and grace periods are the only delay; no extra
  discretionary waiting period applies to enforcement.

OUT OF SCOPE
- Selling the enforced units (the taker uses F003 redemption afterwards).
```

### F006: Monthly distributions

```
Builds on 001 (Fund, Holder record) and 002 (interest counter and
per-holder accrual). Reuse those entities; do not redefine them.

Feature: Pay each holder the interest they earned, once a month, as extra
units or as cash.

WHY
Interest accrues daily (F002) but is paid monthly. Every holder must be paid
exactly once, the exact amount, even if the payout run is interrupted.

USERS
- Transfer agent: opens and closes a distribution cycle.
- Keeper: pays holders.
- Holders: receive interest.

USER STORIES

US1 (P1) Close a cycle
As the transfer agent, I want to freeze the interest counter at month end,
so everyone is paid up to the same point.

US2 (P1) Pay every holder exactly once
As a holder, I want my month's interest as new units (or cash, per fund
terms), with a receipt, never twice.
Independent test: 50 holders with different histories; run payouts;
interrupt halfway; resume; every holder paid once, totals match.
Acceptance:
- Given a closed cycle, when the keeper pays a holder, then they receive
  exactly their earned amount up to the frozen point and a receipt exists.
- Given a receipt already exists, then paying again is rejected.
- Given pledged units, then their interest goes to the giver (owner).

US3 (P2) Finalise
As the transfer agent, I want to confirm all holders were paid and the
total equals the fund's total interest.

EDGE CASES
- Holder suspended at payout time.
- Holder with zero balance but unpaid interest.
- Rounding dust across many holders.

KEY ENTITIES
- Distribution cycle: number, mode, frozen counter, total due, total paid,
  status.
- Payout receipt per holder per cycle.

SUCCESS CRITERIA
- Sum of payouts equals total interest due within stated rounding rules.
- No holder is paid twice, even with interrupted runs.

OUT OF SCOPE
- Tax withholding and reporting.
```

### F007: Governance, audit trail and reconciliation

```
Builds on 001 to 006. Admin actions defined there gain a permanent
reason-coded record here. Reuse existing entities; do not redefine them.

Feature: Give the issuer and compliance strong powers, but make every one
accountable, and let an auditor prove the on-chain register matches the
official one.

WHY
Regulators require the issuer to freeze, recover and force-transfer units
(court orders, lost keys). Institutions fear unchecked admin power. Both are
satisfied if powers are role-gated, reason-coded, recorded, and the
destructive ones are announced in advance.

USERS
- Issuer, compliance, auditor.

USER STORIES

US1 (P1) Reason-coded actions
As compliance, every action I take (freeze, thaw, pause, hold, seize,
forced transfer, rule change) must carry a reason code and case reference
and leave a permanent record.

US2 (P1) Delayed destructive actions
As an institution, I want seizure, forced transfer and rule changes to be
proposed publicly and executable only after a waiting period, cancellable
before then.
Acceptance:
- Given a proposal, when executed before the waiting period, then rejected.
- Given protective actions (freeze, pause, hold), then they apply at once.

US3 (P2) Register fingerprint
As an auditor, I want a fingerprint of every holder's balance, locked units
and status (and of all pledges) at a point in time recorded on-chain, so I
can check it against the transfer agent's register.

US4 (P3) Wind down
As the issuer, I want to wind down the fund and close the token once all
units are redeemed.

EDGE CASES
- Proposal for a holder who is later revoked.
- Two conflicting proposals.
- Fingerprint submitted for an old point in time.

KEY ENTITIES
- Role assignments; action record (kind, reason, case reference, actor,
  time, status); proposal (action, earliest execution time);
  register fingerprint (period, time point, value, submitted by).

SUCCESS CRITERIA
- 100% of admin actions in tests produce a record with a reason.
- An independent recomputation of the register fingerprint matches the
  recorded one.

OUT OF SCOPE
- Regulatory report generation.
```

### F008: Custodian signing, operator API and collateral desk

```
Builds on 001 to 007. No new on-chain data; reads and prepares
transactions for what already exists.

Feature: Let institutions use the system through their custodians, and give
operators and collateral teams a clear view of everything.

WHY
Institutions do not hold keys on laptops; custodians approve transactions
through multi-person workflows that take hours. Collateral teams need to
see coverage, open calls and deadlines at a glance.

USERS
- Custodian, collateral giver and taker teams, fund admin, compliance.

USER STORIES

US1 (P1) Prepare, review, sign later
As a custodian, I want the service to give me an unsigned transaction that
stays valid for hours, plus a simulation showing what it will do and why it
might fail, so I can approve it in my own system.
Acceptance:
- Given a prepared transaction signed hours later, then it still executes.
- Given a simulation, then the rejection reason is human-readable.

US2 (P1) Collateral desk
As a collateral team, I want to see for each agreement the pledged value
at the latest price, the required amount, coverage, open calls with
countdown, and any holds.

US3 (P2) Operator views
As fund admin and compliance, I want views for price publishing, the
holder register, the settlement queue, the action log and reconciliation
status.

US4 (P2) Read model
As an integrator, I want an indexed history of all events, consistent with
on-chain state.

EDGE CASES
- Indexer lag or restart; the UI must show data freshness.
- Prepared transaction used twice.

SUCCESS CRITERIA
- No service component ever holds an institution's signing key.
- The full demo (launch to reconciliation) runs on devnet through the API.
- Indexed balances match on-chain balances at every checkpoint.

OUT OF SCOPE
- Real custodian integrations (mocked signer with delay).
```

---

## 3. Open questions for `/speckit-clarify` (per feature)

Answer these yourselves when Spec Kit asks; each changes behaviour.

**F001**
1. Can an institution register itself, or only compliance registers it?
2. Is there a holder cap, and what happens when it's reached?
3. Does suspension also block receiving, or only sending?

**F002**
1. How is a missed day handled: skip, or the next publish covers several days?
2. What is the maximum allowed daily price move before an override is required?
3. Rounding: truncate per holder, with the remainder kept by the fund?

**F003**
1. Who creates a subscription instruction: the investor or the fund operator?
2. Default expiry for a settlement instruction?
3. Is the daily redemption limit a percentage of total units or a fixed amount?

**F004**
1. Can several pledges exist under one agreement at once?
2. Can the taker set exposure alone, or must the giver confirm it?
3. Maximum price age: one fixed value or per agreement?

**F005**
1. Who may mark a call defaulted: only the keeper role, or anyone after the deadline?
2. Partial and repeated enforcement allowed, or one shot per call?
3. If pledged units don't cover the shortfall, what is recorded?

**F006**
1. Distribution as units only, cash only, or per-fund choice?
2. Who pays the transaction cost of each payout?

**F007**
1. Waiting period length for destructive actions (e.g. 24 h)?
2. How often is the register fingerprint committed (daily?), and by whom?

**F008**
1. Which custodian flow to mock (multi-approver with delay)?
2. Is the UI read-only plus prepare, or does it also submit?

---

## 4. `/speckit-plan`

### 4.1 Shared plan input (use for every feature)

```
Language/Version: Rust (stable), Anchor framework for on-chain programs.
TypeScript for the thin UI and client SDK.

Primary Dependencies:
- SPL Token-2022 for the fund unit mint, with extensions:
  transfer hook (our compliance program runs on every transfer),
  default account state = frozen (new accounts unusable until approved),
  permanent delegate held by the governance program (enforcement and
  court-ordered transfers only), metadata pointer (fund documents),
  mint close authority. Mint authority = fund program; freeze authority =
  compliance hook program.
- Cash leg: SPL token (USDC on devnet or a mock stablecoin).
- Squads v4 multisig for issuer and compliance roles.
- Off-chain: axum, sqlx, Yellowstone gRPC (or RPC polling on devnet).

Programs (one responsibility each):
- compliance_hook: holder register, approval status, locked units,
  interest accrual, checks on every transfer.
- fund: terms, status, daily price and rate, mint/burn, distributions,
  redemption limit.
- dvp_settlement: escrowed both-or-neither settlement.
- collateral: agreements, pledges, calls, release, substitution, dispute,
  holds, enforcement.
- governance: roles, reason-coded actions, time-locked proposals,
  enforcement transfers as permanent delegate, register fingerprints.

Storage: on-chain program accounts are the source of truth; Postgres for
the indexer read model only.

Testing: LiteSVM (or Bankrun) for program tests, one rejection test per
rule; property tests for interest and collateral math; one end-to-end run
on devnet per feature.

Target Platform: Solana devnet (mainnet-compatible).

Project Type: Anchor workspace (programs/), Rust services (services/),
TypeScript client and UI (app/).

Performance Goals: every user transaction, including the transfer-hook
call, fits within Solana compute and account limits; daily publish cost
independent of holder count.

Constraints: no backend holds institutional keys; prepared transactions use
durable nonces so custodian approval can take hours; integer math only with
stated rounding; all checks fail closed.

Scale/Scope: one fund, up to ~10,000 holders, hundreds of agreements.

Before designing, read: docs/shared-model.md, docs/design.md, and the
data-model.md of every earlier spec under specs/. Extend existing accounts
as listed in shared-model.md section 11; never create a second account for
an existing concept. List any change to shared-model.md explicitly in the
plan.
```

### 4.2 Per-feature plan notes (append to the shared input)

| Feature | Add to `/speckit-plan` |
|---|---|
| F001 | Build the mint with all extensions in one setup instruction; ExtraAccountMetaList resolves the holder records of sender and receiver; KYC statement verified via ed25519 signature check. |
| F002 | Global accrual index (fixed-point u128, 1e18) on the fund; each holder record stores index at last touch and accrued amount; the hook settles accrual before every balance change; fund settles accrual before mint/burn (hooks don't fire on mint/burn). |
| F003 | Escrow vaults owned by program PDAs; settle mints into escrow for subscriptions and burns from escrow for redemptions; the instruction stores the price record sequence it priced against. |
| F004 | Lien = locked amount in the holder record, set only by CPI from collateral; hook rejects any transfer leaving balance below the locked amount unless the authority is the governance PDA. |
| F005 | collateral calls governance by CPI; governance signs transfer_checked as permanent delegate; the hook bypasses the lock but still checks the receiver; ActionRecord written in the same transaction. |
| F006 | Distribution cycle freezes the index; payout receipt PDA per holder per cycle makes payouts idempotent. |
| F007 | Proposal accounts with earliest-execution timestamp; register fingerprint = merkle root of sorted (owner, balance, locked, status) built off-chain from RPC at a fixed slot. |
| F008 | API returns v0 transactions with AdvanceNonce first, plus simulation logs; indexer from program events; UI reads from the indexer and shows data freshness. |

---

## 5. Backlog (not in any spec until F001 to F008 are merged)

- Title-transfer pledges (units move to a taker-controlled vault) and manufactured income back to the giver
- Multilateral netting when the operator is collateral agent
- Confidential balances
- Multiple share classes or funds
- Real KYC and custodian integrations
- Cross-chain collateral
