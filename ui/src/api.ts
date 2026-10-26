// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Me {
  id: string;
  display_name: string;
  avatar_url?: string | null;
}

export interface CatalogGame {
  id: string;
  title: string;
  tagline: string;
  description: string;
  status: string;
  store_url: string;
  platforms: string[];
  latest_version?: string | null;
  install_size?: number | null;
  art: { accent: string; cover_url?: string | null };
  /** Price in minor units (cents) if the website sells it; absent = not for sale / not set. */
  price?: { amount: number; currency: string } | null;
  /** false = shown with a price but not on sale yet ("Coming soon"). */
  sales_open?: boolean | null;
  /** Website checkout page for this game, when it sells it directly. */
  checkout_url?: string | null;
  /** Still in early access: show "Report a bug". */
  early_access?: boolean | null;
  /** The game's bug-report forum on the website. */
  bug_report_url?: string | null;
  /** The website's system requirements section, with a one-line minimum. */
  requirements?: { url: string; minimum_target?: string | null; minimum_summary?: string | null } | null;
  /** Where the game keeps saves and settings, e.g. %LOCALAPPDATA%\GranitePit\Saved (never in the install folder). */
  save_location?: string | null;
}

export type JobKind = "install" | "update" | "verify" | "uninstall";

export interface InstallInfo {
  path: string;
  version: string | null;
  latest_version: string | null;
  update_available: boolean;
  paused_version: string | null;
  folder_missing: boolean;
}

export interface ChangelogEntry {
  version: string;
  channel: string;
  released_at: string;
  notes: string;
  current: boolean;
}

export interface ActiveDevice {
  id: string;
  name: string;
  activated_at: string;
  last_seen_at: string;
}

export interface LicenseInfo {
  game_id: string;
  game_title: string;
  source: "purchase" | "key" | "grant" | "early_access";
  granted_at: string;
  order_number: string | null;
  key: string | null;
  key_masked: string | null;
  activations: { id: string; name: string; activated_at: string; last_seen_at: string; this_device: boolean }[];
}

export interface LibraryGame {
  /** Channels the licence allows: "live" (stable) and maybe "beta". */
  branches: string[];
  /** Channel this install follows. */
  branch: string;
  game: CatalogGame;
  owned: boolean;
  entitlement: string | null;
  install: InstallInfo | null;
  job: JobKind | null;
}

export interface AppInfo {
  version: string;
  api_base: string;
  dev_mode: boolean;
  store_url: string | null;
  account_url: string | null;
  support_url: string | null;
  /** Oldest launcher the website supports; below it the update is required. */
  min_launcher_version?: string | null;
}

export interface Settings {
  library_dir: string;
  parallel_downloads: number;
  auto_update_launcher: boolean;
  auto_update_games: boolean;
  share_hardware_info: boolean;
  offer_crash_reports: boolean;
}

export interface JobProgress {
  game_id: string;
  kind: JobKind;
  phase: "preparing" | "downloading" | "verifying" | "committing" | "uninstalling" | "done";
  total_bytes: number;
  done_bytes: number;
  network_bytes: number;
  reused_bytes: number;
  download_bytes: number;
  speed_bps: number;
  eta_s: number | null;
}

export interface JobFinished {
  game_id: string;
  kind: JobKind;
  ok: boolean;
  paused: boolean;
  code: string | null;
  message: string | null;
}

export interface CmdError {
  code: string;
  message: string;
  details?: { active_devices?: ActiveDevice[]; [k: string]: unknown };
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as CmdError).message);
  return String(e);
}

export function errorCode(e: unknown): string | null {
  if (e && typeof e === "object" && "code" in e) return String((e as CmdError).code);
  return null;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  restoreSession: () => invoke<Me | null>("restore_session"),
  signIn: () => invoke<Me>("sign_in"),
  cancelSignIn: () => invoke<void>("cancel_sign_in"),
  signOut: () => invoke<void>("sign_out"),
  redeemKey: (key: string) =>
    invoke<{ entitlement: { game_id: string }; game_title: string }>("redeem_key", { key }),
  library: () => invoke<LibraryGame[]>("library"),
  suggestInstallDir: (title: string, parent?: string) =>
    invoke<string>("suggest_install_dir", { title, parent: parent ?? null }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  diskInfo: (path: string) => invoke<{ free_bytes: number }>("disk_info", { path }),
  install: (gameId: string, path: string) => invoke<void>("install_game", { gameId, path }),
  update: (gameId: string) => invoke<void>("update_game", { gameId }),
  verify: (gameId: string) => invoke<void>("verify_game", { gameId }),
  uninstall: (gameId: string) => invoke<void>("uninstall_game", { gameId }),
  deleteGameSaves: (gameId: string) => invoke<string>("delete_game_saves", { gameId }),
  resolveSaveLocation: (raw: string) => invoke<string>("resolve_save_location", { raw }),
  pause: (gameId: string) => invoke<void>("pause_job", { gameId }),
  forget: (gameId: string) => invoke<void>("forget_install", { gameId }),
  launch: (gameId: string, title?: string) => invoke<void>("launch_game", { gameId, title: title ?? null }),
  deactivateDevice: (activationId: string) => invoke<void>("deactivate_device", { activationId }),
  changelog: (gameId: string) => invoke<{ game_id: string; entries: ChangelogEntry[] }>("game_changelog", { gameId }),
  setBranch: (gameId: string, branch: "live" | "beta") => invoke<void>("set_branch", { gameId, branch }),
  license: (gameId: string) => invoke<LicenseInfo>("game_license", { gameId }),
  hardwareReport: () => invoke<Record<string, unknown>>("hardware_report"),
  pendingCrashes: () => invoke<CrashCandidate[]>("pending_crashes"),
  sendCrashReport: (id: string, comment: string) => invoke<string>("send_crash_report", { id, comment }),
  dismissCrash: (id: string) => invoke<void>("dismiss_crash", { id }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  openFolder: (gameId: string) => invoke<void>("open_install_folder", { gameId }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  checkLauncherUpdate: () =>
    invoke<{ version: string; notes: string | null; date: string | null } | null>("check_launcher_update"),
  installLauncherUpdate: () => invoke<void>("install_launcher_update"),
  startupDeepLink: () => invoke<string | null>("startup_deep_link"),
};

export function onJobProgress(cb: (p: JobProgress) => void): Promise<UnlistenFn> {
  return listen<JobProgress>("job-progress", (e) => cb(e.payload));
}

export function onJobFinished(cb: (p: JobFinished) => void): Promise<UnlistenFn> {
  return listen<JobFinished>("job-finished", (e) => cb(e.payload));
}

export interface CrashCandidate {
  id: string;
  game_id: string;
  game_title: string;
  game_version: string;
  build_id: string;
  exit_code: number | null;
  error: string;
  files: { name: string; size: number }[];
  detected_at: string;
}

export function onGameCrashed(cb: (c: CrashCandidate) => void): Promise<UnlistenFn> {
  return listen<CrashCandidate>("game-crashed", (e) => cb(e.payload));
}

export function onDeepLink(cb: (url: string) => void): Promise<UnlistenFn> {
  return listen<string>("deep-link", (e) => cb(e.payload));
}

export function onLauncherUpdateProgress(cb: (p: { downloaded: number; total: number | null }) => void) {
  return listen<{ downloaded: number; total: number | null }>("launcher-update-progress", (e) => cb(e.payload));
}
