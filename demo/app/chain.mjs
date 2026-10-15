// DEMO ONLY. Holds the three demo wallets (issuer, Alice, bank), seeds the starting state and
// sends every on-chain step. The bank's Tesla-backed loan is simulated here, off-chain, because
// in the story it lives on the bank's own systems; only the margin call, the MMF pledge and its
// release touch the chain.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import anchor from "@anchor-lang/core";
import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  getAccount,
  getOrCreateAssociatedTokenAccount,
  mintTo,
} from "@solana/spl-token";

const { AnchorProvider, Program, Wallet, BN } = anchor;
const here = path.dirname(fileURLToPath(import.meta.url));
const IDL = JSON.parse(fs.readFileSync(path.join(here, "../target/idl/mmf_demo.json"), "utf8"));
const STATE_FILE = path.join(here, ".demo-state.json");

export const RPC_URL = process.env.RPC_URL ?? "http://127.0.0.1:8899";
export const connection = new Connection(RPC_URL, "confirmed");
export const PROGRAM_ID = new PublicKey(IDL.address);

const ONE = 1_000_000; // 6 decimals for shares and cash
const DAY = 86_400;

// The story's numbers.
export const SCENARIO = {
  fundName: "Demo Treasury MMF",
  rateBps: 500, // 5.00% a year
  aliceCash: 10_000_000, // $10M subscribed
  backdateDays: 30, // bought 30 days ago
  loan: 5_000_000, // Alice's loan at the bank
  teslaShares: 30_000, // collateral at the bank
  teslaStart: 250,
  maintenancePct: 130, // collateral must stay ≥ 130% of the loan
};

// ---------------------------------------------------------------------------------------------
// State file: wallets, addresses, the simulated bank book and an activity log.

let state = null;

function save() {
  const out = {
    ...state,
    issuer: Array.from(state.issuer.secretKey),
    alice: Array.from(state.alice.secretKey),
    bank: Array.from(state.bank.secretKey),
  };
  fs.writeFileSync(STATE_FILE, JSON.stringify(out, null, 2));
}

function load() {
  if (!fs.existsSync(STATE_FILE)) return null;
  const raw = JSON.parse(fs.readFileSync(STATE_FILE, "utf8"));
  return {
    ...raw,
    issuer: Keypair.fromSecretKey(Uint8Array.from(raw.issuer)),
    alice: Keypair.fromSecretKey(Uint8Array.from(raw.alice)),
    bank: Keypair.fromSecretKey(Uint8Array.from(raw.bank)),
  };
}

function programFor(kp) {
  const provider = new AnchorProvider(connection, new Wallet(kp), { commitment: "confirmed" });
  return new Program(IDL, provider);
}

const pda = (...seeds) => PublicKey.findProgramAddressSync(seeds, PROGRAM_ID)[0];
const u64le = (n) => new BN(n).toArrayLike(Buffer, "le", 8);

function addresses() {
  const fund = pda(Buffer.from("fund"), state.issuer.publicKey.toBuffer());
  return {
    fund,
    shareMint: pda(Buffer.from("shares"), fund.toBuffer()),
    treasury: pda(Buffer.from("treasury"), fund.toBuffer()),
    position: pda(Buffer.from("position"), fund.toBuffer(), state.alice.publicKey.toBuffer()),
  };
}

function callAddress(callId) {
  const { fund } = addresses();
  return pda(Buffer.from("call"), fund.toBuffer(), state.alice.publicKey.toBuffer(), u64le(callId));
}

function log(actor, label, sig) {
  state.activity.unshift({ actor, label, sig, ts: Math.floor(Date.now() / 1000) });
  save();
}

async function airdrop(pubkey, sol) {
  const sig = await connection.requestAirdrop(pubkey, sol * LAMPORTS_PER_SOL);
  const bh = await connection.getLatestBlockhash();
  await connection.confirmTransaction({ signature: sig, ...bh }, "confirmed");
}

// ---------------------------------------------------------------------------------------------
// Seed: launch the fund, approve Alice, Alice buys. Runs before the demo, not on stage.

export async function seed() {
  state = {
    issuer: Keypair.generate(),
    alice: Keypair.generate(),
    bank: Keypair.generate(),
    teslaPrice: SCENARIO.teslaStart,
    nextCallId: 1,
    activeCallId: null,
    activity: [],
  };
  await Promise.all([state.issuer, state.alice, state.bank].map((k) => airdrop(k.publicKey, 10)));

  const { issuer, alice } = state;
  const usdc = await createMint(connection, issuer, issuer.publicKey, null, 6);
  state.usdcMint = usdc.toBase58();
  const aliceUsdc = await getOrCreateAssociatedTokenAccount(connection, alice, usdc, alice.publicKey);
  await mintTo(connection, issuer, usdc, aliceUsdc.address, issuer, SCENARIO.aliceCash * ONE);

  const a = addresses();
  const issuerProg = programFor(issuer);
  let sig = await issuerProg.methods
    .launchFund(SCENARIO.fundName, SCENARIO.rateBps)
    .accountsPartial({
      issuer: issuer.publicKey,
      fund: a.fund,
      shareMint: a.shareMint,
      cashMint: usdc,
      treasury: a.treasury,
      tokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
  log("Issuer", `Launched ${SCENARIO.fundName} at ${(SCENARIO.rateBps / 100).toFixed(2)}%`, sig);

  sig = await issuerProg.methods
    .onboardInvestor()
    .accountsPartial({
      issuer: issuer.publicKey,
      fund: a.fund,
      investor: alice.publicKey,
      position: a.position,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
  log("Issuer", "Approved Alice (KYC)", sig);

  const aliceShares = await getOrCreateAssociatedTokenAccount(connection, alice, a.shareMint, alice.publicKey);
  state.aliceShares = aliceShares.address.toBase58();
  state.aliceUsdc = aliceUsdc.address.toBase58();
  sig = await programFor(alice)
    .methods.subscribe(new BN(SCENARIO.aliceCash).mul(new BN(ONE)), new BN(SCENARIO.backdateDays * DAY))
    .accountsPartial({
      investor: alice.publicKey,
      fund: a.fund,
      position: a.position,
      investorCash: aliceUsdc.address,
      treasury: a.treasury,
      shareMint: a.shareMint,
      investorShares: aliceShares.address,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .rpc();
  log("Alice", `Bought ${fmtInt(SCENARIO.aliceCash)} MMF shares for $${fmtInt(SCENARIO.aliceCash)}`, sig);
  save();
  return readState();
}

export function init() {
  state = load();
  return state != null;
}

// ---------------------------------------------------------------------------------------------
// The bank's simulated book (off-chain in the story).

function bankBook(pledgedShares, nav) {
  const teslaValue = SCENARIO.teslaShares * state.teslaPrice;
  const mmfValue = (pledgedShares * nav) / ONE;
  const required = (SCENARIO.loan * SCENARIO.maintenancePct) / 100;
  const total = teslaValue + mmfValue;
  return {
    loan: SCENARIO.loan,
    teslaShares: SCENARIO.teslaShares,
    teslaPrice: state.teslaPrice,
    teslaStart: SCENARIO.teslaStart,
    teslaValue,
    mmfValue,
    totalCollateral: total,
    required,
    maintenancePct: SCENARIO.maintenancePct,
    coveragePct: (total / SCENARIO.loan) * 100,
    shortfall: Math.max(0, required - total),
    teslaAloneCovers: teslaValue >= required,
  };
}

// ---------------------------------------------------------------------------------------------
// Steps shown on stage.

export function setTeslaPrice(price) {
  if (!(price > 0 && price < 10_000)) throw new Error("Tesla price must be between $0 and $10,000");
  state.teslaPrice = Math.round(price * 100) / 100;
  save();
}

export async function issueMarginCall() {
  if (state.activeCallId != null) throw new Error("A margin call is already active");
  const s = await readState();
  const shortfall = Math.ceil(s.bank.shortfall);
  if (shortfall <= 0) throw new Error("No shortfall: Alice's collateral covers the loan");
  const callId = state.nextCallId;
  const reference = `MC-${new Date().toISOString().slice(0, 10)}-${String(callId).padStart(3, "0")}`;
  const sig = await programFor(state.bank)
    .methods.recordMarginCall(new BN(callId), new BN(shortfall).mul(new BN(ONE)), reference)
    .accountsPartial({
      bank: state.bank.publicKey,
      fund: addresses().fund,
      borrower: state.alice.publicKey,
      call: callAddress(callId),
      systemProgram: SystemProgram.programId,
    })
    .rpc();
  state.activeCallId = callId;
  state.nextCallId = callId + 1;
  log("Bank", `Margin call ${reference}: $${fmtInt(shortfall)} shortfall`, sig);
}

export async function pledge(sharesWhole) {
  if (state.activeCallId == null) throw new Error("There is no margin call to meet");
  const a = addresses();
  const call = callAddress(state.activeCallId);
  const prog = programFor(state.alice);
  const c = await prog.account.marginCall.fetch(call);
  const shares = sharesWhole ? new BN(sharesWhole).mul(new BN(ONE)) : c.shortfall; // NAV $1.00
  const sig = await prog.methods
    .pledge(shares)
    .accountsPartial({
      borrower: state.alice.publicKey,
      fund: a.fund,
      call,
      shareMint: a.shareMint,
      borrowerShares: new PublicKey(state.aliceShares),
      vault: pda(Buffer.from("vault"), call.toBuffer()),
      tokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
  log("Alice", `Pledged ${fmtInt(shares.toNumber() / ONE)} MMF shares to the bank (${c.reference})`, sig);
}

export async function release() {
  if (state.activeCallId == null) throw new Error("Nothing is pledged");
  const s = await readState();
  if (!s.bank.teslaAloneCovers) throw new Error("Tesla alone doesn't cover the loan yet; the bank keeps the pledge");
  const call = callAddress(state.activeCallId);
  const sig = await programFor(state.bank)
    .methods.release()
    .accountsPartial({
      bank: state.bank.publicKey,
      call,
      borrower: state.alice.publicKey,
      borrowerShares: new PublicKey(state.aliceShares),
      vault: pda(Buffer.from("vault"), call.toBuffer()),
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .rpc();
  log("Bank", `Released ${fmtInt(s.call.pledgedShares)} MMF shares back to Alice`, sig);
  state.lastCallId = state.activeCallId;
  state.activeCallId = null;
  save();
}

// ---------------------------------------------------------------------------------------------
// Read everything the UI shows.

export async function readState() {
  const a = addresses();
  const prog = programFor(state.alice);
  const [fund, position, sharesAcct, slot] = await Promise.all([
    prog.account.fund.fetch(a.fund),
    prog.account.position.fetch(a.position),
    getAccount(connection, new PublicKey(state.aliceShares)),
    connection.getSlot("confirmed"),
  ]);
  const chainTime = (await connection.getBlockTime(slot)) ?? Math.floor(Date.now() / 1000);

  let call = null;
  const callId = state.activeCallId ?? state.lastCallId;
  if (callId != null) {
    const addr = callAddress(callId);
    const c = await prog.account.marginCall.fetch(addr);
    call = {
      address: addr.toBase58(),
      reference: c.reference,
      status: Object.keys(c.status)[0], // open | pledged | released
      shortfall: c.shortfall.toNumber() / ONE,
      pledgedShares: c.pledgedShares.toNumber() / ONE,
      issuedTs: c.issuedTs.toNumber(),
      pledgedTs: c.pledgedTs.toNumber(),
      releasedTs: c.releasedTs.toNumber(),
    };
  }
  const nav = fund.nav.toNumber();
  const pledged = call?.status === "pledged" ? call.pledgedShares * ONE : 0;

  return {
    rpcUrl: RPC_URL,
    programId: PROGRAM_ID.toBase58(),
    chainTime,
    serverTime: Math.floor(Date.now() / 1000),
    fund: {
      address: a.fund.toBase58(),
      name: fund.name,
      rateBps: fund.rateBps,
      nav: nav / ONE,
      shareMint: a.shareMint.toBase58(),
    },
    alice: {
      wallet: state.alice.publicKey.toBase58(),
      kycApproved: position.kycApproved,
      sharesOwned: position.shares.toNumber() / ONE,
      sharesFree: Number(sharesAcct.amount) / ONE,
      accrued: position.accrued.toNumber() / ONE,
      lastAccrualTs: position.lastAccrualTs.toNumber(),
    },
    bank: { wallet: state.bank.publicKey.toBase58(), ...bankBook(pledged, nav) },
    call,
    activity: state.activity,
  };
}

function fmtInt(n) {
  return Math.round(n).toLocaleString("en-US");
}
