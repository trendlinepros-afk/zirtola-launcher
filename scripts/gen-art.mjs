// Generates the NSIS installer bitmaps (24-bit BMP, which NSIS requires) from
// the same geometry as art/zirtola-icon.svg. No dependencies, no external art.
//   node scripts/gen-art.mjs
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "installer");
mkdirSync(outDir, { recursive: true });

const hex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const mix = (a, b, t) => a.map((v, i) => v + (b[i] - v) * t);

// The Z from the icon, in 1024-unit space.
const Z = [
  [272, 268], [752, 268], [752, 372], [452, 652], [752, 652],
  [752, 756], [272, 756], [272, 652], [572, 372], [272, 372],
];

function inside(poly, x, y) {
  let c = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i], [xj, yj] = poly[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c;
  }
  return c;
}

function canvas(w, h) {
  return { w, h, px: new Float32Array(w * h * 3) };
}

function fillBg(cv, top, bottom) {
  const a = hex(top), b = hex(bottom);
  for (let y = 0; y < cv.h; y++) {
    const c = mix(a, b, y / (cv.h - 1));
    for (let x = 0; x < cv.w; x++) cv.px.set(c, (y * cv.w + x) * 3);
  }
}

// Draw the Z scaled to `size` px with its top-left at (ox, oy); 4x4 supersampled.
function drawZ(cv, ox, oy, size) {
  const g0 = hex("#f6c766"), g1 = hex("#e3a33b"), g2 = hex("#b8741f");
  const s = size / 1024;
  for (let y = 0; y < cv.h; y++) {
    for (let x = 0; x < cv.w; x++) {
      let cover = 0;
      for (let sy = 0; sy < 4; sy++)
        for (let sx = 0; sx < 4; sx++) {
          const u = (x + (sx + 0.5) / 4 - ox) / s, v = (y + (sy + 0.5) / 4 - oy) / s;
          if (inside(Z, u, v)) cover++;
        }
      if (!cover) continue;
      const u = (x - ox) / s, v = (y - oy) / s;
      const t = Math.min(1, Math.max(0, (u + v - 540) / 950));
      const col = t < 0.55 ? mix(g0, g1, t / 0.55) : mix(g1, g2, (t - 0.55) / 0.45);
      const i = (y * cv.w + x) * 3;
      const a = cover / 16;
      for (let k = 0; k < 3; k++) cv.px[i + k] = cv.px[i + k] * (1 - a) + col[k] * a;
    }
  }
}

// Faint horizontal "bench" lines, like terraced quarry walls.
function benches(cv, y0, step, color, alpha) {
  const c = hex(color);
  for (let y = y0; y < cv.h; y += step) {
    for (let x = 0; x < cv.w; x++) {
      const i = (y * cv.w + x) * 3;
      for (let k = 0; k < 3; k++) cv.px[i + k] = cv.px[i + k] * (1 - alpha) + c[k] * alpha;
    }
  }
}

function bmp(cv) {
  const row = Math.ceil((cv.w * 3) / 4) * 4;
  const size = 54 + row * cv.h;
  const b = Buffer.alloc(size);
  b.write("BM", 0);
  b.writeUInt32LE(size, 2);
  b.writeUInt32LE(54, 10);
  b.writeUInt32LE(40, 14);
  b.writeInt32LE(cv.w, 18);
  b.writeInt32LE(cv.h, 22);
  b.writeUInt16LE(1, 26);
  b.writeUInt16LE(24, 28);
  b.writeUInt32LE(row * cv.h, 34);
  b.writeInt32LE(2835, 38);
  b.writeInt32LE(2835, 42);
  for (let y = 0; y < cv.h; y++) {
    const dst = 54 + (cv.h - 1 - y) * row;
    for (let x = 0; x < cv.w; x++) {
      const i = (y * cv.w + x) * 3;
      b[dst + x * 3] = Math.round(cv.px[i + 2]);
      b[dst + x * 3 + 1] = Math.round(cv.px[i + 1]);
      b[dst + x * 3 + 2] = Math.round(cv.px[i]);
    }
  }
  return b;
}

// Sidebar (welcome/finish pages): 164x314.
const side = canvas(164, 314);
fillBg(side, "#232a33", "#0d1014");
benches(side, 196, 18, "#e3a33b", 0.06);
drawZ(side, -18, 10, 200);
writeFileSync(join(outDir, "sidebar.bmp"), bmp(side));

// Header (inner pages): 150x57, light so NSIS's black header text stays readable.
const head = canvas(150, 57);
fillBg(head, "#ffffff", "#f1f3f6");
drawZ(head, 88, -11, 80);
writeFileSync(join(outDir, "header.bmp"), bmp(head));

console.log("wrote", outDir);
