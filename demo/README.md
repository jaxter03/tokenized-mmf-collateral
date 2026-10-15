# Demo: tokenized MMF meets a margin call

**Demo only.** A small, separate Anchor program and UI that tell one story. It's kept apart from
the real programs in `programs/` and leaves out their compliance hook, governance and
Token-2022. It uses plain SPL Token and a fixed $1.00 NAV.

## The story

Alice has a $5M loan at a bank, backed by 30,000 Tesla shares, with collateral required to stay
at or above 130% of the loan. Separately, she holds 10,000,000 shares of a tokenized money market
fund paying 5.00%, bought 30 days ago.

1. **Tesla falls** to $190. Collateral is now $5.7M against $6.5M required, so the bank is
   $800,000 short.
2. **The bank issues a margin call.** It's raised on the bank's own system and recorded on-chain
   for the audit trail.
3. **Alice pledges 800,000 MMF shares** to the bank in one transaction. She doesn't have to sell
   Tesla at the bottom, and the pledged shares **keep earning interest for her**.
4. **Tesla recovers** to $262. Tesla alone covers the loan again.
5. **The bank releases the pledge.** All 10,000,000 shares are back with Alice, and they earned
   interest the whole time.

The fund launch, KYC approval and purchase happen before the demo, in the seed step. They're on
the activity feed with explorer links if anyone asks.

## Run it

```bash
demo/run.sh            # fresh local validator + program + seed + UI
open http://localhost:5173
```

Run `anchor build` in `demo/` first if `target/deploy/mmf_demo.so` is missing. The program ID
comes from `target/deploy/mmf_demo-keypair.json`; on a new machine, run `anchor keys sync`, then
rebuild.

The UI (`app/public/index.html`) was designed in Claude Design from
[claude-design-prompt.md](claude-design-prompt.md). Tony, the demo assistant, opens with the setup,
then narrates each step. **Press → (or PageDown, or a presentation clicker) to run the next step.**
If a "mock data" tag shows in the header, the page loaded before the server was ready: refresh
it. The earlier plain UI is still served at `http://localhost:5173/classic` as a fallback.

Use **Reset** in the UI to start over with a fresh fund. To check the whole story without
the UI, run it from the command line against the running validator:

```bash
cd demo/app && npm run e2e
```

Then restart `run.sh`, because the e2e replaces the UI's wallets.

## What's on-chain, what's simulated

| On-chain (`programs/mmf_demo`) | Simulated (in `app/chain.mjs`) |
|---|---|
| `launch_fund`, `onboard_investor`, `subscribe` (seed) | The bank's loan book and Tesla price |
| `record_margin_call` (bank) | When a call is due (collateral below 130%) |
| `pledge` (Alice): shares move to a vault the call controls | When the pledge can be released (Tesla alone ≥ 130%) |
| `release` (bank): shares return, vault closed | |

Interest accrues by the second from the position's last accrual time. Pledging doesn't change
the position, so pledged shares keep earning for Alice. The UI ticks the figure live from
on-chain values.

## Talk track (about 5 minutes)

- **Open.** "Alice holds a tokenized money market fund, earning 5%. Separately, she borrowed
  from a bank against her Tesla stock."
- **Tesla falls.** "Tesla drops 24%. The bank's collateral rule is broken: $800,000 short."
- **Margin call.** "The bank calls for margin. Normally Alice would sell Tesla at the bottom or
  wire cash and lose its yield."
- **Pledge.** "Instead she pledges $800,000 of fund shares. One transaction, settled in about a
  second, any time of day. The bank is covered." *Point at the interest:* "and it's still
  ticking. The pledged shares earn for Alice, not the bank."
- **Recover and release.** "Tesla comes back. The bank no longer needs the extra collateral and
  releases it. Alice has every share back, plus the interest it earned while pledged."
- **Real-world tie-in.** Tokenized MMF shares are already used as collateral. For example, J.P.
  Morgan's Tokenized Collateral Network moved BlackRock MMF shares as collateral to Barclays in
  2023, and tokenized MMFs such as BlackRock's BUIDL and Franklin Templeton's BENJI are accepted
  as collateral on trading venues.
