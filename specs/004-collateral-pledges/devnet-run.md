# Devnet run: 2026-10-15

This was the first devnet deployment of all five programs, followed by the F001/F002, F003 and F004
end-to-end runs. Programs were built from `004-collateral-pledges` at `df5ce8a` with
`anchor build` (Anchor 1.1.2, Solana CLI 3.1.10) and deployed with `solana program deploy`.

## Programs

| Program | Program ID | Deploy signature |
|---|---|---|
| `governance` | [`8Z7an6h8k8dbq7Bxzxyh4HseyaNgTHpVzSwvcq5H9pTV`](https://explorer.solana.com/address/8Z7an6h8k8dbq7Bxzxyh4HseyaNgTHpVzSwvcq5H9pTV?cluster=devnet) | `2gvZBEUBGWxLrfScr658utjcsUSUG2jtN9sPsXCMzmKUGiWJZ4aVyk99i5t21ADLySo4o3iiSWKnWJcyMgZrYAPT` |
| `fund` | [`HV75Ur4qgfHHTNwbJdpiTMkq2sA7YoynBXFtzpsZzYT5`](https://explorer.solana.com/address/HV75Ur4qgfHHTNwbJdpiTMkq2sA7YoynBXFtzpsZzYT5?cluster=devnet) | `21uyXmqtfPoWMAGwwty5G5UhoCdkrUMyX5LxrUyC9FuCyzD7EbtppsN4KRtkS1AyT9hq8LgmeyFqs4uiGcXEWeGU` |
| `compliance_hook` | [`A2R9A9xmCpicyUAYixJZr63oBPmKSR95CxxLoSPQfqtM`](https://explorer.solana.com/address/A2R9A9xmCpicyUAYixJZr63oBPmKSR95CxxLoSPQfqtM?cluster=devnet) | `5SUjW2JaG8gSXu53cHg7YGfL4WBAjoBDm2hLBxBHyn1SFkUNrdtXiXFswVeVLRwfuFnXdUikQmgsic74KWZgnkQD` |
| `dvp_settlement` | [`418DJ7wBLXAWBGA81vGLxEJTMxZMNMqrmj6t3sErck6w`](https://explorer.solana.com/address/418DJ7wBLXAWBGA81vGLxEJTMxZMNMqrmj6t3sErck6w?cluster=devnet) | `D14zTe2cCw6C7XjKamx4eDs9ZDpSUAuxT93r6YL9uFWujgzTNa4Haq7YRdNaMPJajwia13nhbcnFrBMfS4dapkX` |
| `collateral` | [`2RGrj5Jp8PGFVNRFNbJJtZVAph9mBYEubX6QMgqup6Yi`](https://explorer.solana.com/address/2RGrj5Jp8PGFVNRFNbJJtZVAph9mBYEubX6QMgqup6Yi?cluster=devnet) | `4mYR81wXXDBLUPVccYBJHFgYG4xPTtCU872HnyKPch7qy7bSysKGo56dPWovCgZ2Tub38KvSk1yzjBkUig3m832q` |

The upgrade authority of all five is the deploy wallet `ANv8ZTvmR4dRVs4TCCULwLF8HT6zkUNsCLY6gMMUhRko`,
which was also the issuer in every run. Token-2022 is devnet's own deployment.

## End-to-end runs

The full output of each run is in [docs/devnet-runs/](../../docs/devnet-runs/), and every
signature can be looked up on the explorer with `?cluster=devnet`.

| Run | Command | Result | Fund mint |
|---|---|---|---|
| F001 + F002 | `cargo run -p devnet-e2e` | 34 of 34 steps matched | `9ye28Fj8Zhz8TgwGhocYckzrUfGCSoyWngKPPnKV9D1N` |
| F003 | `cargo run -p devnet-e2e -- --feature f003` | 31 of 31 steps matched | `4eWHMKzNZcNHYVjr8sKohPsPUMd3rL8WCqrjmjKJWgjk` (cash mint `4KF91Bodan4m2pEptDf6HTEuQe2Vkyvbc9nU46TELbsv`) |
| F004 | `cargo run -p devnet-e2e -- --feature f004` | 37 of 37 steps matched | `2wPfNNTfswMhv3D6j3F7K4Y1f6VuWKqmUvFF9bhToXEV` (agreement `HKUAtoy3VoBBgRiqfrB5nhmg5F38F4vEcJ9QHmbgnfvZ`) |

The F004 run's pledge was prepared on G's durable nonce and signed 130 s later, past any recent
blockhash. It met the $1M call with no balance changing, and a send one unit over the lock was
refused by the token with `PledgedUnitsLocked`.

The same day, the LiteSVM suite (`cargo test -p tests`) passed 375 tests, with 0 failures and 6
ignored release-only tests.

## Still open

- **Upgrade authority handover.** Moving each program's upgrade authority to the issuer's Squads
  vault with a time lock (F001 T079, Constitution VIII) hasn't been done. Until it is, the deploy
  wallet alone can upgrade the programs.
- **Next-day runs.** F002's next-day publish (`devnet-e2e --continue 9ye28Fj8…`) and F003's
  next-day redemption settlement (`devnet-e2e --feature f003 --continue 4eWHMKzN…`) need a later
  UTC day.
