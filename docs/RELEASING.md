# Releasing

Two things get released: the **launcher** (installer + auto-update, in the
private Railway bucket, served by zirtola.com to people who own a game) and
**games** (chunk store in the same bucket + signed manifest on the website).
Nothing is downloaded from GitHub; the repository can be private.

## One-time setup

### 1. Updater signing key (required)

Installed launchers only accept updates signed with this key. Its public half
is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`).

A key pair was generated on the development machine at
`%USERPROFILE%\.zirtola-launcher-secrets\` (`updater.key`, `updater.key.password`,
`updater.key.pub`); it is not in the repo. Either use it or make your own:

```powershell
npx tauri signer generate -w "$env:USERPROFILE\.zirtola-launcher-secrets\updater.key"
# then paste the contents of updater.key.pub into plugins.updater.pubkey
```

Add GitHub Actions secrets (Settings → Secrets and variables → Actions → New repository secret):

- `TAURI_SIGNING_PRIVATE_KEY` = contents of `updater.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = its password
- `LAUNCHER_BUCKET_ENDPOINT` = the bucket's endpoint (e.g. `https://t3.storageapi.dev`)
- `LAUNCHER_BUCKET_NAME` = the bucket's `BUCKET` value (Credentials tab on Railway)
- `LAUNCHER_BUCKET_REGION` = `auto`
- `LAUNCHER_BUCKET_ACCESS_KEY_ID` = its access key id
- `LAUNCHER_BUCKET_SECRET_ACCESS_KEY` = its secret access key

(Repository variable `LAUNCHER_BUCKET_PATH_STYLE` = `true` only if the bucket's
Credentials tab says path-style.)

**Back the key up** (password manager plus offline copy). If it is lost,
existing installs can never auto-update again; users would have to download a
new installer by hand.

### 2. Authenticode signing (strongly recommended)

See docs/SIGNING.md. Until it is set up, releases build fine but unsigned.

### 3. Point release builds at the website

Repository variable `ZIRTOLA_API_BASE` = `https://www.zirtola.com/api/launcher` (optional; it is the default. Always `www`: the bare domain only forwards its front page)
(this is also the default when the variable is unset).

### 4. Pin the production public keys

Release builds trust only the keys in `src-tauri/keys/trusted-keys.json`.
Generate the studio's keys once, on a trusted machine, and keep the private
halves out of the repo:

```powershell
cargo run -p zirtola-publish --release -- keygen --kid 2026-01 --use manifest --out D:\keys\manifest-2026-01.json
cargo run -p zirtola-publish --release -- keygen --kid launch-2026-01 --use launch --out D:\keys\launch-2026-01.json
cargo run -p zirtola-publish --release -- pubkey D:\keys\manifest-2026-01.json D:\keys\launch-2026-01.json
```

Put the printed public key set into `src-tauri/keys/trusted-keys.json` (and the
launch public key into each game, see GAME_INTEGRATION.md). On the website:
`LAUNCHER_MANIFEST_PUBLIC_KEYS` = the manifest public key set,
`LAUNCHER_LAUNCH_SIGNING_KEY` = the full launch private key JSON.

## Cutting a launcher release

1. Write `release-notes/<version>.md` (players see it in the launcher's update
   prompt), then on `main`:

   ```powershell
   node scripts/release-launcher.mjs --version 0.2.1
   ```

   It bumps `package.json`, `src-tauri/Cargo.toml` and `Cargo.lock`, folds the
   CHANGELOG, commits, tags `v0.2.1` and pushes the tag (`--dry-run` first to
   check).
2. GitHub Actions → **Release** builds and signs on Windows (about 15 minutes)
   and uploads to the bucket under `launcher/0.2.1/`: the installer (which is
   also the update bundle), its `.sig` and `release.json`. **Players don't get
   it yet.** The run's summary says so.
3. Make it live: **Actions → Release → Run workflow → promote = `0.2.1`** (or
   locally `node scripts/launcher-bucket.mjs promote --version 0.2.1`).

### How the latest release is chosen

The website never guesses: `zirtola.com/download/launcher` and the launcher's
update feed read one small pointer in the bucket, `launcher/latest.json`, and
serve exactly the version named there. A release uploads its files first and
the pointer is written last, in a single upload, so a half-finished release is
never served. Every version stays in the bucket, so rolling back is promoting
the previous version (`node scripts/launcher-bucket.mjs list` shows what's
there, `*` = live). The site picks up a new pointer within a minute, with no
redeploy.

### Update prompts and required releases

Installed launchers show a window with the release notes and **Update now** /
**Later** when the pointer moves to a newer version (or update silently if the
player turned on automatic launcher updates). To make a launcher release
**required** (no Later, the reason is shown), upload it with
`--mandatory "<reason>"`, or put a `Mandatory: <reason>` line in
`release-notes/<version>.md` (the line itself isn't shown to players):

```powershell
node scripts/launcher-bucket.mjs upload --version 0.3.0 --mandatory "Security fix"
```

Games work the same way: tick **Required** and give a reason next to the build
in Studio → Licensing → Launcher catalogue → the game, or publish with
`node Tools/release/release.mjs ... --mandatory "<reason>"` from the game repo.
An install older than the newest required release (at or below the live one)
gets the prompt without Later, Play stays disabled, and the website refuses
its launch token (HTTP 426 `update_required`), also when a newer, optional
release is live. Screenshots: `docs/screenshots/update-prompt-*.png`.

### Who can download and update

- `zirtola.com/download/launcher`: signed in **and** owning a game (bought,
  key, studio grant or approved early access; refunds and revocations don't
  count). Others are sent to sign in, or told the launcher comes with their
  first game.
- Launcher self-update: `{api_base}/v1/launcher/update` with the launcher's
  access token, for accounts that own a game. A signed-out launcher doesn't
  check (it can't do anything before sign-in, and it checks right after). The
  update is still verified against `plugins.updater.pubkey`.
- Launchers built before this change looked for updates on GitHub; none are in
  players' hands. Test installs: uninstall and install again from
  zirtola.com/download/launcher.

### Cost with a private repository

GitHub's free plan includes 2,000 Actions minutes a month for private repos,
and Windows minutes count double. A launcher release is about 15 minutes of
Windows (≈ 30 billed minutes); CI on a pull request is about 6–8 minutes of
Windows (≈ 12–16 billed). Promote runs on Linux (≈ 1 minute). Roughly 50 PR
pushes and a few releases a month fit in the free minutes.

## Publishing a game build

1. Package the game in Unreal (Shipping, Win64) into a folder.
2. Chunk, hash and sign it:
   ```powershell
   cargo run -p zirtola-publish --release -- build `
     --game dirt-simulator --version 1.2.0 --exe DirtSimulator.exe `
     --input "D:\Builds\DirtSimulator\Windows" `
     --store D:\cdn\dirt-simulator\chunks `
     --key D:\keys\manifest-2026-01.json `
     --out D:\cdn\dirt-simulator\manifests\1.2.0.json
   ```
   Content-defined chunking means a patch only stores (and players only
   download) the chunks that changed: in the demo builds, a 3 MB insert in a
   320 MB pak produced 19 MB of new chunks.
3. Upload the **new** files in `D:\cdn\dirt-simulator\chunks` to the CDN (any
   static host: Cloudflare R2, S3 + CloudFront, Bunny). Chunks are immutable, so
   upload-if-missing (`rclone copy --ignore-existing`, `aws s3 sync`) is enough.
   The website's `LAUNCHER_CDN_URL_TEMPLATE` must point at it, for example
   `https://cdn.zirtola.com/{game}/chunks/{prefix}/{hash}`.
4. Register the manifest on the website: Studio → Licensing → Launcher
   catalogue → the game → **Add a build** (upload the `.json`), or from CI:
   ```bash
   curl -X POST https://www.zirtola.com/api/launcher/v1/admin/builds \
     -H "Authorization: Bearer $LAUNCHER_PUBLISH_TOKEN" -H "Content-Type: application/json" \
     -d "{\"signed_manifest\": $(cat 1.2.0.json), \"notes\": \"Patch notes\", \"make_live\": true}"
   ```
5. Launchers see the update on their next library refresh (window focus, at
   most once a minute, or every 15 minutes).

Rolling back = make the previous build live again in the same page.
