#!/usr/bin/env node
// Writes installer.json and SHA256SUMS.txt next to the NSIS setup so the
// website can show the installer's SHA-256 ("What to expect when you install")
// and send its download button straight to the file.
//
//   node scripts/installer-info.mjs --dir target/release/bundle/nsis --version 0.2.0 \
//        [--repo trendlinepros-afk/zirtola-launcher] [--tag v0.2.0] [--base-url https://...]
//
// GitHub stores release assets with spaces turned into dots, so the published
// name (and the name players' browsers save) is "Zirtola.Launcher_<v>_x64-setup.exe".
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : def;
};
const dir = arg("dir", "target/release/bundle/nsis");
const version = arg("version");
const repo = arg("repo", "trendlinepros-afk/zirtola-launcher");
const tag = arg("tag", version ? `v${version}` : undefined);
const baseUrl = arg("base-url");
if (!version) {
  console.error("installer-info: --version is required");
  process.exit(1);
}

const setup = readdirSync(dir).find((f) => f.endsWith(`${version}_x64-setup.exe`));
if (!setup) {
  console.error(`installer-info: no *${version}_x64-setup.exe in ${dir}`);
  process.exit(1);
}
const path = join(dir, setup);
const sha256 = createHash("sha256").update(readFileSync(path)).digest("hex");
const published = setup.replace(/ /g, ".");
const url = baseUrl
  ? `${baseUrl.replace(/\/$/, "")}/${encodeURIComponent(published)}`
  : `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(published)}`;

const info = { version, file: published, url, sha256, size: statSync(path).size, generated_at: new Date().toISOString() };
writeFileSync(join(dir, "installer.json"), JSON.stringify(info, null, 2) + "\n");
writeFileSync(join(dir, "SHA256SUMS.txt"), `${sha256}  ${published}\n`);
console.log(`installer-info: ${published} sha256 ${sha256}`);
