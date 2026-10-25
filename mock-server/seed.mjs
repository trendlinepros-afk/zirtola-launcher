// Seeds the mock website with signed demo builds, using the real publishing
// pipeline (tools/zirtola-publish): synthetic "UE-shaped" build folders are
// chunked, hashed and signed exactly as real builds would be.
//
//   npm run mock:seed                 # ~370 MB + ~250 MB of builds
//   MOCK_SCALE=0.1 npm run mock:seed  # tiny builds (CI)
//
// Each game gets 1.0.0 (live) and 1.0.1 (a patch that inserts data in the
// middle of the pak, rewrites a small region, adds one file and removes one).
// Make the patch live with: node mock-server/release.mjs dirt-simulator 1.0.1
import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, openSync, rmSync, writeSync, closeSync } from "node:fs";
import { join } from "node:path";
import { DATA, HERE, ROOT, ensureKey, readJson, writeJson } from "./lib.mjs";

const SCALE = +(process.env.MOCK_SCALE || 1);
const MB = 1024 * 1024;
const fixtures = readJson(join(HERE, "fixtures.json"));

ensureKey(join(DATA, "keys", "manifest-key.json"), "mock-manifest-1", "manifest");
ensureKey(join(DATA, "keys", "launch-key.json"), "mock-launch-1", "launch");

console.log("building tools (cargo build --release)...");
execFileSync("cargo", ["build", "--release", "-p", "zirtola-publish", "-p", "license-check"], {
  cwd: ROOT,
  stdio: "inherit",
});
const exe = process.platform === "win32" ? ".exe" : "";
const publish = join(ROOT, "target", "release", `zirtola-publish${exe}`);
const gameExe = join(ROOT, "target", "release", `license-check${exe}`);

/** Deterministic incompressible bytes (AES-CTR keystream of `seed`). */
function stream(seed) {
  const key = crypto.createHash("sha256").update(seed).digest().subarray(0, 16);
  return crypto.createCipheriv("aes-128-ctr", key, Buffer.alloc(16));
}

/** Write `parts` (each { seed, bytes } or { seed, bytes, skip }) to a file, streaming. */
function writeParts(path, parts) {
  mkdirSync(join(path, ".."), { recursive: true });
  const fd = openSync(path, "w");
  const zero = Buffer.alloc(MB);
  for (const part of parts) {
    const c = stream(part.seed);
    let skip = part.skip || 0;
    while (skip > 0) {
      const n = Math.min(skip, MB);
      c.update(zero.subarray(0, n));
      skip -= n;
    }
    let left = part.bytes;
    while (left > 0) {
      const n = Math.min(left, MB);
      writeSync(fd, c.update(zero.subarray(0, n)));
      left -= n;
    }
  }
  closeSync(fd);
}

const sizes = { "dirt-simulator": { pak: 320, movie: 40 }, "alaskan-bush-simulator": { pak: 220, movie: 24 } };

for (const g of fixtures.games) {
  const s = sizes[g.id] || { pak: 100, movie: 10 };
  const pak = Math.max(4 * MB, Math.round(s.pak * SCALE * MB));
  const movie = Math.max(MB, Math.round(s.movie * SCALE * MB));
  const versions = {
    "1.0.0": (dir) => {
      writeParts(join(dir, g.folder, "Content", "Paks", `${g.folder}-Windows.pak`), [{ seed: `${g.id}-pak`, bytes: pak }]);
      writeParts(join(dir, g.folder, "Content", "Movies", "Intro.mp4"), [{ seed: `${g.id}-movie`, bytes: movie }]);
      writeParts(join(dir, "Engine", "Binaries", "ThirdParty", "Ogg", "libogg.dll"), [{ seed: "ogg", bytes: 3 * MB }]);
      writeParts(join(dir, g.folder, "Content", "Splash", "Splash.bmp"), [{ seed: `${g.id}-splash`, bytes: 600 * 1024 }]);
    },
    "1.0.1": (dir) => {
      const cut = Math.floor(pak * 0.4);
      const tail = pak - cut;
      const patchAt = Math.floor(tail * 0.6);
      // Same pak with 3 MB inserted at 40% and 512 KB rewritten further on.
      writeParts(join(dir, g.folder, "Content", "Paks", `${g.folder}-Windows.pak`), [
        { seed: `${g.id}-pak`, bytes: cut },
        { seed: `${g.id}-pak-101-insert`, bytes: 3 * MB },
        { seed: `${g.id}-pak`, skip: cut, bytes: patchAt },
        { seed: `${g.id}-pak-101-rewrite`, bytes: 512 * 1024 },
        { seed: `${g.id}-pak`, skip: cut + patchAt + 512 * 1024, bytes: tail - patchAt - 512 * 1024 },
      ]);
      writeParts(join(dir, g.folder, "Content", "Movies", "Intro.mp4"), [{ seed: `${g.id}-movie`, bytes: movie }]);
      writeParts(join(dir, "Engine", "Binaries", "ThirdParty", "Ogg", "libogg.dll"), [{ seed: "ogg", bytes: 3 * MB }]);
      // Splash.bmp removed; a new localisation pak added.
      writeParts(join(dir, g.folder, "Content", "Paks", `${g.folder}-Windows_0_P.pak`), [{ seed: `${g.id}-loc`, bytes: 2 * MB }]);
    },
  };

  for (const [version, make] of Object.entries(versions)) {
    const dir = join(DATA, "builds", g.id, version);
    const out = join(DATA, "manifests", g.id, `${version}.json`);
    rmSync(dir, { recursive: true, force: true });
    make(dir);
    copyFileSync(gameExe, join(dir, g.exe));
    console.log(`publishing ${g.id} ${version}...`);
    execFileSync(
      publish,
      [
        "build", "--game", g.id, "--version", version, "--exe", g.exe,
        "--input", dir, "--store", join(DATA, "cdn", g.id, "chunks"),
        "--key", join(DATA, "keys", "manifest-key.json"), "--out", out,
      ],
      { stdio: "inherit" },
    );
    rmSync(dir, { recursive: true, force: true }); // the chunk store is the source of truth now
  }
}

const releasesFile = join(DATA, "releases.json");
const rel = readJson(releasesFile, {});
for (const g of fixtures.games) {
  rel[g.id] = {
    live: rel[g.id]?.live && existsSync(join(DATA, "manifests", g.id, `${rel[g.id].live}.json`)) ? rel[g.id].live : "1.0.0",
    notes: { "1.0.0": "First playable build.", "1.0.1": "Patch: new localisation pak, content fixes." },
  };
}
writeJson(releasesFile, rel);
console.log("seeded. Start the server with: npm run mock");
