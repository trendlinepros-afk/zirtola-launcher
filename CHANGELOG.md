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
