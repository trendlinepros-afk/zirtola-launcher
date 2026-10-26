// Update checks at every start (and before Play when the last check is stale).
// Kept free of DOM and Tauri imports so it can be unit-tested
// (startup.test.ts): main.ts passes in the real calls.
//
// On each start, in parallel:
//   1. the launcher's own update (Tauri updater against latest.json), and
//   2. game updates: entitlements plus each owned game's latest manifest for
//      its channel (the `library` command re-fetches both).
// A launcher update is installed (and the launcher restarts) when the player
// allows automatic updates, and always when this version is below the
// website's min_launcher_version. Game updates are reported, and started when
// the player allows automatic game updates. Offline, nothing blocks: the
// result carries a short note instead.

export const STALE_AFTER_MS = 10 * 60_000;

export interface StartupGame {
  id: string;
  owned: boolean;
  updateAvailable: boolean;
  busy: boolean;
}

export interface StartupDeps {
  currentVersion: string;
  minLauncherVersion: string | null;
  autoUpdateLauncher: boolean;
  autoUpdateGames: boolean;
  signedIn: boolean;
  checkLauncherUpdate: () => Promise<{ version: string } | null>;
  installLauncherUpdate: () => Promise<void>;
  refreshLibrary: () => Promise<StartupGame[]>;
  startGameUpdate: (gameId: string) => Promise<void>;
  now?: () => number;
}

export interface StartupResult {
  checkedAt: number;
  launcherUpdate: string | null;
  launcherUpdateRequired: boolean;
  launcherInstalling: boolean;
  gameUpdates: string[];
  autoUpdating: string[];
  note: string | null;
}

/** -1, 0 or 1, comparing dotted numeric versions ("0.2.10" > "0.2.9"); pre-release tags sort first. */
export function compareVersions(a: string, b: string): number {
  const parse = (v: string) => {
    const [core, pre] = v.trim().replace(/^v/, "").split("-", 2);
    return { nums: core!.split(".").map((n) => Number.parseInt(n, 10) || 0), pre: pre ?? null };
  };
  const x = parse(a);
  const y = parse(b);
  for (let i = 0; i < Math.max(x.nums.length, y.nums.length); i++) {
    const d = (x.nums[i] ?? 0) - (y.nums[i] ?? 0);
    if (d !== 0) return d > 0 ? 1 : -1;
  }
  if (x.pre && !y.pre) return -1;
  if (!x.pre && y.pre) return 1;
  return 0;
}

export function belowMinimum(current: string, min: string | null | undefined): boolean {
  return Boolean(min) && compareVersions(current, min!) < 0;
}

export function needsRecheck(lastCheck: number | null, now: number, maxAgeMs = STALE_AFTER_MS): boolean {
  return lastCheck == null || now - lastCheck > maxAgeMs;
}

export async function runStartupChecks(d: StartupDeps): Promise<StartupResult> {
  const now = d.now ?? Date.now;
  const required = belowMinimum(d.currentVersion, d.minLauncherVersion);
  const res: StartupResult = {
    checkedAt: now(),
    launcherUpdate: null,
    launcherUpdateRequired: required,
    launcherInstalling: false,
    gameUpdates: [],
    autoUpdating: [],
    note: null,
  };
  const [launcher, library] = await Promise.allSettled([
    d.checkLauncherUpdate(),
    d.signedIn ? d.refreshLibrary() : Promise.resolve<StartupGame[]>([]),
  ]);
  const failed = launcher.status === "rejected" || library.status === "rejected";

  if (launcher.status === "fulfilled" && launcher.value) {
    res.launcherUpdate = launcher.value.version;
    if (required || d.autoUpdateLauncher) {
      res.launcherInstalling = true;
      try {
        await d.installLauncherUpdate(); // restarts the launcher on success
        return res;
      } catch {
        res.launcherInstalling = false;
        res.note = required
          ? "This launcher version is no longer supported and the update didn't install. Try again from the update button."
          : "Couldn't install the launcher update. You can try again from the update button.";
      }
    }
  }

  if (library.status === "fulfilled") {
    res.gameUpdates = library.value.filter((g) => g.owned && g.updateAvailable).map((g) => g.id);
    if (d.autoUpdateGames) {
      for (const g of library.value) {
        if (!g.owned || !g.updateAvailable || g.busy) continue;
        try {
          await d.startGameUpdate(g.id);
          res.autoUpdating.push(g.id);
        } catch {
          /* shown as "Update available"; the player can start it */
        }
      }
    }
  }

  if (failed && !res.note) res.note = "Couldn't check for updates. You can still play; we'll try again later.";
  return res;
}
