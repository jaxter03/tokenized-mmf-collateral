# Specification Quality Checklist: Collateral Agreements, Margin Calls and Pledges

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-10-14
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification

## Notes

- Iteration 1 failed "Requirements are testable and unambiguous" on three points, fixed in
  iteration 2: US2 scenario 3 did not say whether a pending (giver-confirmed) raise is rejected on a
  stale price; FR-013 said a lower exposure could open a call, contradicting FR-011; FR-024 named
  "program". All items pass after the fix.
- No [NEEDS CLARIFICATION] markers: the three F004 input questions (`docs/speckit-inputs.md` §3)
  are answered from the input and `docs/shared-model.md` and listed in Assumptions for
  `/speckit-clarify` to confirm.
- Decisions the spec adds beyond the input, for review: open delivery calls are Met together and
  the agreement tracks the demanded part of the shortfall (changed from oldest-first during
  planning, research R5); a permissionless re-check; substitution only to an equal or larger
  pledge; compliance can resolve disputes; pledging stays allowed on a stale price and while
  transfers are paused; "more than" the minimum transfer amount read literally (design.md §6.5.4
  says "at least" for delivery).
- Planning (2026-10-14) updated the spec as listed in plan.md "Spec adjustments made during
  planning"; all checklist items still pass. The shared-model gaps noted at specify time are now
  closed in `docs/shared-model.md` on this branch.
- Items marked incomplete require spec updates before `/speckit-clarify` or `/speckit-plan`
