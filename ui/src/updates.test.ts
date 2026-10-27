// npm run test:ui
import { test } from "node:test";
import assert from "node:assert/strict";
import { PromptMemo, entriesBetween, gamePromptKind, requiredGameText, requiredLauncherText } from "./updates";

const e = (version: string, mandatory = false) => ({ version, released_at: "2026-10-01T00:00:00Z", notes: `Notes ${version}`, mandatory });

test("the prompt lists every release since the installed one, newest first", () => {
  const all = [e("0.4.0"), e("0.1.0"), e("0.3.0", true), e("0.2.0"), e("0.5.0-beta.1")];
  assert.deepEqual(entriesBetween(all, "0.1.0", "0.4.0").map((x) => x.version), ["0.4.0", "0.3.0", "0.2.0"]);
  assert.deepEqual(entriesBetween(all, "0.4.0", "0.4.0"), [], "nothing between equal versions");
  assert.deepEqual(entriesBetween(all, "0.2.10", "0.4.0").map((x) => x.version), ["0.4.0", "0.3.0"], "numeric, not text, comparison");
});

test("prompt kind: optional, required, or nothing to offer", () => {
  assert.equal(gamePromptKind(null), null);
  assert.equal(gamePromptKind({ version: "1.0.0", latest_version: "1.0.0", update_available: false }), null);
  assert.equal(gamePromptKind({ version: null, latest_version: "1.0.0", update_available: true }), null, "not installed: install, not update");
  assert.equal(gamePromptKind({ version: "1.0.0", latest_version: "1.2.0", update_available: true }), "optional");
  assert.equal(gamePromptKind({ version: "1.0.0", latest_version: "1.2.0", update_available: true, update_required: true, min_version: "1.1.0" }), "required");
});

test("required text names the reason and the version to reach", () => {
  const inst = { version: "1.0.0", latest_version: "1.2.0", update_available: true, update_required: true, min_version: "1.1.0", required_reason: "Security fix" };
  assert.equal(requiredGameText("Dirt Digger", inst), "Required update: Security fix. Update Dirt Digger to 1.1.0 or newer to play.");
  assert.equal(requiredGameText("Dirt Digger", { ...inst, required_reason: null }), "Required update. Update Dirt Digger to 1.1.0 or newer to play.");
  assert.match(requiredLauncherText("0.3.0", "Login fix", false), /^Required update: Login fix\./);
  assert.match(requiredLauncherText("0.3.0", null, true), /no longer supported/);
});

test("optional prompts show once per version per session; required ones every time", () => {
  const m = new PromptMemo();
  assert.equal(m.shouldShow("dirt", "1.2.0", false), true);
  assert.equal(m.shouldShow("dirt", "1.2.0", false), false, "Later means not again this session");
  assert.equal(m.shouldShow("dirt", "1.3.0", false), true, "a newer version is offered again");
  assert.equal(m.shouldShow("dirt", "1.3.0", true), true);
  assert.equal(m.shouldShow("dirt", "1.3.0", true), true);
});
