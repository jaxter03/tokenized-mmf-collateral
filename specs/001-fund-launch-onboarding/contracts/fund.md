# Contract: `fund` program (F001 instructions)

Checks run in the order listed; the first failing check aborts the transaction with that error
(Constitution VII). Every instruction emits exactly one event (shared-model §1).

## `create_fund(terms: FundTerms, name: String, symbol: String)`

Creates the Fund and its token together (FR-001–FR-004). One business transition: "create a Fund".
Creating the mint is part of it because FR-004 requires the token to exist exactly when the Fund
does.

| Account | Kind | Constraint |
|---|---|---|
| `issuer` | signer, writable (payer) | must equal `role_registry.issuer` |
| `mint` | signer, writable | new account; becomes the fund mint |
| `fund_state` | writable | new PDA `["fund", mint]` |
| `role_registry` | read | owner = governance; address = PDA `["roles", fund_state]` of governance; `role_registry.fund_state == fund_state` |
| `cash_mint` | read | SPL Token or Token-2022 mint |
| `token_2022_program`, `system_program` | program | |

| # | Check | Error |
|---|---|---|
| 1 | `role_registry` owned by governance at the expected address and bound to this `fund_state` | `RoleRegistryMismatch` |
| 2 | `issuer == role_registry.issuer` | `Unauthorized` |
| 3 | terms valid (see [data-model.md](../data-model.md#fundstate-fund--fields-used-in-f001)) | `InvalidFundId`, `InvalidCashMint`, `ZeroTargetNav`, `ZeroMaxNavMove`, `InvalidRedemptionGate`, `InvalidDistributionMode`, `ZeroDistributionCadence`, `MissingTermsHash`, `MissingTermsUri`, `InvalidName`, `InvalidSymbol` (checked in this order) |

**Effect**: mint created with every extension and authority in the data model, decimals 6, token
metadata initialized (name, symbol, uri, `fund_id`, `terms_hash`); `FundState` created in Setup with
the terms. **Event**: `FundCreated`.

CPI order inside the instruction (research R2): create the mint account sized for the fixed
extensions → TransferHook → DefaultAccountState(Frozen) → PermanentDelegate → MetadataPointer (to
the mint) → MintCloseAuthority → PermissionedBurn (hand-encoded, R3) → InitializeMint2 → top up rent
for the metadata → TokenMetadata initialize (signed by `FundState`) → UpdateField `fund_id` →
UpdateField `terms_hash`.

Rejection tests: one per error above. Also, submitted directly (Principle II): Token-2022
`MintTo`, `UpdateTransferHook`, metadata `UpdateField`, `UpdateDefaultAccountState` and
`SetAuthority` on the mint, each signed by the issuer's key, all fail. A holder's own `Burn` /
`BurnChecked` on its thawed account fails because permissioned burn is enabled.

## `launch()`

| Account | Kind | Constraint |
|---|---|---|
| `issuer` | signer | `== role_registry.issuer` |
| `fund_state` | writable | |
| `role_registry` | read | `fund_state.role_registry` |
| `registry` | read | owner = compliance_hook; address = PDA `["registry", mint]` |
| `extra_account_meta_list` | read | owner = compliance_hook; address = PDA `["extra-account-metas", mint]` |

| # | Check | Error |
|---|---|---|
| 1 | signer is the issuer | `Unauthorized` |
| 2 | `fund_state.status == Setup` | `FundNotInSetup` |
| 3 | all five operating roles assigned | `RolesIncomplete` |
| 4 | `registry` and `extra_account_meta_list` exist and are owned by compliance_hook | `RegisterNotInitialized` |
| 5 | ≥ 1 non-default KYC provider | `NoKycProvider` |
| 6 | ≥ 1 allowed jurisdiction bit | `NoAllowedJurisdiction` |

**Effect**: status Setup → Open. **Event**: `FundLaunched`.

## Not in F001

`publish_daily` (F002), `mint_for_subscription` / `burn_for_redemption` (F003), distributions
(F006), `wind_down` / `close_mint` (F007). The fund program has **no** instruction that mints,
burns, or updates metadata in F001 (FR-035).
