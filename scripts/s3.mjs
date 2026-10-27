// Minimal S3 client (AWS Signature Version 4, header auth) for the Railway
// Storage Bucket (or any S3-compatible store). No SDK. Same code as the game
// repo's Tools/release/s3.mjs, plus GET and LIST.
//
// Credentials come from a JSON file outside every repo (default
// %USERPROFILE%\.zirtola-launcher-secrets\railway-bucket.json):
//   { "endpoint": "https://t3.storageapi.dev", "bucket": "<BUCKET from the Credentials tab>",
//     "region": "auto", "accessKeyId": "...", "secretAccessKey": "...", "pathStyle": false }
// Nothing here ever prints the secret.
import { createHash, createHmac } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const sha256hex = (b) => createHash("sha256").update(b).digest("hex");
const hmac = (k, s) => createHmac("sha256", k).update(s).digest();
const EMPTY_SHA = sha256hex("");

function uriEncode(s, keepSlash) {
  let out = "";
  for (const ch of Buffer.from(s, "utf8")) {
    const c = String.fromCharCode(ch);
    if (/[A-Za-z0-9\-._~]/.test(c) || (keepSlash && c === "/")) out += c;
    else out += `%${ch.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  return out;
}

export function defaultBucketFile() {
  return join(process.env.USERPROFILE ?? "", ".zirtola-launcher-secrets", "railway-bucket.json");
}

export function loadBucket(file = process.env.ZIRTOLA_BUCKET_FILE || defaultBucketFile()) {
  if (!existsSync(file)) throw new Error(`Bucket credentials not found: ${file} (see Tools/release/README.md)`);
  const c = JSON.parse(readFileSync(file, "utf8"));
  for (const k of ["endpoint", "bucket", "accessKeyId", "secretAccessKey"]) if (!c[k]) throw new Error(`${file} is missing "${k}"`);
  const local = /^http:\/\/(127\.0\.0\.1|localhost)[:/]/.test(c.endpoint); // a local S3 stand-in for tests
  if (!/^https:\/\//.test(c.endpoint) && !local) throw new Error(`${file}: endpoint must start with https://`);
  return { region: "auto", pathStyle: false, ...c };
}

// Signed headers for one request. `payloadHash` may be "UNSIGNED-PAYLOAD".
export function signRequest(cfg, method, key, { headers = {}, payloadHash = EMPTY_SHA, query = "", now = new Date() } = {}) {
  const ep = new URL(cfg.endpoint);
  const host = cfg.pathStyle ? ep.host : `${cfg.bucket}.${ep.host}`;
  const path = cfg.pathStyle ? `/${uriEncode(cfg.bucket, false)}/${uriEncode(key, true)}` : `/${uriEncode(key, true)}`;
  const stamp = now.toISOString().replace(/[-:]/g, "").replace(/\.\d{3}/, "");
  const date = stamp.slice(0, 8);
  const all = { ...Object.fromEntries(Object.entries(headers).map(([k, v]) => [k.toLowerCase(), String(v).trim()])), host, "x-amz-content-sha256": payloadHash, "x-amz-date": stamp };
  const names = Object.keys(all).sort();
  const canonicalHeaders = names.map((n) => `${n}:${all[n]}\n`).join("");
  const signedHeaders = names.join(";");
  const canonicalRequest = [method, path, query, canonicalHeaders, signedHeaders, payloadHash].join("\n");
  const scope = `${date}/${cfg.region}/s3/aws4_request`;
  const stringToSign = ["AWS4-HMAC-SHA256", stamp, scope, sha256hex(canonicalRequest)].join("\n");
  const kSigning = hmac(hmac(hmac(hmac(`AWS4${cfg.secretAccessKey}`, date), cfg.region), "s3"), "aws4_request");
  const signature = createHmac("sha256", kSigning).update(stringToSign).digest("hex");
  delete all.host;
  return {
    url: `${ep.protocol}//${host}${path}${query ? `?${query}` : ""}`,
    headers: { ...all, Authorization: `AWS4-HMAC-SHA256 Credential=${cfg.accessKeyId}/${scope}, SignedHeaders=${signedHeaders}, Signature=${signature}` },
  };
}

export async function headObject(cfg, key) {
  const { url, headers } = signRequest(cfg, "HEAD", key);
  const r = await fetch(url, { method: "HEAD", headers });
  if (r.status === 200) return { exists: true, size: Number(r.headers.get("content-length") ?? -1) };
  if (r.status === 404) return { exists: false };
  throw new Error(`HEAD ${key}: HTTP ${r.status}`);
}

export async function putObject(cfg, key, body, contentType = "application/octet-stream") {
  const { url, headers } = signRequest(cfg, "PUT", key, { headers: { "content-type": contentType }, payloadHash: sha256hex(body) });
  for (let attempt = 1; ; attempt++) {
    const r = await fetch(url, { method: "PUT", headers, body });
    if (r.ok) return;
    const text = await r.text().catch(() => "");
    if (attempt >= 4 || (r.status >= 400 && r.status < 500 && r.status !== 429)) throw new Error(`PUT ${key}: HTTP ${r.status} ${text.slice(0, 300)}`);
    await new Promise((res) => setTimeout(res, 1000 * 2 ** attempt));
  }
}

// Upload every chunk of a release that isn't in the bucket yet (chunks are
// immutable, named by their SHA-256, so existing ones are skipped).
export async function uploadChunks(cfg, store, gameId, hashes, { parallel = 6, log = console.log } = {}) {
  let uploaded = 0, skipped = 0, bytes = 0, i = 0;
  const work = async () => {
    while (i < hashes.length) {
      const h = hashes[i++];
      const key = `${gameId}/chunks/${h.slice(0, 2)}/${h}`;
      const local = join(store, h.slice(0, 2), h);
      const data = readFileSync(local);
      if (sha256hex(data) !== h) throw new Error(`local chunk ${local} is corrupt`);
      const have = await headObject(cfg, key);
      if (have.exists && have.size === data.length) { skipped++; continue; }
      await putObject(cfg, key, data);
      uploaded++;
      bytes += data.length;
      if (uploaded % 10 === 0) log(`  uploaded ${uploaded} chunk(s), ${(bytes / 1048576).toFixed(0)} MiB`);
    }
  };
  await Promise.all(Array.from({ length: parallel }, work));
  return { uploaded, skipped, bytes };
}

export async function getObject(cfg, key) {
  const { url, headers } = signRequest(cfg, "GET", key);
  const r = await fetch(url, { headers });
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`GET ${key}: HTTP ${r.status}`);
  return Buffer.from(await r.arrayBuffer());
}

// Common prefixes one level below `prefix` (e.g. launcher/0.2.0/).
export async function listPrefixes(cfg, prefix) {
  const q = `delimiter=%2F&list-type=2&prefix=${encodeURIComponent(prefix)}`;
  const ep = new URL(cfg.endpoint);
  const host = cfg.pathStyle ? ep.host : `${cfg.bucket}.${ep.host}`;
  const base = cfg.pathStyle ? `${ep.protocol}//${host}/${encodeURIComponent(cfg.bucket)}` : `${ep.protocol}//${host}`;
  const signed = signRequest(cfg, "GET", "", { query: q });
  const url = `${base}/?${q}`;
  const r = await fetch(cfg.pathStyle ? url : signed.url, { headers: signed.headers });
  if (!r.ok) throw new Error(`LIST ${prefix}: HTTP ${r.status}`);
  const xml = await r.text();
  return [...xml.matchAll(/<Prefix>([^<]+)<\/Prefix>/g)].map((m) => m[1]).filter((p) => p !== prefix);
}

// Credentials from the environment (CI: LAUNCHER_BUCKET_* secrets) or a file.
export function bucketFromEnv() {
  const e = process.env;
  if (!e.LAUNCHER_BUCKET_NAME) return null;
  return {
    endpoint: e.LAUNCHER_BUCKET_ENDPOINT,
    bucket: e.LAUNCHER_BUCKET_NAME,
    region: e.LAUNCHER_BUCKET_REGION || "auto",
    accessKeyId: e.LAUNCHER_BUCKET_ACCESS_KEY_ID,
    secretAccessKey: e.LAUNCHER_BUCKET_SECRET_ACCESS_KEY,
    pathStyle: /^(1|true|yes)$/i.test(e.LAUNCHER_BUCKET_PATH_STYLE ?? ""),
  };
}
