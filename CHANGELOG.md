# Changelog

## Unreleased

- One device per licence: playing on a second PC shows who has it ("Dirt Digger is
  active on <device> (last used <date>)") with **Deactivate and play here**; device ids
  come from the firmware UUID so reinstalls and upgrades keep the activation.
- Offline play keeps working for the licence's grace period (7 days by default).
- **What's new** per game (newest first, installed version marked), opened by itself
  after an update; **Beta** toggle for accounts with beta access.
- **View license key** button for owned games: masked key with Show and Copy, where
  the licence came from, and which PC it is active on. Fetched live, never stored.
- Games not on sale yet show a disabled **Buy · price** button with "Coming soon".
- Game page: **System requirements** link (opens the website's section) with the
  minimum spec inline, and **Report a bug** (the game's forum) while a game is in
  early access. Both come from the catalogue (`requirements`, `early_access`,
  `bug_report_url`).
- Device dialogs say plainly that a licence moves between your PCs, with a
  **Learn more** link to the website's FAQ.
- Every start checks for a launcher update and for game updates (entitlements and
  each owned game's latest manifest) in parallel before the library shows as
  current. Launcher updates install and restart when allowed, and always when the
  version is below the website's `min_launcher_version`. Game updates show
  "Update available · What's new" and start by themselves when **Update games
  automatically** is on (new setting). Play re-checks if the last check is over
  10 minutes old. Offline, a small "Couldn't check for updates" note; play is
  never blocked.
- Updates, verify, repair and uninstall only ever touch files the manifest lists
  inside the install folder, and never follow a link or junction out of it.
  Uninstall asks "Also delete saves?" (unticked) and shows where the saves are.
- Releases publish `installer.json` and `SHA256SUMS.txt` (the setup file's
  SHA-256) for the website's download page.
- The Store button never fails silently: it falls back to the site's games page and
  shows a message when the browser can't be opened.
- Production manifest and launch keys (2026-10) pinned; the repository is now
  `trendlinepros-afk/zirtola-launcher`.
- Buying: locked games show a lock badge and "Buy on the website" (with the price
  when the website provides one); after clicking it the launcher waits for the
  purchase (every 7 s for up to 15 minutes, immediately on window focus and on
  `zirtola://refresh` links) and highlights the game with Install when it is owned.
- `zirtola://` links: registered by the installer; `zirtola://refresh?game=<id>`
  refreshes the library and selects the game.
- Hardware info (Settings → Privacy, on by default, "Show what is sent").
- Crash reports: a prompt to send the Unreal crash folder (log, crash context,
  minidump) with an optional comment when a game crashes; recent unsent crashes
  are offered at start-up.
- Device names in the activations list are now a hardware label instead of the
  PC name.

## 0.1.0

First version of the Zirtola Launcher:

- Sign in with a Zirtola account in the system browser (OAuth 2.0 + PKCE).
- Library of the studio's games; owned games install, others link to the store.
- Chunked, parallel, resumable downloads with SHA-256 checks; pause and resume.
- Patching downloads only changed chunks; verify and repair; uninstall.
- Launch with a short-lived session token for the game's license check.
- Redeem game keys.
- Self-update from GitHub Releases with signed update metadata.
- Controller support (D-pad / stick navigation, A/B, LB/RB, Start, Y, View).
