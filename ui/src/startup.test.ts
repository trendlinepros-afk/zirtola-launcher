// npm run test:ui
import { test } from "node:test";
import assert from "node:assert/strict";
import { belowMinimum, compareVersions, needsRecheck, runStartupChecks, type StartupDeps, type StartupGame } from "./startup";

function deps(over: Partial<StartupDeps> = {}) {
  const calls: string[] = [];
  const d: StartupDeps = {
    currentVersion: "0.2.0",
    minLauncherVersion: null,
    autoUpdateLauncher: true,
    autoUpdateGames: true,
    signedIn: true,
    checkLauncherUpdate: async () => { calls.push("launcher-check"); return null; },
    installLauncherUpdate: async () => { calls.push("launcher-install"); },
    refreshLibrary: async () => { calls.push("games-check"); return []; },
    startGameUpdate: async (id) => { calls.push(`update:${id}`); },
    now: () => 1_000_000,
    ...over,
  };
  return { d, calls };
}
const game = (id: string, o: Partial<StartupGame> = {}): StartupGame => ({ id, owned: true, updateAvailable: false, busy: false, ...o });

test("a start triggers both checks: launcher self-update and game updates", async () => {
  const { d, calls } = deps();
  const r = await runStartupChecks(d);
  assert.ok(calls.includes("launcher-check"), "launcher update checked");
  assert.ok(calls.includes("games-check"), "entitlements and manifests re-fetched");
  assert.equal(r.note, null);
  assert.equal(r.checkedAt, 1_000_000);
});

test("both checks run in parallel, not one after the other", async () => {
  const order: string[] = [];
  const { d } = deps({
    checkLauncherUpdate: async () => { order.push("launcher-start"); await new Promise((r) => setTimeout(r, 20)); order.push("launcher-end"); return null; },
    refreshLibrary: async () => { order.push("games-start"); await new Promise((r) => setTimeout(r, 20)); order.push("games-end"); return []; },
  });
  await runStartupChecks(d);
  assert.deepEqual(order.slice(0, 2).sort(), ["games-start", "launcher-start"]);
});

test("a launcher update installs (and restarts) when automatic updates are on", async () => {
  const { d, calls } = deps({ checkLauncherUpdate: async () => ({ version: "0.3.0" }) });
  const r = await runStartupChecks(d);
  assert.ok(calls.includes("launcher-install"));
  assert.equal(r.launcherInstalling, true);
  assert.ok(!calls.some((c) => c.startsWith("update:")), "games wait for the launcher restart");
});

test("with automatic launcher updates off, the update is offered in a prompt", async () => {
  const { d, calls } = deps({ autoUpdateLauncher: false, checkLauncherUpdate: async () => ({ version: "0.3.0" }) });
  const r = await runStartupChecks(d);
  assert.equal(r.launcherUpdate, "0.3.0");
  assert.equal(r.launcherPrompt, "optional");
  assert.ok(!calls.includes("launcher-install"));
});

test("below min_launcher_version the prompt is required (no Later)", async () => {
  const { d, calls } = deps({ autoUpdateLauncher: false, currentVersion: "0.1.0", minLauncherVersion: "0.2.0", checkLauncherUpdate: async () => ({ version: "0.3.0" }) });
  const r = await runStartupChecks(d);
  assert.equal(r.launcherUpdateRequired, true);
  assert.equal(r.launcherPrompt, "required");
  assert.ok(!calls.includes("launcher-install"), "the player sees the reason and starts it");
});

test("a launcher release marked mandatory is required too", async () => {
  const { d } = deps({ autoUpdateLauncher: false, checkLauncherUpdate: async () => ({ version: "0.3.0", mandatory: true }) });
  const r = await runStartupChecks(d);
  assert.equal(r.launcherUpdateRequired, true);
  assert.equal(r.launcherPrompt, "required");
});

test("a failed automatic launcher install falls back to the prompt", async () => {
  const { d } = deps({ checkLauncherUpdate: async () => ({ version: "0.3.0", mandatory: true }), installLauncherUpdate: async () => { throw new Error("disk full"); } });
  const r = await runStartupChecks(d);
  assert.equal(r.launcherInstalling, false);
  assert.equal(r.launcherPrompt, "required");
  assert.match(r.note!, /required launcher update/);
});

test("game updates are reported, and started when the setting allows it", async () => {
  const lib = [game("dirt-simulator", { updateAvailable: true }), game("other"), game("busy-one", { updateAvailable: true, busy: true }), game("not-owned", { owned: false, updateAvailable: true })];
  const on = deps({ refreshLibrary: async () => lib });
  const r1 = await runStartupChecks(on.d);
  assert.deepEqual(r1.gameUpdates, ["dirt-simulator", "busy-one"]);
  assert.deepEqual(r1.autoUpdating, ["dirt-simulator"], "never a game that's already busy or not owned");
  assert.deepEqual(r1.gamePrompts, [], "started ones aren't prompted");
  const off = deps({ autoUpdateGames: false, refreshLibrary: async () => lib });
  const r2 = await runStartupChecks(off.d);
  assert.deepEqual(r2.gameUpdates, ["dirt-simulator", "busy-one"]);
  assert.deepEqual(r2.autoUpdating, []);
  assert.deepEqual(r2.gamePrompts, ["dirt-simulator"], "offered in a prompt instead");
  assert.ok(!off.calls.some((c) => c.startsWith("update:")));
});

test("offline: start normally with a small note, never throw", async () => {
  const { d } = deps({
    checkLauncherUpdate: async () => { throw new Error("offline"); },
    refreshLibrary: async () => { throw new Error("offline"); },
  });
  const r = await runStartupChecks(d);
  assert.match(r.note!, /Couldn't check for updates/);
  assert.deepEqual(r.gameUpdates, []);
});

test("signed out: only the launcher is checked", async () => {
  const { d, calls } = deps({ signedIn: false });
  await runStartupChecks(d);
  assert.deepEqual(calls, ["launcher-check"]);
});

test("versions and staleness", () => {
  assert.equal(compareVersions("0.2.10", "0.2.9"), 1);
  assert.equal(compareVersions("1.0.0", "1.0"), 0);
  assert.equal(compareVersions("1.0.0-beta.1", "1.0.0"), -1);
  assert.equal(belowMinimum("0.1.9", "0.2.0"), true);
  assert.equal(belowMinimum("0.2.0", "0.2.0"), false);
  assert.equal(belowMinimum("0.2.0", null), false);
  assert.equal(needsRecheck(null, 0), true);
  assert.equal(needsRecheck(0, 9 * 60_000), false);
  assert.equal(needsRecheck(0, 11 * 60_000), true);
});

test("only the launcher check failing: games still count as checked", async () => {
  const { d } = deps({ checkLauncherUpdate: async () => { throw new Error("no release yet"); } });
  const r = await runStartupChecks(d);
  assert.equal(r.gamesChecked, true);
  assert.match(r.note!, /launcher update/);
});
