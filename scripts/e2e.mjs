// Headless end-to-end check of the API contract against the mock server:
// PKCE sign-in, refresh rotation + reuse detection, entitlements, manifest,
// chunk download + hash check, key redemption, launch token verified by the
// reference game (license-check). Starts its own mock server on a spare port.
//
//   npm run mock:seed   (once)
//   npm run e2e
import crypto from "node:crypto";
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const PORT = 18787;
const BASE = `http://127.0.0.1:${PORT}`;
const b64url = (b) => Buffer.from(b).toString("base64url");
let failures = 0;
const ok = (cond, msg) => {
  console.log(`${cond ? "PASS" : "FAIL"}  ${msg}`);
  if (!cond) failures++;
};

// Fresh state file so redeemed keys don't leak between runs (reuses the seeded builds).
const stateDir = mkdtempSync(join(tmpdir(), "zirtola-e2e-"));
if (!existsSync(join(ROOT, "mock-server", ".data", "manifests"))) {
  console.error("Run `npm run mock:seed` first.");
  process.exit(1);
}

const server = spawn(process.execPath, [join(ROOT, "mock-server", "server.mjs")], {
  env: { ...process.env, MOCK_PORT: String(PORT), MOCK_STATE_FILE: join(stateDir, "state.json"), MOCK_ACCESS_TTL: "60" },
  stdio: ["ignore", "pipe", "inherit"],
});
await new Promise((resolve) => server.stdout.on("data", (d) => String(d).includes("Mock Zirtola API") && resolve()));

async function json(path, opts = {}) {
  const r = await fetch(path.startsWith("http") ? path : BASE + path, opts);
  const text = await r.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: r.status, body, headers: r.headers };
}

async function signIn(accountId) {
  const verifier = b64url(crypto.randomBytes(48));
  const challenge = b64url(crypto.createHash("sha256").update(verifier).digest());
  const redirect = "http://127.0.0.1:50999/callback";
  const auth = new URL(`${BASE}/oauth/authorize`);
  Object.entries({ response_type: "code", client_id: "zirtola-launcher", redirect_uri: redirect, scope: "launcher offline_access", state: "s1", code_challenge: challenge, code_challenge_method: "S256" })
    .forEach(([k, v]) => auth.searchParams.set(k, v));
  const page = await fetch(auth);
  ok(page.status === 200, "authorize page renders");
  const approve = await fetch(`${BASE}/oauth/authorize/approve`, {
    method: "POST",
    redirect: "manual",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ redirect_uri: redirect, state: "s1", code_challenge: challenge, scope: "launcher", account: accountId }),
  });
  const loc = new URL(approve.headers.get("location"));
  ok(loc.searchParams.get("state") === "s1", "redirect carries state");
  const code = loc.searchParams.get("code");
  const form = (o) => ({ method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: new URLSearchParams(o) });
  const bad = await json("/oauth/token", form({ grant_type: "authorization_code", code, redirect_uri: redirect, client_id: "zirtola-launcher", code_verifier: "wrong" }));
  ok(bad.status === 400, "token exchange rejects a wrong PKCE verifier (and burns the code)");
  // Start over with a fresh code for the real exchange.
  const approve2 = await fetch(`${BASE}/oauth/authorize/approve`, {
    method: "POST",
    redirect: "manual",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ redirect_uri: redirect, state: "s1", code_challenge: challenge, scope: "launcher", account: accountId }),
  });
  const code2 = new URL(approve2.headers.get("location")).searchParams.get("code");
  const t = await json("/oauth/token", form({ grant_type: "authorization_code", code: code2, redirect_uri: redirect, client_id: "zirtola-launcher", code_verifier: verifier }));
  ok(t.status === 200 && t.body.access_token && t.body.refresh_token, "token exchange with PKCE");
  return { ...t.body, form };
}

try {
  const cfg = await json("/v1/config");
  ok(cfg.status === 200 && cfg.body.client_id === "zirtola-launcher", "GET /v1/config");
  const keys = await json("/v1/keys");
  ok(keys.body.keys.length === 2, "GET /v1/keys lists manifest + launch keys");
  const cat = await json("/v1/catalog");
  ok(cat.body.games.map((g) => g.id).join() === "dirt-simulator,alaskan-bush-simulator", "GET /v1/catalog");

  const unauth = await json("/v1/entitlements");
  ok(unauth.status === 401, "entitlements need a token");

  const miner = await signIn("acct_miner");
  const H = (t) => ({ headers: { Authorization: `Bearer ${t}`, "Content-Type": "application/json" } });
  const ents = await json("/v1/entitlements", H(miner.access_token));
  ok(ents.body.entitlements.length === 1 && ents.body.entitlements[0].game_id === "dirt-simulator", "miner owns Dirt Digger only");

  const notOwned = await json("/v1/games/alaskan-bush-simulator/manifest?platform=win64&branch=live", H(miner.access_token));
  ok(notOwned.status === 403 && notOwned.body.error === "not_entitled", "manifest for an unowned game is refused");

  const man = await json("/v1/games/dirt-simulator/manifest?platform=win64&branch=live", H(miner.access_token));
  ok(man.status === 200 && man.body.signed_manifest.signature, "signed manifest for an owned game");
  const mbytes = Buffer.from(man.body.signed_manifest.manifest, "base64url");
  const mk = keys.body.keys.find((k) => k.kid === man.body.signed_manifest.kid);
  const pub = crypto.createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: mk.x }, format: "jwk" });
  ok(crypto.verify(null, mbytes, pub, Buffer.from(man.body.signed_manifest.signature, "base64url")), "manifest signature verifies");
  const manifest = JSON.parse(mbytes);
  const chunk = manifest.files[0].chunks[0];
  const url = man.body.chunk_url_template.replace("{prefix}", chunk.hash.slice(0, 2)).replace("{hash}", chunk.hash);
  const data = Buffer.from(await (await fetch(url)).arrayBuffer());
  ok(data.length === chunk.size && crypto.createHash("sha256").update(data).digest("hex") === chunk.hash, "chunk download matches its hash");
  const ranged = await fetch(url, { headers: { Range: "bytes=10-19" } });
  ok(ranged.status === 206 && (await ranged.arrayBuffer()).byteLength === 10, "chunk server honours Range");
  const tampered = await fetch(url.replace(/t=([^&]+)/, (m, t) => `t=${t.slice(0, -2)}xx`));
  ok(tampered.status === 403, "tampered CDN token is refused");

  // Refresh rotation and reuse detection.
  const r1 = await json("/oauth/token", miner.form({ grant_type: "refresh_token", refresh_token: miner.refresh_token, client_id: "zirtola-launcher" }));
  ok(r1.status === 200 && r1.body.refresh_token !== miner.refresh_token, "refresh rotates the refresh token");
  const reuse = await json("/oauth/token", miner.form({ grant_type: "refresh_token", refresh_token: miner.refresh_token, client_id: "zirtola-launcher" }));
  ok(reuse.status === 400, "reusing a rotated refresh token fails");
  const r2 = await json("/oauth/token", miner.form({ grant_type: "refresh_token", refresh_token: r1.body.refresh_token, client_id: "zirtola-launcher" }));
  ok(r2.status === 400, "...and revokes the whole token family");

  // Key redemption.
  const fresh = await signIn("acct_new");
  const bad = await json("/v1/keys/redeem", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ key: "NOPE-NOPE-NOPE-NOPE" }) });
  ok(bad.status === 404 && bad.body.error === "invalid_key", "invalid key is rejected");
  const red = await json("/v1/keys/redeem", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ key: "alas-kabu-sh00-0001" }) });
  ok(red.status === 200 && red.body.entitlement.game_id === "alaskan-bush-simulator", "key redemption grants the game");
  const again = await json("/v1/keys/redeem", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ key: "ALAS-KABU-SH00-0001" }) });
  ok(again.status === 409, "a key can only be redeemed once");

  // Launch token, checked by the reference game.
  const pcA = "e2e-pc-a-0000000000000000";
  const pcB = "e2e-pc-b-0000000000000000";
  const noDevice = await json("/v1/launch-tokens", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ game_id: "alaskan-bush-simulator" }) });
  ok(noDevice.status === 400, "launch token needs a device id");
  const lt = await json("/v1/launch-tokens", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ game_id: "alaskan-bush-simulator", device_id: pcA, device_name: "PC A" }) });
  ok(lt.status === 200 && lt.body.token.split(".").length === 3, "launch token issued");
  const second = await json("/v1/launch-tokens", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ game_id: "alaskan-bush-simulator", device_id: pcB, device_name: "PC B" }) });
  ok(second.status === 409 && second.body.error === "device_conflict" && second.body.active_devices[0].name === "PC A", "second PC is blocked while the first is active");
  const deact = await json("/v1/activations/deactivate", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ activation_id: second.body.active_devices[0].id }) });
  const third = await json("/v1/launch-tokens", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ game_id: "alaskan-bush-simulator", device_id: pcB, device_name: "PC B" }) });
  ok(deact.status === 200 && third.status === 200, "after deactivating PC A, PC B can play");
  const cl = await json("/v1/games/dirt-simulator/changelog");
  ok(cl.status === 200 && Array.isArray(cl.body.entries) && cl.body.entries.length >= 1, "changelog lists releases");
  const v = await json("/v1/launch-tokens/verify", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ token: lt.body.token }) });
  ok(v.body.valid === true && v.body.claims.game === "alaskan-bush-simulator", "online launch-token verification");
  const exe = join(ROOT, "target", "release", process.platform === "win32" ? "license-check.exe" : "license-check");
  const run = (env) => execFileSync(exe, ["--no-wait"], { env: { ...process.env, ...env } }).toString();
  const good = run({ ZIRTOLA_SESSION_TOKEN: lt.body.token, ZIRTOLA_GAME_ID: "alaskan-bush-simulator", ZIRTOLA_API_BASE: BASE });
  ok(good.includes("LICENSE OK"), "reference game accepts the token");
  const wrong = run({ ZIRTOLA_SESSION_TOKEN: lt.body.token, ZIRTOLA_GAME_ID: "dirt-simulator", ZIRTOLA_API_BASE: BASE });
  ok(wrong.includes("NOT LICENSED"), "reference game rejects a token for another game");

  // Prices, hardware report, crash upload.
  ok(cat.body.games.every((g) => g.price && g.price.amount > 0), "catalog carries prices");
  const device = b64url(crypto.randomBytes(24));
  const hw = await json("/v1/hardware", { method: "POST", ...H(fresh.access_token), body: JSON.stringify({ device_id: device, os: { name: "Windows 11 Pro", version: "24H2", build: "26100.1" }, cpu: { model: "Test CPU", cores: 8, threads: 16 }, ram_mb: 32768, gpus: [{ name: "Test GPU", vram_mb: 8192, driver: "1.2.3.4" }], displays: [], launcher_version: "0.1.0" }) });
  ok(hw.status === 200, "hardware report accepted");
  const form = new FormData();
  form.set("metadata", JSON.stringify({ source: "launcher", game_id: "alaskan-bush-simulator", game_version: "1.0.0", device_id: device, exit_code: 3, comment: "e2e" }));
  form.set("file", new Blob([Buffer.from("PK\x05\x06" + "\0".repeat(18), "latin1")], { type: "application/zip" }), "crash.zip");
  const cr = await fetch(`${BASE}/v1/crashes`, { method: "POST", headers: { Authorization: `Bearer ${fresh.access_token}` }, body: form });
  ok(cr.status === 201, "crash report upload accepted");
  const buy = await fetch(`${BASE}/store/dirt-simulator/buy`, { method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: "account=acct_new" });
  ok(buy.status === 200 && (await buy.text()).includes("zirtola://refresh?game=dirt-simulator"), "mock checkout shows the return-to-launcher link");
  const after = await json("/v1/entitlements", H(fresh.access_token));
  ok(after.body.entitlements.some((e) => e.game_id === "dirt-simulator" && e.kind === "purchase"), "purchase shows up in entitlements right away");
} finally {
  server.kill();
  rmSync(stateDir, { recursive: true, force: true });
}

console.log(failures ? `\n${failures} check(s) failed` : "\nall checks passed");
process.exit(failures ? 1 : 0);
