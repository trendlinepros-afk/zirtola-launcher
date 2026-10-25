import "./styles.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  api,
  errorCode,
  errorMessage,
  onJobFinished,
  onJobProgress,
  onLauncherUpdateProgress,
  type AppInfo,
  type CatalogGame,
  type JobProgress,
  type LibraryGame,
  type Me,
  type Settings,
} from "./api";
import { icons, keyArt, logoMark } from "./art";
import * as fmt from "./format";
import { focusFirst, onGamepadActive, onNavAction } from "./nav";

// ------------------------------------------------------------------ state

type View = "library" | "settings";

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
  launcherUpdate: null as { version: string; notes: string | null } | null,
  launcherUpdating: null as { downloaded: number; total: number | null } | null,
  gamepad: false,
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
  return h("div", { class: "shell" }, sidebar(), h("main", { class: "content" }, S.view === "settings" ? settingsView() : libraryView()));
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
        ? h("p", { class: "muted small" }, "No account yet? ", h("a", { href: "#", "data-nav": true, "data-id": "create-account", onclick: (e: Event) => { e.preventDefault(); api.openUrl(S.info!.account_url!); } }, "Create one on the website"), ".")
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
      { class: "nav-item", "data-id": "nav-store", type: "button", onclick: () => S.info?.store_url && api.openUrl(S.info.store_url) },
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
          { class: "update-card", "data-id": "launcher-update", type: "button", onclick: installLauncherUpdate, disabled: !!S.launcherUpdating },
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
                : "Restart to update",
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
  return h("div", { class: "library" }, hero(g), shelf());
}

function hero(lg: LibraryGame) {
  const g = lg.game;
  const p = S.progress.get(g.id);
  const inst = lg.install;
  const meta: string[] = [];
  if (inst?.version) meta.push(`Installed ${inst.version}`);
  if (inst?.update_available && inst.latest_version) meta.push(`Update ${inst.latest_version} available`);
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
      h("span", { class: `pill ${g.status}` }, fmt.statusLabel(g.status)),
      h("h1", {}, g.title),
      h("p", { class: "tagline" }, g.tagline),
      h("p", { class: "desc" }, g.description),
      meta.length ? h("p", { class: "meta" }, meta.join("  ·  ")) : null,
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

function actions(lg: LibraryGame) {
  const g = lg.game;
  const inst = lg.install;
  const row = h("div", { class: "actions" });
  if (!lg.owned) {
    row.append(
      btn("store", "Get it on the store", () => api.openUrl(g.store_url), { cls: "primary big", icon: icons.external }),
      btn("redeem", "Redeem a key", () => redeemDialog(), { cls: "ghost big", icon: icons.key }),
    );
    return row;
  }
  if (!inst) {
    row.append(btn("install", "Install", () => installDialog(lg), { cls: "primary big", icon: icons.download }));
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
    row.append(btn("play", "Play", () => launch(g), { cls: "ghost big", icon: icons.play }));
  } else {
    row.append(btn("play", "Play", () => launch(g), { cls: "primary big play", icon: icons.play }));
  }
  row.append(
    btn("verify", "", () => run(() => api.verify(g.id)), { cls: "ghost square", icon: icons.check, title: "Verify and repair files" }),
    btn("folder", "", () => api.openFolder(g.id), { cls: "ghost square", icon: icons.folder, title: "Open install folder" }),
    btn("uninstall", "", () => confirmUninstall(lg), { cls: "ghost square", icon: icons.trash, title: "Uninstall" }),
  );
  return row;
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
    h("h2", {}, "Your games"),
    h(
      "div",
      { class: "shelf-row" },
      ...S.library.map((lg) => {
        const p = S.progress.get(lg.game.id);
        const badge = lg.job
          ? lg.job === "verify" ? "Verifying" : lg.job === "uninstall" ? "Uninstalling" : "Downloading"
          : !lg.owned
            ? "Not owned"
            : lg.install?.update_available
              ? "Update"
              : lg.install?.version
                ? "Installed"
                : lg.install
                  ? "Paused"
                  : "Ready to install";
        return h(
          "button",
          {
            class: `game-card ${lg.game.id === selectedGame()?.game.id ? "selected" : ""} ${lg.owned ? "" : "unowned"}`,
            "data-id": `card-${lg.game.id}`,
            type: "button",
            style: `--accent:${lg.game.art.accent}`,
            onclick: () => { S.selected = lg.game.id; render(); },
          },
          h("div", { class: "art", html: keyArt(lg.game.id, lg.game.art.accent, lg.game.title) }),
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
  try {
    await api.launch(g.id);
    toast(`Starting ${g.title}…`);
  } catch (e) {
    handleError(e);
  }
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
  } catch (e) {
    handleError(e);
  }
  render();
}

// ------------------------------------------------------------------ modals

function modal(title: string, content: HTMLElement, opts: { onClose?: () => void } = {}) {
  const close = () => {
    backdrop.remove();
    opts.onClose?.();
    render();
    focusFirst();
  };
  const backdrop = h(
    "div",
    { class: "modal-backdrop", onclick: (e: Event) => { if (e.target === backdrop) close(); } },
    h("div", { class: "modal", role: "dialog", "aria-modal": "true" }, h("h2", {}, title), content),
  );
  (backdrop as HTMLElement & { close?: () => void }).close = close;
  document.body.append(backdrop);
  setTimeout(() => focusFirst(), 0);
  return close;
}

function closeTopModal(): boolean {
  const all = document.querySelectorAll<HTMLElement & { close?: () => void }>(".modal-backdrop");
  const top = all[all.length - 1];
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

function confirmUninstall(lg: LibraryGame) {
  const close = modal(
    `Uninstall ${lg.game.title}?`,
    h(
      "div",
      {},
      h("p", { class: "muted" }, `This deletes the game files in ${lg.install?.path}. Saves and settings stored in your user folder are not touched. You can install again at any time.`),
      h(
        "div",
        { class: "modal-actions" },
        btn("cancel", "Keep it", () => close(), { cls: "ghost" }),
        btn("confirm-uninstall", "Uninstall", async () => { close(); await run(() => api.uninstall(lg.game.id)); }, { cls: "danger" }),
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
      h("label", { class: "setting" }, h("div", {}, h("strong", {}, "Update the launcher automatically"), h("small", {}, "Checks for a new launcher version when it starts.")), auto),
      h(
        "div",
        { class: "setting" },
        h("div", {}, h("strong", {}, `Zirtola Launcher ${S.info?.version ?? ""}`), h("small", {}, S.info?.dev_mode ? `Development build · API ${S.info.api_base}` : "Release build")),
        btn("check-update", "Check for updates", checkLauncherUpdate, { cls: "ghost", icon: icons.update }),
      ),
      S.info?.support_url ? h("div", { class: "setting" }, h("div", {}, h("strong", {}, "Help"), h("small", {}, "Support and known issues")), btn("support", "Open support", () => api.openUrl(S.info!.support_url!), { cls: "ghost", icon: icons.external })) : null,
    ),
  );
}

async function checkLauncherUpdate(manual = true) {
  try {
    const u = await api.checkLauncherUpdate();
    S.launcherUpdate = u ? { version: u.version, notes: u.notes } : null;
    if (manual) toast(u ? `Launcher ${u.version} is available.` : "The launcher is up to date.");
    render();
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
  } else if (f.paused) {
    toast(f.kind === "verify" ? "Verification cancelled." : `${title} paused. Resume any time.`);
  } else {
    if (f.code === "signed_out") S.me = null;
    toast(`${title}: ${f.message ?? "failed"}`, "error");
  }
  if (S.me) await loadLibrary();
  else render();
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
  if (S.me) await loadLibrary();
  else render();
  focusFirst();
  if (S.settings?.auto_update_launcher) checkLauncherUpdate(false);
}

// Keep ownership and update status fresh: when the window regains focus (at
// most once a minute) and every 15 minutes in the background.
let lastRefresh = Date.now();
function refreshSoon(minGapMs: number) {
  if (!S.me || Date.now() - lastRefresh < minGapMs) return;
  lastRefresh = Date.now();
  void loadLibrary();
}
window.addEventListener("focus", () => refreshSoon(60_000));
setInterval(() => refreshSoon(0), 15 * 60_000);

boot();
