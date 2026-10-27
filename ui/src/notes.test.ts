// npm run test:ui
import { test } from "node:test";
import assert from "node:assert/strict";
import { parseNotes } from "./notes";

test("wrapped bullet lines join their bullet", () => {
  const md = `## 0.2.0: Gold

- **Gold in the ground.** Placer gold now sits where the geology puts it, in the
  gravels above bedrock. It travels with the dirt.
- **Pickup truck.** A 4x4 crew-cab pickup is parked in camp.
- Lazy continuation without indent
works too.
`;
  assert.deepEqual(parseNotes(md), [
    { kind: "h", text: "0.2.0: Gold" },
    {
      kind: "ul",
      items: [
        "**Gold in the ground.** Placer gold now sits where the geology puts it, in the gravels above bedrock. It travels with the dirt.",
        "**Pickup truck.** A 4x4 crew-cab pickup is parked in camp.",
        "Lazy continuation without indent works too.",
      ],
    },
  ]);
});

test("paragraphs wrap and blank lines separate blocks", () => {
  assert.deepEqual(parseNotes("First line\nsecond line.\n\nNew paragraph.\r\n- a\n\n- b"), [
    { kind: "p", text: "First line second line." },
    { kind: "p", text: "New paragraph." },
    { kind: "ul", items: ["a"] },
    { kind: "ul", items: ["b"] },
  ]);
});

test("section headings without '#' start their own section, even right after a bullet", () => {
  const md = "New\n- Wash plant chain lift and trailer straps.\n- Gold buyer in town.\nChanged\n- Save files move to format 4 (old saves upgrade on first load).\n\n**Fixes:**\n- The dozer blade no longer sticks.\nKnown issues\n- Rain looks dark at dawn.";
  assert.deepEqual(parseNotes(md), [
    { kind: "h", text: "New" },
    { kind: "ul", items: ["Wash plant chain lift and trailer straps.", "Gold buyer in town."] },
    { kind: "h", text: "Changed" },
    { kind: "ul", items: ["Save files move to format 4 (old saves upgrade on first load)."] },
    { kind: "h", text: "Fixes" },
    { kind: "ul", items: ["The dozer blade no longer sticks."] },
    { kind: "h", text: "Known issues" },
    { kind: "ul", items: ["Rain looks dark at dawn."] },
  ]);
  // A wrapped line that merely starts with such a word still joins its bullet.
  assert.deepEqual(parseNotes("- The buyer pays more for\nnew gold."), [{ kind: "ul", items: ["The buyer pays more for new gold."] }]);
});
