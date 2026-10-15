Design and build a single-page demo UI for a live presentation: **"Tokenized MMF as collateral"**.
It runs on a laptop shared to a projector or a video call, in front of bankers and fund people.
It has to look polished and institutional, read clearly from the back of a room, and tell one
story in about 5 minutes, guided by an on-screen assistant called **Tony**.

## The story

- **Alice** is an institutional investor.
- **The fund:** Alice holds 10,000,000 shares of a tokenized money market fund, "Demo Treasury
  MMF". It pays 5.00% a year at a fixed NAV of $1.00, and its shares live on the Solana
  blockchain.
- **The loan:** separately, Alice has a $5,000,000 loan at a bank, backed by 30,000 Tesla shares.
  The bank requires collateral of at least 130% of the loan, which is $6,500,000.

On stage, the presenter clicks through five steps:

1. **Tesla falls** from $250 to $190. Collateral drops to $5,700,000, which is $800,000 short.
2. **The bank issues a margin call** for $800,000. It's raised on the bank's own system, and the
   bank also records it on-chain.
3. **Alice pledges 800,000 MMF shares** to the bank in one transaction, settled in about a second.
   The bank is covered again, and Alice didn't have to sell Tesla at the bottom.
   **The key message: the pledged shares keep earning interest for Alice, not the bank.**
4. **Tesla recovers** to $262. Tesla alone covers the loan again.
5. **The bank releases the pledge.** All 10,000,000 shares are back with Alice, with the interest
   they earned the whole time.

## Tony, the demo assistant

Tony is a friendly, confident guide character: a small avatar plus a speech panel. Tony narrates
each stage, says what just happened, explains why it matters in plain language, and points at the
one button to press next. Tony should feel like part of the product, not a chatbot.

**Opening scene.** The setup happened before the demo, so Tony presents it as a short animated
timeline of three cards, each with an "on-chain ↗" link:

> "Hi, I'm Tony, your demo assistant. Before we started, I set the stage:"
> 1. **Fund launched:** the issuer launched *Demo Treasury MMF* at 5.00%.
> 2. **Alice approved:** compliance approved Alice (KYC).
> 3. **Alice bought:** 10,000,000 shares for $10,000,000 on 15 Sep, and has earned **$41,096**
>    since, still ticking.
>
> "Alice also has a $5M loan at a bank, backed by 30,000 Tesla shares. Let's see what happens
> when Tesla falls." **[Start the demo]**

**Tony's lines per stage.** Rewrite them freely, but keep them short:

- **Ready:** "Everything's healthy: Tesla covers the loan at 150%. Let's drop Tesla." Next:
  *Tesla falls to $190*.
- **Below margin:** "Tesla is down 24%. The bank's collateral is $800,000 short of its 130% rule."
  Next: *Issue margin call*.
- **Margin call open:** "The bank has called for $800,000. Normally Alice would sell Tesla at the
  bottom or wire cash and lose its yield. Instead, Alice can pledge fund shares." Next: *Pledge
  800,000 MMF shares*.
- **Pledged:** "Done in one transaction. The bank is covered. Watch the interest: it's still
  ticking. Pledged shares earn for Alice, not the bank." Next: *Tesla recovers to $262*.
- **Tesla recovered:** "Tesla alone covers the loan again, so the bank doesn't need the extra
  collateral." Next: *Release MMF pledge*.
- **Released:** "All 10,000,000 shares are back with Alice, plus the interest they earned while
  pledged. That's the whole cycle." Offer *Run it again* (reset).

## Layout

- **Header:** title, a small "Demo · local Solana validator" tag, and a Reset button.
- **Tony:** a persistent panel, either a left rail or a docked card, always visible.
- **A 5-step progress tracker:** done, current and upcoming states.
- **Two main panels, side by side:**
  - **"Bank · Alice's loan"**, labelled as the bank's own system (simulated, off-platform):
    - the Tesla price, big, with its % change from $250, a price slider (range $120–$320) and
      two quick buttons
    - a collateral table: loan, Tesla value, MMF pledge value (only when pledged), total
      against required
    - a coverage gauge or bar with a clear 130% line, with Tesla and MMF shown as separate
      segments
    - a status badge: Healthy / Below margin / Margin call open / Covered by MMF pledge / Tesla
      covers loan again
    - action buttons: Issue margin call, Release MMF pledge
  - **"Platform · Alice's MMF"**, labelled as on-chain:
    - **the hero element:** interest earned, ticking live at 4 decimals (the last digits can be
      dimmer), with "5.00% a year · $1,369.86 a day · $0.0159 a second · on all shares, pledged
      or not"
    - shares owned against shares pledged, and a free/pledged split bar
    - a margin-call alert, and once pledged, a pledge card showing the reference
      (e.g. MC-2026-10-15-001), the interest earned on the pledged shares since pledging
      (ticking), and "$109.59 a day to Alice"
    - action button: Pledge N MMF shares to the bank
- **On-chain activity feed:** time, an actor chip (Issuer / Alice / Bank), a description, and the
  transaction signature shortened and linked to the explorer. Briefly highlight new rows.

## Motion

Use motion that helps the story:

- **Pledge:** animate shares flowing from Alice's panel to the bank's panel. This is the money
  shot.
- **Tesla drop:** the gauge goes red and the badge pulses.
- **Release:** the shares flow back.
- **Interest:** the counter ticks smoothly, never jumps.
- Spinners on buttons while a transaction is in flight (1–2 s).

Only the next action is visually enabled. Everything else is clearly disabled, so the presenter
can't click the wrong thing.

## Visual direction

Institutional fintech: think a premium custody or treasury dashboard. Dark theme preferred,
optimized for projectors: high contrast, large numbers, generous spacing. Suggested colors, though
you can choose your own: green for healthy and interest, red for shortfall, amber for the bank,
blue for Alice, violet for pledged shares. Design for 1440×900 first. It must also work at 1280
wide, and at least not break on mobile.

## Technical contract (important)

The UI must plug into an existing backend without changes.

- **One self-contained `index.html`**, with inline CSS and JS and no build step. External
  libraries only from cdnjs or jsDelivr with pinned versions. No wallet connection: the server
  signs everything.
- **Same origin.** It's served by the backend, so call relative URLs.

**`GET /api/state`** returns:

```json
{
  "rpcUrl": "http://127.0.0.1:8899",
  "programId": "6eNynvrVyhyWN6d4dTDiRWps42YPhdHpBM56iXefBkEj",
  "chainTime": 1792000000, "serverTime": 1792000000,
  "fund": { "address": "…", "name": "Demo Treasury MMF", "rateBps": 500, "nav": 1, "shareMint": "…" },
  "alice": { "wallet": "…", "kycApproved": true, "sharesOwned": 10000000, "sharesFree": 10000000,
             "accrued": 0, "lastAccrualTs": 1789408000 },
  "bank": { "wallet": "…", "loan": 5000000, "teslaShares": 30000, "teslaPrice": 250, "teslaStart": 250,
            "teslaValue": 7500000, "mmfValue": 0, "totalCollateral": 7500000, "required": 6500000,
            "maintenancePct": 130, "coveragePct": 150, "shortfall": 0, "teslaAloneCovers": true },
  "call": null,
  "activity": [ { "actor": "Alice", "label": "Bought 10,000,000 MMF shares for $10,000,000",
                  "sig": "5Xy…", "ts": 1792000000 } ]
}
```

`call` is `null` or
`{ "address", "reference", "status": "open" | "pledged" | "released", "shortfall",
"pledgedShares", "issuedTs", "pledgedTs", "releasedTs" }`. Amounts are dollars or whole shares,
and timestamps are unix seconds. `activity` is newest first. The opening timeline uses its
Issuer/Alice entries, and the purchase date is `alice.lastAccrualTs`.

**Actions.** Each one is a `POST` with a JSON body. It returns the new state, or HTTP 400 with
`{ "error": "message" }`; show errors as a toast.

| Endpoint | Body |
|---|---|
| `/api/tesla` | `{ "price": 190 }` |
| `/api/margin-call` | `{}` |
| `/api/pledge` | `{}` (pledges exactly the shortfall) |
| `/api/release` | `{}` |
| `/api/reset` | `{}` (takes about 5 s: show "Tony is setting the stage…") |

**Live interest.** Compute it in the browser at 60 fps:
`accrued + sharesOwned × nav × (rateBps / 10000) × (now − lastAccrualTs) / 31,536,000`, where
`now = Date.now()/1000 + (chainTime − browserNowAtFirstFetch)`.
For the pledged shares, use the same formula with `pledgedShares` from `pledgedTs` to `now`, or
to `releasedTs` once released.

**Bank book while dragging the slider.** Recompute it locally so it feels instant:
- `teslaValue = teslaShares × price`
- `total = teslaValue + mmfValue`
- `shortfall = max(0, required − total)`
- `teslaAloneCovers = teslaValue ≥ required`

Send `/api/tesla` on release of the slider.

**Stage.** Derive it from the state:
- `call.status == "open"` → margin call open
- `"pledged"` → Tesla recovered if `teslaAloneCovers`, otherwise pledged
- `"released"` with no shortfall → done
- otherwise → below margin if `shortfall > 0`, else ready

Show the opening scene on first load, until "Start the demo" is clicked.

**Polling.** Poll `/api/state` every 3 s, but not while a request is in flight or the slider is
being dragged.

**Explorer links:**
`https://explorer.solana.com/tx/{sig}?cluster=custom&customUrl={encodeURIComponent(rpcUrl)}`
(use `/address/{id}` for accounts).

**Mock mode.** If `/api/state` can't be reached, for example in your preview, fall back to a
built-in mock that simulates the same state and actions, with fake signatures, so the whole flow
can be clicked through. Show a small "mock data" tag when it's active.
