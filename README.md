# Tokenized MMF Collateral

Open-source, issuer-side infrastructure for a tokenized money market fund on Solana, built to be
used as collateral. The fund's token enforces its own rules: only approved, KYC-checked
institutions can hold it, and every compliance action is on-chain and on record.

Built feature by feature with [Spec Kit](https://github.com/github/spec-kit). **F001 — fund
launch, roles and holder onboarding** is merged; **F002 — daily price and interest accrual** is
implemented on its branch. F003–F008 (subscription/redemption, collateral, enforcement,
distributions, governance, operator tooling) follow in order; see
[docs/speckit-inputs.md](docs/speckit-inputs.md).

## What F001 does

- **Launch**: the issuer creates a fund and its Token-2022 mint in one instruction, sets holder
  rules (KYC providers, allowed countries, holder cap) and roles, then opens it.
- **Onboarding**: an institution registers; compliance approves it with an ed25519-signed KYC
  statement; its one holding account is thawed.
- **Transfer rules**: a transfer hook rejects any transfer whose sender or receiver isn't approved
  with valid KYC, while transfers are paused, or past the holder cap — however the transfer is
  built.
- **Compliance actions**: suspend, reinstate, revoke, pause, unpause, each reason-coded and
  recorded. Roles can be Squads multisig vaults.
- **No unpaid units**: no instruction mints or burns in F001, and Token-2022's permissioned burn
  stops holders destroying their own units.

## What F002 does

- **Daily publish**: the fund admin publishes the price per unit and the daily interest rate for
  the days it covers: 1 normally, 3 on a Friday, more before a holiday or after an outage.
  Coverage is contiguous: each publish starts the day after the last covered day and can't start
  in the future, so no day is skipped or paid twice. Each publish is kept forever as a numbered
  price record that later features cite. A price that moves more than the fund's daily limit is
  rejected unless compliance has approved an override for exactly that price.
- **Override**: compliance (a Squads vault, typically) approves or withdraws an override through
  governance, with a reason code. The next publish uses it up whether or not it needed it, so an
  old override can never be used later.
- **Interest without touching holders**: a publish adds `rate × days` to one counter on the fund
  and writes no holder account, so it costs the same with 10 or 10 000 holders. The transfer hook
  settles each side's interest on its old balance before every transfer, exactly (in units ×
  10^18, no rounding). Whoever holds units at a publish earns it, whatever their status. Rounding
  to whole units happens only at payout (F006).

The specifications, plans and designs are in [specs/](specs/); the rules every feature follows
are in the [constitution](.specify/memory/constitution.md) and
[docs/shared-model.md](docs/shared-model.md).

## Architecture

### Programs, accounts and authorities

Three Anchor programs share one Token-2022 mint. Every authority over the mint is a program PDA, so
no person's key can mint, freeze or reconfigure the token.

```mermaid
flowchart LR
    subgraph People["Signers"]
        ISS["Issuer"]
        CMP["Compliance<br/>(Squads multisig vault)"]
        INST["Institution<br/>(via its custodian)"]
        KYC["KYC provider<br/>(signs statements off-chain)"]
    end

    subgraph FUND["fund program"]
        FS["FundState PDA<br/>terms · status<br/>mint, burn, close and metadata authority"]
    end

    subgraph GOV["governance program"]
        RR["RoleRegistry PDA<br/>issuer + 5 roles"]
        HA["hook-admin PDA<br/>signs status / pause CPIs"]
        TA["token-admin PDA<br/>transfer-hook authority (unused until F007)"]
        DL["delegate PDA<br/>permanent delegate (unused until F005)"]
    end

    subgraph HOOK["compliance_hook program"]
        REG["Registry PDA<br/>KYC providers · allowed countries<br/>holder cap · holder count · pause flag<br/>freeze authority"]
        HS["HolderState PDA<br/>one per owner: status · class<br/>country · KYC expiry · last balance"]
        EAML["ExtraAccountMetaList PDA"]
        EX["execute: transfer hook"]
    end

    MINT[("Token-2022 fund mint<br/>transfer hook · frozen by default<br/>permanent delegate · metadata<br/>permissioned burn · close authority")]

    ISS -->|"init_roles, set_roles"| RR
    ISS -->|"create_fund, launch"| FS
    ISS -->|"init_registry, set_rules"| REG
    FS -->|"creates, owns authorities"| MINT
    INST -->|"register_holder"| HS
    CMP -->|"approve_investor + KYC statement<br/>approve_fund_account"| HS
    KYC -.->|"ed25519 statement"| CMP
    CMP -->|"suspend · reinstate · revoke<br/>pause · unpause"| GOV
    HA -->|"CPI set_status / set_paused"| HOOK
    MINT -->|"every transfer_checked"| EX
    EAML -.->|"resolves Registry, FundState,<br/>both HolderStates, delegate"| EX
    REG -->|"thaw_holding_account (signed by Registry PDA)"| MINT
```

### Onboarding: two transactions (SC-005)

```mermaid
sequenceDiagram
    autonumber
    participant I as Institution
    participant K as KYC provider
    participant C as Compliance (Squads vault)
    participant H as compliance_hook
    participant T as Token-2022

    I->>H: register_holder(owner = itself, class = Investor)
    H-->>I: HolderState = Pending
    K-->>C: signed 95-byte statement (owner, fund, country, investor type, expiry)
    Note over C,T: one transaction
    C->>T: create the owner's holding account (ATA) — starts frozen
    C->>H: Ed25519 verify instruction (the statement)
    C->>H: approve_investor(statement hash)
    H->>H: 12 statement checks: provider, fund, owner, expiry, country, hash …
    H-->>C: HolderState = Approved
    C->>H: thaw_holding_account (permissionless)
    H->>T: ThawAccount, signed by the Registry PDA
    Note over I,T: the institution can now send and receive units
```

### Every transfer: the hook's fixed check order (FR-026)

```mermaid
flowchart TD
    A["transfer_checked"] --> B{"Token-2022: source or<br/>destination frozen?"}
    B -->|yes| X1["AccountFrozen"]
    B -->|no| C{"Called inside a real transfer?"}
    C -->|no| X2["NotTransferring"]
    C -->|yes| D{"Accounts belong to this fund?"}
    D -->|no| X3["WrongMint"]
    D -->|yes| E{"Transfers paused?"}
    E -->|yes| X4["TransfersPaused"]
    E -->|no| F{"Sender Approved,<br/>KYC unexpired?"}
    F -->|no| X5["SenderNotApproved"]
    F -->|yes| G{"Receiver Approved, KYC unexpired,<br/>class and country allowed?"}
    G -->|no| X6["ReceiverNotApproved"]
    G -->|yes| H{"New investor holder while<br/>the cap is reached?"}
    H -->|yes| X7["HolderCapReached"]
    H -->|no| S["F002: settle both sides' interest<br/>on their old balances"]
    S --> I["Update holder count and recorded balances<br/>emit TransferValidated"]
```

### Holder status

```mermaid
stateDiagram-v2
    [*] --> Pending: register_holder
    Pending --> Approved: approve_investor (KYC statement)<br/>approve_fund_account (treasury)
    Approved --> Approved: renew_investor (later expiry)
    Approved --> Suspended: suspend_holder
    Suspended --> Approved: reinstate_holder
    Pending --> Revoked: revoke_holder
    Approved --> Revoked: revoke_holder
    Suspended --> Revoked: revoke_holder
    Revoked --> [*]: final
```

Suspend, reinstate, revoke, pause and unpause each carry a reason code and emit a permanent event; a
holder's full status history can be rebuilt from those events alone.

## Tests

**157 tests pass in the LiteSVM suite, plus 23 shared-crate unit tests.**
- Every rule has a rejection test asserting the exact error; all 73 error variants are covered.
- 128 randomized F001 operation sequences, now with publishes interleaved, are checked against a
  reference model.
- F002 suites: `f002_publish`, `f002_override` and `f002_accrual`. The spec's own test is in
  `f002_accrual`: A holds 1 000 000 units for 10 days at a different rate each day, sends half to
  B, then 10 more days; interest matches a day-by-day calculation exactly.
- `accrual_prop`: 64 randomized 90-day simulations with transfers, suspensions and pauses. Every
  holder's interest equals an independent day-by-day model exactly after every step, and an
  auditor's rebuild from the records alone agrees.
- Two scale tests, run in release. The hook's cost and a publish's cost are identical with 10 or
  10 000 holders. A settlement's cost doesn't change with the number of holders or with how many
  publishes have passed since the holder's last one; it varies by 1–2 compute units with the
  amounts involved, and the test allows a 16-unit band.

![F001 program tests passing on LiteSVM](docs/images/tests-litesvm.png)

The end-to-end script runs the full flow on a real validator (here a local
`solana-test-validator` with the programs and the mainnet Token-2022 build loaded): 27 steps, each
checked against its expected outcome, ending with each holder's status history rebuilt from
on-chain records.

![F001 end-to-end run on a local validator](docs/images/e2e-localnet.png)

The F002 steps follow on the same fund: an over-limit price rejected, an override approved, the
override price published, a same-day publish rejected (34 steps in all). Running the script with
`--continue <mint>` on later days publishes the next records.

Transfers on a cluster move zero units: there is no way to create units before F003, and a
zero-unit transfer still runs every hook check. Tests that move real units run on LiteSVM, which can
place units in an account. **Devnet deployment is pending**; the same script runs there unchanged
once the programs are deployed. Screenshots are rendered from the captured output with
`scripts/render_terminal.py`.

## Repository layout

```text
programs/
  fund/             fund terms, status, the Token-2022 mint, daily price records (create_fund,
                    launch, publish_daily, set_/clear_nav_override)
  compliance_hook/  holder register, onboarding, and the transfer hook (incl. interest settlement)
  governance/       roles, compliance actions, price overrides
crates/
  mmf-common/       program ids, seeds, enums, ISO 3166 table, KYC statement format, account views
  mmf-sdk/          instruction builders, PDA helpers, mock KYC provider, hooked transfers
tests/              LiteSVM harness and test suites; mainnet fixtures in tests/fixtures/
scripts/
  devnet-e2e/       end-to-end run against devnet or a local validator
  check-error-coverage.sh, check-duplicate-crates.sh
docs/               design, shared model, Spec Kit inputs, workflow
specs/              one folder per feature: spec, plan, research, data model, contracts, tasks
```

## Build and test

Requirements: Rust 1.90 (pinned in `rust-toolchain.toml`), Anchor CLI 1.1.2 (`avm use 1.1.2`),
Solana CLI 3.1.10 or newer.

```bash
anchor build                                          # the three programs
cargo test -p mmf-common                              # shared types
cargo test -p tests                                   # LiteSVM suites, incl. randomized invariants
cargo test -p tests --release --test scale -- --ignored   # 10 vs 10 000 holders (hook, publish, settlement)
scripts/check-error-coverage.sh                       # every error variant has a test
```

The test fixtures (Token-2022 v11 and Squads v4, dumped from mainnet) are committed; how to refresh
and verify them is in [tests/fixtures/README.md](tests/fixtures/README.md).

## End-to-end run

`scripts/devnet-e2e` runs the whole F001 flow and F002's first publish and override against a
cluster, using zero-unit transfers (no units can be created before F003, and a zero-unit transfer
still runs every hook check). `--continue <mint>` publishes the next record on a later UTC day.
Rehearse it free on a local validator, then run it on devnet once the programs are deployed.
Commands are in the [F001](specs/001-fund-launch-onboarding/quickstart.md#devnet-run-r16) and
[F002](specs/002-daily-price-accrual/quickstart.md#devnet-run-research-r13) quickstarts.

## Status

F001 is merged. F002 is implemented and tested on `002-daily-price-accrual`; its devnet run and
human review gate ([review.md](specs/002-daily-price-accrual/review.md)) are pending before it
merges. Devnet deployment of the programs is still pending. Nothing here is audited — don't use it
with real assets.
