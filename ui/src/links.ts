// Website links the launcher opens. Kept free of DOM/Tauri imports so they can
// be unit-tested (links.test.ts). Rule: a click never fails silently; if there
// is no URL or the browser can't be opened, the player gets a toast saying why.

export interface SiteInfo {
  api_base?: string | null;
  store_url?: string | null;
}

export interface GameLinks {
  store_url: string;
  checkout_url?: string | null;
}

/** https://zirtola.com for an API base of https://zirtola.com/api/launcher. */
export function siteOrigin(info: SiteInfo | null | undefined): string | null {
  try {
    return info?.api_base ? new URL(info.api_base).origin : null;
  } catch {
    return null;
  }
}

/** The sidebar Store link: the configured store page, else the website's /games. */
export function storeUrl(info: SiteInfo | null | undefined): string | null {
  if (info?.store_url) return info.store_url;
  const origin = siteOrigin(info);
  return origin ? `${origin}/games` : null;
}

/** Where "Buy" goes: the checkout page when the website has one, else the game's store page. */
export function buyUrl(game: GameLinks): string {
  return game.checkout_url || game.store_url;
}

export type Toast = (text: string, kind?: "info" | "error") => void;

export async function openLink(
  url: string | null | undefined,
  what: string,
  deps: { openUrl: (url: string) => Promise<unknown>; toast: Toast; describeError: (e: unknown) => string },
): Promise<boolean> {
  if (!url) {
    deps.toast(`Can't open the ${what}: the Zirtola website isn't reachable right now. Check your connection and try again.`, "error");
    return false;
  }
  try {
    await deps.openUrl(url);
    return true;
  } catch (e) {
    deps.toast(deps.describeError(e), "error");
    return false;
  }
}
