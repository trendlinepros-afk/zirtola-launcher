// Shared helpers for the mock Zirtola website (no dependencies).
import crypto from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const HERE = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(HERE, "..");
export const DATA = process.env.MOCK_DATA_DIR || join(HERE, ".data");

export const b64url = (buf) => Buffer.from(buf).toString("base64url");
export const sha256 = (data) => crypto.createHash("sha256").update(data).digest();
export const randomToken = (bytes = 32) => b64url(crypto.randomBytes(bytes));

export function readJson(path, fallback) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return fallback;
  }
}

export function writeJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(value, null, 2));
}

/** Private key file in the same format `zirtola-publish keygen` writes. */
export function ensureKey(path, kid, use) {
  if (existsSync(path)) return readJson(path);
  const { privateKey } = crypto.generateKeyPairSync("ed25519");
  const jwk = privateKey.export({ format: "jwk" });
  const key = { kid, kty: "OKP", crv: "Ed25519", use, x: jwk.x, d: jwk.d };
  writeJson(path, key);
  return key;
}

export function publicJwk(k) {
  return { kid: k.kid, kty: "OKP", crv: "Ed25519", use: k.use, x: k.x };
}

export function signJwt(claims, key) {
  const header = { alg: "EdDSA", typ: "JWT", kid: key.kid };
  const input = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(claims))}`;
  const pk = crypto.createPrivateKey({
    key: { kty: "OKP", crv: "Ed25519", x: key.x, d: key.d },
    format: "jwk",
  });
  return `${input}.${b64url(crypto.sign(null, Buffer.from(input), pk))}`;
}

export function verifyJwt(token, key) {
  const parts = String(token).split(".");
  if (parts.length !== 3) return null;
  const pub = crypto.createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: key.x }, format: "jwk" });
  const ok = crypto.verify(null, Buffer.from(`${parts[0]}.${parts[1]}`), pub, Buffer.from(parts[2], "base64url"));
  if (!ok) return null;
  return JSON.parse(Buffer.from(parts[1], "base64url").toString("utf8"));
}

export function semverCmp(a, b) {
  const pa = a.split(/[.-]/).map((x) => (isNaN(+x) ? x : +x));
  const pb = b.split(/[.-]/).map((x) => (isNaN(+x) ? x : +x));
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    if (pa[i] === undefined) return -1;
    if (pb[i] === undefined) return 1;
    if (pa[i] < pb[i]) return -1;
    if (pa[i] > pb[i]) return 1;
  }
  return 0;
}
