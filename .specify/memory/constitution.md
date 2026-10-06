<!--
Sync Impact Report
==================
Version change: unversioned template → 1.0.0 (initial ratification)

Principles (template had 5 placeholder slots; project defines 13):
  [PRINCIPLE_1..5] → I. Correctness Over Features
                     II. Rules Live On-Chain
                     III. Every State Change Has a Named Signer
                     IV. One Instruction, One State Transition
                     V. On-Chain State Is the Source of Truth
                     VI. Every Rule Has a Failing Test First
                     VII. Fail Closed
                     VIII. No Unpaid Units, No Silent Powers
                     IX. Money Math Is Exact
                     X. Stated Assumptions
                     XI. Features Ship in Order
                     XII. Earlier Features Stay Green
                     XIII. One Shared Model

Added sections:
  - [SECTION_2] → Scope and Trust Boundaries
  - [SECTION_3] → Development Workflow and Quality Gates
  - Governance (filled)

Removed sections: none

Reconciled with docs/shared-model.md and docs/design.md (review these):
  - III: permissionless `settle` / `expire` (shared-model §5.1) are allowed only when listed
    there and the caller adds no discretion.
  - IV: bookkeeping inside a transition (accrual settlement, event, audit record) is part of
    that transition, not a second one.
  - V: stored running totals (`pledged_amount`, accrual checkpoints) are allowed only when
    updated in the same instruction and invariant-tested (shared-model I1).
  - VIII: reinvested distributions (`distribution_mode = Reinvest`) are the only mint path
    besides subscription settlement.
  - VIII: freeze / suspend / pause / hold act immediately; collateral enforcement is not
    time-locked (design.md §6.6); records are events until F007 lands (shared-model I8).

Dependent templates: not modified (plan, spec and tasks templates read this file at runtime).
Deferred TODOs: none.
-->

# Tokenized MMF Collateral Platform Constitution

## Core Principles

### I. Correctness Over Features

This code moves regulated assets. A smaller feature set that is provably right beats a larger one
that is mostly right.

- Where correctness and scope conflict, scope MUST give way: a requirement is dropped or deferred
  rather than shipped partly right.
- A feature MUST NOT merge with a known correctness defect, an untested rule, or an untested
  invariant that it introduces.
- A correctness defect found in merged code MUST be fixed before the next feature starts.

Rationale: a mostly-right system can mint unpaid units, overvalue collateral or move a holder's
units wrongly. In a regulated fund each of those is a legal and financial loss.

### II. Rules Live On-Chain

- Who may hold the token, which units are locked, who earns yield, and when enforcement is
  allowed MUST be enforced by the on-chain programs and by the token's own configuration
  (transfer hook, default-frozen accounts, mint and freeze authorities held by program PDAs).
- A UI, API or backend MAY repeat a check for early feedback, but MUST NOT be the only place a
  rule is enforced.
- Every such rule MUST have a test that submits the instruction or transfer directly, with no UI
  or API in the path, and shows that the programs reject it.

Rationale: anyone can bypass an off-chain check by building a transaction by hand. A rule that
the token itself enforces applies to every wallet, venue and client that moves the units.

### III. Every State Change Has a Named Signer

- Every state-changing instruction MUST require a signer whose authority is listed in
  `docs/shared-model.md` §5.1, or a program PDA on a cross-program edge allowed by §5.2.
- No hidden backend keys: any key an off-chain service uses (e.g. the keeper) MUST be a named role
  in the role registry, able to call only that role's instructions.
- No off-chain service may hold, generate or custody an institution's signing key. Services build
  unsigned transactions; the institution or its custodian signs them.
- A permissionless instruction (e.g. `settle`, `expire`) is allowed only when §5.1 lists it as
  such and the caller adds no discretion. Its outcome MUST be fully determined by on-chain state
  that named signers already authorized and by on-chain time.

Rationale: every change to a holder's position must trace back to a party who signed for it. A
backend key that can move assets is a single point of theft and an actor nobody answers for.

### IV. One Instruction, One State Transition

- Each instruction MUST perform exactly one business state transition. If its description needs
  "and then", it MUST be split into two instructions.
- Exception: steps where atomicity is the point (e.g. both legs of a settlement) MAY share one
  instruction. The spec or plan MUST name each such case and explain why splitting it would be
  unsafe.
- Bookkeeping that keeps the transition correct, such as settling accrued yield before a balance
  change, emitting the event or writing the audit record, is part of that transition, not a second
  one.

Rationale: small transitions are easier to state, test and audit, and each has one signer and one
reason. A bundled transition hides intermediate states that nobody reviewed.

### V. On-Chain State Is the Source of Truth

- Program accounts and token balances are the only authoritative record. Indexers, databases,
  APIs and UIs mirror them and MUST NOT decide anything that changes assets or eligibility.
- Values from outside the chain (price, yield rate, exposure) MUST enter only through an
  instruction signed by the responsible role.
- A value that can be derived from other on-chain state (e.g. collateral coverage, current accrued
  yield) MUST be computed when it is read or used, not stored as a separate copy.
- A running total or checkpoint that cannot be recomputed within transaction limits (e.g.
  `HolderState.pledged_amount`, accrual checkpoints) MAY be stored only if it is updated in the
  same instruction as its inputs and an invariant test proves it matches what it summarizes
  (`docs/shared-model.md` §7).

Rationale: two sources of truth eventually disagree, and in a dispute only the chain can be
checked by every party. A stored copy of a computable value can drift; a computed value cannot.

### VI. Every Rule Has a Failing Test First

- Every requirement and rule MUST have a test that is written, and seen to fail, before the code
  that satisfies it.
- Each requirement MUST have at least one test of its rejection path that asserts the specific
  error, in addition to any happy-path test.
- Every invariant in `docs/shared-model.md` §7 MUST be tested in every feature from the one that
  introduces it.

Rationale: a rule with only a happy-path test is unproven, because the test still passes when the
check is missing. Asserting the exact error proves that the intended check fired.

### VII. Fail Closed

- Any failed check MUST abort the whole transaction. No instruction may catch a failed check and
  continue, skip it or apply part of its effect.
- Missing, expired, stale or malformed inputs (e.g. absent holder record, expired KYC, stale
  price, unknown signer) MUST count as failed checks, never as a default allow.
- A process that spans several transactions (e.g. a settlement) MUST end with every leg completed
  or every party back at its starting balance (`docs/shared-model.md` I6).

Rationale: a partial outcome in a money system is a loss that someone has to find and repair by
hand. When unknowns count as failures, a bug blocks a transfer instead of letting a bad one
through.

### VIII. No Unpaid Units, No Silent Powers

- Units MUST be minted only (a) by subscription settlement, in the same transaction that locks the
  subscriber's cash in escrow (`docs/shared-model.md` I5), or (b) as reinvestment of a closed
  distribution cycle, limited to each holder's earned amount and backed by its receipt. No other
  path may mint.
- Every discretionary admin power (freeze, suspend, pause, hold, seize, forced transfer, rule
  change, price override) MUST be role-gated and reason-coded, and MUST leave a permanent on-chain
  record: an Action record once F007 lands, and an event before then (`docs/shared-model.md` I8).
- Destructive powers MUST be time-locked, with the pending proposal visible and cancellable before
  it executes. A power is destructive if it moves or removes units without the holder's signature
  or changes holding or transfer rules: seize, forced transfer, rule change, program upgrade and
  transfer-hook change.
- Protective, reversible powers (freeze, suspend, pause, hold) MAY take effect immediately.
- Enforcement under a signed collateral agreement is not a discretionary power. It is bounded by
  the agreement's terms and cure period, a compliance hold can block it, and it is recorded like
  any admin action.

Rationale: an unpaid unit dilutes every other holder. A power that leaves no record cannot be
audited, and a destructive power with no delay gives holders no chance to contest it.

### IX. Money Math Is Exact

- Amounts, prices, rates and indices MUST use integer or fixed-point integer types. Floating point
  MUST NOT be used on-chain or in any off-chain code that computes an amount placed in a
  transaction.
- Every formula MUST state its rounding direction, or state that it is exact. The direction MUST
  favour the protected party, the one who would lose if it rounded the other way. The rules are in
  `docs/shared-model.md` §8, and a new formula adds a row there.
- All arithmetic MUST be overflow-checked, with intermediate products widened as needed (e.g.
  `u128`). An overflow MUST fail the transaction (Principle VII).

Rationale: a rounding error repeats across thousands of holders, every day. When the direction is
stated up front, the remainder lands on the side that can absorb it, and totals can be checked.

### X. Stated Assumptions

- Anything the system relies on but does not enforce (legal agreement terms, KYC provider honesty,
  price accuracy, custodian behaviour) MUST be written down. It goes in the Assumptions section of
  each spec that relies on it and, if it spans features, in the trust boundaries of
  `docs/design.md` §5.2.
- Each stated assumption MUST name who is trusted and what the system does, or cannot do, if the
  assumption fails.

Rationale: an unwritten assumption is a hidden dependency. Writing it down shows auditors, issuers
and institutions where the code's guarantees end and legal or operational trust begins.

### XI. Features Ship in Order

- Features MUST be built in the order of the feature map in `docs/speckit-inputs.md` (F001 to
  F008). Each feature MUST be complete, tested and merged to `main` before the next feature's spec
  starts.
- Ideas outside the current feature MUST go to the backlog (`docs/speckit-inputs.md` §5), not into
  the code. Fields reserved for backlog work MUST stay zero.

Rationale: each feature builds on the merged code of the features before it. A spec written
against code that does not exist yet drifts, and half-built features hide untested paths.

### XII. Earlier Features Stay Green

- A feature MUST NOT merge unless the full test suite passes: its own tests and those of every
  earlier feature.
- An earlier feature's test MUST NOT be deleted, skipped or weakened to get a merge through. If
  the expected behaviour has really changed, the change MUST be made first in the spec or in
  `docs/shared-model.md`, and the pull request MUST say which tests changed and why.

Rationale: later features extend the same accounts and the same transfer hook. A regression in an
earlier rule is a regression in a live control.

### XIII. One Shared Model

- Every design MUST conform to `docs/shared-model.md`: account layouts, seeds, authorities,
  allowed cross-program calls, hook check order, invariants and rounding.
- A change to the shared model MUST be made explicitly in that file, in the same pull request and
  before the code that depends on it. The plan MUST list the change.
- A feature MUST NOT create a second account, entity name or cross-program edge for a concept the
  shared model already defines.

Rationale: five programs share accounts and call each other. A single written contract keeps
every feature's design compatible and gives reviewers one place to check.

## Scope and Trust Boundaries

The project is open-source, issuer-side infrastructure for a tokenized money market fund used as
collateral on Solana. The fund issuer controls the token. The system covers the full lifecycle:
launch, onboarding, subscription and redemption, daily price and yield, distributions, collateral
pledges, margin calls, enforcement, governance and audit.

- **In scope**: features F001 to F008 in `docs/speckit-inputs.md`.
- **Out of scope**: the non-goals in `docs/design.md` §2 and the backlog in
  `docs/speckit-inputs.md` §5 (Principle XI).
- **Trust boundaries** (`docs/design.md` §5.2):
  - Runtime-enforced (Principle II): eligibility, liens, yield accrual, atomic settlement, and
    enforcement within agreement terms.
  - Trusted with accountability (Principles III, X): the fund admin's price and yield, the
    compliance multisig and KYC statements. Each acts through a named, signed, recorded
    instruction.
  - Untrusted (Principle V): indexer, API and UI. They hold no institutional keys, decide
    nothing, and can be checked against chain state.
- **Platform limits**: every user transaction, including the transfer-hook call, MUST fit within
  Solana compute and account limits. The cost of the daily price publish MUST NOT grow with the
  number of holders.

## Development Workflow and Quality Gates

Each feature follows `docs/workflow.md`: specify, clarify, plan, tasks, analyze, implement, full
test run, merge. Each gate below MUST pass before the next step starts.

- **Spec**: every user story has a priority, an independent test and Given/When/Then scenarios;
  the spec names no technologies; its scope matches the feature's input.
- **Plan**: the Constitution Check passes; `data-model.md` matches `docs/shared-model.md`, and any
  change is listed in the plan and made in that file (Principle XIII).
- **Tasks**: every functional requirement has tasks; every rule has a rejection-test task ordered
  before its implementation task (Principle VI).
- **Review**: a human reviewer has checked money math and rounding (shared-model §8), hook check
  order (§6), signer checks (§5.1), cross-program calls (§5.2) and invariants (§7).
- **Merge**: all tests of this feature and of every earlier feature pass (Principle XII).

## Governance

- This constitution overrides every other practice in the repository. Where documents disagree,
  this constitution wins, then `docs/shared-model.md`, then `docs/design.md`.
- **Compliance review**: every `/speckit-plan` runs the Constitution Check before research and
  again after design, and every pull request review confirms compliance. Complexity Tracking in
  `plan.md` may justify added complexity, but never a breach of a principle. A plan that cannot
  meet a principle MUST change its design, or this constitution MUST be amended first.
- **Amendments**: an amendment is proposed in a pull request that edits
  `.specify/memory/constitution.md` (via `/speckit-constitution`). The pull request states the
  version bump and the reason for it, and the project maintainers approve it. The Sync Impact
  Report is reviewed and removed before commit.
- An amendment that makes merged code non-compliant MUST list the affected features. Those
  features MUST be brought into compliance before the next feature starts (Principle I).
- **Versioning**: semantic. MAJOR: a principle is removed or redefined so that existing compliance
  breaks. MINOR: a principle or section is added, or guidance is materially expanded. PATCH:
  wording, clarification or typo fixes.
- Runtime development guidance lives in `docs/workflow.md`.

**Version**: 1.0.0 | **Ratified**: 2026-10-05 | **Last Amended**: 2026-10-05
