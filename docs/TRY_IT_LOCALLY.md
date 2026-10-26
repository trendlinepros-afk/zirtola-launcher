# Try it locally: sign in, buy, install, update

Everything runs on your PC: a local copy of the website with its own database,
a stand-in for Stripe's test mode (no real payments, no Stripe account needed),
a local download server and the DEV launcher. Nothing touches zirtola.com or Railway.

You need: Node 20+, Rust (`winget install Rustlang.Rustup`), the two repos side by
side: `E:\Game Development\Zirtola Launcher` and `E:\Game Development\retail-store`
(the website repo on its `game-sales` branch).

## 1. Start everything

In PowerShell:

```powershell
cd "E:\Game Development\Zirtola Launcher"
powershell -ExecutionPolicy Bypass -File scripts\try-it-locally.ps1
```

It opens five windows (database, CDN, test payments, website, DEV launcher). The
first run downloads packages and builds the launcher, so give it 5 to 10 minutes.
It ends by printing:

| | |
| --- | --- |
| Website | <http://localhost:3000> |
| Player account | `pixel_pioneer` / `password123` |
| Admin account | `admin@example.test` / `local-admin-pass-123` |
| Test card | `4242 4242 4242 4242`, any future date, any CVC |

## 2. Sign in to the launcher

1. In the DEV launcher window (it says **DEV** at the top), click **Sign in with your browser**.
2. Your browser opens the local website. Sign in as `pixel_pioneer` / `password123`,
   then click **Sign in to the launcher**.
3. Back in the launcher: Dirt Digger shows a **lock** and **$19.99**.

## 3. Buy it (test mode)

1. Click **Buy on the website · $19.99**. The launcher shows "Waiting for your purchase...".
2. In the browser: tick **I agree...** → **Continue to payment**.
3. The test payment page says **TEST MODE**. Click **Pay $19.99**.
   (Try **Simulate: payment fails** or **Cancel** on another attempt to see those pages.)
4. You land on **Dirt Digger is yours** with an order number. The receipt email
   is printed in the **website** window (no real email is sent locally).
5. Switch to the launcher: within a few seconds Dirt Digger turns green,
   **New · Ready to install**. (Or click **Open the Zirtola Launcher** on the page.)

## 4. Install and play

1. **Install** → keep the suggested folder or pick one → **Install** (about 370 MB of demo data).
2. **Play**. The demo "game" is a small console window that runs the same licence
   check a real game does; it prints **LICENSE OK** with your account. Press Enter to close it.

## 5. Updates and the changelog

1. Give yourself beta access: <http://localhost:3000/login> as the admin →
   **Studio → Licensing** → find **Pixel Pioneer** → **Manage** → **Give or change a license** →
   Game **Dirt Digger**, tick **Beta access** → **Give license**.
2. In the launcher, click **Library** (refreshes), then **Beta: off** next to Play → it turns **Beta: on**.
3. The launcher offers **Update to 1.0.1**. Click it: it downloads only the changed part.
4. **What's new** opens by itself, with 1.0.1 marked **Installed** and **Beta**.
   The notes button (page icon) next to Play shows it any time.
5. The website's game page (<http://localhost:3000/games/dirt-digger>) shows the stable changelog.

## 6. More to try

- **Second PC**: the website's **Settings → My games & keys** lists your active PC with **Deactivate**.
- **Your licence key**: the key button next to Play (tooltip "View license key") shows the key masked, with **Show** and **Copy**, where it came from and which PC it is active on. The website shows the same key under **Settings → My games & keys**.
- **Orders**: **Settings → Orders** shows the order and receipt.
- **Refund**: as admin, **Studio → Licensing → Sales → Refund**. The launcher loses the game on its next refresh.
- **Keys**: **Studio → Licensing → Keys** → generate one, then redeem it with another account.

## Stop and start over

Close the five windows. To start from a clean slate, delete
`E:\Game Development\retail-store\.data\pglite` (the local database) and run the script again.
