# New-user test: from zirtola.com to playing Dirt Digger

Two parts. **Part A** is one-time setup on Railway and GitHub (about 45
minutes, most of it waiting). **Part B** is the test itself, as a brand-new
player on a clean Windows user account (about 20 minutes).

Secrets stay in `C:\Users\Wicked\.zirtola-launcher-secrets\`. Never paste them
into chat, a repo or a screenshot.

---

## Part A: one-time setup

### A1. Merge the two pull requests

1. Website: open <https://github.com/trendlinepros-afk/retail-store/pull/26> (`launcher-downloads` → `claude/app-marketplace-platform-cpyvmr`)
   → **Merge pull request** → **Confirm merge**. Railway redeploys the site by
   itself (2–4 minutes). It adds the download route; no database changes.
2. Launcher: open <https://github.com/trendlinepros-afk/zirtola-launcher/pull/3> (`launcher-ready` → `main`) → **Merge pull request** → **Confirm merge**.

### A2. Create the storage bucket on Railway

1. <https://railway.com/dashboard> → your project (the one with the website).
2. Top right of the project canvas: **Create** (or **+ New**) → **Bucket**.
3. **Region**: pick the one closest to your players (US East is fine; it can't be
   changed later). **Name**: `game-files`. Click **Create**/**Add**.
4. Railway shows a banner "1 change staged": click **Deploy** (or **Apply changes**).
5. Click the new **game-files** bucket → **Credentials** tab. Leave this open; you
   need it twice. Note whether it says **virtual-hosted** or **path** style.

### A3. Give the website access to the bucket

1. Click the **website service** → **Variables** tab → **+ New Variable** for each
   row below. In the value box, typing `${{` opens a list: pick **game-files**,
   then the variable. (If the list doesn't appear, paste the value from the
   bucket's Credentials tab instead.)

   | Name | Value |
   | --- | --- |
   | `LAUNCHER_BUCKET_NAME` | `${{game-files.BUCKET}}` |
   | `LAUNCHER_BUCKET_ENDPOINT` | `${{game-files.ENDPOINT}}` |
   | `LAUNCHER_BUCKET_REGION` | `${{game-files.REGION}}` |
   | `LAUNCHER_BUCKET_ACCESS_KEY_ID` | `${{game-files.ACCESS_KEY_ID}}` |
   | `LAUNCHER_BUCKET_SECRET_ACCESS_KEY` | `${{game-files.SECRET_ACCESS_KEY}}` |
   | `LAUNCHER_CDN_URL_TEMPLATE` | `https://www.zirtola.com/api/launcher/v1/chunks/{game}/{prefix}/{hash}` |

   Only if the Credentials tab said **path style**, also add
   `LAUNCHER_BUCKET_PATH_STYLE` = `true`.
2. Click **Deploy** on the "changes staged" banner. Wait for the deployment to show
   **Active** (2–4 minutes).
3. Check: open <https://www.zirtola.com/api/launcher/v1/chunks/dirt-simulator/ab/abababababababababababababababababababababababababababababababab>
   in a browser. You should see `{"error":"link_expired",...}`. That means the
   route is live and refuses links without a valid token, which is correct.

### A4. Put the bucket credentials and publish token on this PC

1. Open Notepad and paste this, filling in the four values from the bucket's
   **Credentials** tab (use `"pathStyle": true` only if it said path style):

   ```json
   {
     "endpoint": "https://t3.storageapi.dev",
     "bucket": "<the BUCKET value>",
     "region": "auto",
     "accessKeyId": "<the ACCESS_KEY_ID value>",
     "secretAccessKey": "<the SECRET_ACCESS_KEY value>",
     "pathStyle": false
   }
   ```

   **File → Save as** → folder `C:\Users\Wicked\.zirtola-launcher-secrets` →
   "Save as type: **All files**" → name `railway-bucket.json` → **Save**.
2. New Notepad window: paste the value of `LAUNCHER_PUBLISH_TOKEN` (from
   `RAILWAY_VARIABLES.txt` in the same folder, the value only, one line) → save as
   `publish-token.txt` in the same folder, "All files".

### A5. Upload Dirt Digger 0.2.0 and make it live

The build is already packaged and signed in `E:\Game Development\DirtDigger-Builds\0.2.0`
(423 MB, 97 chunks, from game commit 2cf9a02).

1. Start menu → **Terminal** (or PowerShell).
2. Run:

   ```powershell
   cd "E:\Game Development\Voxel Mining"
   node Tools\release\release.mjs publish --version 0.2.0 --channel stable
   ```

3. You should see `uploading 97 chunk(s) to bucket …`, progress every 10 chunks,
   then `bucket: 97 uploaded (400 MiB)` and `published 0.2.0 to stable (live)`.
   Running it again only uploads what's missing; publishing the same version
   twice is refused on purpose.
4. Check: <https://www.zirtola.com/api/launcher/v1/catalog> shows
   `"latest_version":"0.2.0"` for `dirt-simulator`. On the bucket's **Files** tab
   you should see a `dirt-simulator/chunks/` folder.

### A6. Release the launcher (first public installer)

The release workflow signs the launcher's auto-updates with your updater key.
GitHub needs that key once.

1. <https://github.com/trendlinepros-afk/zirtola-launcher> → **Settings** →
   **Secrets and variables** → **Actions** → **New repository secret**:
   - Name `TAURI_SIGNING_PRIVATE_KEY`; value: open
     `C:\Users\Wicked\.zirtola-launcher-secrets\updater.key` in Notepad, select
     all, copy, paste. **Add secret**.
   - Name `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; value: the contents of
     `updater.key.password`. **Add secret**.
   (Leave `ZIRTOLA_SIGN_MODE` unset until the Azure code-signing certificate is
   ready: the installer is unsigned for now, so Windows shows the SmartScreen
   notice. That's expected, and Part B checks it.)
2. In Terminal:

   ```powershell
   cd "E:\Game Development\Zirtola Launcher"
   git checkout main
   git pull
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. GitHub → **Actions** → **Release** runs (about 15–20 minutes). When it's green:
   **Releases** → the draft **Zirtola Launcher v0.2.0**. Its assets should include
   `Zirtola.Launcher_0.2.0_x64-setup.exe`, `….exe.sig`, `latest.json`,
   `installer.json` and `SHA256SUMS.txt`.
4. Click **Edit** (pencil) → **Publish release**.
5. Check: <https://www.zirtola.com/download/launcher> downloads
   `Zirtola.Launcher_0.2.0_x64-setup.exe`, and
   <https://www.zirtola.com/help/installing-on-windows> shows its SHA-256 (the
   website caches it for up to an hour).

### A7. Give the test account a licence (buying is still "Coming soon")

Use a **new** website account for the test, so you see exactly what a new player
sees. Create it in Part B step B3, then come back here:

1. Sign in to zirtola.com with **your admin account** → **Studio** →
   **Licensing** (<https://www.zirtola.com/admin/licensing>).
2. In **Give a license**: **Member** = the test account's username, **Game** =
   Dirt Digger, **Type** = Grant (leave Beta unticked) → **Give license**.
3. The licence appears in the list under that member. Revoke it after the test
   from the member's page (**Studio → Licensing → the member → Revoke**) if you like.

---

## Part B: the test, as a new player

Do this on a **new Windows user account** so nothing from your own setup
(launcher data, browser sign-ins) helps. Create one:
**Settings → Accounts → Other users → Add account → I don't have this person's
sign-in information → Add a user without a Microsoft account** → name `Tester`
→ **Next**. Sign out of yours (Start → your name → **Sign out**) and sign in as
`Tester`.

| Step | Do | You should see |
| --- | --- | --- |
| B1 | Open Edge, go to **www.zirtola.com/games/dirt-digger**. | Dirt Digger page: $19.99, **Purchase** greyed out with **Coming soon**, "Still in development" notice, System requirements section. |
| B2 | Under the price, click **Get the Zirtola Launcher** (or open www.zirtola.com/download/launcher). | Edge downloads `Zirtola.Launcher_0.2.0_x64-setup.exe` (about 3.7 MB). Edge may say it "isn't commonly downloaded": **…** → **Keep** → **Keep anyway**. |
| B3 | Before running it: in another tab, **Join** on zirtola.com and create a new account (e.g. `tester-<date>`), confirm the email. Then do **Part A7** from your admin account in a private window to give it a licence. | The test account exists and has a Dirt Digger licence. |
| B4 | Open the downloaded installer. | Blue **Windows protected your PC**. Click **More info**: App = `Zirtola.Launcher_0.2.0_x64-setup.exe`, Publisher = **Unknown publisher**. Click **Run anyway**. (The same screens as on www.zirtola.com/help/installing-on-windows.) |
| B5 | Installer: **Next** → keep the folder → **Install** → **Finish**. | No administrator prompt (it installs for this Windows user only). The launcher opens: "Sign in to Zirtola". |
| B6 | **Sign in with your browser**. | Edge opens zirtola.com: sign in as the test account, then **Sign in to the launcher**. The page says "You're signed in". Back in the launcher: Dirt Digger in the library, "Granted by the studio", **Install**. A short "Checking for updates…" line may show at the top. |
| B6b | (First start only) the launcher shows **Dirt Digger: what it needs**. | Four rows: Windows, Microsoft Visual C++ Runtime, graphics card (DirectX 12, Shader Model 6), free disk space. On a new user account of your PC they're all green: click **Continue**. On a brand-new PC the runtime may be red: click **Install the missing components**, then **Yes** on Windows' prompt; the row turns green. |
| B7 | Click the key button (tooltip **View license key**). | "This game is tied to your Zirtola account. No key needed.", From: Gift from the studio, Activation: not active on any PC yet. **Close**. |
| B8 | **Install** → keep `C:\Zirtola Games\Dirt Digger` → **Install**. Halfway, click **Pause**, wait, then **Resume install**. | Progress with MB, speed and time left; Pause shows "paused, resume any time"; Resume carries on from where it stopped. 423 MB in total. "Dirt Digger is installed." |
| B9 | Click the checkmark button (**Verify and repair files**). | "All 30 files are intact." |
| B10 | **Play**. | The game starts (Shipping build 0.2.0). The first start can take 20–40 seconds (shader compilation). Play a minute; save a site if you like; quit the game. Back in the launcher: hours played updates. |
| B11 | Notes button (**What's new**). | 0.2.0 "Gold" notes with **Installed** on 0.2.0. |
| B12 | Close the launcher (X), then start it again from the Start menu. | "Checking for updates…" at the top for a moment, then nothing to update (the launcher and the game are current). Signed in still. |
| B13 | Optional, the update path: on your own account publish a new build, e.g. `0.2.1` (A5 with `--version 0.2.1`, after packaging it), then restart the launcher as Tester. | **Update available · 0.2.1 · What's new**, and with "Update games automatically" on (Settings, default) it updates by itself, downloading only changed chunks. Your saves are untouched. |
| B14 | Website as Tester: **Settings → My games & keys**. | Dirt Digger, **Active devices**: this PC with Windows version and last played; **Deactivate** is there. |
| B15 | Launcher: trash-can button (**Uninstall**). | "Also delete saves" is **unticked**, with the save folder shown (`C:\Users\Tester\AppData\Local\GranitePit\Saved`). Uninstall without ticking it. |

### If something goes wrong

- **Sign-in page doesn't open, or "Can't reach Zirtola"**: check
  <https://www.zirtola.com/api/launcher/v1/config> shows JSON.
- **Install fails with "Downloads aren't configured yet"**: A3 isn't deployed yet.
- **"chunk … is missing on the server"**: A5 didn't finish uploading; run it again.
- **A red row on the requirements screen**: the runtime can be installed from
  there (with your permission and Windows' prompt). Windows, graphics card and
  disk space can't; the row says what to do (update Windows or the graphics
  driver, free up space).
