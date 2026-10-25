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

export interface LibraryGame {
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
}

export interface Settings {
  library_dir: string;
  parallel_downloads: number;
  auto_update_launcher: boolean;
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
  pause: (gameId: string) => invoke<void>("pause_job", { gameId }),
  forget: (gameId: string) => invoke<void>("forget_install", { gameId }),
  launch: (gameId: string) => invoke<void>("launch_game", { gameId }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  openFolder: (gameId: string) => invoke<void>("open_install_folder", { gameId }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  checkLauncherUpdate: () =>
    invoke<{ version: string; notes: string | null; date: string | null } | null>("check_launcher_update"),
  installLauncherUpdate: () => invoke<void>("install_launcher_update"),
};

export function onJobProgress(cb: (p: JobProgress) => void): Promise<UnlistenFn> {
  return listen<JobProgress>("job-progress", (e) => cb(e.payload));
}

export function onJobFinished(cb: (p: JobFinished) => void): Promise<UnlistenFn> {
  return listen<JobFinished>("job-finished", (e) => cb(e.payload));
}

export function onLauncherUpdateProgress(cb: (p: { downloaded: number; total: number | null }) => void) {
  return listen<{ downloaded: number; total: number | null }>("launcher-update-progress", (e) => cb(e.payload));
}
