// npm run test:ui
import { test } from "node:test";
import assert from "node:assert/strict";
import { blocking, installable, installLine, playBlockedReason, sizeLabel, type PrereqItem } from "./requirements";

const item = (id: string, o: Partial<PrereqItem> = {}): PrereqItem => ({
  id,
  name: id,
  description: "",
  status: "ok",
  found: "",
  required: true,
  fixable: false,
  ...o,
});

test("Play is blocked by any required red item, with a reason", () => {
  const items = [item("windows"), item("vcredist", { status: "missing", fixable: true, name: "Microsoft Visual C++ Runtime" }), item("gpu", { status: "unsupported", name: "Graphics card" })];
  assert.deepEqual(blocking(items).map((i) => i.id), ["vcredist", "gpu"]);
  assert.equal(playBlockedReason(items), "Can't play yet: Microsoft Visual C++ Runtime; Graphics card. Open Requirements to fix it.");
  assert.equal(playBlockedReason([item("windows")]), null);
  assert.equal(playBlockedReason(null), null, "not checked yet: don't block");
});

test("only fixable missing items are offered for install", () => {
  const items = [item("vcredist", { status: "missing", fixable: true }), item("gpu", { status: "unsupported" }), item("disk")];
  assert.deepEqual(installable(items).map((i) => i.id), ["vcredist"]);
});

test("the permission list says what and roughly how big", () => {
  assert.equal(sizeLabel(18_569_648), "about 18 MB");
  assert.equal(sizeLabel(25 * 1024 * 1024), "about 25 MB");
  assert.equal(installLine(item("vcredist", { name: "Microsoft Visual C++ Runtime", size_bytes: 18_569_648, source: "included with the game" })), "Microsoft Visual C++ Runtime: about 18 MB, included with the game");
});
