# Contract: KYC statement (provider → compliance_hook)

A KYC provider vouches for one owner account by signing a fixed 95-byte message with its ed25519
key. Compliance submits the statement in the same transaction as `approve_investor` or
`renew_investor`. In F001 the provider is mocked by a test keypair (spec Assumptions); a real
provider only needs to produce the same bytes.

## Message layout (little-endian, no padding)

| Offset | Size | Field | Rule |
|---|---|---|---|
| 0 | 20 | `domain` | exactly `b"MMF-KYC-STATEMENT-V1"`; separates these signatures from anything else the provider key signs |
| 20 | 32 | `mint` | the fund mint; a statement for another fund is rejected |
| 52 | 32 | `owner` | the owner account being approved (Clarification Q2) |
| 84 | 2 | `jurisdiction` | ISO 3166-1 alpha-2, ASCII uppercase, an assigned code |
| 86 | 1 | `investor_type` | provider-defined code, must be non-zero; recorded, not checked against a list |
| 87 | 8 | `expiry_ts` | i64 Unix seconds |

`attestation_hash = SHA-256(message[0..95])` is stored on the Holder record.

## Transaction shape

```text
ix[k]   Ed25519SigVerify111111111111111111111111111: one signature over the 95-byte message,
        with all offsets pointing inside this same instruction
ix[k+1] compliance_hook::approve_investor(attestation_hash)  (or renew_investor)
        — or, when compliance is a Squads vault, squads::vault_transaction_execute whose approved
          inner instruction is approve_investor(attestation_hash)
```

The instruction "immediately before the current one" is counted among top-level instructions, so
the same layout works when Squads runs `approve_investor` through a call. Because the
`attestation_hash` argument is part of what the multisig members approved, they approve one exact
statement, and the executor can't attach a different valid one (research R8).

## Verification by `compliance_hook` (in this order)

| # | Check | Error |
|---|---|---|
| 1 | the `instructions` account is the real Instructions sysvar, and the top-level instruction immediately before the current one is owned by the Ed25519 program | `StatementMissing` |
| 2 | it holds exactly one signature; its signature, pubkey and message `instruction_index` fields are all `u16::MAX` (data in the same instruction); offsets and sizes lie within its data; message length is 95 | `StatementMalformed` |
| 3 | `domain` matches | `StatementMalformed` |
| 4 | the signing pubkey is one of `registry.kyc_providers` (non-default) | `ProviderNotRecognised` |
| 5 | `mint == registry.mint` | `StatementWrongFund` |
| 6 | `owner == holder_state.owner` | `StatementOwnerMismatch` |
| 7 | `expiry_ts > now` | `StatementExpired` |
| 8 | `jurisdiction` is an assigned ISO 3166-1 code | `UnknownJurisdiction` |
| 9 | `jurisdiction` is allowed by the Register | `JurisdictionNotAllowed` |
| 10 | `investor_type != 0` | `InvalidInvestorType` |
| 11 | renewal only: `expiry_ts > holder_state.kyc_expiry_ts` | `RenewalNotNewer` |
| 12 | `SHA-256(message) == attestation_hash` argument of the instruction | `StatementHashMismatch` |

The Ed25519 program has already checked the signature itself (the transaction fails otherwise);
checks 1–2 make sure the verified bytes really are this message and this signer (research R6).
