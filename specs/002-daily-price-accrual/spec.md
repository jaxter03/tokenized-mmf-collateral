# Feature Specification: Daily Price and Interest Accrual

**Feature Branch**: `002-daily-price-accrual`

**Created**: 2026-10-13

**Status**: Draft

**Input**: User description: "Publish the fund's daily price and interest, and credit every
holder's share fairly without updating every account each day." (full input:
`docs/speckit-inputs.md`, F002). Builds on 001-fund-launch-onboarding: the Fund, Holder record,
Register and roles defined there are reused, not redefined.

## Clarifications

### Session 2026-10-13

- Q: When no price is published for a day (weekend, holiday, outage), how are those days covered?
  → A: Each publish states how many days it covers (1 normally, 3 on a Friday, more before a
  holiday), starting the day after the previous publish's covered period; a late publish covers
  the gap.
- Q: What guard stops a mistyped daily interest rate? → A: None. The admin's rate is trusted as
  stated.
- Q: Before the governance feature (F007) exists, who approves a price override? → A: Compliance,
  acting through governance as it does for suspensions and pauses.

## User Scenarios & Testing *(mandatory)*

A money market fund earns interest every day and announces a price per unit, normally $1.00. Each
holder must earn interest for exactly the units it held on each day, even if it trades between
days. Updating every holder's account every day is slow, and one missed update means someone is
paid wrongly. This feature makes the fund publish one official figure per day and keep a single
running interest counter for the whole fund. Each holder's share is worked out from that counter
whenever its balance changes, so publishing never touches individual accounts.

### User Story 1 - Publish daily price and interest (Priority: P1)

The fund admin publishes today's price per unit and today's daily interest rate. Each publish is
kept permanently with a sequence number, so every later calculation (interest, collateral value,
subscription and redemption amounts) can name the official figure it used.

**Why this priority**: collateral values and interest both depend on it. Without a published
price and rate, no later feature can value or pay anything.

**Independent Test**: on an Open fund, publish three days of figures, then read back the full
history by sequence number and check each record's price, rate, days covered, time and publisher.

**Acceptance Scenarios**:

1. **Given** an Open fund, **When** the fund admin publishes a price and a daily rate, **Then** a
   new permanent price record is stored with the next sequence number, the price, the rate, the
   days it covers, the time and the publisher, and the fund's latest price and interest counter
   are updated.
2. **Given** an Open fund, **When** anyone other than the fund admin tries to publish, **Then** it
   is rejected and nothing changes.
3. **Given** a fund in Setup, **When** the fund admin tries to publish, **Then** it is rejected.
4. **Given** a previous price, **When** the fund admin publishes a price that moves further from it
   than the fund's largest allowed daily price move, **Then** it is rejected, unless an override
   for exactly that price has been approved, in which case it is accepted and the override is used
   up.
5. **Given** three publishes, **When** an auditor asks for record 1, 2 or 3, **Then** each comes
   back exactly as it was first stored, and no operation can change or remove it.
6. **Given** the days already covered by earlier publishes, **When** the fund admin tries to
   publish again for a day already covered, **Then** it is rejected.

---

### User Story 2 - Fair interest for every holder (Priority: P1)

A holder earns interest for exactly the units it held at each publish, at that publish's rate,
however often its balance changes in between. No account is updated when the price is published;
a holder's interest is brought up to date the moment its balance is about to change, on the old
balance, before the change takes effect.

**Why this priority**: getting this wrong is a financial error that real holders suffer.

**Independent Test**: holder A holds 1,000,000 units for 10 publishes, sends half to holder B
after publish 10, then holds the rest for 10 more publishes, with a different rate each day.
Compare A's and B's earned interest with a day-by-day calculation: they must match exactly.

**Acceptance Scenarios**:

1. **Given** a holder whose balance did not change across N publishes, **When** its earned
   interest is read, **Then** it equals balance × the sum of those N publishes' rates, each times
   the days it covers.
2. **Given** a holder with earned interest not yet brought up to date, **When** its balance
   changes, **Then** interest up to that moment is settled on the old balance first, and only
   later publishes use the new balance.
3. **Given** units locked as collateral (F004), **When** a publish happens, **Then** the locked
   units still earn interest for the owner of the account that holds them. (Verified once the
   collateral feature exists; in this feature no lock exists and interest depends only on
   balance.)
4. **Given** rates that differ every day, **When** a holder's interest is settled after several
   publishes, **Then** each publish is credited at its own rate, never at the latest or an average
   rate.
5. **Given** an account that holds no units, **When** it first receives units, **Then** it earns
   nothing for publishes made before it held them.
6. **Given** a transfer and a publish, **When** the transfer is recorded before the publish,
   **Then** the receiver earns that publish's interest on the received units; **When** it is
   recorded after, **Then** the sender does.

---

### Edge Cases

- **No price published for a day (holiday, outage)**: no day is ever skipped and no day is ever
  covered twice. Each publish states how many days it covers, and its period always starts the
  day after the previous publish's period ends. A planned holiday is covered in advance (the
  publish before it covers the holiday too). After an outage, the late publish starts on the
  first uncovered day and covers through the gap, so the holders at that moment earn the missed
  days at that publish's rate.
- **Weekends**: Friday's publish covers 3 days (Friday, Saturday, Sunday). Holders at the moment
  of that publish earn all three days, as in a conventional money market fund where Friday's
  holders of record earn the weekend.
- **Period not yet begun**: a publish whose period would start on a future day (because the
  previous publish already covers today) is rejected. This is the staleness guard: figures can
  only be published for a period that has begun.
- **Zero rate**: accepted. The price record is stored and the counter does not move, so no holder
  earns anything for those days.
- **Negative rate**: rejected. A loss in the fund is shown through the price, not as negative
  interest; nothing already earned is ever taken back by a publish.
- **Rate or days covered entered wrongly (typo)**: not caught. Any rate of zero or more, and any
  period of one day or more, is accepted as stated. Holders are credited at the published figures,
  and nothing in this feature can reverse that (see Assumptions).
- **First publish after launch**: there is no previous price, so the price-move limit is measured
  from the fund's target price.
- **Price move exactly at the limit**: accepted. Only a move beyond the limit needs an override.
- **Zero price**: rejected, with or without an override.
- **Rounding over millions of holders**: no rounding happens when interest is credited. Each
  holder's earned interest is kept exactly, including fractions far smaller than the smallest
  unit. Rounding to whole units happens only when interest is paid (F006), and the remainder stays
  with the same holder for the next payment. So the sum of all holders' earned interest always
  equals what the fund owes in total, and no fraction is lost or invented.
- **Long-idle holder**: a holder whose balance has not changed for 90 publishes is brought up to
  date in one step when it next moves, at the same effort as a holder idle for one publish.
- **Suspended, revoked, KYC-expired or paused**: interest depends only on the units held. A
  holder that cannot currently send or receive still earns interest on its units; whether and how
  it is paid is decided by the distributions feature.
- **Fund-operated accounts** (fund treasury, escrow): earn interest on their units like any other
  holder. The settlement and distributions features decide what happens to that interest.
- **Transfers paused**: publishing is still allowed; a pause stops transfers, not prices.
- **Transfer of zero units, or to oneself**: no balance changes, so no interest is settled and no
  earned amount changes.
- **Interest counter would exceed its range**: the publish is rejected and nothing changes. With
  realistic rates this cannot happen within the fund's life.
- **Unused or unwanted override**: an override allows only the exact price it names, only for the
  next publish, and can be withdrawn before it is used. A publish at any other price is checked
  against the normal limit.

## Requirements *(mandatory)*

### Functional Requirements

**Publishing**

- **FR-001**: System MUST let only the Fund's fund admin publish a price and daily interest rate,
  and only while the Fund is Open.
- **FR-002**: System MUST store each publish as a permanent price record holding: sequence number
  (1 for the first publish, then increasing by exactly one), price per unit, daily rate, the days
  it covers, the interest counter value after the publish, the time, and the publisher.
- **FR-003**: System MUST NOT offer any way to change or remove a price record once stored, and
  MUST let anyone retrieve any record by its sequence number.
- **FR-004**: System MUST keep, on the Fund, the latest price, its time, its sequence number, the
  latest rate and the current interest counter, updated by each publish in the same step.
- **FR-005**: System MUST express the price to 8 decimal places of the cash asset ($1.00 is
  stated exactly) and the daily rate to 12 decimal places of a unit per unit held per day, so a
  typical yield (for example 4.5% a year, about 0.000123288 per day) is stored without being
  rounded to a coarser step.
- **FR-006**: System MUST reject a zero price and a negative rate.
- **FR-007**: System MUST reject a price that moves further from the previous published price (or,
  for the first publish, from the Fund's target price) than the Fund's largest allowed daily price
  move, unless an approved override names exactly that price. A move exactly at the limit is
  allowed.
- **FR-008**: System MUST let only compliance, acting through governance as it does for
  suspensions and pauses, approve an override for one exact price. The override MUST carry a
  reason code, apply only to the next publish, be used up by it, and be withdrawable by compliance
  before use. The fund admin MUST NOT be able to approve an override.
- **FR-009**: Each publish MUST state the number of days it covers, at least one. Its period
  starts on the day it is published for the first publish, and on the day after the previous
  publish's period ends for every later one. So every calendar day from the first publish onward
  is covered exactly once.
- **FR-009a**: System MUST reject a publish that covers zero days, or whose period would start on
  a day that has not yet begun.
- **FR-009b**: System MUST NOT cap the daily rate or the number of days covered; any rate of zero
  or more and any period of one day or more is accepted as stated.
- **FR-010**: System MUST advance the interest counter on each publish by the daily rate times
  the days it covers, and MUST NOT touch any holder's record when publishing.
- **FR-011**: The effort and cost of a publish MUST NOT depend on the number of holders.

**Accrual**

- **FR-012**: Each Holder record MUST store the interest counter value at its last settlement and
  the interest earned so far but not yet paid, kept exactly in fractions of a unit.
- **FR-013**: Before any change to a holder's balance takes effect, System MUST settle that
  holder's interest: earned += old balance × (current counter − counter at last settlement), then
  record the current counter. This applies to both sides of every transfer, and to every other
  way a balance can change that a later feature adds (unit creation, unit destruction, collateral
  enforcement), with no exception.
- **FR-014**: Settlement MUST be exact. No fraction of earned interest may be rounded away,
  dropped or created during accrual.
- **FR-015**: A holder's earned interest MUST depend only on the units in its holding account at
  each publish. Holder status, KYC expiry, pause and (from F004) collateral locks MUST NOT change
  it.
- **FR-016**: A holder whose balance is zero, or that has never held units, MUST earn nothing; when
  it first receives units, its counter starts at the current value, so it never earns for earlier
  publishes.
- **FR-017**: Anyone MUST be able to work out a holder's up-to-date earned interest at any moment
  from the stored figures alone (earned so far + balance × the counter's movement since its last
  settlement), without the system writing anything.
- **FR-018**: The effort of settling one holder MUST NOT depend on the number of holders or on how
  many publishes have happened since its last settlement.
- **FR-019**: Any arithmetic that would exceed its range MUST reject the whole operation, leaving
  the counter, every price record and every Holder record unchanged.

**Records**

- **FR-020**: System MUST leave a permanent record of every publish, every override approval,
  use and withdrawal, and every holder settlement that credits interest. Each record MUST show the
  Fund, the holder where relevant, the amounts or figures involved, the signer, the time and, for
  overrides, the reason.
- **FR-021**: An auditor MUST be able to rebuild every holder's earned interest from the price
  records and the holder's settlement and transfer records alone.

### Key Entities *(include if feature involves data)*

- **Fund** (from F001): now also holds the latest price, its time and sequence number, the latest
  daily rate and the global interest counter.
- **Holder record** (from F001): now also holds the counter value at its last settlement and the
  interest earned so far but not yet paid. Its last recorded balance (from F001) is the balance
  interest is settled on.
- **Price record**: one per publish, permanent. Sequence number, price per unit, daily rate, days
  covered, interest counter after the publish, time, publisher.
- **Global interest counter**: the total interest earned by one unit held continuously since the
  first publish. It only moves when a price is published, and never moves backward.
- **Price override**: a one-time approval, with a reason, allowing one exact price beyond the
  daily move limit at the next publish.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In randomized 90-day simulations (many holders, random transfers between publishes,
  a different rate each day including zero, weekends and missed days), every holder's earned
  interest equals an independent day-by-day calculation exactly, with zero difference at the
  smallest unit.
- **SC-002**: In the same simulations, after every operation, the sum of all holders' earned
  interest equals the sum, over every publish, of all units held at that publish × its rate × its
  days covered. No interest is lost or created by rounding.
- **SC-003**: A publish takes the same effort with 10,000 holders as with 10.
- **SC-004**: Settling a holder takes the same effort whether it was last settled 1 or 90
  publishes ago, and whether the fund has 10 or 10,000 holders.
- **SC-005**: 100% of price records can be retrieved by sequence number after any number of later
  publishes, unchanged from when they were stored.
- **SC-006**: Every rule in this specification has at least one test showing it rejecting a
  violation with the specific reason.
- **SC-007**: An auditor can rebuild any holder's earned interest from the records alone, and the
  result matches the stored figure for 100% of holders in the simulations of SC-001.

## Assumptions

- **Administrator's figures are accurate**: the fund admin is trusted to publish the true price
  and rate. The system guards only against a price typo (the price-move limit) and against
  publishing for a period not yet begun. The rate and the number of days covered are not checked
  (Clarification). A mistyped rate or period overpays or underpays every holder, the credit
  cannot be reversed in this feature, and an oversized period also blocks the next publish until
  that period has passed. Recovery is an operational matter for the issuer and is not handled by
  this feature.
- **Record-date rule**: interest for a publish goes to whoever holds the units at the moment it is
  published, as in a conventional money market fund's daily record date.
- **Interest is counted in units**: the daily rate is the fraction of a unit earned per unit held
  per day. It does not depend on the price. Converting interest to cash, if the Fund pays in cash,
  happens in the distributions feature at that feature's price.
- **No negative interest**: losses are reflected only through the price. A fund whose yield turns
  negative would need a later feature; this one rejects negative rates.
- **One publish per covered period**: there is no same-day republish to correct a figure. A later
  intraday price for collateral purposes, if needed, is a later feature's concern.
- **Largest daily price move**: already a Fund term set at creation (F001, FR-001); this feature
  only enforces it.
- **Rounding**: settled by the shared model's rounding rules: interest is credited exactly and
  rounded down only when paid, with the remainder kept for the same holder.
- **Override records**: until the governance feature adds dedicated action records, the permanent
  record of an override is the system's public activity log for the Fund, as for F001's admin
  actions.
- **No units are created in this feature**: subscriptions arrive in F003, so tests place units
  during setup and move them with F001 transfers. Accrual for unit creation and destruction is
  required here (FR-013) and exercised once those features exist.
- **Scale**: one Fund, up to about 10,000 holders, one publish per business day.
- **Out of scope**: paying out interest (F006); computing the rate from the fund's portfolio;
  intraday prices; negative yields; changing the largest daily price move after launch.
