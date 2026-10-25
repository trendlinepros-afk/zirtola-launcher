# Changelog

## Unreleased

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
