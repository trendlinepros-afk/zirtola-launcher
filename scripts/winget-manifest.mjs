// Generates a winget manifest (schema 1.9) for a published launcher release,
// ready to submit as a pull request to https://github.com/microsoft/winget-pkgs.
// It only writes files; it does not submit anything.
//
//   node scripts/winget-manifest.mjs 0.2.0 "path\to\Zirtola Launcher_0.2.0_x64-setup.exe"
//
// The installer URL must be the GitHub release asset for that version (winget
// requires the installer to come straight from the publisher's release
// location, with no redirector in front of it). Output: packaging/winget/out/<version>/
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";

const [version, installerPath] = process.argv.slice(2);
if (!version || !installerPath) {
  console.error('usage: node scripts/winget-manifest.mjs <version> <path to the release "-setup.exe">');
  process.exit(1);
}
const root = join(fileURLToPath(import.meta.url), "..", "..");
const sha = createHash("sha256").update(readFileSync(installerPath)).digest("hex").toUpperCase();
const asset = encodeURIComponent(basename(installerPath));
const url = `https://github.com/trendlinepros-afk/zitrola-launcher/releases/download/v${version}/${asset}`;
const id = "Zirtola.ZirtolaLauncher";
const outDir = join(root, "packaging", "winget", "out", version);
mkdirSync(outDir, { recursive: true });

const header = (type) => `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${type}.1.9.0.schema.json\n`;

writeFileSync(
  join(outDir, `${id}.yaml`),
  `${header("version")}PackageIdentifier: ${id}
PackageVersion: ${version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.9.0
`,
);

writeFileSync(
  join(outDir, `${id}.installer.yaml`),
  `${header("installer")}PackageIdentifier: ${id}
PackageVersion: ${version}
InstallerType: nullsoft
Scope: user
InstallModes:
  - interactive
  - silent
UpgradeBehavior: install
Protocols:
  - zirtola
ReleaseDate: ${new Date().toISOString().slice(0, 10)}
Installers:
  - Architecture: x64
    InstallerUrl: ${url}
    InstallerSha256: ${sha}
ManifestType: installer
ManifestVersion: 1.9.0
`,
);

writeFileSync(
  join(outDir, `${id}.locale.en-US.yaml`),
  `${header("defaultLocale")}PackageIdentifier: ${id}
PackageVersion: ${version}
PackageLocale: en-US
Publisher: Zirtola
PublisherUrl: https://zirtola.com
PrivacyUrl: https://zirtola.com/privacy
PackageName: Zirtola Launcher
PackageUrl: https://github.com/trendlinepros-afk/zitrola-launcher
License: Proprietary
ShortDescription: Download, update and play games from the Zirtola studio.
Description: The Zirtola Launcher installs, updates and launches games from the Zirtola studio for your Zirtola account.
Moniker: zirtola
Tags:
  - games
  - launcher
ReleaseNotesUrl: https://github.com/trendlinepros-afk/zitrola-launcher/releases/tag/v${version}
ManifestType: defaultLocale
ManifestVersion: 1.9.0
`,
);

console.log(`winget manifest written to ${outDir}`);
console.log(`InstallerSha256 ${sha}`);
console.log("Next: winget validate <that folder>, test with winget-pkgs' Tools/SandboxTest.ps1, then open a PR to microsoft/winget-pkgs.");
