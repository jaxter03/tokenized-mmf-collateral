// DEMO ONLY. Serves the UI and a small JSON API over chain.mjs. One action at a time.
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as chain from "./chain.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const PORT = Number(process.env.PORT ?? 5173);

let queue = Promise.resolve();
const serial = (fn) => {
  const run = queue.then(fn, fn);
  queue = run.catch(() => {});
  return run;
};

async function ensureSeeded() {
  if (chain.init()) {
    try {
      await chain.readState();
      return;
    } catch {
      console.log("Saved demo state isn't on this validator; seeding again.");
    }
  }
  console.log("Seeding: launch fund, approve Alice, Alice buys…");
  await chain.seed();
}

function readBody(req) {
  return new Promise((resolve) => {
    let data = "";
    req.on("data", (c) => (data += c));
    req.on("end", () => {
      try {
        resolve(data ? JSON.parse(data) : {});
      } catch {
        resolve({});
      }
    });
  });
}

function errorMessage(e) {
  const anchorMsg = e?.error?.errorMessage ?? e?.errorMessage;
  if (anchorMsg) return anchorMsg;
  const logs = e?.logs ?? e?.transactionLogs;
  const hit = logs?.find?.((l) => l.includes("Error Message:"));
  if (hit) return hit.split("Error Message:")[1].trim();
  return String(e?.message ?? e);
}

const actions = {
  "/api/tesla": (b) => chain.setTeslaPrice(Number(b.price)),
  "/api/margin-call": () => chain.issueMarginCall(),
  "/api/pledge": (b) => chain.pledge(b.shares ? Number(b.shares) : undefined),
  "/api/release": () => chain.release(),
  "/api/reset": () => chain.seed(),
};

const server = http.createServer(async (req, res) => {
  const send = (code, body, type = "application/json") => {
    res.writeHead(code, { "content-type": type, "cache-control": "no-store" });
    res.end(type === "application/json" ? JSON.stringify(body) : body);
  };
  try {
    if (req.method === "GET" && (req.url === "/" || req.url === "/index.html")) {
      return send(200, fs.readFileSync(path.join(here, "public/index.html")), "text/html; charset=utf-8");
    }
    if (req.method === "GET" && req.url === "/api/state") {
      return send(200, await serial(() => chain.readState()));
    }
    if (req.method === "POST" && actions[req.url]) {
      const body = await readBody(req);
      await serial(() => actions[req.url](body));
      return send(200, await serial(() => chain.readState()));
    }
    send(404, { error: "Not found" });
  } catch (e) {
    console.error(e);
    send(400, { error: errorMessage(e) });
  }
});

await ensureSeeded();
server.listen(PORT, () => console.log(`MMF demo UI: http://localhost:${PORT}`));
