// Key art for each game, hand-authored as SVG (no downloaded or generated-by-
// third-party images). Unknown games get a procedural scene in their accent colour.

const dirtDigger = `
<svg viewBox="0 0 1600 900" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="xMidYMid slice">
  <defs>
    <linearGradient id="dd-sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#1d1512"/>
      <stop offset="0.45" stop-color="#6e3a1d"/>
      <stop offset="0.72" stop-color="#d98a3a"/>
      <stop offset="1" stop-color="#f6c070"/>
    </linearGradient>
    <radialGradient id="dd-sun" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0" stop-color="#fff0c4"/>
      <stop offset="0.35" stop-color="#ffd27e" stop-opacity="0.9"/>
      <stop offset="1" stop-color="#ffb347" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="dd-water" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#f6c070" stop-opacity="0"/>
      <stop offset="0.5" stop-color="#ffd890" stop-opacity="0.85"/>
      <stop offset="1" stop-color="#f6c070" stop-opacity="0"/>
    </linearGradient>
  </defs>
  <rect width="1600" height="900" fill="url(#dd-sky)"/>
  <circle cx="1170" cy="560" r="260" fill="url(#dd-sun)"/>
  <circle cx="1170" cy="560" r="70" fill="#fff3d1"/>
  <path d="M0 560 L140 500 L260 530 L420 450 L560 520 L700 470 L860 540 L1010 480 L1160 545 L1320 470 L1460 520 L1600 480 V900 H0 Z" fill="#5a3426" opacity="0.75"/>
  <path d="M0 620 L180 570 L330 600 L520 560 L690 610 L880 575 L1060 625 L1250 585 L1420 615 L1600 590 V900 H0 Z" fill="#3a231b"/>
  <!-- the pit: benches cut into the ground -->
  <path d="M0 700 H1600 V900 H0 Z" fill="#1d1410"/>
  <path d="M0 700 L1600 700 L1600 712 L0 712 Z" fill="#4a3022"/>
  <path d="M120 760 H1480 L1460 772 H140 Z" fill="#35241a"/>
  <path d="M260 820 H1340 L1320 832 H280 Z" fill="#2c1e16"/>
  <!-- creek catching the light -->
  <path d="M0 690 C300 676 520 700 800 686 C1080 672 1300 694 1600 682 L1600 694 C1300 706 1080 684 800 698 C520 712 300 688 0 702 Z" fill="url(#dd-water)"/>
  <!-- excavator silhouette -->
  <g fill="#100b08">
    <rect x="300" y="700" width="420" height="58" rx="29"/>
    <rect x="330" y="684" width="360" height="22" rx="6"/>
    <path d="M350 684 L350 610 L640 610 L660 640 L660 684 Z"/>
    <path d="M330 640 L360 600 L420 600 L420 684 L330 684 Z"/>
    <path d="M560 610 L560 535 L640 535 L652 610 Z"/>
    <path d="M600 620 L622 588 L842 402 L874 424 L668 640 Z"/>
    <path d="M842 402 L874 424 L946 628 L918 640 Z"/>
    <path d="M914 626 L978 618 L1004 668 L966 700 L918 690 Z"/>
    <rect x="752" y="470" width="14" height="96" rx="7" transform="rotate(-40 759 518)"/>
  </g>
  <g fill="#6b4a2c" opacity="0.6">
    <rect x="562" y="545" width="66" height="34" rx="3" fill="#f6c070" opacity="0.18"/>
  </g>
  <!-- gold in the bucket spill -->
  <g fill="#ffd36e">
    <circle cx="960" cy="712" r="3.5"/><circle cx="982" cy="726" r="2.5"/><circle cx="944" cy="734" r="2"/>
    <circle cx="1004" cy="744" r="3"/><circle cx="970" cy="752" r="2"/>
  </g>
  <path d="M900 700 C930 720 1010 730 1040 760 L880 760 Z" fill="#2a1c13"/>
</svg>`;

const bushPilot = `
<svg viewBox="0 0 1600 900" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="xMidYMid slice">
  <defs>
    <linearGradient id="bp-sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#0b1c2a"/>
      <stop offset="0.5" stop-color="#2b6a8a"/>
      <stop offset="0.8" stop-color="#9fd0e0"/>
      <stop offset="1" stop-color="#d9eef3"/>
    </linearGradient>
    <linearGradient id="bp-lake" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#9fd0e0"/>
      <stop offset="1" stop-color="#2b5f78"/>
    </linearGradient>
  </defs>
  <rect width="1600" height="900" fill="url(#bp-sky)"/>
  <!-- far range -->
  <path d="M0 600 L120 520 L210 560 L340 410 L460 520 L560 470 L700 330 L850 500 L960 440 L1090 300 L1230 470 L1340 420 L1480 510 L1600 450 V900 H0 Z" fill="#46708a"/>
  <path d="M340 410 L380 450 L360 458 L330 444 L300 452 Z M700 330 L750 390 L724 396 L700 382 L668 394 L650 386 Z M1090 300 L1140 360 L1112 368 L1090 352 L1060 366 L1044 356 Z" fill="#eef7fa"/>
  <!-- near range -->
  <path d="M0 660 L160 590 L300 640 L470 560 L620 640 L800 580 L980 650 L1150 590 L1320 650 L1480 600 L1600 640 V900 H0 Z" fill="#203f50"/>
  <path d="M470 560 L520 600 L496 606 L470 594 L440 606 L426 598 Z M1150 590 L1196 626 L1172 632 L1150 620 L1124 632 L1110 624 Z" fill="#dcebf0"/>
  <!-- lake -->
  <path d="M0 700 H1600 V770 H0 Z" fill="url(#bp-lake)"/>
  <path d="M220 730 H520 M760 744 H1120 M1260 722 H1480" stroke="#e6f4f8" stroke-width="3" opacity="0.5"/>
  <!-- spruce shoreline -->
  <g fill="#0b1915">
    <path d="M0 900 V760 H1600 V900 Z"/>
    ${Array.from({ length: 34 }, (_, i) => {
      const x = i * 50 + ((i * 37) % 23);
      const h = 70 + ((i * 53) % 60);
      const b = 770;
      return `<path d="M${x} ${b - h} L${x + 18} ${b - h * 0.55} L${x + 9} ${b - h * 0.55} L${x + 26} ${b - h * 0.2} L${x + 12} ${b - h * 0.2} L${x + 32} ${b} L${x - 32} ${b} L${x - 12} ${b - h * 0.2} L${x - 26} ${b - h * 0.2} L${x - 9} ${b - h * 0.55} L${x - 18} ${b - h * 0.55} Z"/>`;
    }).join("")}
  </g>
  <!-- high-wing taildragger -->
  <g fill="#08131b" transform="translate(980 250) rotate(-6)">
    <rect x="-150" y="-30" width="330" height="16" rx="8"/>
    <path d="M-80 -14 C-40 -22 60 -20 110 -8 L180 0 L182 8 L110 14 C60 24 -40 26 -84 18 Z"/>
    <path d="M-88 -10 L-104 -10 L-108 22 L-90 20 Z"/>
    <path d="M150 -2 L196 -40 L210 -40 L196 6 Z"/>
    <path d="M150 6 L214 10 L214 16 L150 14 Z"/>
    <path d="M-20 -14 L40 -30 L44 -26 L-14 -10 Z"/>
    <path d="M-30 20 L-44 52 L-38 54 L-22 22 Z"/>
    <circle cx="-42" cy="58" r="10"/>
    <circle cx="196" cy="20" r="5"/>
    <rect x="-112" y="-44" width="5" height="72" rx="2"/>
  </g>
</svg>`;

function fallback(accent: string, title: string): string {
  let seed = 0;
  for (const c of title) seed = (seed * 31 + c.charCodeAt(0)) >>> 0;
  const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
  const ridge = (base: number, amp: number) =>
    Array.from({ length: 17 }, (_, i) => `L${i * 100} ${base - rnd() * amp}`).join(" ");
  return `<svg viewBox="0 0 1600 900" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="xMidYMid slice">
  <rect width="1600" height="900" fill="#11161c"/>
  <path d="M0 900 ${ridge(560, 160)} L1600 900 Z" fill="${accent}" opacity="0.25"/>
  <path d="M0 900 ${ridge(680, 120)} L1600 900 Z" fill="${accent}" opacity="0.45"/>
  <path d="M0 900 ${ridge(780, 80)} L1600 900 Z" fill="#0b0f13"/>
</svg>`;
}

export function keyArt(id: string, accent: string, title: string): string {
  if (id === "dirt-simulator" || id === "dirt-digger") return dirtDigger;
  if (id === "alaskan-bush-simulator" || id === "bush-pilot") return bushPilot;
  return fallback(accent, title);
}

export const logoMark = `<svg viewBox="0 0 1024 1024" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
  <defs><linearGradient id="lm-g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f6c766"/><stop offset="0.55" stop-color="#e3a33b"/><stop offset="1" stop-color="#b8741f"/></linearGradient></defs>
  <rect x="48" y="48" width="928" height="928" rx="208" fill="#1b2129" stroke="#3a4552" stroke-width="12"/>
  <path d="M272 268 H752 V372 L452 652 H752 V756 H272 V652 L572 372 H272 Z" fill="url(#lm-g)"/>
</svg>`;

// UI icons: simple strokes on a 24px grid.
const icon = (d: string) =>
  `<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;

export const icons = {
  library: icon('<rect x="3" y="4" width="7" height="16" rx="1.5"/><rect x="14" y="4" width="7" height="16" rx="1.5"/>'),
  store: icon('<path d="M4 9h16l-1.5 10.5a1.5 1.5 0 0 1-1.5 1.3H7a1.5 1.5 0 0 1-1.5-1.3Z"/><path d="M8.5 9V7a3.5 3.5 0 0 1 7 0v2"/>'),
  key: icon('<circle cx="8" cy="15" r="4"/><path d="m11 12 8-8M16 7l2 2M14 9l2 2"/>'),
  settings: icon('<circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1"/>'),
  play: '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path d="M7 4.5v15l12-7.5Z" fill="currentColor"/></svg>',
  download: icon('<path d="M12 3v12M7 10l5 5 5-5M4 20h16"/>'),
  pause: icon('<path d="M9 5v14M15 5v14"/>'),
  folder: icon('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>'),
  check: icon('<path d="m5 12 4.5 4.5L19 7"/>'),
  trash: icon('<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>'),
  external: icon('<path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>'),
  more: icon('<circle cx="5" cy="12" r="1.2"/><circle cx="12" cy="12" r="1.2"/><circle cx="19" cy="12" r="1.2"/>'),
  signout: icon('<path d="M15 4h4a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-4M10 8l-4 4 4 4M6 12h10"/>'),
  update: icon('<path d="M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5"/>'),
  min: '<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2 6h8" stroke="currentColor" stroke-width="1.2"/></svg>',
  max: '<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><rect x="2.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1.2"/></svg>',
  close: '<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.2"/></svg>',
  lock: icon('<rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5"/>'),
  gamepad: icon('<path d="M7 9h10a4 4 0 0 1 3.9 4.9l-.6 2.6a2 2 0 0 1-3.4.9L15 15H9l-1.9 2.4a2 2 0 0 1-3.4-.9l-.6-2.6A4 4 0 0 1 7 9Z"/><path d="M8 12v2M7 13h2"/>'),
};
