# Code signing (Windows Authenticode)

Customers download the launcher installer from the website. An unsigned
`.exe` gets the full-screen SmartScreen "Windows protected your PC" warning and
an "Unknown publisher" UAC prompt. Signing fixes the publisher name and lets
the file build SmartScreen reputation.

## What changed in 2024 (and why EV is no longer worth it)

- Since **March 2024**, EV certificates no longer get instant SmartScreen trust.
  EV, OV and Azure Artifact Signing all build reputation the same way: from
  download volume over time. A brand-new publisher sees the warning on early
  downloads whatever it pays for.
- Since **June 2023**, OV/EV private keys must live on a hardware token or a
  cloud HSM. You can no longer get a `.pfx` file to put in a CI secret; signing
  in CI needs a cloud signing service.

## Options (prices as of late 2026, check before buying)

| Option | Cost | CI-friendly | Who can get it | Notes |
| --- | --- | --- | --- | --- |
| **Azure Artifact Signing** (formerly Trusted Signing) | about **$9.99/month** (Basic, 5,000 signatures) | Yes, built for it | Organisations in the US, Canada, EU, UK; **individuals in the US and Canada** | Microsoft does the identity check, keys stay in Microsoft's HSMs, certificates are short-lived and renewed for you. Same reputation model as OV. |
| OV certificate from a CA with a cloud HSM (e.g. SSL.com eSigner, DigiCert KeyLocker, Certum SimplySign) | roughly **$150–$400/year**, plus cloud-signing fees with some CAs | Yes, through the CA's signing tool or KSP | Individuals and companies almost anywhere | Pick a CA whose cloud signing works on GitHub Actions. |
| EV certificate | roughly **$250–$600/year**, plus a token or cloud HSM | Same as OV | Registered companies only | Since 2024 the only practical extra is the company name in UAC dialogs. Not worth it for this launcher. |

## Recommendation

**Use Azure Artifact Signing** if the studio qualifies (a US or Canadian
individual, or a company in the US, Canada, EU or UK). It is the cheapest
option by far (~$120/year), it is made for CI, there is no hardware token and
no certificate renewal, and Microsoft is the CA SmartScreen trusts most.

If the studio does not qualify, buy an **OV certificate from a CA that offers
cloud signing** and use the `thumbprint` mode below (or adapt `scripts/sign.ps1`
to the CA's signing tool). Skip EV.

Either way, expect a few weeks of SmartScreen warnings for a new publisher.
Signing every release with the same identity is what builds reputation.

## How signing is wired

Tauri calls `scripts/sign.ps1 <file>` for every Windows binary it produces
(launcher `.exe`, uninstaller, setup `.exe`) through `bundle.windows.signCommand`
in `src-tauri/tauri.conf.json`. The script picks a mode from environment
variables, so nothing secret is in the repo and builds without credentials
still work (they are just unsigned):

| `ZIRTOLA_SIGN_MODE` | Uses |
| --- | --- |
| unset / `none` | Leaves files unsigned (local builds, forks). |
| `azure` | `artifact-signing-cli` with `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE`. |
| `thumbprint` | `signtool` with a certificate already in the Windows store (`SIGN_CERT_THUMBPRINT`, `SIGN_TIMESTAMP_URL`), e.g. an OV cert exposed by the CA's cloud KSP on a self-hosted runner. |

The release workflow verifies the installer's signature after building when a
mode is set.

## Setting up Azure Artifact Signing (step by step)

1. In the Azure portal, create a subscription (pay-as-you-go) and register the
   `Microsoft.CodeSigning` resource provider.
2. Create an **Artifact Signing account** (pick a region; its endpoint looks like
   `https://eus.codesigning.azure.net`).
3. Under the account, start **Identity validation** (Individual or
   Organization). Microsoft checks your ID / business documents; this can take
   from hours to a few days.
4. Create a **Certificate profile** of type *Public Trust* using that identity.
5. In Microsoft Entra ID, **register an app** (e.g. `zirtola-launcher-ci`),
   create a **client secret**, and on the Artifact Signing account grant the app
   the **Artifact Signing Certificate Profile Signer** role.
6. In GitHub, repo → Settings → Secrets and variables → Actions:
   - Secrets: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`
   - Variables: `ZIRTOLA_SIGN_MODE` = `azure`, `AZURE_SIGNING_ENDPOINT`,
     `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE`
7. Cut a release (docs/RELEASING.md). The "Verify the Authenticode signature"
   step must show `Valid`.

Optional hardening: replace the client secret with GitHub OIDC federated
credentials on the app registration, so no long-lived Azure secret exists.

## Two different signatures

Do not confuse these:

- **Authenticode** (this page): proves to Windows who published the `.exe`.
- **Updater signature** (minisign key, `TAURI_SIGNING_PRIVATE_KEY`): proves to
  installed launchers that an update came from you. See docs/RELEASING.md. Both
  are needed; losing the updater key means existing installs can never
  auto-update again, so back it up.
