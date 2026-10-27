#!/usr/bin/env node
// Cut a Zirtola Launcher release.
//
//   node scripts/release-launcher.mjs --version 0.2.0 --dry-run     # check everything, change nothing
//   node scripts/release-launcher.mjs --version 0.2.0               # bump, commit, tag, push the tag
//   node scripts/release-launcher.mjs --version 0.2.0 --local-build --no-git
//                                                                   # build + sign the update locally (testing)
//
// Pushing the tag starts .github/workflows/release.yml, which builds the
// installer, Authenticode-signs it (once ZIRTOLA_SIGN_MODE is set), signs the
// updater bundle with TAURI_SIGNING_PRIVATE_KEY and uploads it to the private
// bucket under launcher/<version>/. Promoting it (Actions -> Release -> Run
// workflow, or scripts/launcher-bucket.mjs promote) makes zirtola.com serve it.

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const args = process.argv.slice(2);
const flag = (n) => args.includes(`--${n}`);
const value = (n) => {
  const i = args.indexOf(`--${n}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const fail = (m) => {
  console.error(`\n[release] ${m}\n`);
  process.exit(1);
};
const log = (...a) => console.log("[release]", ...a);
const dry = flag("dry-run");

const version = value("version") ?? "";
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) fail("--version must be semver, e.g. 0.2.0");
const pkgPath = join(ROOT, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
const cmp = (a, b) => {
  const pa = a.split(/[.-]/).map((x) => (isNaN(+x) ? x : +x));
  const pb = b.split(/[.-]/).map((x) => (isNaN(+x) ? x : +x));
  for (let i = 0; i < 3; i++) if (pa[i] !== pb[i]) return pa[i] > pb[i] ? 1 : -1;
  return 0;
};
if (cmp(version, pkg.version) <= 0 && !flag("allow-same")) fail(`${version} is not newer than the current ${pkg.version}`);

const notesFile = join(ROOT, "release-notes", `${version}.md`);
if (!existsSync(notesFile)) fail(`Write the release notes first: release-notes/${version}.md (see release-notes/TEMPLATE.md)`);
const notes = readFileSync(notesFile, "utf8").trim();
if (notes.length < 20) fail("Release notes are too short.");

const git = (...a) => execFileSync("git", ["-C", ROOT, ...a], { encoding: "utf8" }).trim();
if (!flag("no-git")) {
  if (git("status", "--porcelain").replace(/^.. release-notes\/.*$/gm, "").trim()) fail("Commit or stash your changes first (only new release notes may be uncommitted).");
  if (git("tag", "--list", `v${version}`)) fail(`Tag v${version} already exists.`);
}

log(`launcher ${pkg.version} -> ${version}${dry ? " (dry run)" : ""}`);
if (dry) {
  log("would update package.json, src-tauri/Cargo.toml, Cargo.lock and CHANGELOG.md");
  log(`would commit "release: launcher v${version}", tag v${version} and push the tag`);
  log("release notes:\n" + notes);
  process.exit(0);
}

// 1. Bump versions.
pkg.version = version;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
const cargoPath = join(ROOT, "src-tauri", "Cargo.toml");
writeFileSync(cargoPath, readFileSync(cargoPath, "utf8").replace(/^version = "[^"]+"/m, `version = "${version}"`));
spawnSync("cargo", ["update", "-p", "zirtola-launcher", "--offline"], { cwd: ROOT, stdio: "inherit" });

// 2. Changelog: newest first.
const clPath = join(ROOT, "CHANGELOG.md");
const cl = readFileSync(clPath, "utf8");
const body = notes.replace(/^#+ .*\n+/, "");
// Player-facing notes go on top; anything collected under "## Unreleased"
// becomes part of this version's section.
if (!cl.includes(`## ${version}`)) {
  writeFileSync(
    clPath,
    cl.includes("## Unreleased")
      ? cl.replace("## Unreleased\n", `## ${version}\n\n${body}\n`)
      : cl.replace("# Changelog\n", `# Changelog\n\n## ${version}\n\n${body}\n`),
  );
}

// 3. Optional local build with signed update artifacts (for testing updates).
if (flag("local-build")) {
  const keyFile = process.env.TAURI_SIGNING_PRIVATE_KEY_FILE ?? join(process.env.USERPROFILE ?? "", ".zirtola-launcher-secrets", "updater.key");
  if (!existsSync(keyFile)) fail(`Updater key not found at ${keyFile}`);
  const env = {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyFile, "utf8"),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? (existsSync(`${keyFile}.password`) ? readFileSync(`${keyFile}.password`, "utf8").trim() : ""),
  };
  // Pass the config override as a file: a JSON argument loses its quotes in a shell.
  const override = join(ROOT, "target", "release-updater.conf.json");
  mkdirSync(dirname(override), { recursive: true });
  writeFileSync(override, JSON.stringify({ bundle: { createUpdaterArtifacts: true } }));
  const r = spawnSync("npx", ["tauri", "build", "--config", `"${override}"`], { cwd: ROOT, stdio: "inherit", env, shell: true });
  if (r.status !== 0) fail("tauri build failed");
  const dir = join(ROOT, "target", "release", "bundle", "nsis");
  const setup = readdirSync(dir).find((f) => f.endsWith(`${version}_x64-setup.exe`));
  if (!setup || !existsSync(join(dir, `${setup}.sig`))) fail("No signed setup .exe found");
  log(`built and signed ${join(dir, setup)}`);
  // --upload: put it in the bucket under launcher/<version>/ (pointer untouched),
  // with %USERPROFILE%\.zirtola-launcher-secrets\railway-bucket.json.
  if (flag("upload")) {
    const up = spawnSync("node", ["scripts/launcher-bucket.mjs", "upload", "--version", version, "--dir", dir], { cwd: ROOT, stdio: "inherit" });
    if (up.status !== 0) fail("upload failed");
  }
}

// 4. Commit, tag, push the tag (CI does the rest).
if (!flag("no-git")) {
  git("add", "package.json", "src-tauri/Cargo.toml", "Cargo.lock", "CHANGELOG.md", `release-notes/${version}.md`);
  git("commit", "-m", `release: launcher v${version}`);
  git("tag", "-a", `v${version}`, "-m", `Zirtola Launcher ${version}`);
  if (!flag("no-push")) {
    log(`pushing tag v${version} (and the current branch)`);
    git("push", "origin", "HEAD");
    git("push", "origin", `v${version}`);
    log("GitHub Actions is building and uploading it to the bucket (players don't get it yet).");
    log(`When it's green: Actions → Release → Run workflow → promote = ${version}. Or locally: node scripts/launcher-bucket.mjs promote --version ${version}`);
  } else {
    log(`tagged v${version} locally; push with: git push origin HEAD v${version}`);
  }
}
