#!/usr/bin/env node
// The Zirtola Launcher's installer and updates live in the private Railway
// bucket, never on GitHub. Layout:
//
//   launcher/<version>/Zirtola.Launcher_<version>_x64-setup.exe   installer = update bundle
//   launcher/<version>/Zirtola.Launcher_<version>_x64-setup.exe.sig
//   launcher/<version>/release.json   { version, file, key, sha256, size, signature, notes, pub_date }
//   launcher/latest.json              copy of the current release.json: THE POINTER
//
// The website (zirtola.com/download/launcher and the launcher update feed)
// only ever reads the pointer. Uploading never touches it; `promote` writes it
// last, in one PUT, so a half-finished upload is never served. Old versions
// stay, so rolling back is `promote <older version>`.
//
//   node scripts/launcher-bucket.mjs upload  --version 0.2.1 [--dir target/release/bundle/nsis] [--notes release-notes/0.2.1.md] [--mandatory "Security fix"]
//   node scripts/launcher-bucket.mjs promote --version 0.2.1     # make it the one players get (or roll back)
//   node scripts/launcher-bucket.mjs list                        # versions in the bucket, * = current
//   node scripts/launcher-bucket.mjs current
//
// Credentials: LAUNCHER_BUCKET_* environment variables (GitHub Actions
// secrets), else %USERPROFILE%\.zirtola-launcher-secrets\railway-bucket.json.
// Nothing here prints them.
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { bucketFromEnv, defaultBucketFile, getObject, headObject, listPrefixes, loadBucket, putObject } from "./s3.mjs";

const [cmd, ...rest] = process.argv.slice(2);
const arg = (name, def) => {
  const i = rest.indexOf(`--${name}`);
  return i >= 0 ? rest[i + 1] : def;
};
const fail = (m) => {
  console.error(`launcher-bucket: ${m}`);
  process.exit(1);
};
const SEMVER = /^\d+\.\d+\.\d+([-.][\w.]+)?$/;

function bucket() {
  try {
    return bucketFromEnv() ?? loadBucket(process.env.ZIRTOLA_BUCKET_FILE || defaultBucketFile());
  } catch (e) {
    fail(e.message);
  }
}

async function upload() {
  const version = arg("version");
  if (!SEMVER.test(version ?? "")) fail("--version x.y.z is required");
  const dir = arg("dir", "target/release/bundle/nsis");
  const local = readdirSync(dir).find((f) => f.endsWith(`${version}_x64-setup.exe`));
  if (!local) fail(`no *${version}_x64-setup.exe in ${dir}`);
  const sigFile = join(dir, `${local}.sig`);
  if (!existsSync(sigFile)) fail(`${local}.sig is missing: build with createUpdaterArtifacts and TAURI_SIGNING_PRIVATE_KEY`);
  const file = local.replace(/ /g, ".");
  const data = readFileSync(join(dir, local));
  const notesFile = arg("notes", `release-notes/${version}.md`);
  const notesText = existsSync(notesFile) ? readFileSync(notesFile, "utf8") : "";
  // A "Mandatory: <reason>" line in the notes (or --mandatory "<reason>") makes
  // it a required update; the line itself isn't shown to players.
  const notesMandatory = /^Mandatory:\s*(.*)$/im.exec(notesText);
  const rel = {
    version,
    file,
    key: `launcher/${version}/${file}`,
    sha256: createHash("sha256").update(data).digest("hex"),
    size: statSync(join(dir, local)).size,
    signature: readFileSync(sigFile, "utf8").trim(),
    notes: notesText.replace(/^Mandatory:.*$\n?/im, "").trim(),
    pub_date: new Date().toISOString(),
    // --mandatory "<reason>": installed launchers can't postpone this update.
    ...(rest.includes("--mandatory") || notesMandatory
      ? { mandatory: true, mandatory_reason: (rest.includes("--mandatory") && !String(arg("mandatory", "") ?? "").startsWith("--") ? String(arg("mandatory", "")) : notesMandatory?.[1] ?? "").trim().slice(0, 200) }
      : {}),
  };
  const b = bucket();
  console.log(`launcher-bucket: uploading ${version} to ${b.bucket} (pointer unchanged)`);
  await putObject(b, rel.key, data, "application/vnd.microsoft.portable-executable");
  await putObject(b, `${rel.key}.sig`, Buffer.from(rel.signature), "text/plain");
  // release.json last: its presence means the version is complete.
  await putObject(b, `launcher/${version}/release.json`, Buffer.from(JSON.stringify(rel, null, 2)), "application/json");
  console.log(`launcher-bucket: ${file} (${(rel.size / 1048576).toFixed(1)} MB, sha256 ${rel.sha256}) uploaded. Make it live with: promote --version ${version}`);
}

async function promote() {
  const version = arg("version");
  if (!SEMVER.test(version ?? "")) fail("--version x.y.z is required");
  const b = bucket();
  const relBuf = await getObject(b, `launcher/${version}/release.json`);
  if (!relBuf) fail(`launcher/${version}/release.json isn't in the bucket: upload ${version} first`);
  const rel = JSON.parse(relBuf.toString("utf8"));
  if (rel.version !== version || rel.key !== `launcher/${version}/${rel.file}`) fail("release.json doesn't match its version");
  const inst = await headObject(b, rel.key);
  if (!inst.exists || inst.size !== rel.size) fail(`${rel.key} is missing or incomplete: upload ${version} again`);
  const before = await getObject(b, "launcher/latest.json");
  await putObject(b, "launcher/latest.json", relBuf, "application/json");
  const was = before ? JSON.parse(before.toString("utf8")).version : "nothing";
  console.log(`launcher-bucket: latest is now ${version} (was ${was}). zirtola.com serves it within a minute.`);
}

async function current() {
  const b = bucket();
  const cur = await getObject(b, "launcher/latest.json");
  return cur ? JSON.parse(cur.toString("utf8")) : null;
}

async function list() {
  const b = bucket();
  const cur = await current();
  const versions = (await listPrefixes(b, "launcher/")).map((p) => p.slice("launcher/".length).replace(/\/$/, "")).filter((v) => SEMVER.test(v));
  versions.sort((a, z) => a.localeCompare(z, undefined, { numeric: true }));
  if (!versions.length) console.log("launcher-bucket: no launcher versions in the bucket yet");
  for (const v of versions) console.log(`${v === cur?.version ? "*" : " "} ${v}`);
}

if (cmd === "upload") await upload();
else if (cmd === "promote") await promote();
else if (cmd === "list") await list();
else if (cmd === "current") console.log((await current())?.version ?? "no launcher release is live yet");
else fail("usage: upload --version x.y.z | promote --version x.y.z | list | current");
