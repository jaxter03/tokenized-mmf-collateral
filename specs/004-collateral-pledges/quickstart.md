# Quickstart: validating F004 (collateral)

How to show that F004 works, and where each requirement is tested. Accounts and checks are in
[data-model.md](data-model.md) and [contracts/](contracts/); this file doesn't repeat them.

## Prerequisites

- The F001–F003 toolchain: Rust 1.90 (pinned), Anchor 1.1.2, Solana CLI, and `cargo test -p tests`
  passing on `main`.
- A new program keypair for `collateral` (`anchor keys sync`). Its ID goes into
  `mmf_common::ids::COLLATERAL_PROGRAM_ID` and `Anchor.toml`. The keypair stays in `target/deploy/`
  and is never committed.

## Run

```bash
anchor build                      # five programs: fund, compliance_hook, governance, dvp_settlement, collateral
cargo test -p tests               # all F001–F004 suites
cargo test -p mmf-common          # collateral formula vectors (data-model worked checks)
scripts/check-error-coverage.sh   # every CollateralError and new Hook/Governance error has a test asserting it (SC-006)
cargo run -p devnet-e2e -- --feature f004   # one devnet run (below)
```

## Test fixture `collateral_ready_fund`

1. F001–F003: launch a fund with `max_nav_move_bps = 500` (so test publishes of $0.985–$1.01
   pass F002's price-move guard), onboard giver G and taker T, publish $1.00, and give G
   2 000 000 units (`seed_units`, which keeps I4).
2. T proposes an agreement with the worked terms: haircut 200 bps, MTA $10 000, cure 4 h, grace
   2 h, pledge limit 5 000 000 units, max price age 26 h, taker may raise alone, a non-zero
   document hash. G signs it.
3. Variants: `two_step` (taker may not raise alone), `unpriced` (no publish yet), `paused`.

## Independent tests per user story

| Story | Scenario | Expected | Test file |
|---|---|---|---|
| US1 | T proposes; G signs; an outsider signs; T signs twice | Proposed with exact terms → Active; outsider `Unauthorized`; second T `AlreadySigned` | `f004_agreement.rs` |
| US2 | nothing pledged; T sets exposure $1 000 000 | one Delivery call: value `1_000_000_000_000`, units `1_020_408_163_266`, deadline +4 h, enforce-after +6 h, `called_outstanding` = value | `f004_exposure.rs` |
| US3 | after US2, G pledges `1_020_408_163_266` in one transaction | G's balance unchanged (2 000 000); no balance anywhere changed; lock = pledge; call derived Met (`met_below_seq` > its seq) in that transaction; send 979 591.836735 → `PledgedUnitsLocked`; send 979 591.836734 → ok | `f004_pledge.rs`, `f004_lock.rs` |
| US4 | T lowers to $400 000; G opens a return call; keeper releases against it | return value `600_000_000_000`, units `612_244_897_959`; lock `408_163_265_307`; coverage exactly 0; return call Met | `f004_release.rs` |
| US5 | G substitutes the pledge with 1 100 000 units; then tries 1 000 000 | old Substituted, new Active, lock 1 100 000, no balance change, all in one instruction; smaller → `SubstituteTooSmall` | `f004_substitute.rs` |
| US6 | call opens; G disputes, withdraws, disputes again; compliance resolves Cancelled ("operational error") | Disputed (deadlines unchanged) → Open → Disputed → Cancelled; T then opens a new call for $1 000 000 | `f004_dispute.rs` |

## Requirement → test map

| Requirement | Test(s) |
|---|---|
| FR-001–FR-002 proposal and terms | `f004_agreement.rs`: each term check → `SameParty`, `UnsupportedPledgeMode`, `UnsupportedIncomeRouting`, `InvalidHaircut` (10 000 and 10 001; 9 999 passes), `InvalidCureTime`, `InvalidPledgeLimit`, `InvalidPriceAge`, `EmptyAgreementHash`; Fund in Setup → `FundNotOpen`; non-party proposer → `Unauthorized`; zero grace and zero MTA accepted |
| FR-003 activation | `f004_agreement.rs`: `AgreementNotProposed` (sign an Active one), `AlreadySigned`, `Unauthorized` |
| FR-004 eligibility at proposal and signing | `f004_agreement.rs`: pending, suspended, revoked, KYC-expired (at the exact second), unregistered (`WrongHolder`) and fund-operated parties → `PartyNotEligible`; a party suspended between proposal and signing → `PartyNotEligible` at signing |
| FR-005 terms fixed | `f004_agreement.rs`: no instruction takes terms after proposal (IDL check), and a re-proposal at the same address → already in use |
| FR-006 identity | `f004_agreement.rs`: same (fund, giver, taker, nonce) twice → already in use; same pair, new nonce → second agreement |
| FR-007 Active only | every exposure, call, pledge, release and substitute test runs once on a Proposed agreement → `AgreementNotActive` |
| FR-008–FR-010 valuation, freshness, rounding | `mmf-common` vectors (data-model table, ceil vs floor, overflow → `None`); `f004_calls.rs`: `NoPrice` (unpriced fund), `StalePrice` at max age + 1 s (max age exactly passes) |
| FR-011–FR-012 exposure rules | `f004_exposure.rs`: G or outsider → `Unauthorized`; zero ref → `ZeroExternalRef`; lower on a stale price applies; raise on a stale price → `StalePrice`; two-step: Pending, then `confirm_exposure` opens the call with deadlines from the confirmation; stale confirmation after a newer raise → `PendingRaiseMismatch`; none pending → `NoPendingRaise`; lower clears pending |
| FR-013 raise opens the call in the same instruction | `f004_exposure.rs`: shortfall exactly at the MTA → no call, call account not created; MTA + 1 → call |
| FR-014, FR-017 delivery calls | `f004_calls.rs`: price $0.99 → `NothingToCall`; $0.985 → call $15 000, units `15_539_210_609`; second call at an unchanged shortfall → `NothingToCall`; reused ref → `CallRefInUse` / already in use; G opening a Delivery → `Unauthorized`; `InvalidCallKind` |
| FR-015 demanded part | `f004_calls.rs`: call $1M; G pledges $600k of value; T raises $500k → a new call for $500k (not folded in); cancel the first call → `called_outstanding` reduced by `min(D, value)`; I16 checked after each step |
| FR-016 return calls | `f004_release.rs`: excess at MTA → `NoExcess`; a second return call → `ReturnCallAlreadyOpen`; T opening a Return → `Unauthorized`; tiny excess at a high price → `ReturnTooSmall` |
| FR-018–FR-019 met together; re-check | `f004_calls.rs`: two open calls, then a pledge covering both → both derived Met; partial pledge → both still Open; price rises above cover → `check_coverage` by a random key → Met; `check_coverage` on a stale price → `StalePrice`; outcome identical whoever triggers it |
| FR-020 cancel | `f004_calls.rs`: T cancels Open and Disputed delivery calls; G cancels its return call; wrong party → `Unauthorized`; a cancelled or (derived) Met call → `CallNotOpen` |
| FR-021–FR-022 pledge | `f004_pledge.rs`: `ZeroUnits`, `ZeroPledgeRef`, `PledgeLimitExceeded` (cap + 1; cap exactly passes), `InsufficientUnlockedUnits` (unlocked + 1; exactly passes), T or keeper pledging → `Unauthorized`, ineligible giver → `PartyNotEligible`, same ref twice → already in use; pledge fields recorded (seq, price record, zeros) |
| FR-023 transfer rule | `f004_lock.rs`: direct `transfer_checked` built by hand; owner-approved delegate; F003 redemption funding (F003 US2 scenario 3) and secondary funding → `PledgedUnitsLocked`; `hook_order.rs` step-5 rows |
| FR-024 only these steps change locks | `f004_lien.rs`: `set_lien` from any key, a governance PDA, the FundState PDA, another program's `["hook-caller"]` → `NotCollateralCaller`; a registry with empty `collateral_program` → `NotCollateralCaller`; wrong holder or holding account → `WrongHolder`; `LienUnderflow`; `init_registry` fills `collateral_program` |
| FR-025 lock totals | I1 and I15 after every step in all F004 tests and in `collateral_prop.rs` |
| FR-026 interest; receiving | `f004_interest.rs` (SC-004); `f004_lock.rs`: a fully locked holder receives units |
| FR-027–FR-029 release | `f004_release.rs`: T releases without a return call; keeper without a return call → `KeeperNeedsReturnCall`; beyond the call's remainder → `ExceedsReturnCall`; a closed or other agreement's return call → `WrongReturnCall`; beyond the pledge → `ExceedsPledgeRemaining`; a Released pledge → `PledgeNotActive`; coverage below 0 by 1 base unit → `WouldUndercollateralise`; G or outsider → `Unauthorized`; zero exposure with a stale price → release succeeds; required > 0 with a stale price → `StalePrice`; a release not naming the return call doesn't count towards it |
| FR-030 substitution | `f004_substitute.rs`: equal units pass; `SubstituteTooSmall`; net increase above unlocked → `InsufficientUnlockedUnits`; above the cap → `PledgeLimitExceeded`; a Substituted or Released old pledge → `PledgeNotActive`; lock, `active_liens` and `pledged_units` unchanged for an equal swap |
| FR-031–FR-033 disputes | `f004_dispute.rs` + governance contract tests: `WrongCallKind` (return), `CallNotOpen`, `NotDisputed`, `InvalidOutcome`, `InvalidReason`, `Unauthorized` (each other role), direct `resolve_dispute` → `UnauthorizedCaller`; a Disputed call is Met when covered; a Disputed call still counts in `called_outstanding` |
| FR-034 parties' status | `f004_status.rs`: suspended or KYC-expired giver: pledge, substitute, confirm, return call → `PartyNotEligible`; release, dispute, cancel return call still succeed; suspended taker: raise and delivery call → `PartyNotEligible`; lower, release and cancel succeed; deadlines unchanged by suspension |
| FR-035 paused | `f004_status.rs`: pledge, release and substitute succeed while paused; a transfer of unlocked units fails `TransfersPaused` |
| FR-036 slow signers, at most once | `f004_slow_signers.rs` (below) |
| FR-037 records | `tests/src/rebuild.rs` extended: agreement histories, call outcomes (from `met_below_seq`) and every holder's lock rebuilt from events equal on-chain state (SC-009) |

## Slow signers (`f004_slow_signers.rs`, LiteSVM `with_blockhash_check(true)`)

- G's pledge prepared on G's durable nonce and submitted after 6 h → succeeds (SC-008).
- A pledge prepared before T opens a second call, and a release prepared before G opens a return
  call → both succeed, unaffected (R5, R8).
- A confirmation prepared for a raise that T has since replaced → `PendingRaiseMismatch`.
- The same signed pledge submitted twice → the second fails (nonce advanced, and the pledge
  account already exists).
- An ordinary-blockhash control after 6 h → `BlockhashNotFound`.

## Success criteria

| SC | How |
|---|---|
| SC-001 | `f004_pledge.rs`: snapshot every token balance before and after the pledge transaction; all equal; call derived Met |
| SC-002 | `collateral_prop.rs`: random sequences (R15) against `tests/src/collateral_model.rs`; I1, I2, I15, I16 after every step |
| SC-003 | `f004_lock.rs` and `collateral_prop.rs` (transfers and settlement funding drawn over the lock always fail; no step reduces a balance below its lock) |
| SC-004 | `f004_interest.rs`: two holders with 1 000 000 units, one fully locked, five publishes at different rates and a transfer each → identical `accrued_e18` |
| SC-005 | `collateral_prop.rs`: after pledging a call's `units_required` at its price, coverage ≥ 0; after releasing a return call's units at its price, coverage ≥ 0 |
| SC-006 | `check-error-coverage.sh` over `CollateralError` and the new Hook/Governance errors |
| SC-007 | `scale.rs`: `pledge`, `release`, `issue_call` CU equal (±0) with 10 vs 10 000 holders and 1 vs 300 agreements; `publish_daily` CU unchanged from F003 |
| SC-008 | `f004_slow_signers.rs` |
| SC-009 | `rebuild.rs` |

## Devnet run (`scripts/devnet-e2e`, `--feature f004`)

`devnet-e2e --feature f004` builds a new fund, so its Registry carries `collateral_program` (R16).
It uses plain keys for every role, as F003 did. It then:
1. launches the fund, publishes $1.00 and onboards giver G and taker T; it gives G 2 000 000 units
   through an F003 subscription;
2. T proposes the worked agreement and G signs it;
3. T sets exposure $1 000 000 and a call opens. G's pledge of 1 020 408.163266 units is prepared on
   G's durable nonce and signed 130 s later. The call is Met in that transaction, and no balance
   changed;
4. G tries to send 979 591.836735 units and gets `PledgedUnitsLocked`; it then sends
   979 591.836734 units successfully;
5. T lowers exposure to $400 000, G opens a return call, and the keeper releases
   612 244.897959 units against it. The return call is Met and G's lock is 408 163.265307.

Each step prints its signature, and the run exits 0 only if every outcome matches.

## Known behaviours (documented, not handled)

- A price move counts from the first step that values the agreement at the new price (R5).
  Anyone can run `check_coverage` after a publish to record it.
- Registries created before F004 can't hold liens (`NotCollateralCaller`, R16).
- An open return call isn't cancelled when a delivery call opens. Releases against it remain
  bounded by coverage (R8).
- Enforcement fairness across several open calls is F005's job: enforcement is limited to the
  shortfall and to the defaulted call's own value (R5).
