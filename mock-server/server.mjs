// Mock Zirtola website: implements docs/API.md so the launcher works end to end
// before the real website exists. Zero dependencies; state is in memory except
// redeemed keys and the live-version table, which persist in .data/.
//
//   npm run mock:seed     # once: keys, demo builds, chunk store, signed manifests
//   npm run mock          # http://127.0.0.1:8787
//
// Env: MOCK_PORT (8787), MOCK_ACCESS_TTL (seconds, 900), MOCK_CDN_TTL (3600),
//      MOCK_THROTTLE_KBPS (0 = unthrottled, per chunk response), MOCK_FAIL_RATE (0..1,
//      random 503s on chunk requests to exercise retries), MOCK_STATE_FILE (redeemed keys),
//      MOCK_AUTO_APPROVE (account id: approve sign-ins without showing the picker).
import http from "node:http";
import crypto from "node:crypto";
import { createReadStream, existsSync, mkdirSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  DATA, HERE, b64url, ensureKey, publicJwk, randomToken, readJson, sha256, signJwt, verifyJwt, writeJson,
} from "./lib.mjs";

const PORT = +(process.env.MOCK_PORT || 8787);
const BASE = `http://127.0.0.1:${PORT}`;
const ACCESS_TTL = +(process.env.MOCK_ACCESS_TTL || 900);
const CDN_TTL = +(process.env.MOCK_CDN_TTL || 3600);
const THROTTLE = +(process.env.MOCK_THROTTLE_KBPS || 0);
const FAIL_RATE = +(process.env.MOCK_FAIL_RATE || 0);
const CLIENT_ID = "zirtola-launcher";

const fixtures = readJson(join(HERE, "fixtures.json"));
const manifestKey = ensureKey(join(DATA, "keys", "manifest-key.json"), "mock-manifest-1", "manifest");
const launchKey = ensureKey(join(DATA, "keys", "launch-key.json"), "mock-launch-1", "launch");
const cdnSecret = crypto.randomBytes(32);

const STATE_FILE = process.env.MOCK_STATE_FILE || join(DATA, "state.json");
const state = readJson(STATE_FILE, { redeemed: {}, extra_entitlements: {} });
const saveState = () => writeJson(STATE_FILE, state);
const releasesFile = join(DATA, "releases.json");

// In-memory OAuth state.
const authCodes = new Map(); // code -> {...}
const accessTokens = new Map(); // token -> { account, exp }
const refreshTokens = new Map(); // token -> { account, family, used }
const revokedFamilies = new Set();
const deviceCodes = new Map(); // device_code -> { user_code, account|null, exp, interval }
const activations = {}; // "<account>:<game>" -> Map(device_id -> { name, last })
const hardware = {}; // "<account>:<device_id>" -> last hardware report (in memory)
const playtime = {}; // "<account>" -> { "<session_id>": report } (in memory, same rules as the website)
// Career stats from games (docs/GAME_STATS_API.md), in memory, same rules as
// the website: "<account>:<game>" -> { mined_mg, recovered_mg, held_mg }, and
// runs by launch-token jti -> { seqs: Map(seq -> result), mined_mg, claimed }.
const careers = {};
const statRuns = {};
const GOLD_GPH = +(process.env.MOCK_GOLD_MAX_GRAMS_PER_HOUR || 50);
const GOLD_BURST_G = 5;

function playtimeTotals(acct) {
  const by = {};
  for (const r of Object.values(playtime[acct] || {})) {
    const g = (by[r.game_id] ||= { game_id: r.game_id, active_seconds: 0, total_seconds: 0, last_played_at: null });
    g.active_seconds += r.active_seconds;
    g.total_seconds += r.total_seconds;
    g.last_played_at = r.seen_at;
  }
  return { hours_step: 0.25, games: Object.values(by).map((g) => ({ ...g, hours: Math.floor(g.active_seconds / 900) / 4 })) };
}

const now = () => Math.floor(Date.now() / 1000);
const account = (id) => fixtures.accounts.find((a) => a.id === id);
const game = (id) => fixtures.games.find((g) => g.id === id);

function entitlementsFor(accountId) {
  const a = account(accountId);
  return [...(a?.entitlements || []), ...(state.extra_entitlements[accountId] || [])].map((e) => ({
    branches: ["live"],
    expires_at: null,
    ...e,
  }));
}
const owns = (accountId, gameId) => entitlementsFor(accountId).some((e) => e.game_id === gameId);

function releases() {
  return readJson(releasesFile, {});
}

function liveVersion(gameId) {
  return releases()[gameId]?.live || null;
}

function manifestPath(gameId, version) {
  return join(DATA, "manifests", gameId, `${version}.json`);
}

function loadManifest(gameId, version) {
  const env = readJson(manifestPath(gameId, version), null);
  if (!env) return null;
  return { env, m: JSON.parse(Buffer.from(env.manifest, "base64url").toString("utf8")) };
}

// ------------------------------------------------------------------ career stats (from the game)

async function gameStats(req, res, gid) {
  const h = req.headers.authorization || "";
  const claims = h.startsWith("Bearer ") ? verifyJwt(h.slice(7), launchKey) : null;
  if (!claims || claims.game !== gid || claims.aud !== `zirtola-game:${gid}`) return err(res, 401, "unauthorized", "Launch token is not valid.");
  if (claims.iat > now() + 120) return err(res, 401, "unauthorized", "Launch token was issued in the future.");
  if (now() > claims.iat + 30 * 86400) return err(res, 401, "session_expired", "This game session is too old to report (30 days).");
  if (!owns(claims.sub, gid)) return err(res, 403, "not_entitled", "This account does not own the game.");
  const ck = `${claims.sub}:${gid}`;
  const career = (careers[ck] ||= { mined_mg: 0, recovered_mg: 0, held_mg: 0 });
  const careerJson = () => ({
    game_id: gid,
    gold_mined_g: career.mined_mg / 1000,
    gold_mined_ozt: +(career.mined_mg / 1000 / 31.1034768).toFixed(6),
    gold_recovered_g: career.recovered_mg / 1000,
    gold_recovered_ozt: +(career.recovered_mg / 1000 / 31.1034768).toFixed(6),
  });
  if (req.method === "GET") return send(res, 200, { career: careerJson() });
  if (req.method !== "POST") return err(res, 404, "not_found");
  const body = await readBody(req);
  if (!Array.isArray(body.reports) || !body.reports.length || body.reports.length > 50) return err(res, 400, "bad_request", "Expected { reports: [ … ] } (1–50).");
  const run = (statRuns[claims.jti] ||= { seqs: new Map(), mined_mg: 0, recovered_mg: 0, held_mg: 0, claimed: 0, allowed: 0, ps: null });
  if (!run.ps && typeof body.play_session_id === "string") run.ps = body.play_session_id;
  const cap = (secs) => Math.round((GOLD_GPH * secs / 3600 + GOLD_BURST_G) * 1000);
  const order = body.reports.map((rep, i) => ({ rep, i })).sort((a, b) => (+a.rep?.seq || 0) - (+b.rep?.seq || 0));
  const results = new Array(body.reports.length);
  for (const { rep, i } of order) {
    const seq = rep?.seq;
    const mined = rep?.gold_mined_g ?? 0, recovered = rep?.gold_recovered_g ?? 0, active = rep?.active_seconds;
    if (!Number.isInteger(seq) || seq < 1 || seq > 1e6 || typeof mined !== "number" || typeof recovered !== "number" || mined < 0 || recovered < 0 || mined > 1000 || recovered > 1000 || !Number.isInteger(active) || active < 0) {
      results[i] = { seq: Number.isInteger(seq) ? seq : null, status: "rejected", error: "bad_request", message: "Bad report values." };
      continue;
    }
    if (run.seqs.has(seq)) {
      results[i] = { ...run.seqs.get(seq), status: "duplicate" };
      continue;
    }
    run.claimed = Math.max(run.claimed, Math.min(active, 48 * 3600));
    let allowed = Math.min(run.claimed, now() - claims.iat + 120, 48 * 3600);
    const ps = run.ps && playtime[claims.sub]?.[run.ps];
    if (ps) allowed = Math.min(allowed, ps.active_seconds + 600);
    run.allowed = Math.max(run.allowed, allowed);
    const minedMg = Math.round(mined * 1000), recoveredMg = Math.round(recovered * 1000);
    const accM = Math.min(minedMg, Math.max(0, cap(allowed) - run.mined_mg));
    const accR = Math.min(recoveredMg, Math.max(0, career.mined_mg + accM + GOLD_BURST_G * 1000 - career.recovered_mg));
    run.mined_mg += accM; run.recovered_mg += accR; run.held_mg += minedMg - accM + recoveredMg - accR;
    career.mined_mg += accM; career.recovered_mg += accR; career.held_mg += minedMg - accM + recoveredMg - accR;
    const out = { seq, status: accM < minedMg || accR < recoveredMg ? "capped" : "accepted", accepted_mined_g: accM / 1000, accepted_recovered_g: accR / 1000, held_mined_g: (minedMg - accM) / 1000, held_recovered_g: (recoveredMg - accR) / 1000 };
    run.seqs.set(seq, out);
    results[i] = out;
    console.log(`[mock] stats ${claims.sub} ${gid} run ${claims.jti} #${seq}: +${mined} g mined, +${recovered} g recovered, ${active}s active -> ${out.status}`);
  }
  return send(res, 200, {
    results,
    session: { allowed_seconds: run.allowed, gold_mined_g: run.mined_mg / 1000, gold_recovered_g: run.recovered_mg / 1000, held_g: run.held_mg / 1000, cap_g: cap(run.allowed) / 1000 },
    career: careerJson(),
  });
}

// ------------------------------------------------------------------ http helpers

function send(res, status, body, headers = {}) {
  const data = typeof body === "string" ? body : JSON.stringify(body);
  res.writeHead(status, {
    "Content-Type": typeof body === "string" ? "text/html; charset=utf-8" : "application/json",
    "Cache-Control": "no-store",
    ...headers,
  });
  res.end(data);
}
const err = (res, status, error, message) => send(res, status, { error, message: message || error });

function readBody(req) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on("data", (c) => {
      size += c.length;
      if (size > 1e6) {
        reject(new Error("body too large"));
        req.destroy();
      } else chunks.push(c);
    });
    req.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      const type = req.headers["content-type"] || "";
      if (type.includes("application/json")) {
        try {
          resolve(JSON.parse(raw || "{}"));
        } catch {
          resolve({});
        }
      } else resolve(Object.fromEntries(new URLSearchParams(raw)));
    });
    req.on("error", reject);
  });
}

function bearer(req) {
  const h = req.headers.authorization || "";
  const t = h.startsWith("Bearer ") ? h.slice(7) : null;
  const rec = t && accessTokens.get(t);
  if (!rec || rec.exp < now()) return null;
  return rec.account;
}

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

function pageShell(title, body) {
  return `<!doctype html><html><head><meta charset="utf-8"><title>${esc(title)}</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0d1014;color:#e8ecf1;font:15px system-ui,"Segoe UI",sans-serif}
.card{width:min(440px,92vw);background:#161b22;border:1px solid #2a323d;border-radius:16px;padding:28px}
.mark{width:44px;height:44px;border-radius:11px;background:#e3a33b;color:#0d1014;font:800 26px/44px system-ui;text-align:center;margin-bottom:14px}
h1{font-size:20px;margin:0 0 6px}p{color:#9aa6b2;margin:0 0 18px}
button,.btn{display:block;width:100%;box-sizing:border-box;margin:8px 0;padding:12px 14px;border-radius:10px;border:1px solid #2f3946;background:#1d242d;color:#e8ecf1;font:inherit;text-align:left;cursor:pointer}
button:hover,button:focus{border-color:#e3a33b;outline:none}
small{display:block;color:#8693a0;margin-top:2px}.ghost{text-align:center;background:none}
.banner{font-size:12px;color:#e3a33b;border:1px dashed #e3a33b55;border-radius:8px;padding:8px 10px;margin-bottom:16px}
input{width:100%;box-sizing:border-box;padding:12px;border-radius:10px;border:1px solid #2f3946;background:#0d1014;color:#e8ecf1;font:600 18px ui-monospace,monospace;letter-spacing:2px;text-transform:uppercase}
</style></head><body><div class="card"><div class="mark">Z</div>${body}</div></body></html>`;
}

function accountButtons(hidden) {
  return fixtures.accounts
    .map(
      (a) => `<form method="post" action="${hidden.action}">${Object.entries(hidden.fields)
        .map(([k, v]) => `<input type="hidden" name="${esc(k)}" value="${esc(v)}">`)
        .join("")}<input type="hidden" name="account" value="${esc(a.id)}">
<button type="submit">${esc(a.display_name)}<small>${esc(a.note)}</small></button></form>`,
    )
    .join("");
}

// ------------------------------------------------------------------ OAuth

function validRedirect(uri) {
  try {
    const u = new URL(uri);
    return u.protocol === "http:" && (u.hostname === "127.0.0.1" || u.hostname === "[::1]") && u.pathname === "/callback";
  } catch {
    return false;
  }
}

function authorizePage(res, q) {
  if (q.client_id !== CLIENT_ID) return send(res, 400, pageShell("Error", "<h1>Unknown application</h1>"));
  if (!validRedirect(q.redirect_uri)) return send(res, 400, pageShell("Error", "<h1>Invalid redirect URI</h1>"));
  if (q.response_type !== "code" || q.code_challenge_method !== "S256" || !q.code_challenge || !q.state) {
    return send(res, 400, pageShell("Error", "<h1>Invalid request</h1><p>PKCE (S256) and state are required.</p>"));
  }
  const fields = {
    redirect_uri: q.redirect_uri,
    state: q.state,
    code_challenge: q.code_challenge,
    scope: q.scope || "launcher offline_access",
  };
  if (process.env.MOCK_AUTO_APPROVE && account(process.env.MOCK_AUTO_APPROVE)) {
    // Automated testing: skip the account picker and approve straight away.
    const code = randomToken();
    authCodes.set(code, { account: process.env.MOCK_AUTO_APPROVE, redirect_uri: q.redirect_uri, challenge: q.code_challenge, scope: fields.scope, exp: now() + 120 });
    const u = new URL(q.redirect_uri);
    u.searchParams.set("code", code);
    u.searchParams.set("state", q.state);
    res.writeHead(302, { Location: u.toString() });
    return res.end();
  }
  const cancel = new URL(q.redirect_uri);
  cancel.searchParams.set("error", "access_denied");
  cancel.searchParams.set("state", q.state);
  send(
    res,
    200,
    pageShell(
      "Sign in to Zirtola",
      `<div class="banner">Mock website for development. Pick a demo account; no password.</div>
<h1>Sign in to Zirtola</h1><p>The Zirtola Launcher wants to see which games your account owns and download them.</p>
${accountButtons({ action: "/oauth/authorize/approve", fields })}
<a class="btn ghost" href="${esc(cancel.toString())}">Cancel</a>`,
    ),
  );
}

async function authorizeApprove(req, res) {
  const f = await readBody(req);
  if (!validRedirect(f.redirect_uri) || !account(f.account)) return err(res, 400, "invalid_request");
  const code = randomToken();
  authCodes.set(code, {
    account: f.account,
    redirect_uri: f.redirect_uri,
    challenge: f.code_challenge,
    scope: f.scope,
    exp: now() + 120,
  });
  const u = new URL(f.redirect_uri);
  u.searchParams.set("code", code);
  u.searchParams.set("state", f.state);
  res.writeHead(302, { Location: u.toString() });
  res.end();
}

function issueTokens(accountId, family = randomToken(12)) {
  const access_token = randomToken();
  const refresh_token = randomToken();
  accessTokens.set(access_token, { account: accountId, exp: now() + ACCESS_TTL });
  refreshTokens.set(refresh_token, { account: accountId, family, used: false });
  return { access_token, token_type: "Bearer", expires_in: ACCESS_TTL, refresh_token, scope: "launcher offline_access" };
}

async function token(req, res) {
  const f = await readBody(req);
  if (f.client_id !== CLIENT_ID) return err(res, 401, "invalid_client");
  if (f.grant_type === "authorization_code") {
    const c = authCodes.get(f.code);
    authCodes.delete(f.code); // single use
    if (!c || c.exp < now()) return err(res, 400, "invalid_grant", "Code expired or already used.");
    if (c.redirect_uri !== f.redirect_uri) return err(res, 400, "invalid_grant", "redirect_uri mismatch.");
    if (!f.code_verifier || b64url(sha256(f.code_verifier)) !== c.challenge) {
      return err(res, 400, "invalid_grant", "PKCE verification failed.");
    }
    return send(res, 200, issueTokens(c.account));
  }
  if (f.grant_type === "refresh_token") {
    const r = refreshTokens.get(f.refresh_token);
    if (!r || revokedFamilies.has(r.family)) return err(res, 400, "invalid_grant", "Refresh token is not valid.");
    if (r.used) {
      // Reuse of a rotated token: assume theft, revoke the whole family.
      revokedFamilies.add(r.family);
      return err(res, 400, "invalid_grant", "Refresh token reuse detected.");
    }
    r.used = true;
    return send(res, 200, issueTokens(r.account, r.family));
  }
  if (f.grant_type === "urn:ietf:params:oauth:grant-type:device_code") {
    const d = deviceCodes.get(f.device_code);
    if (!d || d.exp < now()) return err(res, 400, "expired_token");
    if (d.denied) return err(res, 400, "access_denied");
    if (!d.account) return err(res, 400, "authorization_pending");
    deviceCodes.delete(f.device_code);
    return send(res, 200, issueTokens(d.account));
  }
  return err(res, 400, "unsupported_grant_type");
}

async function deviceAuthorization(req, res) {
  const f = await readBody(req);
  if (f.client_id !== CLIENT_ID) return err(res, 401, "invalid_client");
  const device_code = randomToken();
  const alphabet = "BCDFGHJKLMNPQRSTVWXZ";
  const pick = () => Array.from(crypto.randomBytes(4), (b) => alphabet[b % alphabet.length]).join("");
  const user_code = `${pick()}-${pick()}`;
  deviceCodes.set(device_code, { user_code, account: null, exp: now() + 600 });
  send(res, 200, {
    device_code,
    user_code,
    verification_uri: `${BASE}/device`,
    verification_uri_complete: `${BASE}/device?user_code=${user_code}`,
    expires_in: 600,
    interval: 5,
  });
}

function devicePage(res, q) {
  send(
    res,
    200,
    pageShell(
      "Activate a device",
      `<div class="banner">Mock website for development.</div><h1>Activate a device</h1><p>Enter the code shown on the device, then pick an account.</p>
${fixtures.accounts
  .map(
    (a) => `<form method="post" action="/device"><input name="user_code" value="${esc(q.user_code || "")}" placeholder="XXXX-XXXX">
<input type="hidden" name="account" value="${esc(a.id)}"><button type="submit">Approve as ${esc(a.display_name)}</button></form>`,
  )
  .join("")}`,
    ),
  );
}

async function deviceApprove(req, res) {
  const f = await readBody(req);
  const entry = [...deviceCodes.values()].find((d) => d.user_code === String(f.user_code || "").toUpperCase());
  if (!entry || !account(f.account)) return send(res, 400, pageShell("Error", "<h1>Unknown code</h1>"));
  entry.account = f.account;
  send(res, 200, pageShell("Done", "<h1>Device activated</h1><p>You can return to your device.</p>"));
}

async function revoke(req, res) {
  const f = await readBody(req);
  const r = refreshTokens.get(f.token);
  if (r) revokedFamilies.add(r.family);
  accessTokens.delete(f.token);
  res.writeHead(200);
  res.end();
}

// ------------------------------------------------------------------ API

function catalog() {
  return {
    games: fixtures.games.map((g) => {
      const v = liveVersion(g.id);
      const lm = v && loadManifest(g.id, v);
      return {
        id: g.id,
        title: g.title,
        tagline: g.tagline,
        description: g.description,
        status: g.status,
        store_url: `${BASE}/store/${g.id}`,
        platforms: g.platforms,
        latest_version: v,
        install_size: lm ? lm.m.total_size : null,
        art: g.art,
        price: g.price ?? null,
        sales_open: true,
        checkout_url: `${BASE}/store/${g.id}`,
        features: g.features ?? [],
        early_access: g.early_access ?? false,
        bug_report_url: g.bug_report_url ?? null,
        requirements: g.requirements ?? null,
        save_location: g.save_location ?? null,
      };
    }),
  };
}

function cdnToken(gameId, accountId) {
  const exp = now() + CDN_TTL;
  const sig = crypto.createHmac("sha256", cdnSecret).update(`${gameId}:${accountId}:${exp}`).digest("base64url");
  return { t: `${exp}.${accountId}.${sig}`, exp };
}

function checkCdnToken(gameId, t) {
  const [exp, accountId, sig] = String(t || "").split(".");
  if (!exp || +exp < now()) return false;
  const want = crypto.createHmac("sha256", cdnSecret).update(`${gameId}:${accountId}:${exp}`).digest("base64url");
  if (!sig || sig.length !== want.length || !crypto.timingSafeEqual(Buffer.from(sig), Buffer.from(want))) return false;
  return owns(accountId, gameId);
}

async function redeem(req, res, accountId) {
  const { key } = await readBody(req);
  const norm = String(key || "").trim().toUpperCase();
  const k = fixtures.keys.find((x) => x.key === norm);
  if (!k) return err(res, 404, "invalid_key", "That key isn't valid.");
  if (state.redeemed[norm]) return err(res, 409, "key_already_redeemed", "That key has already been redeemed.");
  if (owns(accountId, k.game_id)) return err(res, 409, "already_owned", "You already own this game.");
  const ent = { game_id: k.game_id, kind: "key", granted_at: new Date().toISOString() };
  state.redeemed[norm] = { account: accountId, at: ent.granted_at };
  (state.extra_entitlements[accountId] ||= []).push(ent);
  saveState();
  send(res, 200, { entitlement: { branches: ["live"], expires_at: null, ...ent }, game_title: game(k.game_id).title });
}

function serveChunk(req, res, gameId, prefix, hash, query) {
  // MOCK_OPEN_CDN=1 serves chunks without a token, so a local copy of the real
  // website can use this server as its CDN (LAUNCHER_CDN_URL_TEMPLATE).
  if (!process.env.MOCK_OPEN_CDN && !checkCdnToken(gameId, query.get("t"))) {
    return err(res, 403, "forbidden", "Download link expired or invalid.");
  }
  if (!/^[0-9a-f]{64}$/.test(hash) || hash.slice(0, 2) !== prefix) return err(res, 404, "not_found");
  if (FAIL_RATE && Math.random() < FAIL_RATE) return err(res, 503, "unavailable", "Injected failure.");
  const path = join(DATA, "cdn", gameId, "chunks", prefix, hash);
  if (!existsSync(path)) return err(res, 404, "not_found");
  const size = statSync(path).size;
  let start = 0;
  let end = size - 1;
  let status = 200;
  const range = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range || "");
  if (range) {
    start = range[1] ? +range[1] : size - +range[2];
    end = range[1] && range[2] ? Math.min(+range[2], size - 1) : size - 1;
    if (start > end || start >= size) {
      res.writeHead(416, { "Content-Range": `bytes */${size}` });
      return res.end();
    }
    status = 206;
  }
  res.writeHead(status, {
    "Content-Type": "application/octet-stream",
    "Content-Length": end - start + 1,
    "Accept-Ranges": "bytes",
    "Cache-Control": "public, max-age=31536000, immutable",
    ...(status === 206 ? { "Content-Range": `bytes ${start}-${end}/${size}` } : {}),
  });
  const stream = createReadStream(path, { start, end, highWaterMark: 64 * 1024 });
  if (!THROTTLE) return stream.pipe(res);
  // Crude per-response throttle for pause/resume testing.
  const perTick = Math.max(1024, (THROTTLE * 1024) / 10);
  stream.on("data", (buf) => {
    stream.pause();
    res.write(buf);
    setTimeout(() => stream.resume(), (buf.length / perTick) * 100);
  });
  stream.on("end", () => res.end());
  req.on("close", () => stream.destroy());
}

const money = (p) => (p ? `$${(p.amount / 100).toFixed(2)}` : "free");

function storePage(res, gameId) {
  const g = game(gameId);
  if (!g) return send(res, 404, pageShell("Not found", "<h1>Not found</h1>"));
  send(
    res,
    200,
    pageShell(
      g.title,
      `<div class="banner">Mock store page. No payment: pick who "buys" it.</div><h1>${esc(g.title)}</h1><p>${esc(g.description)}</p>
${fixtures.accounts
  .map(
    (a) => `<form method="post" action="/store/${esc(g.id)}/buy"><input type="hidden" name="account" value="${esc(a.id)}">
<button type="submit">Buy for ${money(g.price)} as ${esc(a.display_name)}<small>${owns(a.id, g.id) ? "already owns it" : "does not own it yet"}</small></button></form>`,
  )
  .join("")}
<p>Demo keys: ${fixtures.keys.filter((k) => k.game_id === g.id).map((k) => `<code>${esc(k.key)}</code>`).join(", ")}</p>`,
    ),
  );
}

// Mock checkout: grants a purchase entitlement, then shows the same
// "return to the launcher" page the real website shows after a licence is granted.
async function storeBuy(req, res, gameId) {
  const g = game(gameId);
  const { account: acct } = await readBody(req);
  if (!g || !account(acct)) return send(res, 400, pageShell("Error", "<h1>Unknown game or account</h1>"));
  if (!owns(acct, g.id)) {
    (state.extra_entitlements[acct] ||= []).push({ game_id: g.id, kind: "purchase", granted_at: new Date().toISOString() });
    saveState();
  }
  send(res, 200, pageShell("Ready", readyBody(g)));
}

function readyBody(g) {
  const link = `zirtola://refresh?game=${encodeURIComponent(g.id)}`;
  return `<h1>Your game is ready</h1><p>Return to the Zirtola Launcher; ${esc(g.title)} is ready to install.</p>
<a class="btn" href="${esc(link)}" style="text-align:center;background:#e3a33b;color:#0d1014;border-color:#e3a33b;font-weight:700">Open the Zirtola Launcher</a>
<p style="margin-top:14px;font-size:13px">The launcher notices the purchase by itself within a few seconds; the button just brings it forward.</p>`;
}

// Crash reports: multipart "metadata" (JSON) + "file" (zip). Saved under
// .data/crashes/<id>.zip and <id>.json for inspection.
async function crashUpload(req, res, acct) {
  const chunks = [];
  let size = 0;
  for await (const c of req) {
    size += c.length;
    if (size > 60 * 1024 * 1024) return err(res, 413, "too_large", "Crash report is too large.");
    chunks.push(c);
  }
  let form;
  try {
    form = await new Request("http://mock/upload", { method: "POST", headers: { "content-type": req.headers["content-type"] || "" }, body: Buffer.concat(chunks) }).formData();
  } catch {
    return err(res, 400, "bad_request", "Expected multipart/form-data.");
  }
  let meta;
  try {
    meta = JSON.parse(String(form.get("metadata") || "{}"));
  } catch {
    return err(res, 400, "bad_request", "metadata must be JSON.");
  }
  if (!game(meta.game_id)) return err(res, 404, "not_found", "Unknown game.");
  const id = randomToken(9);
  const dir = join(DATA, "crashes");
  mkdirSync(dir, { recursive: true });
  const file = form.get("file");
  if (file && typeof file === "object") writeFileSync(join(dir, `${id}.zip`), Buffer.from(await file.arrayBuffer()));
  writeFileSync(join(dir, `${id}.json`), JSON.stringify({ account: acct, received_at: new Date().toISOString(), ...meta }, null, 2));
  console.log(`[mock] crash report ${id} for ${meta.game_id} ${meta.game_version} from ${acct}`);
  send(res, 201, { id });
}

// ------------------------------------------------------------------ router

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, BASE);
  const q = Object.fromEntries(url.searchParams);
  const p = url.pathname;
  const m = (re) => re.exec(p);
  let r;
  try {
    if (req.method === "GET" && p === "/v1/config") {
      return send(res, 200, {
        issuer: BASE,
        authorization_endpoint: `${BASE}/oauth/authorize`,
        token_endpoint: `${BASE}/oauth/token`,
        device_authorization_endpoint: `${BASE}/oauth/device_authorization`,
        revocation_endpoint: `${BASE}/oauth/revoke`,
        client_id: CLIENT_ID,
        scopes: ["launcher", "offline_access"],
        store_url: `${BASE}/store`,
        account_url: `${BASE}/account`,
        support_url: `${BASE}/support`,
        min_launcher_version: "0.1.0",
      });
    }
    if (req.method === "GET" && p === "/v1/keys") {
      return send(res, 200, { keys: [publicJwk(manifestKey), publicJwk(launchKey)] });
    }
    if (req.method === "GET" && p === "/v1/catalog") return send(res, 200, catalog());
    if (req.method === "GET" && (r = m(/^\/v1\/games\/([a-z0-9-]+)\/changelog$/))) {
      const gid = r[1];
      if (!game(gid)) return err(res, 404, "not_found", "Unknown game.");
      const rel = releases()[gid] || {};
      const dir = join(DATA, "manifests", gid);
      const versions = existsSync(dir) ? readdirSync(dir).map((f) => f.replace(/\.json$/, "")) : [];
      const entries = versions
        .map((v) => ({ v, m: loadManifest(gid, v)?.m }))
        .filter((x) => x.m)
        .sort((a, b) => b.m.created_at.localeCompare(a.m.created_at))
        .map(({ v, m }) => ({ version: v, channel: "stable", released_at: m.created_at, notes: rel.notes?.[v] ?? "", current: rel.live === v }));
      return send(res, 200, { game_id: gid, entries });
    }
    if (req.method === "GET" && p === "/oauth/authorize") return authorizePage(res, q);
    if (req.method === "POST" && p === "/oauth/authorize/approve") return authorizeApprove(req, res);
    if (req.method === "POST" && p === "/oauth/token") return token(req, res);
    if (req.method === "POST" && p === "/oauth/revoke") return revoke(req, res);
    if (req.method === "POST" && p === "/oauth/device_authorization") return deviceAuthorization(req, res);
    if (req.method === "GET" && p === "/device") return devicePage(res, q);
    if (req.method === "POST" && p === "/device") return deviceApprove(req, res);
    if (req.method === "GET" && (r = m(/^\/cdn\/([a-z0-9-]+)\/chunks\/([0-9a-f]{2})\/([0-9a-f]+)$/))) {
      return serveChunk(req, res, r[1], r[2], r[3], url.searchParams);
    }
    if (req.method === "GET" && (r = m(/^\/store\/([a-z0-9-]+)$/))) return storePage(res, r[1]);
    if (req.method === "POST" && (r = m(/^\/store\/([a-z0-9-]+)\/buy$/))) return storeBuy(req, res, r[1]);
    if (req.method === "GET" && ["/store", "/account", "/support"].includes(p)) {
      return send(res, 200, pageShell("Zirtola", `<div class="banner">Mock website page: ${esc(p)}</div><h1>Zirtola</h1><p>The real website goes here.</p>`));
    }
    if (req.method === "POST" && p === "/v1/launch-tokens/verify") {
      const { token: t } = await readBody(req);
      const claims = verifyJwt(t, launchKey);
      const valid = !!claims && claims.exp >= now();
      return send(res, 200, { valid, claims: valid ? claims : null });
    }
    if ((r = m(/^\/v1\/games\/([a-z0-9-]+)\/stats$/))) return gameStats(req, res, r[1]);
    if (req.method === "POST" && p === "/dev/release") {
      // Development only: switch which version is live, to exercise patching.
      const { game_id, version } = await readBody(req);
      if (!loadManifest(game_id, version)) return err(res, 404, "not_found", "No such build.");
      const rel = releases();
      rel[game_id] = { ...(rel[game_id] || {}), live: version };
      writeJson(releasesFile, rel);
      return send(res, 200, rel[game_id]);
    }

    // ---- authenticated
    if (p.startsWith("/v1/")) {
      const acct = bearer(req);
      if (!acct) return err(res, 401, "unauthorized", "Access token missing or expired.");
      if (req.method === "GET" && p === "/v1/me") {
        const a = account(acct);
        return send(res, 200, { id: a.id, display_name: a.display_name, avatar_url: null });
      }
      if (req.method === "GET" && p === "/v1/entitlements") {
        return send(res, 200, { entitlements: entitlementsFor(acct) });
      }
      if (req.method === "POST" && p === "/v1/keys/redeem") return redeem(req, res, acct);
      if (req.method === "POST" && p === "/v1/hardware") {
        const body = await readBody(req);
        if (!/^[A-Za-z0-9_-]{16,128}$/.test(String(body.device_id || ""))) return err(res, 400, "bad_request", "device_id is required.");
        hardware[`${acct}:${body.device_id}`] = { ...body, reported_at: new Date().toISOString() };
        console.log(`[mock] hardware from ${acct}: ${body.os?.name ?? "?"} / ${body.gpus?.[0]?.name ?? "?"}`);
        return send(res, 200, { ok: true });
      }
      if (req.method === "GET" && p === "/v1/playtime") return send(res, 200, playtimeTotals(acct));
      if (req.method === "POST" && p === "/v1/playtime") {
        const body = await readBody(req);
        const reports = Array.isArray(body.sessions) ? body.sessions.slice(0, 50) : [body];
        const mine = (playtime[acct] ||= {});
        const results = reports.map((r) => {
          const sid = String(r.session_id || "");
          if (!/^[A-Za-z0-9_-]{8,64}$/.test(sid) || !game(String(r.game_id || ""))) return { session_id: sid, ok: false, error: "bad_request" };
          const prev = mine[sid];
          mine[sid] = {
            game_id: r.game_id,
            active_seconds: Math.max(prev?.active_seconds ?? 0, Math.min(r.active_seconds | 0, r.total_seconds | 0)),
            total_seconds: Math.max(prev?.total_seconds ?? 0, r.total_seconds | 0),
            seen_at: new Date().toISOString(),
          };
          console.log(`[mock] playtime ${acct} ${r.game_id} ${sid}: ${r.active_seconds}s active / ${r.total_seconds}s${r.ended ? " (ended)" : ""}`);
          return { session_id: sid, ok: true };
        });
        return send(res, 200, { ...playtimeTotals(acct), results });
      }
      if (req.method === "POST" && p === "/v1/crashes") return crashUpload(req, res, acct);
      if (req.method === "GET" && p === "/v1/dev/hardware") return send(res, 200, hardware);
      if (req.method === "GET" && (r = m(/^\/v1\/games\/([a-z0-9-]+)\/license$/))) {
        const gid = r[1];
        const ent = entitlementsFor(acct).find((e) => e.game_id === gid);
        if (!ent) return err(res, 404, "not_found", "No licence found.");
        const ownKey = Object.keys(state.redeemed).find((k) => state.redeemed[k].account === acct && fixtures.keys.find((x) => x.key === k)?.game_id === gid) ?? null;
        const set = activations[`${acct}:${gid}`] || new Map();
        return send(res, 200, {
          game_id: gid, game_title: game(gid).title, source: ent.kind, granted_at: ent.granted_at, order_number: null,
          key: ownKey, key_masked: ownKey ? ownKey.replace(/^(.{4})-(.{4})-(.{4})/, "XXXX-XXXX-XXXX") : null,
          activations: [...set.entries()].map(([dev, a]) => ({ id: `${acct}:${gid}:${dev}`, name: a.name || "a PC", activated_at: new Date(a.first * 1000).toISOString(), last_seen_at: new Date(a.last * 1000).toISOString(), this_device: dev === req.headers["x-zirtola-device"] })),
        });
      }
      if (req.method === "POST" && p === "/v1/activations/deactivate") {
        const { activation_id } = await readBody(req);
        const [a, gid, dev] = String(activation_id || "").split(":");
        if (a !== acct || !activations[`${a}:${gid}`]?.delete(dev)) return err(res, 404, "not_found", "That device isn't active on your account.");
        return send(res, 200, { ok: true });
      }
      if (req.method === "POST" && p === "/v1/launch-tokens") {
        const { game_id, device_id, device_name } = await readBody(req);
        if (!game(game_id)) return err(res, 404, "not_found", "Unknown game.");
        if (!owns(acct, game_id)) return err(res, 403, "not_entitled", "This account does not own the game.");
        if (!/^[A-Za-z0-9_-]{16,128}$/.test(String(device_id || ""))) return err(res, 400, "bad_request", "A device id is required.");
        // Activations: one device per licence (in memory), like the website.
        const key = `${acct}:${game_id}`;
        const set = (activations[key] ||= new Map());
        if (!set.has(device_id) && set.size >= 1) {
          const [otherId, other] = [...set.entries()][0];
          return send(res, 409, {
            error: "device_conflict",
            message: `${game(game_id).title} is active on ${other.name || "another PC"} (last used ${new Date(other.last * 1000).toDateString()}). Deactivate it to play here.`,
            limit: 1,
            active_devices: [{ id: `${key}:${otherId}`, name: other.name || "another PC", activated_at: new Date(other.first * 1000).toISOString(), last_seen_at: new Date(other.last * 1000).toISOString() }],
          });
        }
        const prev = set.get(device_id);
        set.set(device_id, { name: String(device_name || ""), first: prev?.first ?? now(), last: now() });
        const ent = entitlementsFor(acct).find((e) => e.game_id === game_id);
        const iat = now();
        const claims = {
          iss: BASE, sub: acct, aud: `zirtola-game:${game_id}`, game: game_id, ent: ent.kind,
          iat, exp: iat + 300, jti: randomToken(12), name: account(acct).display_name,
          addons: [], grace: 7 * 86400,
        };
        return send(res, 200, { token: signJwt(claims, launchKey), expires_at: new Date((iat + 300) * 1000).toISOString() });
      }
      if ((r = m(/^\/v1\/games\/([a-z0-9-]+)\/(latest|manifest)$/)) && req.method === "GET") {
        const [, gid, what] = r;
        if (!game(gid)) return err(res, 404, "not_found", "Unknown game.");
        if (!owns(acct, gid)) return err(res, 403, "not_entitled", "This account does not own the game.");
        if ((q.platform || "win64") !== "win64") return err(res, 404, "no_build", "No build for that platform.");
        const version = what === "manifest" && q.version ? q.version : liveVersion(gid);
        const lm = version && loadManifest(gid, version);
        if (!lm) return err(res, 404, "no_build", "No build has been published yet.");
        if (what === "latest") {
          return send(res, 200, {
            game_id: gid, version, build_id: lm.m.build_id, total_size: lm.m.total_size,
            released_at: lm.m.created_at, notes: releases()[gid]?.notes?.[version] || null,
          });
        }
        const { t, exp } = cdnToken(gid, acct);
        return send(res, 200, {
          version,
          signed_manifest: lm.env,
          chunk_url_template: `${BASE}/cdn/${gid}/chunks/{prefix}/{hash}?t=${t}`,
          chunk_url_expires_at: new Date(exp * 1000).toISOString(),
        });
      }
    }
    err(res, 404, "not_found");
  } catch (e) {
    console.error(e);
    if (!res.headersSent) err(res, 500, "server_error", String(e.message || e));
  }
});

const builds = existsSync(join(DATA, "manifests")) ? readdirSync(join(DATA, "manifests")) : [];
server.listen(PORT, "127.0.0.1", () => {
  console.log(`Mock Zirtola API on ${BASE}`);
  if (!builds.length) console.log("No demo builds yet: run `npm run mock:seed` first.");
  for (const g of fixtures.games) console.log(`  ${g.id}: live ${liveVersion(g.id) || "(none)"}`);
  if (THROTTLE) console.log(`  chunk throttle: ${THROTTLE} KB/s per response`);
});

