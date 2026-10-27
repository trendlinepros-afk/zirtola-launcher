# New-user test: from zirtola.com to playing Dirt Digger

Two parts. **Part A** is one-time setup on Railway and GitHub (about 45
minutes, most of it waiting). **Part B** is the test itself, as a brand-new
player on a clean Windows user account (about 20 minutes).

Secrets stay in `C:\Users\Wicked\.zirtola-launcher-secrets\`. Never paste them
into chat, a repo or a screenshot.

---

## Part A: one-time setup

### A1. Merge the pull requests

1. Website: <https://github.com/trendlinepros-afk/retail-store/pull/28> (owner-only
   launcher downloads) → **Merge pull request** → **Confirm merge**. Railway
   redeploys the site by itself (2–4 minutes). No database changes.
2. Launcher: <https://github.com/trendlinepros-afk/zirtola-launcher/pull/5>
   → **Merge pull request** → **Confirm merge**.
3. If the website service still has a `LAUNCHER_DOWNLOAD_URL` variable, delete it
   (Variables tab → ⋮ → Remove); it isn't used any more.

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

### A6. Release the launcher to the bucket (never GitHub)

The launcher is built and signed on GitHub Actions and uploaded to the same
bucket as the game. GitHub never hosts a download, so the repository can be
private. GitHub needs six secrets once:

1. <https://github.com/trendlinepros-afk/zirtola-launcher> → **Settings** →
   **Secrets and variables** → **Actions** → **New repository secret**, one by one:

   | Name | Value |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | open `C:\Users\Wicked\.zirtola-launcher-secrets\updater.key` in Notepad, select all, copy, paste |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the contents of `updater.key.password` |
   | `LAUNCHER_BUCKET_ENDPOINT` | from the bucket's **Credentials** tab on Railway (e.g. `https://t3.storageapi.dev`) |
   | `LAUNCHER_BUCKET_NAME` | the `BUCKET` value from the Credentials tab |
   | `LAUNCHER_BUCKET_REGION` | `auto` |
   | `LAUNCHER_BUCKET_ACCESS_KEY_ID` | the access key id from the Credentials tab |
   | `LAUNCHER_BUCKET_SECRET_ACCESS_KEY` | the secret access key from the Credentials tab |

   Only if the Credentials tab says **path style**: **Variables** tab → **New
   repository variable** `LAUNCHER_BUCKET_PATH_STYLE` = `true`.
   Leave `ZIRTOLA_SIGN_MODE` unset until the Azure code-signing certificate is
   ready: the installer is unsigned for now, so Windows shows the SmartScreen
   notice. That's expected, and Part B checks it.
2. In Terminal:

   ```powershell
   cd "E:\Game Development\Zirtola Launcher"
   git checkout main
   git pull
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. GitHub → **Actions** → **Release** runs (about 15 minutes). When it's green its
   summary says "Launcher 0.2.0 uploaded. Players don't have it yet".
4. Make it live: **Actions** → **Release** → **Run workflow** → **promote** =
   `0.2.0` → **Run workflow**. About a minute later that run is green: the bucket's
   pointer `launcher/latest.json` now names 0.2.0.
5. Check, signed in to zirtola.com as **your** account (it owns the game):
   **Download Launcher** at the top of the site downloads
   `Zirtola.Launcher_0.2.0_x64-setup.exe`, and
   <https://www.zirtola.com/help/installing-on-windows> shows its SHA-256.

**How the latest release is chosen:** the site and the launcher's update check
only ever read the pointer `launcher/latest.json` in the bucket, so they serve
exactly the version you last promoted. Each release's files are uploaded first
and the pointer moves last, so a half-uploaded release is never served. Old
versions stay in the bucket; to roll back, run **promote** with the older
version. The site picks up a change within a minute; no redeploy.

**When the repository can go private:** once this is merged, the Release run in
step 3 has uploaded 0.2.0 and step 4 has promoted it. From then on nothing
players use comes from GitHub. (Launchers built before this change checked
GitHub for updates; none are with players. Uninstall any test install and
install again from zirtola.com.)

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
| B1 | Open Edge, go to **www.zirtola.com/games/dirt-digger**. | Dirt Digger page: $19.99, **Purchase** greyed out with **Coming soon**, "Still in development" notice, System requirements section. There is **no** launcher link in the price box (players get it with a game). |
| B2 | Click **Download Launcher** at the top of the site while signed out. | You're sent to **Sign in**. Click **Join** and create a new account (e.g. `tester-<date>`); confirm the email. |
| B3 | Signed in as the new account, click **Download Launcher** again. | "The launcher comes with your first game", with **See the games** and **Redeem a key**. No file downloads. Now do **Part A7** from your admin account (a private window) to give this account a licence. |
| B3b | Click **Download Launcher** once more. | Edge downloads `Zirtola.Launcher_0.2.0_x64-setup.exe` (about 3.7 MB) from zirtola.com's download storage. Edge may say it "isn't commonly downloaded": **…** → **Keep** → **Keep anyway**. |
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
