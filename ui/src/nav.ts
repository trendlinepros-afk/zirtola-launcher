// Spatial navigation for controllers and arrow keys, plus Gamepad API polling.
//
// Focusable things are buttons, inputs, selects and [data-nav] elements inside
// the active scope (the open modal if there is one, else the whole app).
// D-pad / left stick / arrow keys move focus to the nearest element in that
// direction. Buttons: A = activate, B = back, LB/RB = previous/next game,
// Start = primary action, Y = Redeem key, Select (View) = Settings.

export type NavAction = "back" | "prevGame" | "nextGame" | "primary" | "redeem" | "settings";

type Dir = "up" | "down" | "left" | "right";

let actionHandler: (a: NavAction) => void = () => {};
let gamepadListeners: ((active: boolean) => void)[] = [];
let gamepadActive = false;

export function onNavAction(h: (a: NavAction) => void) {
  actionHandler = h;
}

export function onGamepadActive(cb: (active: boolean) => void) {
  gamepadListeners.push(cb);
}

function setGamepadActive(v: boolean) {
  if (gamepadActive === v) return;
  gamepadActive = v;
  document.body.classList.toggle("gamepad", v);
  gamepadListeners.forEach((l) => l(v));
}

function scope(): HTMLElement {
  const modals = document.querySelectorAll<HTMLElement>(".modal-backdrop");
  return modals.length ? modals[modals.length - 1]! : document.getElementById("app")!;
}

function focusables(): HTMLElement[] {
  const sel = "button:not([disabled]), input:not([disabled]), select:not([disabled]), [data-nav]";
  return Array.from(scope().querySelectorAll<HTMLElement>(sel)).filter((el) => {
    if (el.closest("[data-nav-skip]")) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden";
  });
}

export function focusFirst(preferred?: string) {
  const list = focusables();
  const el =
    (preferred && list.find((e) => e.dataset.id === preferred)) ||
    list.find((e) => e.classList.contains("primary")) ||
    list[0];
  el?.focus({ preventScroll: false });
}

function move(dir: Dir) {
  const list = focusables();
  const cur = document.activeElement as HTMLElement | null;
  if (!cur || !list.includes(cur)) {
    focusFirst();
    return;
  }
  const a = cur.getBoundingClientRect();
  const ax = a.left + a.width / 2;
  const ay = a.top + a.height / 2;
  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  for (const el of list) {
    if (el === cur) continue;
    const b = el.getBoundingClientRect();
    const bx = b.left + b.width / 2;
    const by = b.top + b.height / 2;
    const dx = bx - ax;
    const dy = by - ay;
    const along = dir === "left" ? -dx : dir === "right" ? dx : dir === "up" ? -dy : dy;
    const across = dir === "left" || dir === "right" ? Math.abs(dy) : Math.abs(dx);
    if (along <= 4) continue;
    // Prefer elements roughly in line; penalise sideways distance heavily.
    const score = along + across * 2.5;
    if (score < bestScore) {
      bestScore = score;
      best = el;
    }
  }
  if (best) {
    best.focus();
    best.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
}

function activate() {
  const el = document.activeElement as HTMLElement | null;
  if (!el) return;
  if (el instanceof HTMLInputElement && el.type === "checkbox") el.click();
  else if (el instanceof HTMLInputElement || el instanceof HTMLSelectElement) el.focus();
  else el.click();
}

// ---- keyboard

window.addEventListener("keydown", (e) => {
  const t = e.target as HTMLElement;
  const typing = t instanceof HTMLInputElement && t.type !== "checkbox" && t.type !== "range";
  const map: Record<string, Dir> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" };
  if (map[e.key] && !(typing && (e.key === "ArrowLeft" || e.key === "ArrowRight")) && !(t instanceof HTMLSelectElement)) {
    if (t instanceof HTMLInputElement && t.type === "range" && (e.key === "ArrowLeft" || e.key === "ArrowRight")) return;
    e.preventDefault();
    setGamepadActive(false);
    move(map[e.key]!);
  } else if (e.key === "Escape") {
    actionHandler("back");
  }
});

window.addEventListener("mousemove", () => setGamepadActive(false), { passive: true });

// ---- gamepad (standard mapping)

const BTN = { A: 0, B: 1, X: 2, Y: 3, LB: 4, RB: 5, SELECT: 8, START: 9, UP: 12, DOWN: 13, LEFT: 14, RIGHT: 15 };
const prev: Record<number, boolean> = {};
const repeatAt: Record<string, number> = {};

function pressed(gp: Gamepad, i: number) {
  return !!gp.buttons[i]?.pressed;
}

function edge(gp: Gamepad, i: number): boolean {
  const p = pressed(gp, i);
  const was = prev[i] ?? false;
  prev[i] = p;
  return p && !was;
}

function held(key: string, down: boolean, now: number): boolean {
  if (!down) {
    delete repeatAt[key];
    return false;
  }
  if (repeatAt[key] === undefined) {
    repeatAt[key] = now + 380; // initial delay
    return true;
  }
  if (now >= repeatAt[key]!) {
    repeatAt[key] = now + 110; // repeat rate
    return true;
  }
  return false;
}

function poll(now: number) {
  const pads = navigator.getGamepads ? Array.from(navigator.getGamepads()).filter(Boolean) as Gamepad[] : [];
  const gp = pads[0];
  if (gp && document.hasFocus()) {
    const ax = gp.axes[0] ?? 0;
    const ay = gp.axes[1] ?? 0;
    const dirs: [Dir, boolean][] = [
      ["up", pressed(gp, BTN.UP) || ay < -0.55],
      ["down", pressed(gp, BTN.DOWN) || ay > 0.55],
      ["left", pressed(gp, BTN.LEFT) || ax < -0.55],
      ["right", pressed(gp, BTN.RIGHT) || ax > 0.55],
    ];
    let any = false;
    for (const [d, down] of dirs) {
      if (held(d, down, now)) {
        any = true;
        const t = document.activeElement;
        if (t instanceof HTMLInputElement && t.type === "range" && (d === "left" || d === "right")) {
          t.value = String(+t.value + (d === "right" ? +t.step || 1 : -(+t.step || 1)));
          t.dispatchEvent(new Event("input", { bubbles: true }));
          t.dispatchEvent(new Event("change", { bubbles: true }));
        } else move(d);
      }
    }
    const fire = (b: number, fn: () => void) => {
      if (edge(gp, b)) {
        any = true;
        fn();
      }
    };
    fire(BTN.A, activate);
    fire(BTN.B, () => actionHandler("back"));
    fire(BTN.LB, () => actionHandler("prevGame"));
    fire(BTN.RB, () => actionHandler("nextGame"));
    fire(BTN.START, () => actionHandler("primary"));
    fire(BTN.Y, () => actionHandler("redeem"));
    fire(BTN.SELECT, () => actionHandler("settings"));
    if (any) {
      setGamepadActive(true);
      if (!document.activeElement || document.activeElement === document.body) focusFirst();
    }
  }
  requestAnimationFrame(poll);
}

window.addEventListener("gamepadconnected", () => setGamepadActive(true));
window.addEventListener("gamepaddisconnected", () => setGamepadActive(false));
requestAnimationFrame(poll);
