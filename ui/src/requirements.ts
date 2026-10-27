// Requirements screen logic, kept pure for tests (requirements.test.ts).
// The checks themselves run in Rust (src-tauri/src/prereqs.rs).

export interface PrereqItem {
  id: string;
  name: string;
  description: string;
  /** ok = installed/met; missing = can be installed here; unsupported = advice only */
  status: "ok" | "missing" | "unsupported";
  found: string;
  required: boolean;
  fixable: boolean;
  size_bytes?: number | null;
  source?: string | null;
  advice?: string | null;
  advice_url?: string | null;
  manual_url?: string | null;
}

/** Items that stop a game from launching. */
export function blocking(items: PrereqItem[] | null | undefined): PrereqItem[] {
  return (items ?? []).filter((i) => i.required && i.status !== "ok");
}

/** Items the launcher can install (after the player says yes). */
export function installable(items: PrereqItem[] | null | undefined): PrereqItem[] {
  return (items ?? []).filter((i) => i.status === "missing" && i.fixable);
}

export function sizeLabel(bytes: number | null | undefined): string {
  if (!bytes) return "";
  const mb = bytes / (1024 * 1024);
  return mb >= 1024 ? `about ${(mb / 1024).toFixed(1)} GB` : `about ${Math.max(1, Math.round(mb))} MB`;
}

/** "Microsoft Visual C++ Runtime: about 18 MB, included with the game" */
export function installLine(i: PrereqItem): string {
  const parts = [sizeLabel(i.size_bytes), i.source ?? ""].filter(Boolean);
  return parts.length ? `${i.name}: ${parts.join(", ")}` : i.name;
}

/** Tooltip on a disabled Play button. */
export function playBlockedReason(items: PrereqItem[] | null | undefined): string | null {
  const b = blocking(items);
  if (!b.length) return null;
  return `Can't play yet: ${b.map((i) => i.name).join("; ")}. Open Requirements to fix it.`;
}
