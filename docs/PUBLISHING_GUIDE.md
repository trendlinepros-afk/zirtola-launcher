# Publishing guide: selling Dirt Digger through the Zirtola Launcher

This guide is for doing it the first time. It covers what you set up once, what
you do for every release, what players see, and a launch-day checklist.
Where something happens on a website you control (Azure, Stripe, Railway,
Cloudflare, GitHub), it says where to click.

Words used below:

- **Launcher**: the Zirtola Launcher app players install.
- **Website**: zirtola.com (the `retail-store` repo, hosted on Railway).
- **Build**: one packaged version of the game (for example Dirt Digger 0.2.0).
- **Channel**: `beta` (testers) or `stable` (everyone). The website calls stable `live`.
- **Secret**: a value that must never be shared, posted, committed to git or
  pasted into a chat. Your secrets live in `C:\Users\Wicked\.zirtola-launcher-secrets\`.

---

## Part 1: one-time setup

Do these in order. Steps 1 to 3 can start today; each one takes a few days of
waiting on someone else, so start them early.

### 1. Code-signing certificate (Azure Artifact Signing)

Why: an unsigned installer shows "Windows protected your PC" and "Unknown
publisher". Signing shows your name and builds trust with Windows over time.
(Details and alternatives: `docs/SIGNING.md`.)

1. Go to <https://portal.azure.com> and sign in (create a free Microsoft account if needed).
2. Search for **Subscriptions** → **Add** → choose **Pay-As-You-Go**. Add a card.
   Cost: about **$9.99/month** for Artifact Signing Basic.
3. Search for **Artifact Signing accounts** (it may still say "Trusted Signing") → **Create**.
   - Resource group: **Create new** → `zirtola`
   - Account name: `zirtola-signing`
   - Region: pick the one closest to you (East US is fine) → **Review + create** → **Create**.
4. Open the new account → **Identity validation** → **New identity** → **Individual**
   (or **Organization** if you have a registered company). Fill in your legal name and
   address exactly as on your ID. Microsoft emails you to verify; this can take
   **1 to 10 working days**.
5. When the validation says **Completed**: **Certificate profiles** → **Create** →
   **Public Trust** → name `zirtola-launcher` → pick your validated identity → **Create**.
6. Search for **App registrations** → **New registration** → name `zirtola-launcher-ci` → **Register**.
   Copy the **Application (client) ID** and **Directory (tenant) ID**.
   Then **Certificates & secrets** → **New client secret** → copy the **Value** right away (shown once). This is a secret.
7. Back on the Artifact Signing account → **Access control (IAM)** → **Add role assignment** →
   role **Artifact Signing Certificate Profile Signer** → **Members** → select `zirtola-launcher-ci` → **Review + assign**.
8. Note the account's **Endpoint** (on its Overview page, like `https://eus.codesigning.azure.net`).

Turning signing on is then **one setting** (step 4 below adds them to GitHub):
`ZIRTOLA_SIGN_MODE = azure`. Until then releases still build, just unsigned.

Players see the "Windows protected your PC" notice until Windows trusts the
launcher. The website explains it with pictures ("What to expect when you
install", at zirtola.com/help/installing-on-windows, on the checkout page,
after payment and in the receipt). After your first signed release:

- Set `LAUNCHER_PUBLISHER_NAME` on Railway to the name on your certificate
  (for example `Zirtola`). The guide then shows that name instead of
  "Unknown publisher". Run `npm run guide:images -- --publisher "Zirtola"` in
  the website repo and commit, so the pictures in the receipt email match.
- SmartScreen still needs a few weeks of downloads to trust a new certificate.
  When players stop seeing the notice, set `SHOW_SMARTSCREEN_NOTICE=false`:
  the checkout panel goes away and the success page and receipt show a short
  install note. The help page stays.

### 2. Stripe (payments)

1. Go to <https://dashboard.stripe.com/register> and create the account (you, not me).
2. **Settings → Business details**: legal name, address, what you sell
   ("Downloadable video games for Windows").
3. **Settings → Payouts (Bank accounts and currencies)**: add the bank account Stripe pays you into.
4. **Settings → Tax**: turn on **Stripe Tax**, set your origin address, and add the
   registrations you have (your home state/country). Stripe then tells you when
   you get close to other countries' or states' thresholds. Digital games are
   taxed in many places (EU/UK VAT, some US states, Australia GST...). Ask an
   accountant once which registrations you need.
5. **Developers → API keys**: you'll need the **Secret key** (`sk_live_...`) for Railway. Secret.
   (Also try everything with the **Test mode** toggle on first; test keys start with `sk_test_`.)
6. **Developers → Webhooks → Add endpoint**:
   - URL: `https://www.zirtola.com/api/webhooks/stripe`
   - Events: `checkout.session.completed`, `checkout.session.async_payment_succeeded`,
     `checkout.session.async_payment_failed`, `checkout.session.expired`, `charge.refunded`,
     `charge.dispute.created`, `charge.dispute.closed`
   - After saving, **Reveal** the **Signing secret** (`whsec_...`). Secret.
   If you already set this up for donations, add the ones that are missing (at least
   `charge.refunded` and the two `charge.dispute` events): the same endpoint handles games.
   What they do: a refund or an opened chargeback takes the game away (licence, order
   key and device activations); a won chargeback gives it back. Bank payments that
   settle later unlock the game only when the money arrives. An order whose paid
   amount doesn't match is flagged in **Studio → Licensing → Sales** and not unlocked.

### 3. Railway Storage Bucket (where the game files are downloaded from)

Game files live in a private Railway Storage Bucket next to the website
(storage $0.015/GB-month, downloads free). The website hands the launcher
15-minute links to it, so only people who own the game can download. Setting
it up is click-by-click in **docs/NEW_USER_TEST.md, steps A2–A5**: create the
bucket, give the website its credentials, put the credentials and the publish
token in your secrets folder, then upload and publish a build with

```powershell
cd "E:\Game Development\Voxel Mining"
node Tools\release\release.mjs publish --version 0.2.0 --channel stable
```

### 4. GitHub (launcher releases)

Repo `trendlinepros-afk/zirtola-launcher` → **Settings → Secrets and variables → Actions**:

**Secrets** (New repository secret):

| Name | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | contents of `C:\Users\Wicked\.zirtola-launcher-secrets\updater.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | contents of `updater.key.password` in the same folder |
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` | from step 1.6 (when signing is ready) |

**Variables** (Variables tab → New repository variable):

| Name | Value |
| --- | --- |
| `ZIRTOLA_API_BASE` | `https://www.zirtola.com/api/launcher` (optional: this is the built-in default; always `www`, the bare domain only forwards its front page) |
| `ZIRTOLA_SIGN_MODE` | `azure` (only once step 1 is finished; leave unset before) |
| `AZURE_SIGNING_ENDPOINT` | the endpoint from step 1.8 |
| `AZURE_SIGNING_ACCOUNT` | `zirtola-signing` |
| `AZURE_SIGNING_PROFILE` | `zirtola-launcher` |

Back up the whole `.zirtola-launcher-secrets` folder now (password manager or an
encrypted USB stick). Losing `updater.key` means installed launchers can never
update themselves again; losing `manifest-2026-10.json` means you need a launcher
update before you can ship game builds.

### 5. Railway (the website)

1. The licensing pull request (website #21) and the Dirt Digger store copy
   (#23) are already live. Merge **website PR #22** (checkout, one device per
   licence, licence keys, install guide, system requirements):
   <https://github.com/trendlinepros-afk/retail-store/pull/22>. It targets
   `claude/app-marketplace-platform-cpyvmr`, the branch Railway deploys from, so
   Railway redeploys and applies migration `0016_game_sales` by itself (it only
   adds tables and columns and changes no existing rows).
   Then merge **launcher PR #1** into `main`:
   <https://github.com/trendlinepros-afk/zirtola-launcher/pull/1>. Nothing is
   released until you push a version tag (Part 2).
2. Railway → your project → the website service → **Variables** → **New Variable**,
   one by one. The values for the launcher ones are in
   `C:\Users\Wicked\.zirtola-launcher-secrets\RAILWAY_VARIABLES.txt`.

| Variable | Where the value comes from |
| --- | --- |
| `LAUNCHER_LAUNCH_SIGNING_KEY` | RAILWAY_VARIABLES.txt (secret) |
| `LAUNCHER_MANIFEST_PUBLIC_KEYS` | RAILWAY_VARIABLES.txt |
| `LAUNCHER_PUBLISH_TOKEN` | RAILWAY_VARIABLES.txt (secret) |
| `LAUNCHER_CDN_URL_TEMPLATE` | `https://cdn.zirtola.com/{game}/chunks/{prefix}/{hash}` |
| `LAUNCHER_DOWNLOAD_URL` | `https://github.com/trendlinepros-afk/zirtola-launcher/releases/latest` |
| `LAUNCHER_OFFLINE_GRACE_DAYS` | `7` (how long a game runs offline) |
| `LAUNCHER_DEACTIVATIONS_PER_PERIOD` / `LAUNCHER_DEACTIVATION_PERIOD_DAYS` | `3` / `30` |
| `SUPPORT_EMAIL` | the address players write to if the installer looks different from the pictures (until set, the guide says "reply to any email from us") |
| `LAUNCHER_PUBLISHER_NAME` | leave empty until releases are signed (see step 1) |
| `SHOW_SMARTSCREEN_NOTICE` | leave unset (on); `false` retires the notice later |
| `LICENSE_KEY_SECRET` | optional: a long random string (lets owners view their licence key again; set once, never change). Left empty it is derived from `AUTH_SECRET`. |
| `STRIPE_SECRET_KEY` | Stripe step 5 (`sk_live_...`; test with `sk_test_...` first) |
| `STRIPE_WEBHOOK_SECRET` | Stripe step 6 (`whsec_...`) |
| `MAILGUN_*` | already set if account emails work; receipts use them |

Never set `STRIPE_API_BASE` on Railway: it is only for local tests.

3. Redeploy (Railway → **Deployments** → **Redeploy**) and open
   `https://www.zirtola.com/api/launcher/v1/config`: it should show JSON.

### 6. The store page and price

Some of the game page lives in the website's code, not in Studio, so it goes
live with a deploy and needs no database change. It is in
`src/content/game-specs.ts` (one entry per game slug):

- **System requirements** (Minimum and Recommended). A game without an entry
  shows no requirements section.
- **earlyAccess**: `true` shows the "Still in development" notice (game page,
  checkout, success page, receipt) and the launcher's "Report a bug" link.
  Set it to `false` when the game leaves early access.
- **bugForum**: the forum category for bug reports (Dirt Digger: `bugs`,
  filtered to the game).

Wording for buying, devices and early access is in
`src/lib/launcher/sales-copy.ts`; the install guide's wording is in
`src/lib/launcher/install-guide.ts`.

1. Sign in to zirtola.com as admin → **Studio → Games → Dirt Digger**: check the
   title (Dirt Digger), tagline, short description, description, feature list,
   add-on note, screenshots and cover. They were set by the website's store-copy
   update (#23); from now on edit them here. The game page is at
   zirtola.com/games/dirt-digger (the old /games/dirt-simulator address redirects).
2. **Studio → Licensing → Launcher catalogue → Dirt Digger → Manage**:
   - **Price**: `19.99`, currency `USD` (already set by #23).
   - **Devices per license**: `1` (already set by #23; check it says 1).
   - **Sales open**: leave **off** until launch day. Players see the Purchase
     button greyed out with "Coming soon".
   - **Show this game in the Zirtola Launcher**: on.
3. Do the same for Alaskan Bush Simulator when it's ready.

---

## Part 2: shipping a game release

### Write the release notes

Create `Tools\release\release-notes\<version>.md` in the game repo
(`E:\Game Development\Voxel Mining`). Players see this text in the launcher's
"What's new" and on the website's Changelog. Write it for players:

```markdown
## Dirt Digger 0.2.0

- New: the wheel loader can now fill trucks.
- Fixed: the bucket no longer clips through rock.
```

Versions use `MAJOR.MINOR.PATCH` (0.2.0, 0.2.1 ...). Never reuse a number.

### Release to beta, check it, then stable

From `E:\Game Development\Voxel Mining` in PowerShell:

```powershell
$env:ZIRTOLA_PUBLISH_TOKEN = "<LAUNCHER_PUBLISH_TOKEN from RAILWAY_VARIABLES.txt>"
$env:ZIRTOLA_RCLONE_REMOTE = "r2:zirtola-cdn"

# 1. Dry run: packages nothing, uploads nothing, shows what would happen
node Tools\release\release.mjs --version 0.2.0 --channel beta --dry-run

# 2. Beta: packages (10-60 min), signs, uploads, publishes to testers
node Tools\release\release.mjs --version 0.2.0 --channel beta
```

Testers are players you gave beta access (**Studio → Licensing → a player →
Give a license → tick Beta access**). They switch **Beta: on** next to Play in the launcher.

**Check it worked:**

1. **Studio → Licensing → Launcher catalogue → Dirt Digger**: 0.2.0 shows as **live** on `beta`.
2. In your own launcher (with beta on): it offers "Update to 0.2.0", the update
   downloads only what changed, and "What's new" shows your notes.
3. Start the game. Watch **Studio → Crash reports** for a day or two.

When happy, give it to everyone:

```powershell
node Tools\release\release.mjs promote --version 0.2.0
```

Everyone's launcher offers the update on its next refresh (when they open it,
or within 15 minutes).

### Rolling back

**Studio → Licensing → Launcher catalogue → Dirt Digger → Builds** → on the
previous version click **Make live**. Launchers offer that version as the
update. Then fix and release a new version number (never re-release the same number).

### Shipping a launcher release

From `E:\Game Development\Zirtola Launcher`:

1. Write `release-notes\0.2.0.md` (see `release-notes\TEMPLATE.md`).
2. `node scripts\release-launcher.mjs --version 0.2.0 --dry-run`, then without `--dry-run`.
   It bumps the version, updates `CHANGELOG.md`, commits, tags `v0.2.0` and pushes the tag.
3. GitHub → **Actions** shows "Release" running (about 15 minutes). When green:
   GitHub → **Releases** → the draft **Zirtola Launcher v0.2.0** → check the installer
   is attached (and signed: right-click the downloaded `.exe` → Properties → **Digital Signatures**) → **Publish release**.
4. Installed launchers show "Launcher 0.2.0 · Restart to update" in the sidebar.

Rolling back a launcher release: GitHub → Releases → edit the bad release →
**Set as a pre-release** (or delete it). `latest.json` then points at the previous release.

---

## Part 3: what players see

1. On zirtola.com, the Dirt Digger page shows **$19.99** with **Purchase** (or a
   greyed-out Purchase and "Coming soon" while sales are closed), and the
   "what's included" note: *The base game includes every machine released so far.
   Some future, highly detailed machines may be sold separately as optional add-ons.*
2. **Purchase** → checkout page (sign in or create an account, tick the terms) →
   Stripe's secure payment page → **"Dirt Digger is yours"** with the order number,
   a receipt email, and **Open the Zirtola Launcher**.
3. In the launcher (signed in with the same account) Dirt Digger turns from
   locked to **Ready to install** within seconds (or immediately via the button).
4. Install → Play. The first PC is activated. On a second PC the launcher says
   *Dirt Digger is active on <PC> (last used <date>). Deactivate it to play here.*
   with a button to move it; or the player does it on **Settings → My games & keys**.
5. Updates show up as **Update to x.y.z**; after updating, **What's new** opens with
   the notes, the installed version highlighted. Settings → **Orders** lists receipts.
6. Offline, the game keeps working for 7 days on its last check.

Screenshots of every step (from the local test-mode run): `docs/screenshots/sale-e2e/`.

---

## Part 4: launch-day checklist

The week before:

- [ ] Signing done: a test launcher release's installer shows your name under **Digital Signatures**.
- [ ] Stripe account activated, payouts and tax set, webhook added with all 5 events.
- [ ] Railway variables set; `https://www.zirtola.com/api/launcher/v1/config` answers.
- [ ] Dirt Digger stable build published; you installed it on a clean PC from the website's download link.
- [ ] **Test mode run on the live site**: switch Railway's Stripe keys to `sk_test_`/test webhook secret,
      buy with card `4242 4242 4242 4242`, see it in the launcher, refund it in **Studio → Licensing → Sales**,
      see it disappear. Switch back to live keys.
- [ ] Terms page (**/terms#buying**) reads right for you; support email works.

On the day:

- [ ] **Studio → Licensing → Launcher catalogue → Dirt Digger → Sales open** on → **Save**.
- [ ] Buy it yourself once with a real card and refund it (proves live payments end to end).
- [ ] Post the announcement with the link `https://zirtola.com/games/dirt-digger`.
- [ ] Keep **Studio → Licensing → Sales** and **Studio → Crash reports** open for the day.

Afterwards: payouts arrive per Stripe's schedule (usually 2 to 7 days); Stripe's
fee is roughly 2.9% + 30¢ per card payment (more for international cards).

---

## What I could not do for you

These need your accounts, identity or money, so they're yours:

1. Azure: subscription, identity validation, certificate profile, app registration (Part 1.1).
2. Stripe: account, business details, bank, tax, live keys, webhook (Part 1.2).
3. Railway Storage Bucket and its website variables (Part 1.3, steps in NEW_USER_TEST.md A2–A5).
4. GitHub secrets and variables (Part 1.4).
5. Railway variables and merging the website PRs (Part 1.5).
6. Optional: winget submission (`docs/DISTRIBUTION.md`).
