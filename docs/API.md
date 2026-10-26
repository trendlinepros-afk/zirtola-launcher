# Zirtola website API for the launcher (contract v1)

The launcher talks to the studio website. This document is the contract between
them. It is implemented by:

- `mock-server/` in this repo (local development, zero dependencies), and
- the website (`retail-store` repo, `src/app/api/launcher` and `src/app/oauth`).

`scripts/e2e.mjs` checks the mock against this contract; `scripts/site-contract.mjs`
checks a running website (local or staging).

## Conventions

- **API base** (`{base}`): `https://zirtola.com/api/launcher` in production,
  `http://127.0.0.1:8787` for the mock. Baked into release builds via the
  `ZIRTOLA_API_BASE` build variable; dev builds also honour it at runtime.
- JSON everywhere except the OAuth token/revoke endpoints (form-encoded, RFC 6749).
- Authenticated calls send `Authorization: Bearer <access_token>`.
- Errors: HTTP status plus `{ "error": "<code>", "message": "<text for humans>" }`.
  The launcher shows `message` and branches on `error`.
- Binary values (keys, signatures, manifests) are unpadded **base64url**.
- Game ids are the website's game slugs (`[a-z0-9-]{1,64}`), e.g. `dirt-simulator`.
- Platform is `win64` today; branch is `live` unless an entitlement lists more.

| Status | `error` codes |
| --- | --- |
| 400 | `bad_request`, `invalid_grant`, `unsupported_grant_type`, `invalid_manifest` |
| 401 | `unauthorized` (access token missing/expired: refresh and retry once), `invalid_client` |
| 403 | `not_entitled` |
| 409 | `device_conflict` (another PC holds the licence; see launch tokens) |
| 404 | `not_found`, `no_build`, `invalid_key` |
| 409 | `key_already_redeemed`, `already_owned` |
| 410 | `key_expired` |
| 429 | `slow_down` |
| 503 | `launcher_not_configured` (a server-side key or CDN setting is missing) |

## Public endpoints

### `GET {base}/v1/config`

Service discovery. The launcher calls this first.

```json
{
  "issuer": "https://zirtola.com",
  "authorization_endpoint": "https://zirtola.com/oauth/authorize",
  "token_endpoint": "https://zirtola.com/oauth/token",
  "revocation_endpoint": "https://zirtola.com/oauth/revoke",
  "device_authorization_endpoint": null,
  "client_id": "zirtola-launcher",
  "scopes": ["launcher", "offline_access"],
  "store_url": "https://zirtola.com/games",
  "account_url": "https://zirtola.com/register",
  "support_url": "https://zirtola.com/community",
  "min_launcher_version": "0.1.0"
}
```

### `GET {base}/v1/keys`

Public Ed25519 keys as a JWK set. `use` is `manifest` (signs build manifests)
or `launch` (signs launch tokens). Keys are identified by `kid` so they can be
rotated: publish the new key, sign with it, retire the old one later.

```json
{ "keys": [ { "kid": "2026-01", "kty": "OKP", "crv": "Ed25519", "use": "manifest", "x": "<base64url 32 bytes>" } ] }
```

Release launchers do **not** trust this endpoint for manifest keys: production
public keys are pinned in `src-tauri/keys/trusted-keys.json` at build time. Only
development builds talking to a loopback API add the keys served here.

### `GET {base}/v1/catalog`

Games the launcher lists (published and visible in the launcher).

```json
{ "games": [ {
  "id": "dirt-simulator",
  "title": "Dirt Digger",
  "tagline": "Real dirt. Real gold.",
  "description": "…",
  "status": "early_access",            // released | early_access | in_development | on_hold | coming_soon
  "store_url": "https://zirtola.com/games/dirt-digger",
  "platforms": ["win64"],
  "latest_version": "1.0.1",           // live build on win64/live, or null
  "install_size": 389210112,           // bytes, or null
  "art": { "accent": "#FFCD00", "cover_url": "https://…/key-art.jpg" },
  "price": { "amount": 1999, "currency": "usd" },  // optional, minor units; shown on "Buy on the website"
  "sales_open": false,                 // false: disabled "Buy" with "Coming soon"
  "checkout_url": "https://zirtola.com/checkout/dirt-simulator",
  "features": ["…"],
  "early_access": true,                // shows "Report a bug" on the game page
  "bug_report_url": "https://zirtola.com/community/bugs?game=dirt-simulator",
  "requirements": {                    // optional: "System requirements" link + minimum line
    "url": "https://zirtola.com/games/dirt-digger#system-requirements",
    "minimum_target": "1080p · Low · 30 fps",
    "minimum_summary": "Ryzen 5 3600 / Core i5-10400 · 16 GB RAM · RTX 2060 / RX 6600 · SSD"
  }
} ] }
```

`save_location` (e.g. `%LOCALAPPDATA%\GranitePit\Saved`) is where the game keeps saves
and settings; the launcher shows it in the uninstall dialog and deletes it only if
the player ticks "Also delete saves" (it must resolve under `%LOCALAPPDATA%`,
`%APPDATA%` or `%USERPROFILE%`, at least two folders deep).

`early_access`, `bug_report_url`, `requirements` and `save_location` come from the website's
`src/content/game-specs.ts` (per game, in the repo, not the database).

**Buying from the launcher.** A locked game's button reads "Buy on the website · $24.99"
(price when provided) and opens `store_url`. The launcher remembers the game as a
pending purchase and, while it is pending, re-fetches `/v1/entitlements` every 7 s
for up to 15 minutes, immediately when the window regains focus, and on a
`zirtola://refresh[?game=<id>]` link (registered by the installer). As soon as the
entitlement shows up the game is highlighted with Install ready. Any page the site
shows after granting a licence can link to `zirtola://refresh?game=<id>`.

## Sign-in: OAuth 2.0 authorization code + PKCE

The launcher is a public client (`client_id = zirtola-launcher`, no secret).
It follows RFC 8252 (native apps): the system browser does the sign-in and
redirects to a loopback listener the launcher opens on a random port.

1. Launcher creates `code_verifier` (48 random bytes, base64url), `code_challenge =
   base64url(SHA-256(verifier))` and a random `state`, listens on
   `http://127.0.0.1:<port>/callback`.
2. Opens `GET {authorization_endpoint}?response_type=code&client_id=zirtola-launcher
   &redirect_uri=http://127.0.0.1:<port>/callback&scope=launcher offline_access
   &state=…&code_challenge=…&code_challenge_method=S256` in the browser.
3. The website signs the user in with its normal session (same account as the
   website), shows a consent screen, then redirects to
   `redirect_uri?code=…&state=…` (or `?error=access_denied&state=…`).
   Rules: only loopback `http://127.0.0.1` or `http://[::1]` redirect URIs with
   path `/callback`, any port; `S256` is required; codes are single use and
   expire in 2 minutes.
4. Launcher checks `state` and exchanges the code:

```
POST {token_endpoint}
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code&code=…&redirect_uri=…&client_id=zirtola-launcher&code_verifier=…
```

```json
{ "access_token": "…", "token_type": "Bearer", "expires_in": 900, "refresh_token": "…", "scope": "launcher offline_access" }
```

**Refresh** (`grant_type=refresh_token&refresh_token=…&client_id=zirtola-launcher`)
returns a new pair. Refresh tokens **rotate**: each can be used once. Presenting
a used one is treated as theft and revokes the whole token family (both the
mock and the website do this). Password resets, "sign out everywhere" and bans
on the website end launcher sessions too.

**Revoke**: `POST {revocation_endpoint}` with `token=<refresh_token>&client_id=…`
(RFC 7009), always 200. The launcher calls it on sign-out.

The launcher keeps the refresh token in Windows Credential Manager and the
access token only in memory.

*Device authorization grant (RFC 8628)* is reserved for a future console /
Steam Deck style flow: the mock implements `POST /oauth/device_authorization`
and the `urn:ietf:params:oauth:grant-type:device_code` grant; the website
leaves `device_authorization_endpoint` null for now.

## Authenticated endpoints

### `GET {base}/v1/me`

```json
{ "id": "ckx…", "display_name": "Miner49", "avatar_url": null }
```

### `GET {base}/v1/entitlements`

Games this account may install: purchases, redeemed keys and studio grants
(the website also counts approved early-access requests as grants).

```json
{ "entitlements": [ {
  "game_id": "dirt-simulator",
  "kind": "purchase",                 // purchase | key | grant
  "granted_at": "2026-09-01T12:00:00Z",
  "expires_at": null,
  "branches": ["live"]
} ] }
```

### `POST {base}/v1/keys/redeem`

```json
{ "key": "ABCD-EFGH-JKMN-PQRS" }
```

Keys are normalised (case and dashes ignored). Success:

```json
{ "entitlement": { "game_id": "alaskan-bush-simulator", "kind": "key", "granted_at": "…", "expires_at": null, "branches": ["live"] },
  "game_title": "Alaskan Bush Simulator" }
```

Errors: `404 invalid_key`, `409 key_already_redeemed` (used up), `409 already_owned`,
`410 key_expired`, `429 slow_down` (10 attempts per hour per account on the website).
A key can allow one account (default) or up to N accounts, and can expire.

### `GET {base}/v1/games/{id}/latest?platform=win64&branch=live`

Cheap update check.

```json
{ "game_id": "dirt-simulator", "version": "1.0.1", "build_id": "4f6879aa3580…", "total_size": 389210112,
  "released_at": "2026-10-25T15:00:00Z", "notes": "Patch notes" }
```

`403 not_entitled` if the account does not own the game, `404 no_build` if nothing is live.

### `GET {base}/v1/games/{id}/manifest?platform=win64&branch=live[&version=1.0.0]`

The signed manifest of the live build (or of `version`, used by Verify/Repair),
plus where to download chunks.

```json
{
  "version": "1.0.1",
  "signed_manifest": { "kid": "2026-01", "manifest": "<base64url manifest JSON bytes>", "signature": "<base64url Ed25519>" },
  "chunk_url_template": "https://cdn.zirtola.com/dirt-simulator/chunks/{prefix}/{hash}?t=…",
  "chunk_url_expires_at": "2026-10-25T21:00:00Z"
}
```

The signature covers the exact manifest bytes (no JSON canonicalisation).
Manifest format (`schema` 1):

```json
{
  "schema": 1, "game_id": "dirt-simulator", "version": "1.0.1", "build_id": "4f6879aa…",
  "platform": "win64", "created_at": "2026-10-25T14:58:00Z",
  "launch": { "executable": "DirtSimulator.exe", "args": [], "working_dir": null },
  "total_size": 389210112,
  "files": [ { "path": "DirtSimulator/Content/Paks/pak0.pak", "size": 338690048, "sha256": "…",
               "chunks": [ { "hash": "<sha256 hex>", "offset": 0, "size": 4194304 } ] } ]
}
```

Rules the launcher enforces after checking the signature: relative paths only
(no `..`, drive letters, `:` streams, reserved device names, duplicates),
chunks contiguous and covering each file, chunk sizes 1 byte to 256 MiB,
`total_size` equal to the sum of files.

**Chunks** are content-addressed: `GET` the template with `{hash}` (SHA-256 hex of
the chunk bytes) and `{prefix}` (its first two hex characters) substituted.
Chunks are immutable, so the CDN may cache them forever. `401/403` means the
link expired: the launcher fetches the manifest endpoint again for a fresh
template and resumes. Servers should support `Range` (the mock does).

### `POST {base}/v1/launch-tokens`

```json
{ "game_id": "dirt-simulator", "device_id": "<device id, 16-128 base64url chars>", "device_name": "Windows 11 Pro · NVIDIA GeForce RTX 3070" }
```

```json
{ "token": "<JWT>", "expires_at": "2026-10-25T15:35:00Z" }
```

`device_id` is a salted SHA-256 of stable hardware ids (the SMBIOS system UUID,
else Windows' MachineGuid), computed on the PC; the raw ids never leave it, and
the website stores only a hash of the id. It survives reinstalling the
launcher and Windows updates, so a reinstall never uses up an activation.
`device_name` is a hardware label (OS + GPU), never the computer or user name.

Each (account, game, device) is an **activation**. A licence is good for one
device by default (`Game.maxActivations`, set per game in Studio). Launching on
another PC while one is active answers:

```json
409 { "error": "device_conflict",
      "message": "Dirt Digger is active on Windows 10 Home · GTX 1060 (last used Oct 23, 2026). Deactivate it to play here.",
      "limit": 1,
      "active_devices": [ { "id": "<activation id>", "name": "…", "activated_at": "…", "last_seen_at": "…" } ] }
```

The launcher shows that message with a "Deactivate and play here" button (the
player confirms), which calls:

### `POST {base}/v1/activations/deactivate`

`{ "activation_id": "…" }` → `{ "ok": true }`. Frees the slot at once.
Rate-limited per licence (`LAUNCHER_DEACTIVATIONS_PER_PERIOD`, default 3 per
`LAUNCHER_DEACTIVATION_PERIOD_DAYS`, default 30): over the limit answers
`429 deactivation_limit` with a message pointing to support. Players can also
deactivate from Settings → My games & keys on the website; staff from Studio →
Licensing (not rate-limited). Every activation, block and deactivation is logged.

`GET {base}/v1/activations[?game_id=]` lists the account's active devices.

The token is a compact JWS, header `{"alg":"EdDSA","typ":"JWT","kid":"…"}`,
signed with the `launch` key:

```json
{ "iss": "https://zirtola.com", "sub": "<account id>", "aud": "zirtola-game:dirt-simulator",
  "game": "dirt-simulator", "ent": "purchase", "iat": 1761406200, "exp": 1761406500,
  "jti": "…", "name": "Miner49", "addons": [], "dev": "<device hash>", "grace": 604800 }
```

The launcher passes it to the game in the environment (see GAME_INTEGRATION.md).

### `POST {base}/v1/launch-tokens/verify` (public)

The game's online check: `{ "token": "…" }` → `{ "valid": true, "claims": { … } }`
or `{ "valid": false, "reason": "…", "claims": null }`. A token stays checkable
past its 5-minute `exp` for its offline grace period (`grace` seconds after
`iat`, from `LAUNCHER_OFFLINE_GRACE_DAYS`, default 7). Reasons:
`licence_revoked` (refunded / revoked), `device_deactivated` (this PC was
deactivated: the game should stop and ask the player to start it from the
launcher), `grace_expired`, `issued_in_future` (clock more than 2 minutes off).
Offline, games verify the signature themselves and accept the token until
`iat + grace` (`zirtola_core::token::verify_license`, see GAME_INTEGRATION.md).

### `GET {base}/v1/games/{id}/changelog?branch=live|beta` (public)

Release notes per version, newest first: `{ "game_id", "entries": [ { "version",
"channel": "stable"|"beta", "released_at", "notes": "<markdown>", "current" } ] }`.
`beta` includes stable releases. The launcher shows these under "What's new"
(and right after an update); the website shows the stable ones on the game page.

### `POST {base}/v1/hardware`

Sent on sign-in and when the hardware changes, only if the player leaves
"Share hardware info" on (default on, Settings → Privacy). Exactly these fields:

```json
{
  "device_id": "<device id>",
  "os": { "name": "Windows 11 Pro", "version": "24H2", "build": "26100.2314" },
  "cpu": { "model": "AMD Ryzen 7 7800X3D 8-Core Processor", "cores": 8, "threads": 16 },
  "ram_mb": 32768,
  "gpus": [ { "name": "NVIDIA GeForce RTX 4070", "vram_mb": 12282, "driver": "32.0.15.6094" } ],
  "displays": [ { "width": 2560, "height": 1440, "scale": 1.0, "refresh_hz": 0 } ],
  "install_drive_free_mb": 812345,
  "launcher_version": "0.1.0"
}
```

No serial numbers, MAC addresses, user or computer names, or file paths. → `{ "ok": true }`.

### `POST {base}/v1/crashes`

`multipart/form-data` with `metadata` (JSON) and `file` (zip of the Unreal crash
folder: `*.log` (last 8 MB), `CrashContext.runtime-xml`, `*.dmp` up to 40 MB;
total under 48 MB). Sent only after the player clicks "Send report".

```json
{ "source": "launcher", "game_id": "dirt-simulator", "game_version": "1.0.1", "build_id": "4f68…",
  "platform": "win64", "device_id": "…", "exit_code": 3, "comment": "optional, from the player",
  "crash_guid": "UECC-Windows-…_0000", "hardware": { …same shape as /v1/hardware, or {} if sharing is off… } }
```

→ `201 { "id": "…" }`. The server reads the error message and callstack from
`CrashContext.runtime-xml` (or the log) and groups reports by a hash of the
normalised top frames. `413 too_large`, `429 slow_down` (20 per hour per account).

### Crash reports without the launcher (website only)

- `POST {site}/api/crash/upload?key=<game crash key>`: same multipart body as
  above, sent by the game itself; include `launch_token` (the
  `ZIRTOLA_SESSION_TOKEN` it was started with) in `metadata` to tie it to the player.
- `POST {site}/api/crash/ue?key=<game crash key>`: Unreal's CrashReportClient
  `DataRouterUrl` target (compressed crash archive; unpacked best-effort, always stored).

The crash key is per game (Studio → Licensing → Launcher catalogue). See
GAME_INTEGRATION.md.

## Publishing builds (website only)

```
POST {base}/v1/admin/builds
Authorization: Bearer <LAUNCHER_PUBLISH_TOKEN>
{ "signed_manifest": { "kid", "manifest", "signature" }, "branch": "live", "notes": "…", "make_live": true }
```

`201 { "id", "game_id", "version", "live" }`. The website re-checks the
manifest signature against its configured manifest public keys, so a leaked
publish token alone cannot ship a build. Staff can also upload manifests and
switch the live build in Studio → Licensing → Launcher catalogue.
