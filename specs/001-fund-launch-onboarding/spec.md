# Feature Specification: Fund Launch, Roles and Holder Onboarding

**Feature Branch**: `001-fund-launch-onboarding`

**Created**: 2026-10-05

**Status**: Draft

**Input**: User description: "Launch a tokenized money market fund and control who may hold it."
(full input: `docs/speckit-inputs.md`, F001)

## Clarifications

### Session 2026-10-12

- Q: When an investor institution wants to hold the fund, who may create its Pending holder record?
  → A: The institution may register itself, only as an investor; compliance may register any class
  (investor, fund treasury, escrow).
- Q: Should the system identify an institution by the account its custodian controls, or by its
  legal identity? → A: By account. One Holder record per owner account; the KYC statement names
  that owner; an institution using several owner accounts has several records.
- Q: When the fund sets a maximum number of holders, which investors count toward it? → A:
  Investors that currently hold units, checked when a transfer would give units to an investor
  with none; a place frees when a balance reaches zero; fund-operated accounts are not counted.
- Q: May the same account or group hold more than one operating role? → A: The keeper must differ
  from every other role, and the issuer must differ from compliance; other roles may share an
  account or group.
- Q: Which jurisdictions must the system be able to represent? → A: Any country in the
  international standard country list (two-letter codes); each Fund allows any subset of them.

## User Scenarios & Testing *(mandatory)*

A regulated fund may only be held by approved, KYC-checked institutions. Today a transfer agent's
database enforces that. This feature makes the fund's token itself refuse any holder who is not
approved, so the rule cannot be bypassed, while the issuer and the compliance team keep full
control.

### User Story 1 - Launch a fund (Priority: P1)

The issuer creates a fund with its terms, sets the rules for who may hold it, assigns the
operating roles, and opens it so that institutions can start onboarding.

**Why this priority**: nothing else works without a fund.

**Independent Test**: create a fund, complete its setup, launch it, then read back its terms,
holder rules, roles and status.

**Acceptance Scenarios**:

1. **Given** no fund, **When** the issuer creates one with valid terms, **Then** the fund exists in
   Setup status with exactly those terms, and nobody can hold units.
2. **Given** a fund in Setup with its holder rules and roles in place, **When** the issuer launches
   it, **Then** it is Open.
3. **Given** a fund, **When** anyone other than the issuer tries to launch it, **Then** the launch
   is rejected and the status is unchanged.
4. **Given** a fund in Setup whose holder rules or roles are not yet in place, **When** the issuer
   tries to launch it, **Then** the launch is rejected.
5. **Given** terms with a zero target price or a daily redemption limit above 100%, **When** the
   issuer tries to create the fund, **Then** creation is rejected and no fund exists.
6. **Given** an Open fund, **When** the issuer tries to launch it again, **Then** it is rejected.

---

### User Story 2 - Approve holders (Priority: P1)

An institution, acting through its custodian, applies to hold the fund. It becomes able to hold
units only after a recognised KYC provider has signed a statement for it and compliance has
approved it.

**Why this priority**: this is the core regulatory promise of the product.

**Independent Test**: register an institution, approve it with a valid statement, then send it
units successfully; then show that every invalid statement and every unapproved receiver is
rejected.

**Acceptance Scenarios**:

1. **Given** a newly registered institution, **Then** its holder record is Pending and its holding
   accounts can neither send nor receive units.
2. **Given** a valid statement signed by a recognised provider, **When** compliance approves the
   institution, **Then** it is Approved and can receive units.
3. **Given** a statement from a provider that is not recognised, **When** compliance tries to
   approve, **Then** approval is rejected and the holder stays Pending.
4. **Given** a statement whose expiry is in the past, **When** compliance tries to approve, **Then**
   approval is rejected.
5. **Given** a statement that names a different owner account, **When** compliance tries to
   approve with it, **Then** approval is rejected.
6. **Given** a statement naming a jurisdiction that the fund does not allow, **When** compliance
   tries to approve, **Then** approval is rejected.
7. **Given** a receiver that is not Approved, **When** anyone sends units to it, **Then** the
   transfer is rejected and no balance changes.
8. **Given** an owner account that already has a holder record, **When** it is registered again,
   **Then** registration is rejected and the existing record is unchanged.
9. **Given** an institution, **When** it tries to register itself as a fund treasury or escrow
   account, or to register a different owner, **Then** registration is rejected and no record is
   created.

---

### User Story 3 - Suspend, revoke and pause (Priority: P2)

Compliance can stop an individual holder (temporarily or permanently) and, in an emergency, stop
all transfers.

**Why this priority**: the fund must be able to react immediately to sanctions hits, court orders
and operational incidents, but onboarding (US2) must exist first.

**Independent Test**: suspend a holder and show it can no longer send or receive; reinstate it and
show it can again; pause and show no transfer succeeds; unpause and show transfers work again.

**Acceptance Scenarios**:

1. **Given** a suspended holder, **Then** it can neither send nor receive units.
2. **Given** a suspended holder, **When** compliance reinstates it, **Then** it can send and receive
   again, as long as its KYC has not expired.
3. **Given** a revoked holder, **When** compliance tries to approve, reinstate or re-register it,
   **Then** each attempt is rejected.
4. **Given** transfers are paused, **Then** every transfer is rejected, while compliance can still
   register, approve, suspend and revoke holders.
5. **Given** transfers are paused, **When** compliance unpauses, **Then** valid transfers succeed
   again.
6. **Given** a holder whose KYC has expired, **Then** it is treated as not approved: it can neither
   send nor receive.
7. **Given** a holder whose KYC has expired, **When** compliance renews it with a new valid
   statement, **Then** it can send and receive again.
8. **Given** any suspension, reinstatement, revocation, pause or unpause, **Then** it carries a
   reason from the fixed list and leaves a permanent record.

---

### User Story 4 - Roles (Priority: P2)

The issuer assigns each operating role (fund admin, transfer agent, compliance, keeper, auditor)
to a specific account or multi-person group, so every action has an accountable owner.

**Why this priority**: role assignment is part of setup (US1). This story adds the guarantee that
each action is accepted only from the holder of the role it belongs to.

**Independent Test**: assign every role to a different party, then attempt each role-restricted
action from every party and show that only the right one succeeds.

**Acceptance Scenarios**:

1. **Given** a role assignment, **When** the holder of that role performs one of the role's
   actions, **Then** it is accepted.
2. **Given** a role assignment, **When** any other party (including the issuer) attempts that
   role's action, **Then** it is rejected.
3. **Given** compliance is a multi-person group, **When** fewer members than the group's own rules
   require have approved an action, **Then** the action is not performed.
4. **Given** an Open fund, **When** anyone tries to change a role assignment, **Then** it is
   rejected in this feature (role changes after launch arrive with governance).
5. **Given** a Fund in Setup, **When** the issuer tries to give the keeper role to an account or
   group that holds any other role (including the issuer), or to give the compliance role to the
   issuer, **Then** the assignment is rejected and the existing roles are unchanged.
6. **Given** a Fund in Setup, **When** the issuer gives the fund admin and transfer agent roles to
   the same account or group, **Then** the assignment is accepted.

---

### Edge Cases

- **Holder cap reached**: a transfer to an investor that holds no units is rejected once the
  number of investors holding units equals the cap. Investors that already hold units can still
  receive more. A place frees up when an investor's balance falls to zero. Fund treasury and
  escrow accounts never count. With no cap set, there is no limit.
- **KYC expires while the holder has units**: the units stay where they are. The holder can
  neither send nor receive until compliance renews it with a new valid statement. KYC counts as
  expired from the exact moment its expiry is reached.
- **Institution registered twice**: a second registration for the same owner account is rejected
  and the first record is unchanged. One holder record covers every holding account that owner
  account controls. An institution that uses a second owner account (for example, through a second
  custodian) is a separate holder: it needs its own registration and KYC statement, and counts
  separately toward the holder cap.
- **Same sender and receiver**: the transfer goes through the same checks as any other transfer
  (sender and receiver must both be approved, transfers must not be paused). It does not change
  the holder count.
- **Transfer of zero units**: the same checks apply, so a zero-unit transfer to an unapproved
  receiver is rejected.
- **Several checks fail at once**: the transfer is rejected with the first failure in a fixed,
  documented order, so the reported reason is predictable.
- **Revoked holder that still has units**: the units stay where they are and cannot be moved by
  anything in this feature. Recovering them is a governance action in a later feature. While it
  holds units it still counts toward the holder cap; the same applies to a suspended holder or one
  whose KYC has expired.
- **Unknown jurisdiction code**: a KYC statement whose jurisdiction is not a code in the standard
  country list is rejected like any other invalid statement, and a Register rule naming such a
  code is rejected.
- **Fund-operated accounts** (fund treasury, escrow): only compliance can register them, and only
  for accounts controlled by the system itself. They need no KYC statement, but pause and
  suspension apply to them like any other holder.

## Requirements *(mandatory)*

### Functional Requirements

**Fund creation and launch**

- **FR-001**: System MUST let an issuer create a Fund with these terms: name, symbol, fund
  identifier, target price per unit, cash asset used for settlement, largest allowed daily price
  move, daily redemption limit (as a share of units outstanding), distribution style (reinvest as
  units, or pay in cash) and frequency, and the link to and fingerprint of the legal documents.
- **FR-002**: System MUST record the creator of a Fund as its issuer and start the Fund in Setup
  status.
- **FR-003**: System MUST reject a Fund whose terms are missing or out of range: zero target price,
  zero largest daily price move, daily redemption limit of zero or above 100%, zero distribution
  frequency, or a missing document link or fingerprint.
- **FR-004**: System MUST create the Fund's token together with the Fund, so there is exactly one
  token per Fund. From creation onward, only the system's rules may create units of the token or
  freeze and unfreeze holding accounts; no individual's key may.
- **FR-005**: System MUST allow only the issuer to launch a Fund, moving it from Setup to Open.
- **FR-006**: System MUST reject a launch unless the Fund is in Setup, every role is assigned, and
  the Register names at least one recognised KYC provider and at least one allowed jurisdiction.
- **FR-007**: System MUST let anyone read a Fund's terms, status, roles and Register rules.

**Register (holder rules)**

- **FR-008**: System MUST let the issuer, while the Fund is in Setup, set the Register's rules:
  up to four recognised KYC providers, the allowed jurisdictions, and an optional holder cap. A
  jurisdiction is a country identified by its two-letter code in the international standard
  country list (ISO 3166-1); the allowed jurisdictions may be any subset of that list.
- **FR-009**: System MUST reject any change to Fund terms or Register rules once the Fund is Open.

**Roles**

- **FR-010**: System MUST let the issuer, while the Fund is in Setup, assign each role (fund admin,
  transfer agent, compliance, keeper, auditor) to an account or multi-person group. System MUST
  reject an assignment that would make the keeper the same account or group as any other role
  (including the issuer), or make compliance the same account or group as the issuer. Any other
  role may share an account or group with another role.
- **FR-011**: System MUST accept each role-restricted action only when the current holder of that
  role signs it, and MUST reject it from any other party, including the issuer.
- **FR-012**: System MUST treat a multi-person group holding a role as one role holder. The action
  is performed only when the group's own approval rules are met.

**Onboarding**

- **FR-013**: System MUST, while the Fund is Open, let an institution register itself as an
  investor, and let compliance register an institution as an investor or register a fund-operated
  account (fund treasury or escrow). An institution MUST NOT register an owner other than itself or
  any class other than investor; such a registration MUST be rejected. Every new Holder record MUST
  start as Pending.
- **FR-014**: System MUST reject a registration for an owner that already has a Holder record, and
  MUST leave the existing record unchanged.
- **FR-015**: System MUST ensure that one Holder record governs every holding account its owner has
  for the Fund, and that a newly opened holding account cannot send or receive units until its
  owner is Approved.
- **FR-016**: System MUST let compliance approve a Pending investor only with a KYC statement that
  (a) is signed by a provider recognised in the Register, (b) names that Holder record's owner
  account, (c) has an expiry in the future, and (d) gives a valid country code (FR-008) that the
  Fund allows. Any other statement MUST be rejected and the Holder record left unchanged.
- **FR-017**: System MUST store on the Holder record, on approval: jurisdiction, investor type, KYC
  expiry, the statement's fingerprint, and which provider signed it.
- **FR-018**: System MUST let compliance renew an Approved investor with a newer statement, under
  the same checks as FR-016.
- **FR-019**: System MUST let compliance approve a fund-operated account without a KYC statement,
  and only when the account is controlled by the system itself rather than by any person or
  institution.

**Transfer rules**

- **FR-020**: System MUST apply the rules in FR-021 to FR-025 to every transfer of units, however
  and by whomever it is built or submitted. If any check fails, the transfer MUST be rejected and
  no balance or record changes.
- **FR-021**: System MUST reject every transfer while transfers are paused.
- **FR-022**: System MUST reject a transfer whose sender is not Approved or whose sender's KYC has
  expired.
- **FR-023**: System MUST reject a transfer whose receiver is not Approved, whose receiver's KYC has
  expired, or whose receiver's jurisdiction is not allowed.
- **FR-024**: System MUST reject a transfer to an investor that holds no units when a holder cap is
  set and the number of investors holding units has reached it. The count includes only investors
  (not fund treasury or escrow accounts), rises when an investor's balance goes from zero to above
  zero, and falls when it returns to zero, in the same transfer that causes the change.
- **FR-025**: System MUST treat KYC as expired from the moment its expiry is reached.
- **FR-026**: System MUST report a rejected transfer's first failed check in a fixed, documented
  order.

**Suspend, revoke and pause**

- **FR-027**: System MUST let compliance suspend an Approved holder. A suspended holder can neither
  send nor receive units.
- **FR-028**: System MUST let compliance reinstate a suspended holder to Approved. KYC expiry still
  applies after reinstatement.
- **FR-029**: System MUST let compliance revoke a holder in any status other than Revoked.
  Revocation MUST be final: a revoked holder cannot be approved, renewed, reinstated or registered
  again.
- **FR-030**: System MUST let compliance pause and unpause all transfers. Pausing MUST NOT block
  registration, approval, renewal, suspension, reinstatement or revocation.
- **FR-031**: System MUST require every suspension, reinstatement, revocation, pause and unpause to
  carry a reason from a fixed list (court order, sanctions hit, lost key, operational error,
  other).
- **FR-032**: System MUST make every compliance action take effect for every transaction after the
  one that performed it, with no delay.

**Records**

- **FR-033**: System MUST leave a permanent record of every Fund creation, launch, role assignment,
  Register setup, Holder registration, Holder status change (approval, renewal, suspension,
  reinstatement, revocation), pause and unpause. Each record MUST show what changed, for which Fund
  and Holder, who signed, when, and the reason where one is required.
- **FR-034**: System MUST let anyone auditing the Fund retrieve these records, so that a Holder's
  full status history can be rebuilt from the records alone.

**Units**

- **FR-035**: System MUST NOT offer any way to create or destroy units in this feature. Units are
  created only by subscription settlement in a later feature.

### Key Entities *(include if feature involves data)*

- **Fund**: the tokenized money market fund. Holds its terms (FR-001), status (Setup, Open, Gated,
  Suspended, Winding down, Closed), its issuer and its token. This feature uses only Setup and
  Open; later features enter the other statuses.
- **Register**: the Fund's holder rules. Holds recognised KYC providers (up to four), allowed
  jurisdictions (any subset of all countries in the standard list), optional holder cap, current
  count of investors holding units (fund-operated accounts excluded), and the pause flag.
- **Holder record**: one per owner account (the account that controls the holding accounts),
  whether an institution's or fund-operated. Holds owner, status (Pending, Approved, Suspended,
  Revoked), class (investor, fund treasury, escrow), jurisdiction (a country code), investor type,
  KYC expiry, fingerprint of the KYC statement, and which provider signed it.
- **Role assignment**: the account or group that holds each role (issuer, fund admin, transfer
  agent, compliance, keeper, auditor) for a Fund.
- **KYC statement**: a provider-signed statement that names an owner account, the institution's
  jurisdiction (a country code) and investor type, and an expiry. The system stores only its
  fingerprint and signer, not its contents.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Across the whole test suite, including randomized sequences of registrations, status
  changes and transfers submitted directly (not through the project's own software), zero
  transfers deliver units to an owner that was not Approved with unexpired KYC at that moment.
- **SC-002**: 100% of transfers submitted after a suspension, revocation or pause are rejected, and
  100% of valid transfers submitted after an approval, reinstatement, renewal or unpause succeed,
  with no waiting period.
- **SC-003**: 100% of holder status changes, pauses and unpauses leave a permanent record that
  identifies the action, signer, time and (where required) reason. For any holder, the full status
  history can be rebuilt from these records alone.
- **SC-004**: Every rule in this specification has at least one test showing it rejecting a
  violation with the specific reason.
- **SC-005**: An institution with a valid KYC statement becomes able to receive units after
  exactly one registration and one compliance approval, with no other manual step.
- **SC-006**: All rules behave the same with 10,000 registered holders as with 10, and the effort
  to check a single transfer does not grow with the number of holders.
- **SC-007**: After every operation in the test suite, including the randomized sequences of
  SC-001, the Register's count of investors holding units equals the actual number of investors
  with a balance above zero.

## Assumptions

- **KYC providers are trusted**: providers sign honestly. The system checks only the signature, the
  owner account named, the expiry and the jurisdiction. Real provider integration is mocked. If a
  provider signs falsely, the system admits that institution until compliance suspends or revokes
  it.
- **Issuer controls the token**: the issuer creates and controls the token. This is not a layer on
  top of someone else's fund token.
- **Institution identity is the owner account**: the system identifies a holder only by its owner
  account. The KYC provider is trusted to confirm that the owner account belongs to the institution
  it vetted. The system cannot tell that two owner accounts belong to the same institution; such
  an institution has two Holder records and counts twice toward the holder cap, which over-counts
  and so never lets the cap be exceeded.
- **Institutions act through custodians**: an institution's custodian signs on its behalf, and the
  system never holds an institution's signing key.
- **Open questions from the input**: all three are settled. Self-registration and the holder cap
  were confirmed in Clarifications (FR-013, FR-024; the cap is optional, with no cap by default).
  Suspension blocks both sending and receiving, as the input states (FR-027).
- **Fund-operated accounts**: fund treasury and escrow accounts are approved by compliance without
  a KYC statement, and only when the system itself controls them. This closes the path by which a
  KYC-free class could be granted to an outside party.
- **No units yet**: subscriptions arrive in a later feature, so tests that need units place them
  during test setup. No operation of the system in this feature creates units.
- **Setup-only configuration**: terms, Register rules and role assignments are set while the Fund is
  in Setup and are fixed once it is Open. Changing them after launch is a time-locked governance
  action in a later feature.
- **Records**: until the governance feature adds a dedicated audit record, the permanent record is
  the system's public activity log for the Fund.
- **Scale**: one Fund, up to about 10,000 holders, up to four recognised KYC providers.
- **Jurisdiction is a country**: regional rules (for example, "EU only") are expressed by allowing
  each country in the region. The country list is fixed when the system is built; a country added
  to the standard list later needs a system upgrade before a Fund can allow it. The current
  `docs/shared-model.md` allows only 64 jurisdiction codes, so the plan must change it to cover the
  full list (Constitution Principle XIII).
- **Separation of duties**: the system enforces only two separations, keeper from every other role
  and issuer from compliance, by comparing the assigned accounts or groups. It cannot tell that two
  different accounts or groups are controlled by the same people. Any further separation (for
  example, fund admin from transfer agent) is the issuer's responsibility.
- **Transfer agent role**: the input lists fund admin, compliance, keeper and auditor. The transfer
  agent role is added so that the role set is complete from launch; the distributions feature uses
  it.
- **Out of scope**: real KYC provider APIs; investor-facing onboarding screens; creating or
  redeeming units (subscriptions and redemptions); prices and yield; changing terms, rules or roles
  after launch; seizure, forced transfers and collateral enforcement.
