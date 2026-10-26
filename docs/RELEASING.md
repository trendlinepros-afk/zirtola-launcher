# Releasing

Two things get released: the **launcher** (GitHub Releases + auto-update feed)
and **games** (chunk store on a CDN + signed manifest on the website).

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

Add GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY` = contents of `updater.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = its password

**Back the key up** (password manager plus offline copy). If it is lost,
existing installs can never auto-update again; users would have to download a
new installer by hand.

### 2. Authenticode signing (strongly recommended)

See docs/SIGNING.md. Until it is set up, releases build fine but unsigned.

### 3. Point release builds at the website

Repository variable `ZIRTOLA_API_BASE` = `https://zirtola.com/api/launcher`
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

1. Bump `version` in `package.json` (Tauri reads it from there) and
   `src-tauri/Cargo.toml`, add a CHANGELOG entry, commit.
2. Tag and push:
   ```bash
   git tag v0.2.0
   git push origin main v0.2.0
   ```
3. The **Release** workflow tests, builds the NSIS installer, Authenticode-signs
   every binary (if configured), signs the updater bundle, and creates a
   **draft** GitHub Release with:
   - `Zirtola Launcher_0.2.0_x64-setup.exe` (link this from the website)
   - `…-setup.exe.sig` (updater signature)
   - `latest.json` (the update feed)
   - `installer.json` and `SHA256SUMS.txt`: the setup file's published name,
     URL and SHA-256 (made by `scripts/installer-info.mjs`). The website reads
     `installer.json` from the latest release to show the checksum next to its
     download button and to send that button straight to the file.
4. Download the installer from the draft, install it on a clean Windows
   machine or VM, sign in, install a game.
5. Click **Publish release**. Installed launchers check
   `https://github.com/trendlinepros-afk/zirtola-launcher/releases/latest/download/latest.json`
   at start-up and offer the update (Settings → "Check for updates" checks on demand).

The website's download buttons all go to `zirtola.com/download/launcher`, which
redirects to the installer named in the latest release's `installer.json`
(falling back to `LAUNCHER_DOWNLOAD_URL`). GitHub turns spaces in asset names
into dots, so players download `Zirtola.Launcher_<version>_x64-setup.exe`;
the release job checks that the uploaded asset has exactly that name.

A broken release can be pulled by deleting or re-drafting it: `latest.json`
then resolves to the previous published release.

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
   curl -X POST https://zirtola.com/api/launcher/v1/admin/builds \
     -H "Authorization: Bearer $LAUNCHER_PUBLISH_TOKEN" -H "Content-Type: application/json" \
     -d "{\"signed_manifest\": $(cat 1.2.0.json), \"notes\": \"Patch notes\", \"make_live\": true}"
   ```
5. Launchers see the update on their next library refresh (window focus, at
   most once a minute, or every 15 minutes).

Rolling back = make the previous build live again in the same page.
