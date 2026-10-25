// Contract test against a real Zirtola website (local or staging), exercising
// the same API the launcher uses. Sign-in is interactive: the script prints an
// authorize URL; open it in a browser where you are signed in to the site and
// click "Sign in to the launcher".
//
//   SITE=http://localhost:3000 \
//   PUBLISH_TOKEN=... (optional: uploads the mock's signed demo builds first) \
//   REDEEM_KEY=XXXX-XXXX-XXXX-XXXX (optional) \
//   node scripts/site-contract.mjs
//
// Never point PUBLISH_TOKEN at production unless you mean to publish builds there.
import crypto from "node:crypto";
import http from "node:http";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const SITE = (process.env.SITE || "http://localhost:3000").replace(/\/$/, "");
const API = `${SITE}/api/launcher`;
const b64url = (b) => Buffer.from(b).toString("base64url");
let failures = 0;
const ok = (cond, msg, extra = "") => {
  console.log(`${cond ? "PASS" : "FAIL"}  ${msg}${!cond && extra ? `  (${extra})` : ""}`);
  if (!cond) failures++;
};
async function call(path, opts = {}) {
  const r = await fetch(path.startsWith("http") ? path : API + path, opts);
  const text = await r.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: r.status, body };
}
const authed = (t, extra = {}) => ({ ...extra, headers: { Authorization: `Bearer ${t}`, "Content-Type": "application/json", ...(extra.headers || {}) } });
const form = (o) => ({ method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: new URLSearchParams(o) });

const cfg = await call("/v1/config");
ok(cfg.status === 200 && cfg.body.client_id === "zirtola-launcher", "GET /v1/config", JSON.stringify(cfg.body).slice(0, 200));
const keys = await call("/v1/keys");
ok(Array.isArray(keys.body.keys), `GET /v1/keys (${keys.body.keys?.length ?? 0} keys)`);

// Optional: publish the mock's signed demo builds through the admin API.
if (process.env.PUBLISH_TOKEN) {
  for (const [game, version, live] of [["dirt-simulator", "1.0.0", true], ["dirt-simulator", "1.0.1", false], ["alaskan-bush-simulator", "1.0.0", true]]) {
    const file = join(ROOT, "mock-server", ".data", "manifests", game, `${version}.json`);
    if (!existsSync(file)) continue;
    const r = await call("/v1/admin/builds", {
      method: "POST",
      headers: { Authorization: `Bearer ${process.env.PUBLISH_TOKEN}`, "Content-Type": "application/json" },
      body: JSON.stringify({ signed_manifest: JSON.parse(readFileSync(file, "utf8")), make_live: live, notes: `Demo build ${version}` }),
    });
    ok(r.status === 201 || (r.status === 400 && /already published/.test(r.body.message)), `publish ${game} ${version}`, JSON.stringify(r.body));
  }
  const badTok = await call("/v1/admin/builds", { method: "POST", headers: { Authorization: "Bearer nope", "Content-Type": "application/json" }, body: "{}" });
  ok(badTok.status === 401, "publish with a wrong token is refused");
}

const cat = await call("/v1/catalog");
ok(cat.status === 200 && Array.isArray(cat.body.games), `GET /v1/catalog: ${cat.body.games?.map((g) => `${g.id}@${g.latest_version}`).join(", ")}`);

// ---- interactive PKCE sign-in
const verifier = b64url(crypto.randomBytes(48));
const challenge = b64url(crypto.createHash("sha256").update(verifier).digest());
const state = b64url(crypto.randomBytes(16));
const { code, redirect } = await new Promise((resolve, reject) => {
  const srv = http.createServer((req, res) => {
    const u = new URL(req.url, "http://127.0.0.1");
    if (u.pathname !== "/callback") return res.writeHead(404).end();
    res.writeHead(200, { "Content-Type": "text/plain" }).end("Signed in. You can close this tab.");
    srv.close();
    if (u.searchParams.get("state") !== state) reject(new Error("state mismatch"));
    else if (u.searchParams.get("error")) reject(new Error(u.searchParams.get("error")));
    else resolve({ code: u.searchParams.get("code"), redirect: `http://127.0.0.1:${srv.address()?.port ?? port}/callback` });
  });
  let port;
  srv.listen(0, "127.0.0.1", () => {
    port = srv.address().port;
    const a = new URL(cfg.body.authorization_endpoint);
    Object.entries({ response_type: "code", client_id: cfg.body.client_id, redirect_uri: `http://127.0.0.1:${port}/callback`, scope: cfg.body.scopes.join(" "), state, code_challenge: challenge, code_challenge_method: "S256" })
      .forEach(([k, v]) => a.searchParams.set(k, v));
    console.log(`\nOpen this URL in a browser signed in to ${SITE}:\n\n${a}\n`);
    console.log(`AUTHORIZE_URL=${a}`);
  });
  setTimeout(() => reject(new Error("timed out waiting for sign-in")), 10 * 60_000);
}).catch((e) => {
  console.error(e.message);
  process.exit(1);
});

const wrong = await call(cfg.body.token_endpoint, form({ grant_type: "authorization_code", code, redirect_uri: redirect, client_id: "zirtola-launcher", code_verifier: b64url(crypto.randomBytes(48)) }));
ok(wrong.status === 400, "wrong PKCE verifier is rejected (and burns the code)");
console.log("\nOpen the URL again and approve once more (the first code was burned on purpose).");
const second = await new Promise((resolve) => {
  const srv = http.createServer((req, res) => {
    const u = new URL(req.url, "http://127.0.0.1");
    if (u.pathname !== "/callback") return res.writeHead(404).end();
    res.writeHead(200, { "Content-Type": "text/plain" }).end("Signed in. You can close this tab.");
    srv.close();
    resolve({ code: u.searchParams.get("code"), redirect: `http://127.0.0.1:${port2}/callback` });
  });
  let port2;
  srv.listen(0, "127.0.0.1", () => {
    port2 = srv.address().port;
    const a = new URL(cfg.body.authorization_endpoint);
    Object.entries({ response_type: "code", client_id: cfg.body.client_id, redirect_uri: `http://127.0.0.1:${port2}/callback`, scope: cfg.body.scopes.join(" "), state, code_challenge: challenge, code_challenge_method: "S256" })
      .forEach(([k, v]) => a.searchParams.set(k, v));
    console.log(`AUTHORIZE_URL=${a}`);
  });
});
const tok = await call(cfg.body.token_endpoint, form({ grant_type: "authorization_code", code: second.code, redirect_uri: second.redirect, client_id: "zirtola-launcher", code_verifier: verifier }));
ok(tok.status === 200 && tok.body.access_token && tok.body.refresh_token, "token exchange with PKCE", JSON.stringify(tok.body));
const at = tok.body.access_token;

const me = await call("/v1/me", authed(at));
ok(me.status === 200 && me.body.id, `GET /v1/me: ${me.body.display_name}`);
const unauth = await call("/v1/entitlements");
ok(unauth.status === 401, "entitlements need a token");

if (process.env.REDEEM_KEY) {
  const red = await call("/v1/keys/redeem", authed(at, { method: "POST", body: JSON.stringify({ key: process.env.REDEEM_KEY }) }));
  ok(red.status === 200, `redeem key -> ${red.body.game_title ?? red.body.message}`, JSON.stringify(red.body));
  const again = await call("/v1/keys/redeem", authed(at, { method: "POST", body: JSON.stringify({ key: process.env.REDEEM_KEY }) }));
  ok(again.status === 409, "redeeming the same key twice is refused");
}
const bad = await call("/v1/keys/redeem", authed(at, { method: "POST", body: JSON.stringify({ key: "NOPE-NOPE-NOPE-NOPE" }) }));
ok(bad.status === 404 && bad.body.error === "invalid_key", "invalid key is rejected");

const ents = await call("/v1/entitlements", authed(at));
ok(ents.status === 200, `GET /v1/entitlements: ${ents.body.entitlements?.map((e) => `${e.game_id}(${e.kind})`).join(", ") || "none"}`);
const owned = ents.body.entitlements?.[0]?.game_id;
if (owned) {
  const latest = await call(`/v1/games/${owned}/latest?platform=win64&branch=live`, authed(at));
  ok(latest.status === 200 || latest.status === 404, `latest build for ${owned}: ${latest.body.version ?? latest.body.error}`);
  const man = await call(`/v1/games/${owned}/manifest?platform=win64&branch=live`, authed(at));
  if (man.status === 200) {
    const bytes = Buffer.from(man.body.signed_manifest.manifest, "base64url");
    const k = keys.body.keys.find((x) => x.kid === man.body.signed_manifest.kid);
    const pub = crypto.createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: k.x }, format: "jwk" });
    ok(crypto.verify(null, bytes, pub, Buffer.from(man.body.signed_manifest.signature, "base64url")), "manifest signature verifies with /v1/keys");
    const m = JSON.parse(bytes);
    const c = m.files[0].chunks[0];
    const url = man.body.chunk_url_template.replace("{prefix}", c.hash.slice(0, 2)).replace("{hash}", c.hash);
    const data = Buffer.from(await (await fetch(url)).arrayBuffer());
    ok(crypto.createHash("sha256").update(data).digest("hex") === c.hash, "chunk from the CDN matches its hash");
  } else {
    ok(false, `manifest for ${owned}`, JSON.stringify(man.body));
  }
  const device = b64url(crypto.randomBytes(24));
  const lt = await call("/v1/launch-tokens", authed(at, { method: "POST", body: JSON.stringify({ game_id: owned, device_id: device, device_name: "Contract test PC" }) }));
  ok(lt.status === 200 && lt.body.token, "launch token issued (device recorded)", JSON.stringify(lt.body));
  if (lt.status === 200) {
    const v = await call("/v1/launch-tokens/verify", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ token: lt.body.token }) });
    ok(v.body.valid === true, "online launch-token verification");
    const exe = join(ROOT, "target", "release", process.platform === "win32" ? "license-check.exe" : "license-check");
    if (existsSync(exe)) {
      const out = execFileSync(exe, ["--no-wait"], { env: { ...process.env, ZIRTOLA_SESSION_TOKEN: lt.body.token, ZIRTOLA_GAME_ID: owned, ZIRTOLA_API_BASE: API } }).toString();
      ok(out.includes("LICENSE OK"), "reference game accepts the site's launch token", out.trim().split("\n").pop());
    }
  }
  const notMine = cat.body.games.find((g) => !ents.body.entitlements.some((e) => e.game_id === g.id));
  if (notMine) {
    const r = await call(`/v1/games/${notMine.id}/manifest?platform=win64&branch=live`, authed(at));
    ok(r.status === 403, `manifest for unowned ${notMine.id} is refused`);
  }
}

const r1 = await call(cfg.body.token_endpoint, form({ grant_type: "refresh_token", refresh_token: tok.body.refresh_token, client_id: "zirtola-launcher" }));
ok(r1.status === 200 && r1.body.refresh_token !== tok.body.refresh_token, "refresh rotates the refresh token");
const reuse = await call(cfg.body.token_endpoint, form({ grant_type: "refresh_token", refresh_token: tok.body.refresh_token, client_id: "zirtola-launcher" }));
ok(reuse.status === 400, "reusing a rotated refresh token fails");
const r2 = await call(cfg.body.token_endpoint, form({ grant_type: "refresh_token", refresh_token: r1.body.refresh_token, client_id: "zirtola-launcher" }));
ok(r2.status === 400, "...and revokes the whole family");

console.log(failures ? `\n${failures} check(s) failed` : "\nall checks passed");
process.exit(failures ? 1 : 0);
