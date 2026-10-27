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
