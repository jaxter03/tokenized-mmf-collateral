# Implementation Plan: Fund Launch, Roles and Holder Onboarding

**Branch**: `001-fund-launch-onboarding` | **Date**: 2026-10-12 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/001-fund-launch-onboarding/spec.md`, the shared plan
input (`docs/speckit-inputs.md` §4.1), the F001 plan note (§4.2) and the shared-model changes from
the clarify session.

## Summary

F001 creates the fund's Token-2022 mint with every extension it will ever need, the three programs
that control it (`fund`, `compliance_hook`, `governance`), and the onboarding flow that decides who
may hold units. The issuer creates and launches the fund and assigns roles. Institutions register
(themselves or through compliance), and compliance approves them with an ed25519-signed KYC
statement. The transfer hook then rejects every transfer whose sender or receiver isn't an approved
holder, while paused, or past the holder cap. Compliance can suspend, reinstate, revoke, pause and
unpause, each with a reason code and a permanent event.

Key design choices (details in [research.md](research.md)): an owner holds units in exactly one
account, its ATA (R9); Token-2022's permissioned burn stops holders destroying their own units
(R3); the issuer is bound to the fund through a role list signed by the mint keypair (R10); the
hook keeps a holder count in the `Registry`, so the Registry is writable on every transfer (R5);
jurisdictions are ISO 3166-1 codes in a 676-bit set (R11); suspension changes status only (R13).

## Technical Context

**Language/Version**: Rust 1.90 (pinned) with Anchor 1.1.2 (`anchor-lang`, `anchor-spl` =1.1.2,
matching the installed CLI). Fallback "configuration A" (Rust ≥ 1.97.1, Anchor 1.2.1 `solana-v4`) only
if the task-1 spike fails (R1, R8). No TypeScript in F001; the client/UI layer is F008.

**Primary Dependencies**: `anchor-spl` 1.1.2 (Token-2022 interface 2.1.0 and extension CPIs);
`spl-transfer-hook-interface` 2.1.0 and `spl-tlv-account-resolution` 0.11.4; `solana-ed25519-program`
3.0.0; Token-2022 program v11 on-chain (permissioned burn initialized by a hand-encoded instruction,
R3). Squads v4 (`SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf`) holds the multisig roles; its
instructions are hand-built in tests because the published crate targets Solana 1.17 (R8).

**Storage**: on-chain program accounts only (`FundState`, `RoleRegistry`, `Registry`,
`ExtraAccountMetaList`, `HolderState`) plus the Token-2022 mint and token accounts. No database in
F001 (the Postgres indexer is F008).

**Testing**: LiteSVM 0.12.0 (`precompiles` feature) with the mainnet Token-2022 v11 and Squads v4
binaries as fixtures. One rejection test per rule asserting the exact error; transfers through real
`transfer_checked` with the hook; `proptest` randomized operation sequences for SC-001 / SC-007; a
script that fails CI if any error variant is never asserted (SC-004). One devnet end-to-end run using
zero-unit transfers (R16). Map in [quickstart.md](quickstart.md).

**Target Platform**: Solana devnet (Agave 4.4-beta), mainnet-compatible (Agave 4.3). Local tests run on LiteSVM's Agave 3.1 runtime (R8).

**Project Type**: Anchor workspace (on-chain programs) plus Rust support crates and test crate.

**Performance Goals**: every F001 instruction, including the transfer hook, fits within Solana
compute (1.4M CU), size (1 232 bytes) and call-depth (4 nested calls) limits. The hook does constant
work (no loops over holders), ≤ 60 000 CU per transfer, asserted in tests (SC-006). The writable
Registry allows about 330 fund transfers per block (R5, R8).

**Constraints**: no backend or test helper holds an institution's signing key in production paths;
fail closed on any missing, malformed or unknown input; checked integer math only; no mint, burn or
metadata-update path in F001 (FR-035); every state change has a named signer from shared-model §5.1.

**Scale/Scope**: one fund, up to ~10 000 holders, up to four KYC providers.

## Constitution Check

*GATE: checked before Phase 0 research and again after Phase 1 design. Both pass.*

| Principle | How F001 complies | Pre | Post |
|---|---|---|---|
| I. Correctness over features | Escrow registration fails closed until F003 rather than being half-built; hook steps 2 and 5 wait for the features that can test them | ✅ | ✅ |
| II. Rules live on-chain | Eligibility, pause and cap are in the hook; new accounts are frozen by default; mint, freeze, delegate, hook and metadata authorities are program PDAs. Tests send raw `transfer_checked` and raw Token-2022 admin instructions directly | ✅ | ✅ |
| III. Named signer | Every instruction's signer is in shared-model §5.1 (updated). The only permissionless instruction, `thaw_holding_account`, is listed there and fully determined by the owner's Approved status. Payers are separate from authorities | ✅ | ✅ |
| IV. One instruction, one transition | Approval and thaw are separate instructions. Named atomic case: `create_fund` creates the Fund and its mint together, because FR-004 requires the token to exist exactly when the Fund does. Governance → `set_status` CPI is one transition routed through two programs | ✅ | ✅ |
| V. On-chain source of truth | `holder_count` is a stored running total that can't be recomputed in one transaction. It is updated in the same transfer as its inputs and invariant-tested (I10). The issuer is stored once (R10) | ✅ | ✅ |
| VI. Failing test first | [quickstart.md](quickstart.md) maps every FR and rule to a rejection test with its exact error; tasks must order each test before its code | ✅ | ✅ |
| VII. Fail closed | Missing `HolderState` → not approved; unknown jurisdiction, reason, provider or class → rejected; overflow aborts; malformed statement → rejected | ✅ | ✅ |
| VIII. No unpaid units, no silent powers | No mint path, and permissioned burn stops holders burning. Suspend, revoke and pause are role-gated, reason-coded and emit permanent events. No destructive power is exercisable through the programs (token-admin and delegate PDAs sign nothing). Program upgrade, the one destructive power outside the programs, is held by a Squads vault with a time lock (see Deployment and authorities) | ✅ | ✅ |
| IX. Money math is exact | No price or amount formulas in F001. Counts and timestamps use checked integer math. No §8 row needed | ✅ | ✅ |
| X. Stated assumptions | Spec Assumptions cover KYC provider trust, identity = owner account, separation of duties and jurisdictions; R3's self-burn is added (see Spec adjustments) | ✅ | ✅ |
| XI. Features ship in order | Only F001's three programs are created; dvp_settlement and collateral are not; F002+ fields stay zero | ✅ | ✅ |
| XII. Earlier features stay green | First feature; the full suite runs in CI from day one | ✅ | ✅ |
| XIII. One shared model | All changes below are made in `docs/shared-model.md` in this same branch, before code | ✅ | ✅ |

No violations, so Complexity Tracking is empty.

## Shared-model changes

Made in `docs/shared-model.md` on this branch (Principle XIII):

| # | Section | Change | Why |
|---|---|---|---|
| 1 | §4.1 ExtraAccountMetaList | `Registry` entry becomes **writable** (stated effect: every fund transfer write-locks the Registry) | holder count (FR-024, R5) |
| 2 | §4.1 Registry | `allowed_jurisdictions: u64` → `[u64; 11]` (676-bit set) | every ISO country (FR-008, R11) |
| 3 | §4.1 HolderState | `jurisdiction: u8` → `[u8; 2]` (ISO alpha-2); `last_balance`, `last_slot` retagged F002 → F001 | R11; holder count uses `last_balance` |
| 4 | §1 Conventions | add Jurisdiction encoding and the one-holding-account (ATA) rule | R9, R11 |
| 5 | §3 Mint | name the transfer hook authority (governance `["token-admin", mint]`) and metadata pointer/update authority (`FundState`) | no unlisted authority can change transfer behaviour |
| 6 | §4.2, §4.5 | new PDAs: fund `["treasury", fund_state]`; governance `["hook-admin"]`, `["token-admin", mint]`; `init_roles` needs the mint signature; role separation rules | R10, Clarification Q4 |
| 7 | §5.1 | add `set_roles`, `set_rules`, `approve_investor`, `renew_investor`, `approve_fund_account`, `thaw_holding_account` (permissionless, state-checked), `reinstate_holder`; self-registration limited to Investor | Clarification Q1, R9 |
| 8 | §6 | step 6 checks jurisdiction for Investors only; steps 7 and 9 made precise; steps 2 and 5 move to F005 and F004; missing record = not approved; Token-2022 frozen check documented as first | R15, Constitution VI |
| 9 | §7 | new invariant I10 (holder count) | SC-007 |
| 10 | §9 | new events `RolesInitialized`, `RolesAssigned`, `RegistryInitialized`, `RegisterRulesSet`, `HoldingAccountThawed` | FR-033 |
| 11 | §11 | F001 row updated; step 2 added to F005 | — |
| 12 | §3, new §12 | §3 adds the **permissioned burn** extension (burn authority = `FundState` PDA) and lists Pausable as unused; new §12 records the Token-2022 behaviours later features must design for (self-transfers skip the hook, outer CPI callers must pass the writable Registry, holder-side memo/CPI-guard settings, …) | R3, R4; Constitution X |

## Spec adjustments made during planning

Planning surfaced facts the spec has to state (workflow rule: fix the spec before code). These
change no decision you made in clarify:

| Spec section | Change | Why |
|---|---|---|
| Edge Cases: Same sender and receiver | Now says the token doesn't run the holder checks for a transfer from an account to itself, and nothing changes | Token-2022 returns before calling the hook for same-account transfers (R4) |
| FR-015 | Each owner holds units in exactly one designated holding account; other accounts can never send or receive | Per-owner holder count and later per-owner accrual need one balance per owner (R9) |
| Edge Cases: Fund-operated accounts | Escrow registration is rejected until the settlement feature exists | No escrow account is system-controlled before F003 (R17) |
| FR-035 + new edge case | A holder's attempt to destroy its own units is rejected | Permissioned burn closes a Token-2022 path that bypasses the hook (R3) |
| FR-018 | Renewal needs a statement whose expiry is later than the recorded one | Resolves the "newer statement" item left open in clarify; blocks replay (R14) |

## Deployment and authorities

- **Program upgrade authority**: after deployment, each program's upgrade authority moves to the
  issuer's Squads vault with a non-zero time lock (≥ 1 h on devnet, 24 h on mainnet). Upgrades are a
  destructive power (Constitution VIII); a Squads proposal is visible and cancellable until it
  executes. F007 may move this under the governance program's own time lock.
- **Program IDs**: program keypairs stay in `target/deploy/` and are never committed (the repository
  is public). `crates/mmf-common` carries the public program IDs that each program checks.
- **Mint authorities**: all held by program PDAs as listed in [data-model.md](data-model.md#fund-mint-token-2022);
  none can be exercised through an F001 instruction.

## Project Structure

### Documentation (this feature)

```text
specs/001-fund-launch-onboarding/
├── plan.md              # this file
├── research.md          # Phase 0
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1: validation guide and FR → test map
├── contracts/           # Phase 1: instruction contracts
│   ├── fund.md
│   ├── governance.md
│   ├── compliance_hook.md   # includes the hook's check order
│   └── kyc-statement.md
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks (not created here)
```

### Source Code (repository root)

```text
Anchor.toml
Cargo.toml                      # workspace
rust-toolchain.toml
programs/
├── fund/src/                   # create_fund, launch; FundState
├── compliance_hook/src/        # Registry, HolderState, ExtraAccountMetaList, execute,
│                               # register/approve/renew/thaw, set_status, set_paused
└── governance/src/             # RoleRegistry, init_roles, set_roles, compliance actions
crates/
├── mmf-common/src/             # program ids, seeds, enums, ISO 3166 table, statement layout,
│                               # read-only cross-program account views (R12)
└── mmf-sdk/src/                # instruction builders, PDA helpers, extra-meta resolution,
                                # mock KYC provider (sign statement), used by tests and devnet run
tests/
├── src/                        # LiteSVM harness: fixtures, Squads helpers, event decoding
├── tests/                      # us1_launch.rs, us2_onboarding.rs, us3_suspend_pause.rs,
│                               # us4_roles.rs, hook_order.rs, direct_token_calls.rs,
│                               # invariants_prop.rs, scale.rs
└── fixtures/                   # mainnet dumps: spl_token_2022.so (v11), squads_multisig_program.so,
                                # squads_program_config.json (see quickstart)
scripts/
├── devnet-e2e/                 # Rust binary: the R16 run against devnet
└── check-error-coverage.sh     # SC-004: fails if an error variant has no asserting test
```

**Structure Decision**: an Anchor workspace with three programs. No `services/` or `app/` until
F008; no `dvp_settlement` or `collateral` program until F003 and F004 (Principle XI). Shared types
live in `crates/mmf-common` so the programs don't depend on each other's crates (R12).

## Complexity Tracking

No constitution violations to justify.
