# Contract: `compliance_hook` changes in F002

Only the transfer hook changes. Every other instruction is as in
[F001's contract](../../001-fund-launch-onboarding/contracts/compliance_hook.md). No new
instruction, error or account: `compliance_hook::accrue` moves to F003 (research R8).

## Transfer hook `execute(amount)` — step 8 filled

The fixed order of shared-model §6 is unchanged. F002 builds step 8:

```text
0. transferring flag on both accounts                          → NotTransferring
1. accounts belong to this mint                                 → WrongMint
   (F002: FundState is now decoded with FundStateView, which checks its owner program,
    address == registry.fund_state and discriminator          → WrongMint)
3. paused                                                       → TransfersPaused
4. sender approved, KYC valid                                   → SenderNotApproved
6. receiver approved, class and jurisdiction allowed            → ReceiverNotApproved
7. holder cap                                                   → HolderCapReached
8. accrue, before step 9 changes last_balance                   → Overflow       [F002]
     index = fund.accrual_index
     same owner:  settle(src, post_src + post_dst, index)
     otherwise:   settle(src, post_src + amount, index)
                  settle(dst, post_dst − amount, index)
     settle(h, pre, index):
       h.accrued_e18 += pre × (index − h.index_at_last_touch)    checked u128
       h.index_at_last_touch = index
9. holder count, last_balance, last_slot (unchanged)
10. emit! TransferValidated (payload extended)
```

- **Order**: step 8 comes after every check, so a rejected transfer settles nothing. It comes
  before step 9, which overwrites `last_balance`.
- **Pre-balances**: they are computed from the token accounts. Invariant I10 guarantees they
  equal `last_balance`, so the two sources agree.
- **Single write**: each `HolderState` is still serialized exactly once, together with step 9's
  changes (F001 research R4).
- **Pause**: a paused transfer fails at step 3, so nothing settles. Holder status doesn't affect
  accrual for units that don't move (FR-015). A suspended holder simply has no transfers.

**`TransferValidated`** **[SM change: payload]**: F001 fields + `accrual_index: u128`,
`source_accrued_e18: u128`, `destination_accrued_e18: u128`, all after settlement.

**Compute**: one view decode and two checked multiplications. The CI ceiling stays at 60 000 CU
per transfer, and the effort is independent of the number of holders and of the number of
publishes since the last settlement (SC-004, `scale.rs`).

**Tests**:
- A FundState from another fund, or with the wrong discriminator, gets `WrongMint`.
- `Overflow` is reached by patching `HolderState.index_at_last_touch` above the fund index with
  `set_account`. Together with the publish `Overflow` and forged-FundState tests, these are F002's
  only state patches, each for a condition no instruction can reach (F001's precedent).
- Ordering: a transfer and a publish in consecutive transactions, both ways round.
- Same-slot publish-then-transfer and transfer-then-publish.
- Accrual for a fund-treasury holder.
- A suspended holder accrues while suspended and is settled correctly when reinstated and moved.
