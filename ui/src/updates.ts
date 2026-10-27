// Update prompts: what an "Update now / Later" window shows and when.
// Kept free of DOM and Tauri imports so it can be unit-tested (updates.test.ts).
//
// - The prompt lists every release between the installed version and the
//   newest one (newest first), so skipping three updates shows all three.
// - A required (mandatory) update has no "Later": the window says why, and
//   Play stays disabled until the game is updated. An install older than the
//   newest required release must update even when the newest release itself
//   is optional (the website works this out and sends min_version).
// - An optional update is offered once per version per session; "Later" means
//   "not this session", and the update button stays in the library.

import { compareVersions } from "./startup";

export interface PromptEntry {
  version: string;
  released_at: string;
  notes: string;
  channel?: string;
  mandatory?: boolean;
  mandatory_reason?: string | null;
}

/** Releases newer than `installed` and not newer than `latest`, newest first. */
export function entriesBetween<T extends PromptEntry>(entries: T[], installed: string | null, latest: string | null): T[] {
  return entries
    .filter((e) => (!installed || compareVersions(e.version, installed) > 0) && (!latest || compareVersions(e.version, latest) <= 0))
    .sort((a, b) => compareVersions(b.version, a.version));
}

export interface InstallState {
  version: string | null;
  latest_version: string | null;
  update_available: boolean;
  update_required?: boolean;
  min_version?: string | null;
  required_reason?: string | null;
}

export type PromptKind = "required" | "optional" | null;

export function gamePromptKind(inst: InstallState | null | undefined): PromptKind {
  if (!inst?.version || !inst.update_available) return null;
  return inst.update_required ? "required" : "optional";
}

/** Tooltip and banner text for a required game update. */
export function requiredGameText(title: string, inst: InstallState): string {
  const why = inst.required_reason ? `Required update: ${inst.required_reason}.` : "Required update.";
  const to = inst.min_version ?? inst.latest_version;
  return `${why} Update ${title}${to ? ` to ${to} or newer` : ""} to play.`;
}

/** Tooltip and banner text for a required launcher update. */
export function requiredLauncherText(version: string, reason: string | null | undefined, belowMinimum: boolean): string {
  if (reason) return `Required update: ${reason}. Update the launcher to ${version} to keep playing.`;
  if (belowMinimum) return `This launcher version is no longer supported. Update to ${version} to keep playing online.`;
  return `Required update. Update the launcher to ${version} to keep playing.`;
}

/** Remembers which optional prompts were shown this session. Required ones always show. */
export class PromptMemo {
  private seen = new Set<string>();
  shouldShow(key: string, version: string, required: boolean): boolean {
    if (required) return true;
    const k = `${key}@${version}`;
    if (this.seen.has(k)) return false;
    this.seen.add(k);
    return true;
  }
}
