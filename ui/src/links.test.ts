// node --import tsx --test ui/src/links.test.ts   (npm run test:ui)
import { test } from "node:test";
import assert from "node:assert/strict";
import { buyUrl, openLink, storeUrl } from "./links";

function harness(fail?: string) {
  const opened: string[] = [];
  const toasts: string[] = [];
  return {
    opened,
    toasts,
    deps: {
      openUrl: async (u: string) => {
        if (fail) throw { code: "browser_suppressed", message: fail };
        opened.push(u);
      },
      toast: (t: string) => void toasts.push(t),
      describeError: (e: unknown) => (e as { message: string }).message,
    },
  };
}

test("clicking Store opens the configured store URL", async () => {
  const h = harness();
  const info = { api_base: "https://zirtola.com/api/launcher", store_url: "https://zirtola.com/games" };
  assert.equal(await openLink(storeUrl(info), "store", h.deps), true);
  assert.deepEqual(h.opened, ["https://zirtola.com/games"]);
  assert.deepEqual(h.toasts, []);
});

test("Store falls back to the website's /games when the config hasn't loaded", async () => {
  const h = harness();
  await openLink(storeUrl({ api_base: "http://localhost:3000/api/launcher", store_url: null }), "store", h.deps);
  assert.deepEqual(h.opened, ["http://localhost:3000/games"]);
});

test("Store never fails silently", async () => {
  const none = harness();
  assert.equal(await openLink(storeUrl(null), "store", none.deps), false);
  assert.equal(none.opened.length, 0);
  assert.match(none.toasts[0]!, /isn't reachable/);

  const suppressed = harness("Test mode: the browser is switched off (ZIRTOLA_NO_BROWSER).");
  assert.equal(await openLink("https://zirtola.com/games", "store", suppressed.deps), false);
  assert.match(suppressed.toasts[0]!, /browser is switched off/);
});

test("Buy goes to checkout when the website has one, else the store page", () => {
  assert.equal(buyUrl({ store_url: "https://zirtola.com/games/dirt-simulator", checkout_url: "https://zirtola.com/checkout/dirt-simulator" }), "https://zirtola.com/checkout/dirt-simulator");
  assert.equal(buyUrl({ store_url: "https://zirtola.com/games/dirt-simulator" }), "https://zirtola.com/games/dirt-simulator");
});
