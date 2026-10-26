# Getting the launcher to players before we have a code-signing certificate

**Question.** Can players on Windows get the Zirtola Launcher legitimately and
professionally before we have our own code-signing certificate and SmartScreen
reputation? The owner was told that some distribution methods "don't need the
cert". This document checks each candidate against current primary sources and
only recommends honest routes. Hiding, suppressing or working around the
SmartScreen warning is ruled out.

Checked on 2026-10-26 against Microsoft Learn, Microsoft Store Policies 7.20,
Tauri 2 docs, Steamworks and itch.io. Re-check prices and policies before you
commit money.

## Short answer

- **One channel really avoids the warning without a certificate: the Microsoft
  Store with an MSIX package.** Microsoft signs the package and Store installs
  never show SmartScreen.
  - But Store policy does not allow *this* launcher. A Store app may only offer
    our other products if those are also distributed and acquired through the
    Store (policy 10.1.5). A "storefront" exception needs at least 20 products
    (10.1.6), and we have two.
  - Also, Tauri does not build MSIX.
- **winget, Steam and the itch app are real, legitimate channels** that do not
  need our certificate:
  - winget delivers the launcher through Microsoft's package manager. Installs
    started by winget do not go through the browser download plus Explorer
    double-click path where SmartScreen asks.
  - Steam and itch deliver the games, not the launcher.
- **The other ideas do not help:**
  - A zip keeps the warning: Explorer copies the Mark of the Web onto extracted
    files.
  - A small bootstrapper is just another unsigned download, so it gets the same
    warning.
  - A web page cannot install a native app.
- **A certificate is still the real fix for website downloads**, and it is
  cheaper and faster than it sounds:
  - Azure Artifact Signing costs about $9.99/month. Microsoft's guide says an
    individual's ID check finishes within minutes of the ID-and-selfie check
    passing.
  - Individuals must be in the US or Canada. Elsewhere, buy an OV certificate
    with cloud signing.
  - Even signed, a new publisher sees warnings until reputation builds. Microsoft
    says this can take "several weeks and hundreds of clean installs".

**Recommendation:** ship now through **our website and winget**, with honest
first-run guidance on the website, and **start Artifact Signing (or an OV
certificate) today** so website downloads are signed within days. Details and
steps are at the end.

## Background: what triggers the warning, and what does not

- **Mark of the Web (MotW).** Browsers tag downloaded files with a
  `Zone.Identifier` alternate data stream. When a tagged program is started
  through the Windows shell (a double-click in Explorer, `ShellExecute`),
  SmartScreen checks its reputation. Starting it with `CreateProcess` (for
  example from a console or another program) does not trigger the check.
  Explorer's built-in zip extraction copies the tag onto extracted files.
  ([Eric Lawrence, "Downloads and the Mark-of-the-Web"](https://textslashplain.com/2016/04/04/downloads-and-the-mark-of-the-web))
- **How reputation builds.** SmartScreen looks at publisher reputation (the
  certificate) and file-hash reputation:
  - Unsigned files start from zero with every new version.
  - Signed files can inherit the publisher's reputation.
  - EV certificates have not bypassed SmartScreen since 2024.
  - "There is no need (or mechanism) to manually submit a file for SmartScreen
    reputation review."

  ([SmartScreen reputation for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation))
- **Games downloaded by our launcher are not tagged.** The launcher writes those
  files itself, so no browser adds MotW. Players do not get a SmartScreen prompt
  when the launcher starts a game. This is how Steam, Epic, GOG and itch work, and
  it is a property of the platform, not a trick.
- **Smart App Control caveat (Windows 11).** Where Smart App Control is on, it
  "will block execution of unsigned files unless the file has a positive
  reputation", for **all** executables, not only downloads (same Microsoft page).
  It is off on most PCs, but where it is on, an unsigned launcher or unsigned game
  `.exe` will not start. This is the strongest argument for signing the launcher
  and, later, the game binaries.

## The routes

### 1. Microsoft Store (MSIX)

| | |
| --- | --- |
| **Cost** | Free for individual developers (no registration fee since September 2025, ID + selfie check). Companies pay $99 once. ([Windows Developer Blog](https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/)) |
| **Signing** | "If you publish your app as an MSIX package through the Microsoft Store, code signing is free and handled for you automatically." EXE/MSI submissions must instead be signed with a certificate from a Trusted Root Program CA, so they do **not** avoid the certificate. ([Code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options); Store Policy 10.2.9) |
| **What players see** | A Store page and a one-click install, with no SmartScreen ("Store-distributed apps … are never subject to SmartScreen download warnings"). |
| **Policy fit for this launcher** | **Not allowed as designed.** Policy 10.1.5: a product may enable acquisition of "other products published by you as long as the other products are also distributed through the Store and the acquisition of those products is through the Store." The storefront exception (10.1.6) needs "a minimum of 20 distinct products", and a single franchise does not count. Policy 10.2.5: games in the Store must be "installed and updated only through the Store". A Zirtola launcher that downloads Zirtola games from our CDN and sells them on our website breaks 10.1.5. ([Microsoft Store Policies](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies)) |
| **Tauri support** | Tauri 2 does not build MSIX: "Currently Tauri only generates [EXE and MSI] installers." Its Store guide uses the EXE/MSI path, which needs our own signature. ([Tauri: Microsoft Store](https://v2.tauri.app/distribute/microsoft-store/)) A hand-made MSIX (MSIX Packaging Tool or `makeappx`) is possible, but it would not fix the policy problem. |
| **Updates** | The Store updates MSIX apps. The Tauri updater cannot replace files of an MSIX install (our understanding: MSIX packages live in the protected `WindowsApps` folder). It would have to be turned off in a Store build. |
| **Time to first install** | Account sign-up plus certification, typically days. Moot here. |
| **Effort** | High: a separate MSIX package, a Store build flavour without the updater, and a launcher redesign that limits it to Store-acquired games. |
| **Verdict** | Not for the launcher. The legitimate Store option is to publish **the games themselves** as MSIX products bought and updated through the Store. That is a separate business decision (Store pricing, Microsoft's revenue share, Store licensing instead of our accounts), not a way to ship the launcher. |

### 2. winget (Windows Package Manager)

| | |
| --- | --- |
| **Cost** | Free. |
| **Signing** | Not required by the repository. Every submission is validated automatically: installer scans by several antivirus engines and Defender, URL reputation, a silent install/uninstall test, and the installer must come "directly from the publisher's website". Then a moderator reviews it. ([Submit to the winget repository](https://learn.microsoft.com/en-us/windows/package-manager/package/repository)) |
| **What players see** | They run `winget install Zirtola.ZirtolaLauncher` (or use a winget front end such as UniGetUI) and the launcher installs silently. winget writes MotW on its downloads, but it starts the installer itself, not through an Explorer double-click, so the browser-download SmartScreen prompt does not appear. Microsoft does not document this as a SmartScreen exemption; it follows from how MotW works (see Background). Smart App Control still applies on PCs where it is on. |
| **Time to first install** | One pull request to `microsoft/winget-pkgs`. Validation runs automatically and a moderator approves it, usually within days. |
| **Updates** | The launcher's own Tauri updater keeps working (the install is the same NSIS install). Also add each new version to winget so `winget upgrade` works. A bot such as `wingetcreate` or Komac can open those PRs from CI. |
| **Policy risk** | Low. Requirements: an honest manifest, an installer URL on our release page, silent install supported (our NSIS installer supports `/S`), clean uninstall. |
| **Effort** | Small. **Prototyped on this branch:** `scripts/winget-manifest.mjs` writes the three manifest files for a release, and `winget validate` passes on the 0.1.0 build. |
| **Verdict** | **Use it.** It is the only certificate-free channel that installs *the launcher* without a warning. It is a command-line route, though, so it complements the website rather than replacing it. |

### 3. Steam (games, or a free "launcher app")

| | |
| --- | --- |
| **Cost** | $100 per product, recoupable after $1,000 revenue. ([Steam Direct](https://partner.steamgames.com/steamdirect)) |
| **Time to first release** | Paperwork 15–30 days, then a 30-day wait after paying the fee, a public "coming soon" page for at least 2 weeks, and a 1–5 day review. Six to eight weeks in practice. |
| **What players see** | The signed Steam client installs and updates the game. No SmartScreen for the game (files written by Steam are not tagged; see Background). |
| **Our accounts and launcher** | Selling games on Steam means Steam's checkout and Steam keys. We can sell Steam keys on our website too, with price parity ("don't give Steam customers a worse deal") and limits. ([Steam keys](https://partner.steamgames.com/doc/features/keys)) Putting the Zirtola Launcher itself on Steam only so it can download games from outside Steam adds nothing for players and is not a sensible use of Steam. |
| **Updates** | Steam (SteamPipe). Our chunk/patch pipeline would not be used for Steam copies. |
| **Verdict** | A legitimate *additional* channel for the **games**, with real reach. It does not get the launcher onto PCs. Worth doing per game when a game is ready for a store page. |

### 4. itch.io and the itch app

| | |
| --- | --- |
| **Cost** | Free to list. We choose itch's share of each sale (0–100%, default 10%), plus payment fees. ([itch.io open revenue sharing](https://itch.io/updates/open-revenue-sharing)) |
| **What players see** | A browser download from itch is an ordinary download: unsigned `.exe` files get the warning. Players report that games installed through the itch desktop app run without the prompt, which is consistent with how MotW works. itch does not document this officially. |
| **Updates** | `butler` uploads patches; the itch app updates games. Again the games, not our launcher. |
| **Verdict** | A reasonable channel for **game** demos and early access with no wait. Not a way to ship the launcher. |

### 5. Portable zip

| | |
| --- | --- |
| **Cost / effort** | Free and trivial (zip the release folder). |
| **What players see** | Explorer's built-in extraction copies the MotW onto the extracted `.exe`, so starting it from Explorer shows the **same** SmartScreen prompt. Some third-party unzip tools have historically dropped MotW, and that is treated as a security bug: 7-Zip has had CVEs for it. |
| **Updates** | Tauri's updater targets the NSIS install, so a portable build would need its own update path. |
| **Verdict** | **No.** It does not remove the warning, and relying on unzip tools that lose MotW would be exploiting a security bug, which is exactly what we ruled out. |

### 6. Web-only or bootstrapper flow

| | |
| --- | --- |
| **Web-only** | A website (or PWA) cannot install a native launcher or write multi-GB game files and start `.exe` files. Not possible. |
| **Bootstrapper** | A tiny downloader `.exe` is still an unsigned file downloaded through the browser, with the same warning and one more step. The Store's EXE path even forbids stub installers ("not a downloader stub/web installer", 10.2.9). |
| **Verdict** | **No.** |

### 7. Signing options (the real fix for website downloads)

| | Azure Artifact Signing | OV certificate (cloud HSM) | SignPath Foundation |
| --- | --- | --- | --- |
| **Cost** | ~$9.99/month | ~$150–300/year (Microsoft's figure). Some CAs advertise cloud OV from about €49/year; verify the price and that it supports Authenticode before buying. | Free |
| **Eligibility** | Organisations in the US, Canada, EU, UK, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway, Israel. **Individuals: US and Canada only.** The Azure billing account must be of type Individual, with legal name and address matching the ID. ([Artifact Signing quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)) | Anyone, worldwide | Open-source projects only: "an OSI-approved Open Source license without commercial dual-licensing", already released, built from source in CI, manual approval per release. ([SignPath Foundation terms](https://signpath.org/terms)) The launcher is currently proprietary. |
| **Time to first signed build** | Individuals: identity check through Microsoft Verified ID (ID plus selfie in Microsoft Authenticator). Microsoft's guide says the portal shows Completed "a couple of minutes" after a successful check. Organisations: "1 to 20 business days". Once validated, our existing `ZIRTOLA_SIGN_MODE=azure` pipeline signs on the next tag. | "Several business days" of validation | Application review, then per-release approval |
| **What players see** | "Unknown publisher" becomes **our verified name**. Expect SmartScreen's "unrecognized app" prompt for the first weeks. A consistent identity lets reputation carry over to later versions. | Same | The certificate names SignPath Foundation rather than Zirtola (our reading), which looks less professional. Same reputation behaviour. |
| **Effort in this repo** | None beyond setting the secrets: `scripts/sign.ps1` and `release.yml` already support it (docs/SIGNING.md). | Small (`thumbprint` mode, or adapt `sign.ps1` to the CA's signing tool) | Medium (license change, SignPath pipeline) |

## Recommendation

**Ship now through the website and winget, and start Azure Artifact Signing (or
an OV certificate if not eligible) today.**

- winget is the only legitimate certificate-free route that puts the launcher on
  a PC without a SmartScreen prompt.
- The website stays the main route. For the few days until signing is live, the
  page should say plainly what Windows will show.
- Signing is the only fix for the website download, and it also protects
  against Smart App Control.

The Store, Steam and itch are channels for the games, not for this launcher.

### Steps

1. **Today (owner):** check eligibility.
   - If you are an individual in the US or Canada (or a registered company in
     one of the listed countries), create an Azure subscription with an
     *Individual* billing account and an Artifact Signing account, then run the
     ID check. Follow docs/SIGNING.md.
   - Otherwise, order an OV code-signing certificate with cloud signing from a
     CA that supports GitHub Actions.
2. **As soon as validation completes:** add the `AZURE_*` secrets and
   variables (docs/SIGNING.md), push a new `vX.Y.Z` tag, and check that the
   release workflow's "Verify the Authenticode signature" step says `Valid`.
   From then on every release is signed with the same identity.
3. **Publish the first release** (docs/RELEASING.md): publish the draft GitHub
   Release.
4. **winget** (owner, with their GitHub account; nothing has been submitted):
   1. Run `node scripts/winget-manifest.mjs <version> "<path to the release -setup.exe>"`.
   2. Run `winget validate packaging/winget/out/<version>`.
   3. Optionally test with `Tools/SandboxTest.ps1` from a clone of
      `microsoft/winget-pkgs`.
   4. Open a pull request adding the three files under
      `manifests/z/Zirtola/ZirtolaLauncher/<version>/`.
   5. For later releases, automate the same PR with `wingetcreate update` in
      `release.yml` once the first version is accepted.
5. **Website download page:**
   - Primary button: the signed installer from GitHub Releases (unsigned until
     step 2 lands).
   - Second line: "Prefer the command line? `winget install Zirtola.ZirtolaLauncher`".
   - Until signing is live, an honest note: Windows may show "Windows protected
     your PC" for a new app, how to verify the download (the SHA-256 from the
     release page), and where the source and builds come from.
   - Do not tell players to turn off SmartScreen or Smart App Control.
6. **Next (games):**
   - Sign the game executables too, ideally in the `zirtola-publish` pipeline
     before chunking, with the same identity, so Smart App Control and
     reputation work for the games.
   - Decide per game whether a Steam page (and later an itch page for demos) is
     worth adding alongside the launcher.

## Gemini's claims, checked

| Claim | Verdict |
| --- | --- |
| "The Microsoft Store doesn't need a certificate" | **True for MSIX only.** EXE/MSI Store submissions must be signed. And policy 10.1.5/10.1.6 rules out this launcher, which downloads our games from outside the Store. |
| "winget doesn't need a certificate" | **True.** It is free and validated by Microsoft's scans. It suits technical users, so it complements the website rather than replacing it. |
| "Ship on Steam/itch instead" | **True for games**, at Steam's cost and timeline. It does not distribute the launcher. |
| "A zip avoids SmartScreen" | **False.** Explorer copies MotW to extracted files. Tools that drop it have had that treated as a security bug. |
| "A web installer/bootstrapper avoids it" | **False.** The bootstrapper is itself an unsigned download. |
| "EV certificate removes the warning" | **No longer true** since 2024 (Microsoft Learn). |
