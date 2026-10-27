import "./styles.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  api,
  errorCode,
  errorMessage,
  onDeepLink,
  onGameCrashed,
  onPlaytimeUpdated,
  type CrashCandidate,
  onJobFinished,
  onJobProgress,
  onLauncherUpdateProgress,
  onPrereqProgress,
  type PrereqItem,
  type PrereqProgress,
  type AppInfo,
  type CatalogGame,
  type CmdError,
  type JobProgress,
  type LauncherUpdateInfo,
  type LibraryGame,
  type Me,
  type Settings,
} from "./api";
import { icons, keyArt, logoMark } from "./art";
import * as fmt from "./format";
import { focusFirst, onGamepadActive, onNavAction } from "./nav";
import { buyUrl, openLink, storeUrl } from "./links";
import { belowMinimum, needsRecheck, runStartupChecks } from "./startup";
import { PromptMemo, entriesBetween, gamePromptKind, requiredGameText, requiredLauncherText } from "./updates";
import { parseNotes } from "./notes";
import { blocking, installable, installLine, playBlockedReason } from "./requirements";

// Open a website link; on failure (no URL yet, browser blocked) tell the player.
function openSite(url: string | null | undefined, what: string) {
  return openLink(url, what, { openUrl: api.openUrl, toast, describeError: errorMessage });
}

// ------------------------------------------------------------------ state

type View = "library" | "settings" | "requirements";

const S = {
  info: null as AppInfo | null,
  me: null as Me | null,
  booted: false,
  offline: null as string | null,
  view: "library" as View,
  library: [] as LibraryGame[],
  catalog: [] as CatalogGame[],
  selected: null as string | null,
  progress: new Map<string, JobProgress>(),
  signingIn: false,
  settings: null as Settings | null,
  launcherUpdate: null as LauncherUpdateInfo | null,
  launcherUpdating: null as { downloaded: number; total: number | null } | null,
  // Update checks: when the last one succeeded, and a small note if it couldn't run.
  lastUpdateCheck: null as number | null,
  checkingUpdates: false,
  // Requirements (prereqs.rs): last check, and the open requirements screen.
  prereqs: null as PrereqItem[] | null,
  req: null as null | { game: string | null; thenPlay: boolean; installing: boolean; checking: boolean; progress: Map<string, PrereqProgress>; errors: Map<string, string> },
  updateNote: null as string | null,
  gamepad: false,
  // Purchases the player started from the launcher: game id -> start time.
  pending: new Map<string, number>(),
  // Games that became owned while the launcher was open (highlighted).
  justOwned: new Set<string>(),
};

// ------------------------------------------------------------------ dom helpers

type Attrs = Record<string, unknown> & { class?: string; html?: string };
type Child = Node | string | null | undefined | false;

function h(tag: string, attrs: Attrs = {}, ...children: Child[]): HTMLElement {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") el.className = String(v);
    else if (k === "html") el.innerHTML = String(v);
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v as EventListener);
    else if (v === true) el.setAttribute(k, "");
    else el.setAttribute(k, String(v));
  }
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(typeof c === "string" ? document.createTextNode(c) : c);
  }
  return el;
}

function btn(id: string, label: string, onclick: () => void, opts: { cls?: string; icon?: string; disabled?: boolean; title?: string } = {}) {
  return h(
    "button",
    { class: `btn ${opts.cls ?? ""}`, "data-id": id, onclick, disabled: opts.disabled, title: opts.title, type: "button" },
    opts.icon ? h("span", { class: "ico", html: opts.icon }) : null,
    label ? h("span", {}, label) : null,
  );
}

const app = document.getElementById("app")!;
let lastRefresh = Date.now();

function render() {
  const focused = (document.activeElement as HTMLElement | null)?.dataset?.id;
  const inModal = !!document.querySelector(".modal-backdrop");
  app.replaceChildren(...([titlebar(), body(), hints()].filter(Boolean) as Node[]));
  if (!inModal && focused) {
    const el = app.querySelector<HTMLElement>(`[data-id="${CSS.escape(focused)}"]`);
    el?.focus({ preventScroll: true });
  }
}

// ------------------------------------------------------------------ chrome

function titlebar() {
  const w = getCurrentWindow();
  return h(
    "header",
    { class: "titlebar", "data-tauri-drag-region": true },
    h("div", { class: "brand", "data-tauri-drag-region": true }, h("span", { class: "brand-mark", html: logoMark }), h("span", { class: "brand-word" }, "ZIRTOLA")),
    S.info?.dev_mode ? h("span", { class: "dev-pill", title: S.info.api_base }, "DEV") : null,
    h("div", { class: "spacer", "data-tauri-drag-region": true }),
    h(
      "div",
      { class: "winctl", "data-nav-skip": true },
      h("button", { class: "wc", title: "Minimize", onclick: () => w.minimize(), html: icons.min, tabindex: "-1" }),
      h("button", { class: "wc", title: "Maximize", onclick: () => w.toggleMaximize(), html: icons.max, tabindex: "-1" }),
      h("button", { class: "wc close", title: "Close", onclick: () => w.close(), html: icons.close, tabindex: "-1" }),
    ),
  );
}

function hints(): HTMLElement | null {
  if (!S.gamepad) return null;
  const k = (b: string, t: string) => h("span", { class: "hint" }, h("kbd", {}, b), t);
  return h(
    "footer",
    { class: "hints" },
    k("A", "Select"),
    k("B", "Back"),
    S.me ? k("LB / RB", "Switch game") : null,
    S.me ? k("Start", "Play / Install") : null,
    S.me ? k("Y", "Redeem key") : null,
    S.me ? k("View", "Settings") : null,
  );
}

function body() {
  if (!S.booted) return h("main", { class: "boot" }, h("div", { class: "spinner" }));
  if (!S.me) return signInScreen();
  return h("div", { class: "shell" }, sidebar(), h("main", { class: "content" }, S.view === "settings" ? settingsView() : S.view === "requirements" ? requirementsView() : libraryView()));
}

// ------------------------------------------------------------------ sign in

function signInScreen() {
  const games = S.catalog;
  return h(
    "main",
    { class: "signin" },
    h(
      "section",
      { class: "signin-card" },
      h("div", { class: "signin-logo", html: logoMark }),
      h("h1", {}, "Sign in to Zirtola"),
      h("p", { class: "muted" }, "Use the same account as on the Zirtola website. Your web browser opens so you can sign in there; the launcher never sees your password."),
      S.signingIn
        ? h(
            "div",
            { class: "waiting" },
            h("div", { class: "spinner small" }),
            h("span", {}, "Finish signing in in your browser…"),
            btn("cancel-signin", "Cancel", () => api.cancelSignIn(), { cls: "ghost" }),
          )
        : btn("signin", "Sign in with your browser", signIn, { cls: "primary big" }),
      S.offline ? h("p", { class: "error-text" }, S.offline) : null,
      S.offline ? btn("retry", "Retry", boot, { cls: "ghost" }) : null,
      S.info?.account_url
        ? h("p", { class: "muted small" }, "No account yet? ", h("a", { href: "#", "data-nav": true, "data-id": "create-account", onclick: (e: Event) => { e.preventDefault(); void openSite(S.info?.account_url, "account page"); } }, "Create one on the website"), ".")
        : null,
    ),
    h(
      "section",
      { class: "signin-showcase" },
      ...games.map((g) =>
        h(
          "div",
          { class: "showcase-card" },
          h("div", { class: "art", html: keyArt(g.id, g.art.accent, g.title) }),
          h("div", { class: "showcase-text" }, h("strong", {}, g.title), h("span", {}, g.tagline)),
        ),
      ),
    ),
  );
}

async function signIn() {
  S.signingIn = true;
  render();
  try {
    S.me = await api.signIn();
    S.offline = null;
    await loadLibrary();
    toast(`Signed in as ${S.me.display_name}`);
  } catch (e) {
    if (!/cancel/i.test(errorMessage(e))) toast(errorMessage(e), "error");
  } finally {
    S.signingIn = false;
    render();
    focusFirst();
  }
}

// ------------------------------------------------------------------ sidebar

function sidebar() {
  const item = (id: View, label: string, ico: string) =>
    h(
      "button",
      {
        class: `nav-item ${S.view === id ? "active" : ""}`,
        "data-id": `nav-${id}`,
        onclick: () => {
          S.view = id;
          render();
          if (id === "library") void loadLibrary();
        },
        type: "button",
      },
      h("span", { class: "ico", html: ico }),
      h("span", {}, label),
    );
  return h(
    "nav",
    { class: "sidebar" },
    item("library", "Library", icons.library),
    h(
      "button",
      { class: "nav-item", "data-id": "nav-store", type: "button", onclick: () => void openSite(storeUrl(S.info), "store") },
      h("span", { class: "ico", html: icons.store }),
      h("span", {}, "Store"),
      h("span", { class: "ico trailing", html: icons.external }),
    ),
    h(
      "button",
      { class: "nav-item", "data-id": "nav-redeem", type: "button", onclick: () => redeemDialog() },
      h("span", { class: "ico", html: icons.key }),
      h("span", {}, "Redeem a key"),
    ),
    item("settings", "Settings", icons.settings),
    h("div", { class: "grow" }),
    S.launcherUpdate
      ? h(
          "button",
          { class: `update-card ${launcherUpdateRequired() ? "required" : ""}`, "data-id": "launcher-update", type: "button", onclick: () => void launcherUpdatePrompt({ force: true }), disabled: !!S.launcherUpdating },
          h("span", { class: "ico", html: icons.update }),
          h(
            "span",
            {},
            h("strong", {}, S.launcherUpdating ? "Updating launcher…" : `Launcher ${S.launcherUpdate.version}`),
            h(
              "small",
              {},
              S.launcherUpdating
                ? S.launcherUpdating.total
                  ? `${Math.round((S.launcherUpdating.downloaded / S.launcherUpdating.total) * 100)}%`
                  : fmt.bytes(S.launcherUpdating.downloaded)
                : launcherUpdateRequired() ? "Required update" : "Restart to update",
            ),
          ),
        )
      : null,
    h(
      "div",
      { class: "account" },
      h("div", { class: "avatar" }, fmt.initials(S.me!.display_name)),
      h("div", { class: "account-name" }, h("strong", {}, S.me!.display_name), h("small", {}, `Launcher ${S.info?.version ?? ""}`)),
      h("button", { class: "icon-btn", title: "Sign out", "data-id": "signout", onclick: signOut, html: icons.signout, type: "button" }),
    ),
  );
}

async function signOut() {
  if (S.progress.size) {
    toast("Pause your downloads before signing out.", "error");
    return;
  }
  await api.signOut();
  S.me = null;
  S.library = [];
  render();
  focusFirst();
}

// ------------------------------------------------------------------ library

function selectedGame(): LibraryGame | undefined {
  return S.library.find((g) => g.game.id === S.selected) ?? S.library[0];
}

function libraryView() {
  const g = selectedGame();
  if (!g) return h("div", { class: "empty" }, h("div", { class: "spinner" }));
  const note = S.checkingUpdates
    ? h("p", { class: "update-note", "data-id": "update-note" }, h("span", { class: "spinner small" }), "Checking for updates…")
    : S.updateNote
      ? h("p", { class: "update-note", "data-id": "update-note" }, S.updateNote)
      : null;
  return h("div", { class: "library" }, note, hero(g), shelf());
}

function hero(lg: LibraryGame) {
  const g = lg.game;
  const p = S.progress.get(g.id);
  const inst = lg.install;
  const meta: string[] = [];
  if (S.me && (lg.owned || lg.hours_played > 0)) meta.push(fmt.hoursPlayed(lg.hours_played));
  if (inst?.version) meta.push(`Installed ${inst.version}`);
  if (inst?.update_available && inst.latest_version) meta.push(`Update ${inst.latest_version} ${inst.update_required ? "required" : "available"}`);
  if (!inst && g.install_size) meta.push(`${fmt.bytes(g.install_size)} download`);
  if (lg.entitlement) meta.push(lg.entitlement === "purchase" ? "Purchased" : lg.entitlement === "key" ? "Key redeemed" : "Granted by the studio");

  return h(
    "section",
    { class: "hero", style: `--accent:${g.art.accent}` },
    h("div", { class: "hero-art", html: keyArt(g.id, g.art.accent, g.title) }),
    h("div", { class: "hero-shade" }),
    h(
      "div",
      { class: "hero-body" },
      h(
        "div",
        { class: "pills" },
        h("span", { class: `pill ${g.status}` }, fmt.statusLabel(g.status)),
        lg.owned && inst?.update_available
          ? h(
              "button",
              {
                class: `pill update ${inst.update_required ? "required" : ""}`,
                type: "button",
                "data-id": "update-available",
                title: inst.update_required ? requiredGameText(g.title, inst) : "See what's new",
                onclick: () => void gameUpdatePrompt(lg, { force: true }),
              },
              `${inst.update_required ? "Required update" : "Update available"}${inst.latest_version ? ` · ${inst.latest_version}` : ""} · What's new`,
            )
          : null,
      ),
      h("h1", {}, g.title),
      h("p", { class: "tagline" }, g.tagline),
      h("p", { class: "desc" }, g.description),
      meta.length ? h("p", { class: "meta" }, meta.join("  ·  ")) : null,
      requirementsLine(g),
      lg.job || p ? progressPanel(lg, p) : actions(lg),
    ),
  );
}

function progressPanel(lg: LibraryGame, p?: JobProgress) {
  const kind = p?.kind ?? lg.job ?? "install";
  const pct = p && p.total_bytes ? Math.min(100, (p.done_bytes / p.total_bytes) * 100) : 0;
  const label =
    !p || p.phase === "preparing"
      ? "Preparing…"
      : p.phase === "verifying"
        ? "Verifying files"
        : p.phase === "committing"
          ? "Finishing up"
          : p.phase === "uninstalling"
            ? "Uninstalling"
            : kind === "update"
              ? "Updating"
              : kind === "verify"
                ? "Repairing"
                : "Downloading";
  return h(
    "div",
    { class: "progress-panel", "data-progress": lg.game.id },
    h(
      "div",
      { class: "progress-head" },
      h("strong", { class: "p-label" }, label),
      h("span", { class: "p-pct" }, p && p.phase !== "uninstalling" ? `${pct.toFixed(1)}%` : ""),
    ),
    h("div", { class: "bar" }, h("div", { class: "fill", style: `width:${pct}%` })),
    h("div", { class: "progress-foot" }, h("span", { class: "p-detail" }, progressDetail(p))),
    kind !== "uninstall" ? btn("pause", kind === "verify" ? "Cancel" : "Pause", () => api.pause(lg.game.id), { cls: "ghost", icon: icons.pause }) : null,
  );
}

function progressDetail(p?: JobProgress): string {
  if (!p) return "";
  if (p.phase === "verifying") return `${fmt.bytes(p.done_bytes)} of ${fmt.bytes(p.total_bytes)} checked`;
  if (p.phase !== "downloading") return "";
  const parts = [`${fmt.bytes(p.done_bytes)} of ${fmt.bytes(p.total_bytes)}`];
  if (p.speed_bps > 0) parts.push(fmt.speed(p.speed_bps));
  if (p.eta_s != null && p.eta_s > 0) parts.push(`${fmt.duration(p.eta_s)} left`);
  if (p.reused_bytes > 0) parts.push(`${fmt.bytes(p.reused_bytes)} reused from disk`);
  return parts.join("  ·  ");
}

function updateProgressDom(p: JobProgress) {
  const panel = app.querySelector<HTMLElement>(`[data-progress="${CSS.escape(p.game_id)}"]`);
  if (!panel) return false;
  const pct = p.total_bytes ? Math.min(100, (p.done_bytes / p.total_bytes) * 100) : 0;
  panel.querySelector<HTMLElement>(".fill")!.style.width = `${pct}%`;
  panel.querySelector<HTMLElement>(".p-pct")!.textContent = p.phase === "uninstalling" ? "" : `${pct.toFixed(1)}%`;
  panel.querySelector<HTMLElement>(".p-detail")!.textContent = progressDetail(p);
  const label = panel.querySelector<HTMLElement>(".p-label")!;
  const want =
    p.phase === "verifying" ? "Verifying files" : p.phase === "committing" ? "Finishing up" : p.phase === "preparing" ? "Preparing…" : null;
  if (want) label.textContent = want;
  else if (p.phase === "downloading") label.textContent = p.kind === "update" ? "Updating" : p.kind === "verify" ? "Repairing" : "Downloading";
  return true;
}

// The website's FAQ about moving a licence between PCs.
function devicesFaqUrl(): string | null {
  try {
    return S.info ? `${new URL(S.info.api_base).origin}/terms#faq-devices` : null;
  } catch {
    return null;
  }
}

// Small links under the game info: "System requirements" (the website's
// section, with the minimum inline) and, while in early access, "Report a bug"
// (the game's bug forum). Crash reports stay in their own prompt.
function requirementsLine(g: CatalogGame) {
  const r = g.requirements;
  const bug = g.early_access && g.bug_report_url ? g.bug_report_url : null;
  if (!r?.url && !bug) return null;
  const link = (id: string, label: string, url: string, what: string, icon?: string) =>
    h("a", { href: "#", "data-id": id, onclick: (e: Event) => { e.preventDefault(); void openSite(url, what); } }, ...(icon ? [h("span", { class: "ico", html: icon })] : []), label);
  return h(
    "div",
    { class: "reqs" },
    h(
      "p",
      { class: "reqs-links" },
      r?.url ? link("requirements", "System requirements", r.url, "system requirements") : null,
      bug ? link("report-bug", "Report a bug", bug, "bug forum", icons.bug) : null,
    ),
    r?.minimum_summary ? h("p", { class: "reqs-min" }, `Minimum: ${r.minimum_summary}${r.minimum_target ? ` (${r.minimum_target})` : ""}`) : null,
  );
}

function buyLabel(g: CatalogGame): string {
  const price = fmt.price(g.price);
  return price ? `Buy on the website · ${price}` : "Buy on the website";
}

function actions(lg: LibraryGame) {
  const g = lg.game;
  const inst = lg.install;
  const row = h("div", { class: "actions" });
  if (!lg.owned) {
    if (S.pending.has(g.id)) return waitingPanel(g);
    if (g.sales_open === false) {
      row.append(
        btn("store", buyLabel(g).replace("Buy on the website", "Buy"), () => {}, { cls: "primary big", icon: icons.store, disabled: true, title: "Not on sale yet" }),
        h("span", { class: "soon-badge" }, "Coming soon"),
        btn("store-page", "Store page", () => void openSite(g.store_url, "store page"), { cls: "ghost big", icon: icons.external }),
        btn("redeem", "Redeem a key", () => redeemDialog(), { cls: "ghost big", icon: icons.key }),
      );
      return row;
    }
    row.append(
      btn("store", buyLabel(g), () => buy(g), { cls: "primary big", icon: icons.store }),
      btn("redeem", "Redeem a key", () => redeemDialog(), { cls: "ghost big", icon: icons.key }),
    );
    return row;
  }
  if (!inst) {
    row.append(btn("install", "Install", () => installDialog(lg), { cls: "primary big", icon: icons.download }));
    row.append(btn("license", "", () => void licenseDialog(lg), { cls: "ghost square", icon: icons.key, title: "View license key" }));
    if (S.justOwned.has(g.id)) row.append(h("span", { class: "ready-note" }, h("span", { class: "ico", html: icons.check }), "Purchase complete. Ready to install."));
    return row;
  }
  if (inst.folder_missing) {
    row.append(
      btn("reinstall", "Install again", async () => { await api.forget(g.id); await loadLibrary(); installDialog(lg); }, { cls: "primary big", icon: icons.download }),
      h("span", { class: "warn-text" }, `The install folder is missing: ${inst.path}`),
    );
    return row;
  }
  if (!inst.version) {
    row.append(btn("resume", `Resume install${inst.paused_version ? ` (${inst.paused_version})` : ""}`, () => run(() => api.update(g.id)), { cls: "primary big", icon: icons.download }));
    row.append(btn("cancel-install", "Remove", () => confirmUninstall(lg), { cls: "ghost big", icon: icons.trash }));
    return row;
  }
  if (inst.update_available || inst.paused_version) {
    const label = inst.paused_version ? `Resume update to ${inst.paused_version}` : `Update to ${inst.latest_version}`;
    row.append(btn("update", label, () => run(() => api.update(g.id)), { cls: "primary big", icon: icons.update }));
    row.append(...playButton(g, "ghost big", inst.update_required ? requiredGameText(g.title, inst) : null));
  } else {
    row.append(...playButton(g, "primary big play"));
  }
  if (lg.branches.includes("beta")) {
    const onBeta = lg.branch === "beta";
    row.append(
      btn("channel", onBeta ? "Beta: on" : "Beta: off", async () => {
        await run(() => api.setBranch(g.id, onBeta ? "live" : "beta"));
        toast(onBeta ? `${g.title} follows the stable channel again.` : `${g.title} now gets beta updates.`);
      }, { cls: `ghost ${onBeta ? "beta-on" : ""}`, title: "Switch between the stable and beta channels" }),
    );
  }
  row.append(
    btn("whats-new", "", () => void whatsNew(lg), { cls: "ghost square", icon: icons.notes, title: "What's new (release notes)" }),
    btn("verify", "", () => run(() => api.verify(g.id)), { cls: "ghost square", icon: icons.check, title: "Verify and repair files" }),
    btn("license", "", () => void licenseDialog(lg), { cls: "ghost square", icon: icons.key, title: "View license key" }),
    btn("folder", "", () => api.openFolder(g.id), { cls: "ghost square", icon: icons.folder, title: "Open install folder" }),
    btn("uninstall", "", () => confirmUninstall(lg), { cls: "ghost square", icon: icons.trash, title: "Uninstall" }),
  );
  return row;
}

// Play, or a disabled Play (with the reason as a tooltip) plus Requirements
// while something the game needs is missing.
function playButton(g: CatalogGame, cls: string, updateReason: string | null = null): HTMLElement[] {
  // A required (mandatory) update blocks Play until it's installed.
  if (updateReason) {
    return [h("span", { class: "disabled-wrap", title: updateReason }, btn("play", "Play", () => {}, { cls, icon: icons.play, disabled: true, title: updateReason }))];
  }
  const reason = playBlockedReason(S.prereqs);
  if (!reason) return [btn("play", "Play", () => launch(g), { cls, icon: icons.play })];
  return [
    h("span", { class: "disabled-wrap", title: reason }, btn("play", "Play", () => {}, { cls, icon: icons.play, disabled: true, title: reason })),
    btn("open-requirements", "Requirements", () => openRequirements(g.id, true), { cls: "ghost big", icon: icons.cross, title: "See what's missing and fix it" }),
  ];
}

// ------------------------------------------------------------------ buying

// Games the player went to buy on the website. Remembered across restarts so a
// launcher that was closed while buying still notices the new game.
const PENDING_KEY = "zirtola.pendingPurchases";
const PENDING_MAX_MS = 15 * 60_000;
const PENDING_POLL_MS = 7_000;

function loadPending(): Map<string, number> {
  try {
    const raw = JSON.parse(localStorage.getItem(PENDING_KEY) || "{}") as Record<string, number>;
    return new Map(Object.entries(raw).filter(([, t]) => Date.now() - t < PENDING_MAX_MS));
  } catch {
    return new Map();
  }
}

function savePending() {
  try {
    localStorage.setItem(PENDING_KEY, JSON.stringify(Object.fromEntries(S.pending)));
  } catch {
    /* storage unavailable: polling still works for this session */
  }
}

function buy(g: CatalogGame) {
  S.pending.set(g.id, Date.now());
  savePending();
  void openSite(buyUrl(g), "checkout page");
  render();
  schedulePurchasePoll();
}

function stopWaiting(id: string) {
  S.pending.delete(id);
  savePending();
  render();
}

function waitingPanel(g: CatalogGame) {
  return h(
    "div",
    { class: "waiting-panel" },
    h("div", { class: "spinner small" }),
    h(
      "div",
      { class: "waiting-text" },
      h("strong", {}, "Waiting for your purchase…"),
      h("small", {}, "Finish buying on the website. The game appears here as soon as the purchase goes through."),
    ),
    btn("purchase-refresh", "Refresh", () => void refreshNow(), { cls: "ghost", icon: icons.update }),
    btn("purchase-store", "", () => void openSite(buyUrl(g), "checkout page"), { cls: "ghost square", icon: icons.external, title: "Open the store page again" }),
    btn("purchase-cancel", "", () => stopWaiting(g.id), { cls: "ghost square", icon: icons.close, title: "Stop waiting" }),
  );
}

let pollTimer: number | undefined;

// Poll entitlements every few seconds while a purchase is pending, for up to
// 15 minutes per game, then stop.
function schedulePurchasePoll() {
  if (pollTimer !== undefined) return;
  const tick = async () => {
    pollTimer = undefined;
    for (const [id, t] of S.pending) if (Date.now() - t > PENDING_MAX_MS) S.pending.delete(id);
    savePending();
    if (!S.pending.size || !S.me) {
      render();
      return;
    }
    await loadLibrary();
    if (S.pending.size) pollTimer = window.setTimeout(tick, PENDING_POLL_MS);
  };
  pollTimer = window.setTimeout(tick, PENDING_POLL_MS);
}

// Called after every library load: pending games that are now owned get
// highlighted and selected, with Install ready.
function settlePurchases() {
  for (const lg of S.library) {
    if (lg.owned && S.pending.has(lg.game.id)) {
      S.pending.delete(lg.game.id);
      S.justOwned.add(lg.game.id);
      S.selected = lg.game.id;
      S.view = "library";
      toast(`${lg.game.title} is yours. Ready to install.`);
    }
  }
  savePending();
}

async function refreshNow() {
  lastRefresh = Date.now();
  await loadLibrary();
}

function primaryAction() {
  const g = selectedGame();
  if (!g || S.view !== "library" || g.job) return;
  const b = app.querySelector<HTMLButtonElement>(".hero .btn.primary");
  b?.click();
}

function shelf() {
  return h(
    "section",
    { class: "shelf" },
    h("h2", {}, "Games"),
    h(
      "div",
      { class: "shelf-row" },
      ...S.library.map((lg) => {
        const p = S.progress.get(lg.game.id);
        const pending = !lg.owned && S.pending.has(lg.game.id);
        const fresh = lg.owned && S.justOwned.has(lg.game.id);
        const badge = lg.job
          ? lg.job === "verify" ? "Verifying" : lg.job === "uninstall" ? "Uninstalling" : "Downloading"
          : pending
            ? "Waiting for purchase…"
            : !lg.owned
              ? fmt.price(lg.game.price) || "Locked"
              : fresh && !lg.install
                ? "New · Ready to install"
                : lg.install?.update_available
                  ? "Update"
                  : lg.install?.version
                    ? "Installed"
                    : lg.install
                      ? "Paused"
                      : "Ready to install";
        const state = pending ? "pending" : !lg.owned ? "locked" : fresh ? "owned fresh" : "owned";
        return h(
          "button",
          {
            class: `game-card ${state} ${lg.game.id === selectedGame()?.game.id ? "selected" : ""}`,
            "data-id": `card-${lg.game.id}`,
            type: "button",
            style: `--accent:${lg.game.art.accent}`,
            title: lg.owned ? `${lg.game.title} (owned)` : `${lg.game.title} (not owned)`,
            onclick: () => { S.selected = lg.game.id; render(); },
          },
          h("div", { class: "art", html: keyArt(lg.game.id, lg.game.art.accent, lg.game.title) }),
          !lg.owned ? h("span", { class: "lock", html: pending ? icons.update : icons.lock, title: pending ? "Waiting for purchase" : "Not owned" }) : null,
          lg.owned ? h("span", { class: "owned-tick", html: icons.check, title: "Owned" }) : null,
          h("div", { class: "card-text" }, h("strong", {}, lg.game.title), h("span", { class: "badge" }, badge)),
          p && p.total_bytes ? h("div", { class: "mini-bar" }, h("div", { style: `width:${(p.done_bytes / p.total_bytes) * 100}%` })) : null,
        );
      }),
    ),
  );
}

async function run(fn: () => Promise<unknown>) {
  try {
    await fn();
    await loadLibrary();
  } catch (e) {
    handleError(e);
  }
}

async function launch(g: CatalogGame) {
  const items = await refreshPrereqs(g.id);
  if (blocking(items).length) {
    openRequirements(g.id, true);
    return;
  }
  // Re-check for a game update if the last check is more than ~10 minutes old.
  if (S.me && needsRecheck(S.lastUpdateCheck, Date.now())) {
    await loadLibrary();
    const lg = S.library.find((x) => x.game.id === g.id);
    if (lg?.install?.update_required && !lg.job) {
      void gameUpdatePrompt(lg, { force: true });
      return;
    }
    if (lg?.install?.update_available && !lg.job) {
      if (S.settings?.auto_update_games) {
        toast(`${g.title} ${lg.install.latest_version ?? ""} is out. Updating first; press Play when it's done.`);
        await run(() => api.update(g.id));
        return;
      }
      toast(`${g.title} ${lg.install.latest_version ?? ""} is available. Starting the version you have.`);
    }
  }
  try {
    await api.launch(g.id, g.title);
    toast(`Starting ${g.title}…`);
  } catch (e) {
    if (errorCode(e) === "device_conflict") deviceConflictDialog(g, e as CmdError);
    else if (errorCode(e) === "update_required") {
      // The website refused an install older than a required release.
      await loadLibrary();
      const lg = S.library.find((x) => x.game.id === g.id);
      if (lg?.install?.version) void gameUpdatePrompt(lg, { force: true, requiredMessage: errorMessage(e) });
      else toast(errorMessage(e), "error");
    } else handleError(e);
  }
}

// One PC per licence: the server refused this PC because another one is
// active. Offer to deactivate it here (the player is signed in; they confirm).
function deviceConflictDialog(g: CatalogGame, e: CmdError) {
  const devices = e.details?.active_devices ?? [];
  const msg = h("p", { class: "muted" }, e.message);
  const list = h(
    "div",
    { class: "device-list" },
    ...devices.map((d) =>
      h(
        "div",
        { class: "device-row" },
        h("div", {}, h("strong", {}, d.name), h("small", {}, `Activated ${new Date(d.activated_at).toLocaleDateString()} · last used ${new Date(d.last_seen_at).toLocaleDateString()}`)),
        btn(`deactivate-${d.id}`, "Deactivate and play here", async () => {
          if (!confirmBox.checked) {
            note.textContent = "Tick the box to confirm first.";
            note.className = "error-text";
            return;
          }
          try {
            await api.deactivateDevice(d.id);
            close();
            toast(`${d.name} deactivated. Starting ${g.title} on this PC…`);
            await launch(g);
          } catch (err) {
            note.textContent = errorMessage(err);
            note.className = "error-text";
          }
        }, { cls: "primary" }),
      ),
    ),
  );
  const confirmBox = h("input", { type: "checkbox", "data-id": "confirm-deactivate" }) as HTMLInputElement;
  const note = h(
    "p",
    { class: "muted small" },
    "Your licence moves with you: it plays on one PC at a time, and you can switch whenever you like. Deactivating is free and the other PC simply stops until you move it back. You can also do this from your account page on the website. ",
    h("a", { href: "#", "data-id": "devices-learn-more", onclick: (e: Event) => { e.preventDefault(); void openSite(devicesFaqUrl(), "help page"); } }, "Learn more"),
  );
  const close = modal(
    `${g.title} is active on another PC`,
    h(
      "div",
      {},
      msg,
      list,
      h("label", { class: "confirm-row" }, confirmBox, h("span", {}, `I'm signed in as ${S.me?.display_name ?? "me"} and want to move ${g.title} to this PC.`)),
      note,
      h("div", { class: "modal-actions" }, btn("cancel", "Not now", () => close(), { cls: "ghost" })),
    ),
  );
}

// ------------------------------------------------------------------ license key

const SOURCE_LABEL: Record<string, string> = { purchase: "Purchase", key: "Redeemed key", grant: "Gift from the studio", early_access: "Early access" };

// Fetched live from the website for the signed-in account each time; the key
// is only kept in this dialog's memory, never written to disk.
async function licenseDialog(lg: LibraryGame) {
  let lic: Awaited<ReturnType<typeof api.license>>;
  try {
    lic = await api.license(lg.game.id);
  } catch (e) {
    handleError(e);
    return;
  }
  let shown = false;
  const keyText = h("code", { class: "license-key", "data-id": "license-key" }, lic.key_masked ?? "");
  const showBtn = btn("license-show", "Show", () => {
    shown = !shown;
    keyText.textContent = shown ? lic.key : lic.key_masked;
    showBtn.querySelector("span:last-child")!.textContent = shown ? "Hide" : "Show";
  }, { cls: "ghost" });
  const keyBlock = lic.key
    ? h(
        "div",
        { class: "license-key-row" },
        keyText,
        showBtn,
        btn("license-copy", "Copy", async () => {
          try {
            await navigator.clipboard.writeText(lic.key!);
            toast("Copied");
          } catch {
            toast("Couldn't copy. Show the key and copy it by hand.", "error");
          }
        }, { cls: "ghost" }),
      )
    : h("p", { class: "license-none" }, "This game is tied to your Zirtola account. No key needed.");

  const details = h(
    "dl",
    { class: "license-details" },
    h("dt", {}, "Game"), h("dd", {}, lic.game_title),
    h("dt", {}, "From"), h("dd", {}, SOURCE_LABEL[lic.source] ?? lic.source),
    h("dt", {}, "Date"), h("dd", {}, new Date(lic.granted_at).toLocaleDateString()),
    ...(lic.order_number ? [h("dt", {}, "Order"), h("dd", {}, lic.order_number)] : []),
  );

  const act = h("div", { class: "license-activation" });
  const here = lic.activations.find((a) => a.this_device);
  const other = lic.activations.find((a) => !a.this_device);
  if (here) act.append(h("p", {}, h("strong", {}, "Active on this PC"), ` (${here.name})`));
  if (other) {
    act.append(
      h(
        "div",
        { class: "device-row" },
        h("div", {}, h("strong", {}, `Active on ${other.name}`), h("small", {}, `last seen ${new Date(other.last_seen_at).toLocaleDateString()}`)),
        btn("license-deactivate", "Deactivate", async () => {
          if (!window.confirm(`Deactivate ${lg.game.title} on ${other.name}? It stops working there until you move it back.`)) return;
          try {
            await api.deactivateDevice(other.id);
            close();
            toast(`${other.name} deactivated.`);
          } catch (e) {
            handleError(e);
          }
        }, { cls: "ghost" }),
      ),
    );
  }
  if (!here && !other) act.append(h("p", { class: "muted" }, "Not active on any PC yet. Playing activates this PC."));
  act.append(
    h(
      "p",
      { class: "muted small" },
      "Plays on one PC at a time; switch whenever you like. ",
      h("a", { href: "#", onclick: (e: Event) => { e.preventDefault(); void openSite(devicesFaqUrl(), "help page"); } }, "Learn more"),
    ),
  );
  const accountUrl = (() => {
    try {
      return S.info ? `${new URL(S.info.api_base).origin}/settings/games` : null;
    } catch {
      return null;
    }
  })();
  act.append(
    h("a", { href: "#", class: "small", "data-nav": true, "data-id": "license-account", onclick: (e: Event) => { e.preventDefault(); void openSite(accountUrl, "account page"); } }, "All your devices on the website"),
  );

  const close = modal(
    `${lg.game.title}: license`,
    h(
      "div",
      { class: "license" },
      keyBlock,
      details,
      h("h4", {}, "Activation"),
      act,
      lic.key ? h("p", { class: "muted small license-safety" }, "Keep your key private. Anyone with it can try to claim your game.") : null,
      h("div", { class: "modal-actions" }, btn("license-close", "Close", () => close(), { cls: "ghost" })),
    ),
  );
}

// ------------------------------------------------------------------ what's new

// Tiny Markdown subset for release notes: headings, lists, bold, code, links
// as text. Everything is escaped first; no HTML from the server is trusted.
function renderNotes(md: string): HTMLElement {
  const box = h("div", { class: "notes" });
  const inline = (text: string) => {
    const span = h("span", {});
    const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
    for (const p of parts) {
      if (/^\*\*[^*]+\*\*$/.test(p)) span.append(h("strong", {}, p.slice(2, -2)));
      else if (/^`[^`]+`$/.test(p)) span.append(h("code", {}, p.slice(1, -1)));
      else span.append(p.replace(/\[([^\]]+)\]\([^)]+\)/g, "$1"));
    }
    return span;
  };
  for (const b of parseNotes(md)) {
    if (b.kind === "h") box.append(h("h4", {}, inline(b.text)));
    else if (b.kind === "p") box.append(h("p", {}, inline(b.text)));
    else box.append(h("ul", {}, ...b.items.map((t) => h("li", {}, inline(t)))));
  }
  return box;
}

async function whatsNew(lg: LibraryGame, opts: { afterUpdate?: boolean } = {}) {
  let entries: Awaited<ReturnType<typeof api.changelog>>["entries"] = [];
  try {
    entries = (await api.changelog(lg.game.id)).entries;
  } catch (e) {
    toast(errorMessage(e), "error");
    return;
  }
  const installed = lg.install?.version ?? null;
  const body = h(
    "div",
    { class: "changelog" },
    entries.length === 0
      ? h("p", { class: "muted" }, "No release notes yet.")
      : h(
          "div",
          {},
          ...entries.map((en) =>
            h(
              "section",
              { class: `cl-entry ${en.version === installed ? "installed" : ""}` },
              h(
                "div",
                { class: "cl-head" },
                h("strong", {}, en.version),
                en.channel === "beta" ? h("span", { class: "pill beta" }, "Beta") : null,
                en.version === installed ? h("span", { class: "pill installed" }, "Installed") : null,
                h("small", {}, new Date(en.released_at).toLocaleDateString()),
              ),
              renderNotes(en.notes || "No notes for this version."),
            ),
          ),
        ),
  );
  modal(opts.afterUpdate ? `${lg.game.title} updated: what's new` : `${lg.game.title}: updates`, h("div", {}, body));
}

// ------------------------------------------------------------------ update prompts

// "Update now / Later" with the notes of every release since the installed
// one (updates.ts). A required update has no Later and says why.
const prompts = new PromptMemo();

type NoteEntry = { version: string; released_at: string; notes: string; channel?: string; mandatory?: boolean; mandatory_reason?: string | null };

function changelogList(entries: NoteEntry[]) {
  return h(
    "div",
    { class: "changelog" },
    ...entries.map((en) =>
      h(
        "section",
        { class: "cl-entry" },
        h(
          "div",
          { class: "cl-head" },
          h("strong", {}, en.version),
          en.channel === "beta" ? h("span", { class: "pill beta" }, "Beta") : null,
          en.mandatory ? h("span", { class: "pill required", title: en.mandatory_reason ?? "" }, "Required") : null,
          en.released_at && !Number.isNaN(Date.parse(en.released_at)) ? h("small", {}, new Date(en.released_at).toLocaleDateString()) : null,
        ),
        renderNotes(en.notes || "No notes for this version."),
      ),
    ),
  );
}

function updateWindow(o: { id: string; title: string; required: boolean; reason: string | null; intro: string; entries: NoteEntry[]; onUpdate: () => unknown }): Promise<void> {
  return new Promise((resolve) => {
    const content = h(
      "div",
      { class: "update-prompt" },
      o.required && o.reason ? h("div", { class: "required-banner", "data-id": "required-reason" }, h("span", { class: "ico", html: icons.update }), h("span", {}, o.reason)) : null,
      h("p", { class: "muted" }, o.intro),
      o.entries.length ? changelogList(o.entries) : h("p", { class: "muted" }, "No release notes for this update."),
      h(
        "div",
        { class: "modal-actions" },
        o.required ? null : btn("update-later", "Later", () => close(), { cls: "ghost" }),
        btn("update-now", "Update now", () => { close(); void o.onUpdate(); }, { cls: "primary", icon: icons.update }),
      ),
    );
    const close = modal(o.title, content, { locked: o.required, id: o.id, onClose: resolve });
  });
}

/** Offer a game update. Optional ones show once per version per session unless forced. */
async function gameUpdatePrompt(lg: LibraryGame, opts: { force?: boolean; requiredMessage?: string } = {}) {
  const inst = lg.install;
  const kind = opts.requiredMessage ? "required" : gamePromptKind(inst);
  if (!kind || !inst?.version || !inst.latest_version || lg.job) return;
  if (!opts.force && !prompts.shouldShow(lg.game.id, inst.latest_version, kind === "required")) return;
  if (document.querySelector(`[data-id="game-update-${lg.game.id}"]`)) return;
  let entries: NoteEntry[] = [];
  try {
    entries = entriesBetween((await api.changelog(lg.game.id)).entries, inst.version, inst.latest_version);
  } catch {
    /* the window still offers the update */
  }
  const required = kind === "required";
  await updateWindow({
    id: `game-update-${lg.game.id}`,
    title: required ? `${lg.game.title}: required update` : `${lg.game.title} ${inst.latest_version} is available`,
    required,
    reason: required ? (opts.requiredMessage ?? requiredGameText(lg.game.title, inst)) : null,
    intro: `You have ${inst.version}. ${entries.length > 1 ? `Here is what changed in the ${entries.length} updates since then.` : "Here is what's new."} Your saves are kept.`,
    entries,
    onUpdate: () => run(() => api.update(lg.game.id)),
  });
}

function launcherUpdateRequired(): boolean {
  const u = S.launcherUpdate;
  return Boolean(u && (u.mandatory || belowMinimum(S.info?.version ?? "0.0.0", S.info?.min_launcher_version)));
}

async function launcherUpdatePrompt(opts: { force?: boolean } = {}) {
  const u = S.launcherUpdate;
  if (!u || S.launcherUpdating) return;
  const required = launcherUpdateRequired();
  if (!opts.force && !prompts.shouldShow("launcher", u.version, required)) return;
  if (document.querySelector('[data-id="launcher-update-prompt"]')) return;
  await updateWindow({
    id: "launcher-update-prompt",
    title: required ? "Launcher: required update" : `Zirtola Launcher ${u.version} is available`,
    required,
    reason: required ? requiredLauncherText(u.version, u.mandatory_reason, belowMinimum(S.info?.version ?? "0.0.0", S.info?.min_launcher_version)) : null,
    intro: `You have ${S.info?.version ?? "an older version"}. The launcher restarts to finish; your games and saves aren't touched.`,
    entries: [{ version: u.version, released_at: u.date ?? "", notes: u.notes ?? "" }],
    onUpdate: installLauncherUpdate,
  });
}

function handleError(e: unknown) {
  if (errorCode(e) === "signed_out") {
    S.me = null;
    render();
  }
  toast(errorMessage(e), "error");
}

function step(delta: number) {
  if (!S.library.length || S.view !== "library") return;
  const i = Math.max(0, S.library.findIndex((g) => g.game.id === selectedGame()?.game.id));
  const n = (i + delta + S.library.length) % S.library.length;
  S.selected = S.library[n]!.game.id;
  render();
  focusFirst();
}

async function loadLibrary() {
  try {
    const lib = await api.library();
    // Owned first, then catalogue order.
    S.library = [...lib.filter((g) => g.owned), ...lib.filter((g) => !g.owned)];
    if (!S.selected || !S.library.some((g) => g.game.id === S.selected)) S.selected = S.library[0]?.game.id ?? null;
    S.offline = null;
    S.lastUpdateCheck = Date.now();
    S.updateNote = null;
    settlePurchases();
  } catch (e) {
    handleError(e);
  }
  render();
}

// ------------------------------------------------------------------ modals

// `locked`: a window that must be answered (a required update): no closing by
// clicking outside, Escape or the gamepad's B button.
function modal(title: string, content: HTMLElement, opts: { onClose?: () => void; locked?: boolean; id?: string } = {}) {
  const close = () => {
    backdrop.remove();
    opts.onClose?.();
    render();
    focusFirst();
  };
  const backdrop = h(
    "div",
    { class: "modal-backdrop", onclick: (e: Event) => { if (e.target === backdrop && !opts.locked) close(); } },
    h("div", { class: "modal", role: "dialog", "aria-modal": "true", ...(opts.id ? { "data-id": opts.id } : {}) }, h("h2", {}, title), content),
  );
  if (opts.locked) backdrop.dataset.locked = "1";
  (backdrop as HTMLElement & { close?: () => void }).close = close;
  document.body.append(backdrop);
  setTimeout(() => focusFirst(), 0);
  return close;
}

function closeTopModal(): boolean {
  const all = document.querySelectorAll<HTMLElement & { close?: () => void }>(".modal-backdrop");
  const top = all[all.length - 1];
  if (top?.dataset.locked) return true; // answered with its own buttons only
  if (top?.close) {
    top.close();
    return true;
  }
  return false;
}

async function installDialog(lg: LibraryGame) {
  const g = lg.game;
  let path = await api.suggestInstallDir(g.title);
  const pathInput = h("input", { class: "text", value: path, "data-id": "install-path", spellcheck: "false" }) as HTMLInputElement;
  const space = h("div", { class: "space" });
  const installBtn = btn("do-install", "Install", () => void doInstall(), { cls: "primary" });

  const refreshSpace = async () => {
    path = pathInput.value.trim();
    try {
      const { free_bytes } = await api.diskInfo(path);
      const need = g.install_size ?? 0;
      const ok = free_bytes > need + 1e9;
      space.replaceChildren(
        h("div", {}, h("span", { class: "muted" }, "Download size"), h("strong", {}, fmt.bytes(need))),
        h("div", {}, h("span", { class: "muted" }, "Free space"), h("strong", { class: ok ? "" : "error-text" }, fmt.bytes(free_bytes))),
      );
      (installBtn as HTMLButtonElement).disabled = !ok;
      if (!ok) space.append(h("p", { class: "error-text" }, "Not enough free space on that drive."));
    } catch (e) {
      space.replaceChildren(h("p", { class: "error-text" }, errorMessage(e)));
    }
  };
  pathInput.addEventListener("change", refreshSpace);

  const doInstall = async () => {
    try {
      await api.install(g.id, pathInput.value.trim());
      close();
      await loadLibrary();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };

  const content = h(
    "div",
    {},
    h("p", { class: "muted" }, `Choose where to install ${g.title}. You can pause and resume at any time.`),
    h("label", { class: "field" }, h("span", {}, "Install folder"), h("div", { class: "row" }, pathInput, btn("browse", "Browse…", async () => {
      const parent = await api.pickFolder();
      if (parent) {
        pathInput.value = await api.suggestInstallDir(g.title, parent);
        refreshSpace();
      }
    }, { cls: "ghost", icon: icons.folder }))),
    space,
    h("div", { class: "modal-actions" }, btn("cancel", "Cancel", () => close(), { cls: "ghost" }), installBtn),
  );
  const close = modal(`Install ${g.title}`, content);
  refreshSpace();
}

function redeemDialog() {
  if (document.querySelector(".modal-backdrop")) return;
  const input = h("input", { class: "text key", placeholder: "XXXX-XXXX-XXXX-XXXX", "data-id": "key-input", maxlength: "64", spellcheck: "false", autocomplete: "off" }) as HTMLInputElement;
  const msg = h("p", { class: "muted" }, "Enter a key from the studio, a bundle or a giveaway. The game is added to your Zirtola account.");
  const submit = async () => {
    try {
      const r = await api.redeemKey(input.value);
      close();
      toast(`${r.game_title} was added to your account.`);
      S.selected = r.entitlement.game_id;
      S.view = "library";
      await loadLibrary();
    } catch (e) {
      msg.className = "error-text";
      msg.textContent = errorMessage(e);
    }
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
  input.addEventListener("input", () => {
    const raw = input.value.toUpperCase().replace(/[^A-Z0-9]/g, "").slice(0, 16);
    input.value = raw.match(/.{1,4}/g)?.join("-") ?? "";
  });
  const close = modal(
    "Redeem a key",
    h("div", {}, msg, input, h("div", { class: "modal-actions" }, btn("cancel", "Cancel", () => close(), { cls: "ghost" }), btn("redeem-go", "Redeem", submit, { cls: "primary" }))),
  );
  setTimeout(() => input.focus(), 0);
}

async function confirmUninstall(lg: LibraryGame) {
  const raw = lg.game.save_location ?? null;
  const where = raw ? await api.resolveSaveLocation(raw).catch(() => null) : null;
  const alsoSaves = h("input", { type: "checkbox", "data-id": "delete-saves" }) as HTMLInputElement;
  const close = modal(
    `Uninstall ${lg.game.title}?`,
    h(
      "div",
      {},
      h("p", { class: "muted" }, `This deletes the game files in ${lg.install?.path}. Only files the game was installed with are removed; anything else in that folder stays. You can install again at any time.`),
      where
        ? h(
            "div",
            { class: "saves-box" },
            h("p", {}, "Your saves and settings are kept separately, in:"),
            h("code", { class: "path", "data-id": "save-path" }, where),
            h("label", { class: "confirm-row" }, alsoSaves, h("span", {}, "Also delete saves (this can't be undone)")),
          )
        : h("p", { class: "muted small" }, "Saves and settings are stored outside the install folder and are not touched."),
      h(
        "div",
        { class: "modal-actions" },
        btn("cancel", "Keep it", () => close(), { cls: "ghost" }),
        btn("confirm-uninstall", "Uninstall", async () => {
          const deleteSaves = alsoSaves.checked;
          close();
          await run(() => api.uninstall(lg.game.id));
          if (deleteSaves) {
            try {
              const gone = await api.deleteGameSaves(lg.game.id);
              toast(`Saves deleted from ${gone}.`);
            } catch (e) {
              toast(errorMessage(e), "error");
            }
          }
        }, { cls: "danger" }),
      ),
    ),
  );
}

// ------------------------------------------------------------------ settings

function settingsView() {
  const s = S.settings;
  if (!s) return h("div", { class: "empty" }, h("div", { class: "spinner" }));
  const save = async (patch: Partial<Settings>) => {
    const next = { ...s, ...patch };
    try {
      await api.setSettings(next);
      S.settings = next;
      render();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };
  const par = h("input", { type: "range", min: "1", max: "16", step: "1", value: String(s.parallel_downloads), "data-id": "parallel" }) as HTMLInputElement;
  const parVal = h("strong", {}, String(s.parallel_downloads));
  par.addEventListener("input", () => (parVal.textContent = par.value));
  par.addEventListener("change", () => save({ parallel_downloads: +par.value }));
  const auto = h("input", { type: "checkbox", "data-id": "auto-update", checked: s.auto_update_launcher }) as HTMLInputElement;
  auto.addEventListener("change", () => save({ auto_update_launcher: auto.checked }));
  const autoGames = h("input", { type: "checkbox", "data-id": "auto-update-games", checked: s.auto_update_games }) as HTMLInputElement;
  autoGames.addEventListener("change", () => save({ auto_update_games: autoGames.checked }));
  const hw = h("input", { type: "checkbox", "data-id": "share-hw", checked: s.share_hardware_info }) as HTMLInputElement;
  hw.addEventListener("change", () => save({ share_hardware_info: hw.checked }));
  const crashes = h("input", { type: "checkbox", "data-id": "offer-crash", checked: s.offer_crash_reports }) as HTMLInputElement;
  crashes.addEventListener("change", () => save({ offer_crash_reports: crashes.checked }));

  return h(
    "div",
    { class: "settings" },
    h("h1", {}, "Settings"),
    h(
      "section",
      { class: "card" },
      h("h3", {}, "Downloads"),
      h(
        "div",
        { class: "setting" },
        h("div", {}, h("strong", {}, "Default library folder"), h("small", {}, s.library_dir)),
        btn("change-lib", "Change…", async () => {
          const p = await api.pickFolder();
          if (p) save({ library_dir: p });
        }, { cls: "ghost", icon: icons.folder }),
      ),
      h("div", { class: "setting" }, h("div", {}, h("strong", {}, "Parallel downloads"), h("small", {}, "More can be faster on fast connections.")), h("div", { class: "range" }, par, parVal)),
    ),
    h(
      "section",
      { class: "card" },
      h("h3", {}, "Launcher"),
      h("label", { class: "setting" }, h("div", {}, h("strong", {}, "Update the launcher automatically"), h("small", {}, "Checks every time the launcher starts and installs a new version. Required updates always install.")), auto),
      h("label", { class: "setting" }, h("div", {}, h("strong", {}, "Update games automatically"), h("small", {}, "Checks every time the launcher starts and before you play. Updates never touch your saves or settings.")), autoGames),
      h(
        "div",
        { class: "setting" },
        h("div", {}, h("strong", {}, `Zirtola Launcher ${S.info?.version ?? ""}`), h("small", {}, S.info?.dev_mode ? `Development build · API ${S.info.api_base}` : "Release build")),
        btn("check-update", "Check for updates", checkLauncherUpdate, { cls: "ghost", icon: icons.update }),
      ),
      S.info?.support_url ? h("div", { class: "setting" }, h("div", {}, h("strong", {}, "Help"), h("small", {}, "Support and known issues")), btn("support", "Open support", () => void openSite(S.info?.support_url, "support page"), { cls: "ghost", icon: icons.external })) : null,
    ),
    h(
      "section",
      { class: "card" },
      h("h3", {}, "Privacy"),
      h(
        "label",
        { class: "setting" },
        h(
          "div",
          {},
          h("strong", {}, "Share hardware info"),
          h("small", { class: "wrap" }, "Sends your Windows version, CPU, RAM, graphics card (memory and driver), display resolutions, free space on the install drive and the launcher version, so the studio knows what to optimise for. No serial numbers, network addresses, user or PC names, or file paths."),
        ),
        hw,
      ),
      h("div", { class: "setting" }, h("div", {}, h("strong", {}, "What is sent"), h("small", {}, "See the exact report from this PC.")), btn("show-hw", "Show", showHardware, { cls: "ghost" })),
      h(
        "label",
        { class: "setting" },
        h(
          "div",
          {},
          h("strong", {}, "Offer to send crash reports"),
          h("small", { class: "wrap" }, "When a game crashes, ask whether to send its crash log, crash details and memory dump (with an optional comment) to the studio. Nothing is sent without asking."),
        ),
        crashes,
      ),
      h("p", { class: "privacy-note" }, "Signing in also records each PC you play on as a random install id, so the studio can count and reset activations. See the privacy page on the website for details."),
    ),
  );
}

async function showHardware() {
  try {
    const report = await api.hardwareReport();
    modal(
      "Hardware report",
      h(
        "div",
        {},
        h("p", { class: "muted" }, S.settings?.share_hardware_info ? "This is sent when you sign in and when it changes." : "Sharing is off: nothing is sent."),
        h("pre", { class: "json" }, JSON.stringify(report, null, 2)),
        h("div", { class: "modal-actions" }, btn("close-hw", "Close", () => void closeTopModal(), { cls: "primary" })),
      ),
    );
  } catch (e) {
    toast(errorMessage(e), "error");
  }
}

// ------------------------------------------------------------------ crash reports

const crashQueue: CrashCandidate[] = [];

function queueCrash(c: CrashCandidate) {
  if (crashQueue.some((x) => x.id === c.id)) return;
  crashQueue.push(c);
  if (crashQueue.length === 1) crashDialog(c);
}

function nextCrash() {
  crashQueue.shift();
  if (crashQueue[0]) setTimeout(() => crashDialog(crashQueue[0]!), 300);
}

function crashDialog(c: CrashCandidate) {
  const title = S.library.find((g) => g.game.id === c.game_id)?.game.title ?? c.game_title;
  const comment = h("textarea", { class: "text comment", rows: "3", maxlength: "2000", placeholder: "What were you doing when it crashed? (optional)", "data-id": "crash-comment" }) as HTMLTextAreaElement;
  let done = false;
  const sendBtn = btn("crash-send", "Send report", () => void send(), { cls: "primary" }) as HTMLButtonElement;
  const send = async () => {
    sendBtn.disabled = true;
    sendBtn.textContent = "Sending...";
    try {
      await api.sendCrashReport(c.id, comment.value);
      done = true;
      close();
      toast("Crash report sent. Thank you!");
    } catch (e) {
      sendBtn.disabled = false;
      sendBtn.textContent = "Send report";
      toast(errorMessage(e), "error");
    }
  };
  const what = c.files.length
    ? c.files.map((f) => h("li", {}, `${f.name} (${fmt.bytes(f.size)})`))
    : [h("li", {}, c.exit_code != null ? `The game exited with code ${c.exit_code}; no crash folder was found.` : "No crash files found.")];
  const close = modal(
    `${title} crashed`,
    h(
      "div",
      {},
      h("p", { class: "muted" }, c.error ? `"${c.error}"` : "The game closed unexpectedly."),
      h("p", {}, "Send a crash report to the studio? It helps find and fix the problem."),
      h("p", { class: "small muted" }, "The report contains:"),
      h(
        "ul",
        { class: "crash-files" },
        ...what,
        S.settings?.share_hardware_info ? h("li", {}, "your hardware summary (Windows, CPU, RAM, GPU)") : null,
        h("li", {}, `${title} ${c.game_version}`),
      ),
      comment,
      h("div", { class: "modal-actions" }, btn("crash-skip", "Don't send", () => close(), { cls: "ghost" }), sendBtn),
    ),
    {
      onClose: () => {
        if (!done) api.dismissCrash(c.id).catch(() => undefined);
        nextCrash();
      },
    },
  );
}

onGameCrashed(queueCrash);
// A game exited (or a heartbeat landed): show the new hours played.
onPlaytimeUpdated(() => refreshSoon(0));

async function checkLauncherUpdate(manual = true) {
  try {
    const u = await api.checkLauncherUpdate();
    S.launcherUpdate = u;
    render();
    if (u) void launcherUpdatePrompt({ force: manual });
    else if (manual) toast("The launcher is up to date.");
  } catch (e) {
    if (manual) toast(`Could not check for updates: ${errorMessage(e)}`, "error");
  }
}

async function installLauncherUpdate() {
  S.launcherUpdating = { downloaded: 0, total: null };
  render();
  try {
    await api.installLauncherUpdate(); // restarts the app on success
  } catch (e) {
    S.launcherUpdating = null;
    toast(errorMessage(e), "error");
    render();
  }
}

// ------------------------------------------------------------------ requirements

// What a game needs on this PC (src-tauri/src/prereqs.rs). Shown on the first
// start and before a launch whenever something is missing. Nothing is
// installed until the player clicks "Install the missing components".
const REQ_SEEN_KEY = "zirtola.requirementsSeen";

function requirementsSeen(): boolean {
  try {
    return localStorage.getItem(REQ_SEEN_KEY) === "1";
  } catch {
    return true;
  }
}

function markRequirementsSeen() {
  try {
    localStorage.setItem(REQ_SEEN_KEY, "1");
  } catch {
    /* fine */
  }
}

async function refreshPrereqs(gameId: string | null): Promise<PrereqItem[] | null> {
  const lg = gameId ? S.library.find((g) => g.game.id === gameId) : null;
  const need = lg && !lg.install ? lg.game.install_size ?? null : null;
  try {
    S.prereqs = await api.prereqsCheck(gameId, need);
  } catch (e) {
    toast(`Couldn't check this PC: ${errorMessage(e)}`, "error");
  }
  return S.prereqs;
}

function openRequirements(gameId: string | null, thenPlay = false) {
  S.req = { game: gameId, thenPlay, installing: false, progress: new Map(), errors: new Map(), checking: true };
  S.view = "requirements";
  render();
  void refreshPrereqs(gameId).then(() => {
    if (S.req) S.req.checking = false;
    render();
    focusFirst();
  });
}

function closeRequirements() {
  markRequirementsSeen();
  S.req = null;
  S.view = "library";
  render();
  focusFirst();
}

async function installMissing(ids: string[]) {
  if (!S.req || !ids.length) return;
  S.req.installing = true;
  for (const id of ids) {
    S.req.errors.delete(id);
    S.req.progress.delete(id);
  }
  render();
  try {
    await api.prereqsInstall(ids, S.req.game);
  } catch {
    /* the failing item's row shows the error (prereq-progress "error") */
  }
  if (!S.req) return;
  S.req.installing = false;
  S.req.checking = true;
  render();
  await refreshPrereqs(S.req.game);
  if (S.req) S.req.checking = false;
  render();
  focusFirst();
}

function requirementsView() {
  const r = S.req;
  const game = r?.game ? S.library.find((g) => g.game.id === r.game)?.game ?? null : null;
  const items = S.prereqs ?? [];
  const missing = installable(items);
  const blockers = blocking(items);
  const advisory = blockers.filter((i) => !i.fixable);
  const title = game ? `${game.title}: what it needs` : "What your games need";

  const row = (i: PrereqItem) => {
    const ok = i.status === "ok";
    const p = r?.progress.get(i.id);
    const err = r?.errors.get(i.id);
    const pct = p && p.total ? Math.min(100, (p.done / p.total) * 100) : p?.phase === "install" || p?.phase === "verify" ? 100 : 0;
    return h(
      "div",
      { class: `req-row ${ok ? "ok" : "bad"}`, "data-req": i.id },
      h("span", { class: "req-mark", "aria-label": ok ? "Installed" : "Missing", html: ok ? icons.check : icons.cross }),
      h(
        "div",
        { class: "req-text" },
        h("strong", {}, i.name),
        h("small", {}, i.description),
        h("span", { class: "req-found" }, i.found),
        !ok && i.advice
          ? h(
              "p",
              { class: "req-advice" },
              i.advice,
              " ",
              i.advice_url ? h("a", { href: "#", "data-id": `advice-${i.id}`, onclick: (e: Event) => { e.preventDefault(); void openSite(i.advice_url, "help page"); } }, i.id === "gpu" ? "Driver downloads" : "How to") : null,
            )
          : null,
        p && !err && p.phase !== "done"
          ? h(
              "div",
              { class: "req-progress" },
              h("div", { class: "bar" }, h("div", { class: `fill ${p.phase === "install" ? "pulse" : ""}`, style: `width:${pct}%` })),
              h("small", {}, `${p.message ?? p.phase}${p.total && p.phase === "download" ? ` · ${fmt.bytes(p.done)} of ${fmt.bytes(p.total)}` : ""}`),
            )
          : null,
        err
          ? h(
              "div",
              { class: "req-error" },
              h("p", { class: "error-text" }, err),
              h(
                "div",
                { class: "req-error-actions" },
                btn(`retry-${i.id}`, "Retry", () => void installMissing([i.id]), { cls: "primary", disabled: r?.installing }),
                i.manual_url ? btn(`manual-${i.id}`, "Install it yourself from Microsoft", () => void openSite(i.manual_url, "Microsoft download page"), { cls: "ghost", icon: icons.external }) : null,
              ),
            )
          : null,
      ),
    );
  };

  const busy = r?.installing || r?.checking;
  let footer: HTMLElement | null = null;
  if (!busy && missing.length && !missing.some((i) => r?.errors.has(i.id))) {
    footer = h(
      "div",
      { class: "req-ask", "data-id": "req-ask" },
      h("strong", {}, "Install the missing components?"),
      h("p", { class: "muted" }, "The launcher will install:"),
      h("ul", {}, ...missing.map((i) => h("li", {}, installLine(i)))),
      h("p", { class: "muted small" }, "These come straight from Microsoft and are checked for Microsoft's signature before they run. Windows will then ask for permission to make changes to your PC: that is Windows' own prompt, click Yes."),
      h(
        "div",
        { class: "modal-actions" },
        btn("req-cancel", "Cancel", closeRequirements, { cls: "ghost" }),
        btn("req-install", "Install the missing components", () => void installMissing(missing.map((i) => i.id)), { cls: "primary" }),
      ),
    );
  } else if (!busy && !blockers.length && items.length) {
    footer = h(
      "div",
      { class: "req-done" },
      h("p", {}, h("span", { class: "ok-text" }, "All set."), " Everything the game needs is on this PC."),
      r?.thenPlay && game
        ? btn("req-play", `Play ${game.title}`, () => { closeRequirements(); void launch(game); }, { cls: "primary big play", icon: icons.play })
        : btn("req-continue", "Continue", closeRequirements, { cls: "primary" }),
    );
  } else if (!busy && advisory.length) {
    footer = h(
      "div",
      { class: "req-done" },
      h("p", { class: "muted" }, "The items in red can't be installed by the launcher. Follow the advice next to them, then check again."),
      h("div", { class: "modal-actions" }, btn("req-back", "Back to the library", closeRequirements, { cls: "ghost" }), btn("req-recheck", "Check again", () => openRequirements(r?.game ?? null, r?.thenPlay ?? false), { cls: "primary" })),
    );
  }

  return h(
    "div",
    { class: "settings requirements" },
    h("h1", {}, title),
    h("p", { class: "muted" }, "The launcher checks these before you play. Anything missing is listed in red; it can install what's safe to install, with your permission."),
    r?.checking && !items.length ? h("div", { class: "waiting" }, h("div", { class: "spinner small" }), h("span", {}, "Checking this PC…")) : null,
    h("div", { class: "req-list" }, ...items.map(row)),
    r?.checking && items.length ? h("p", { class: "muted small" }, "Checking again…") : null,
    footer,
  );
}

// ------------------------------------------------------------------ toasts

function toast(text: string, kind: "info" | "error" = "info") {
  let box = document.querySelector(".toasts");
  if (!box) {
    box = h("div", { class: "toasts", "aria-live": "polite" });
    document.body.append(box);
  }
  const t = h("div", { class: `toast ${kind}` }, text);
  box.append(t);
  setTimeout(() => t.classList.add("out"), kind === "error" ? 7000 : 4000);
  setTimeout(() => t.remove(), kind === "error" ? 7400 : 4400);
}

// ------------------------------------------------------------------ events & boot

onNavAction((a) => {
  if (a === "back") {
    if (closeTopModal()) return;
    if (S.view === "requirements") {
      if (!S.req?.installing) closeRequirements();
      return;
    }
    if (S.view !== "library") {
      S.view = "library";
      render();
      focusFirst();
    }
  } else if (document.querySelector(".modal-backdrop") || !S.me) {
    return;
  } else if (a === "prevGame") step(-1);
  else if (a === "nextGame") step(1);
  else if (a === "primary") primaryAction();
  else if (a === "redeem") redeemDialog();
  else if (a === "settings") {
    S.view = S.view === "settings" ? "library" : "settings";
    render();
    focusFirst();
  }
});

onGamepadActive((v) => {
  S.gamepad = v;
  render();
});

onJobProgress((p) => {
  const had = S.progress.has(p.game_id);
  S.progress.set(p.game_id, p);
  if (!had || !updateProgressDom(p)) render();
});

onJobFinished(async (f) => {
  S.progress.delete(f.game_id);
  const title = S.library.find((g) => g.game.id === f.game_id)?.game.title ?? f.game_id;
  if (f.ok) {
    const done = { install: `${title} is installed.`, update: `${title} is up to date.`, verify: f.message ?? `${title} verified.`, uninstall: `${title} was uninstalled.` }[f.kind];
    toast(done);
    if (f.kind === "update") {
      // Offer the release notes right after an update.
      await loadLibrary();
      const lg = S.library.find((g) => g.game.id === f.game_id);
      if (lg) void whatsNew(lg, { afterUpdate: true });
      return;
    }
  } else if (f.paused) {
    toast(f.kind === "verify" ? "Verification cancelled." : `${title} paused. Resume any time.`);
  } else {
    if (f.code === "signed_out") S.me = null;
    toast(`${title}: ${f.message ?? "failed"}`, "error");
  }
  if (S.me) await loadLibrary();
  else render();
});

onPrereqProgress((p) => {
  if (!S.req) return;
  S.req.progress.set(p.id, p);
  if (p.phase === "error") S.req.errors.set(p.id, p.message ?? "Install failed.");
  render();
});

onLauncherUpdateProgress((p) => {
  S.launcherUpdating = p;
  render();
});

async function boot() {
  S.booted = false;
  render();
  try {
    S.info = await api.appInfo();
  } catch (e) {
    S.offline = errorMessage(e);
  }
  try {
    S.settings = await api.getSettings();
  } catch {
    /* defaults on the Rust side */
  }
  try {
    S.catalog = (await api.library()).map((g) => g.game);
  } catch {
    S.catalog = [];
  }
  try {
    S.me = await api.restoreSession();
    S.offline = null;
  } catch (e) {
    S.offline = `Can't reach Zirtola right now (${errorMessage(e)}).`;
  }
  S.booted = true;
  await startupUpdateChecks();
  await refreshPrereqs(selectedGame()?.owned ? S.selected : null);
  if (!requirementsSeen()) openRequirements(selectedGame()?.owned ? S.selected : null);
  render();
  focusFirst();
  if (S.pending.size) schedulePurchasePoll();
  if (S.me) api.pendingCrashes().then((list) => list.forEach(queueCrash)).catch(() => undefined);
  const link = await api.startupDeepLink().catch(() => null);
  if (link) handleDeepLink(link);
}

// Every start: the launcher's own update and every owned game's update, in
// parallel, before the library is shown as current. Offline never blocks.
async function startupUpdateChecks() {
  S.checkingUpdates = true;
  render();
  const res = await runStartupChecks({
    currentVersion: S.info?.version ?? "0.0.0",
    minLauncherVersion: S.info?.min_launcher_version ?? null,
    autoUpdateLauncher: S.settings?.auto_update_launcher ?? false,
    autoUpdateGames: S.settings?.auto_update_games ?? false,
    signedIn: Boolean(S.me),
    checkLauncherUpdate: async () => {
      const u = await api.checkLauncherUpdate();
      S.launcherUpdate = u;
      return u;
    },
    installLauncherUpdate: async () => {
      S.launcherUpdating = { downloaded: 0, total: null };
      render();
      try {
        await api.installLauncherUpdate(); // restarts the launcher
      } catch (e) {
        S.launcherUpdating = null;
        throw e;
      }
    },
    refreshLibrary: async () => {
      const lib = await api.library();
      S.library = [...lib.filter((g) => g.owned), ...lib.filter((g) => !g.owned)];
      if (!S.selected || !S.library.some((g) => g.game.id === S.selected)) S.selected = S.library[0]?.game.id ?? null;
      settlePurchases();
      return lib.map((g) => ({ id: g.game.id, owned: g.owned, updateAvailable: Boolean(g.install?.update_available), busy: Boolean(g.job) }));
    },
    startGameUpdate: (id) => api.update(id),
  });
  S.checkingUpdates = false;
  if (res.gamesChecked) S.lastUpdateCheck = res.checkedAt;
  S.updateNote = res.note;
  if (res.autoUpdating.length) {
    const names = res.autoUpdating.map((id) => S.library.find((g) => g.game.id === id)?.game.title ?? id);
    toast(`Updating ${names.join(", ")}.`);
  }
  render();
  // Offer what was found, one window at a time: the launcher first.
  if (res.launcherPrompt) await launcherUpdatePrompt();
  for (const id of res.gamePrompts) {
    const lg = S.library.find((g) => g.game.id === id);
    if (lg) await gameUpdatePrompt(lg);
  }
}

// Keep ownership and update status fresh: when the window regains focus (at
// most once a minute, or immediately while a purchase is pending) and every 15
// minutes in the background.
function refreshSoon(minGapMs: number) {
  if (!S.me || Date.now() - lastRefresh < minGapMs) return;
  lastRefresh = Date.now();
  void loadLibrary();
}
window.addEventListener("focus", () => refreshSoon(S.pending.size ? 0 : 60_000));
setInterval(() => refreshSoon(0), 15 * 60_000);

// zirtola://refresh[?game=<id>] (the website's "Return to the launcher" link):
// refresh right away and select the game.
function handleDeepLink(raw: string) {
  try {
    const u = new URL(raw);
    if (u.protocol !== "zirtola:") return;
    const game = u.searchParams.get("game");
    if (game && /^[a-z0-9-]{1,64}$/.test(game)) {
      S.selected = game;
      S.view = "library";
    }
    void refreshNow();
  } catch {
    /* ignore malformed links */
  }
}
onDeepLink(handleDeepLink);

S.pending = loadPending();
boot();
