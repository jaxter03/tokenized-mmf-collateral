// DEMO ONLY. Runs the whole story against the local validator and checks each step.
import assert from "node:assert/strict";
import * as chain from "./chain.mjs";

const step = (msg) => console.log(`✓ ${msg}`);
const expectFail = async (fn, re) => {
  await assert.rejects(fn, (e) => re.test(String(e?.message ?? e)));
};

let s = await chain.seed();
assert.equal(s.alice.sharesOwned, 10_000_000);
assert.equal(s.alice.sharesFree, 10_000_000);
assert.ok(s.alice.lastAccrualTs <= s.chainTime - 30 * 86_400 + 5, "accrual backdated 30 days");
step("seed: fund launched, Alice approved and holds 10,000,000 shares bought 30 days ago");

await expectFail(() => chain.issueMarginCall(), /No shortfall/);
step("no margin call while Tesla covers the loan");

chain.setTeslaPrice(190);
s = await chain.readState();
assert.equal(s.bank.shortfall, 6_500_000 - 30_000 * 190);
await chain.issueMarginCall();
s = await chain.readState();
assert.equal(s.call.status, "open");
assert.equal(s.call.shortfall, 800_000);
step("Tesla at $190: bank records an $800,000 margin call on-chain");

await expectFail(() => chain.pledge(1_000), /PledgeTooSmall|worth less/);
step("a pledge smaller than the shortfall is rejected");

await chain.pledge();
s = await chain.readState();
assert.equal(s.call.status, "pledged");
assert.equal(s.alice.sharesFree, 9_200_000);
assert.equal(s.alice.sharesOwned, 10_000_000, "pledged shares still owned (and earning)");
assert.equal(s.bank.shortfall, 0);
assert.equal(s.bank.mmfValue, 800_000);
assert.equal(s.bank.totalCollateral, 6_500_000);
step("Alice pledges 800,000 shares; the bank is covered; she still owns all 10,000,000");

await expectFail(() => chain.release(), /doesn't cover/);
step("bank keeps the pledge while Tesla is still down");

chain.setTeslaPrice(262);
await chain.release();
s = await chain.readState();
assert.equal(s.call.status, "released");
assert.equal(s.alice.sharesFree, 10_000_000);
step("Tesla back to $262: bank releases; Alice has all 10,000,000 shares back");

console.log("\nAll steps passed.");
