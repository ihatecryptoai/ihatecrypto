// What every part of the window shares: the engine's last state, the requests to it, and the little helpers that
// build elements. The page holds no key and no money logic: a move is a card the engine made, and Confirm is the only
// way it runs.

import { logoFiles } from "./logos.js";
import { qrMatrix } from "./qr.js";

const tauri = window.__TAURI__;
export const invoke = (cmd, args) => tauri.core.invoke(cmd, args);

/** The window's own state, beside the engine's (`ui.state`). Kept across redraws; a few parts across launches. */
export const ui = {
  /** The engine's last «state» event. */
  state: null,
  /** «canvas», «settings», «history» or «topup»: what fills the window beside the rail. */
  view: "canvas",
  /** A screen over everything else (importing a phrase or a key), kept while new states arrive. */
  overlay: null,
  /** The chat field's text: the field is drawn again with every state. */
  composerText: "",
  /** A block pointed at for the next message (right-click › Ask in chat). */
  attached: null,
  /** Finished moves opened from their pill, and step cards opened to their details. */
  opened: new Set(),
  /** While the canvas is dragged, new states wait: redrawing would drop the drag. */
  dragging: false,
  renderWaiting: false,
};

// MARK: The engine

let nextId = 1;
const waiting = new Map();

export function engine(op, args = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    waiting.set(id, { resolve, reject });
    invoke("engine_send", { line: JSON.stringify({ id, op, ...args }) }).catch((e) => {
      waiting.delete(id);
      reject(new Error(String(e)));
    });
  });
}

/** The engine's answer to one of our requests; false when the line isn't one. */
export function answered(msg) {
  if (typeof msg.id !== "number" || !waiting.has(msg.id)) return false;
  const w = waiting.get(msg.id);
  waiting.delete(msg.id);
  msg.ok ? w.resolve(msg.result) : w.reject(new Error(msg.error || "Failed"));
  return true;
}

/** An action from a button: its refusal (the signer's, Windows Hello cancelled) shows as a toast. */
export async function act(op, args = {}) {
  try {
    return await engine(op, args);
  } catch (e) {
    toast(e.message);
    return null;
  }
}

// MARK: Drawing

let root = () => {};
/** app.js hands over the function that draws the whole window. */
export function setRoot(draw) { root = draw; }
/** Draws the window again from `ui.state`; waits while the canvas is being dragged. */
export function render() {
  if (!ui.state) return;
  if (ui.dragging) { ui.renderWaiting = true; return; }
  root();
}

export const t = (en, ru) => (ui.state && ui.state.russian ? ru : en);

/** An element from a tag, its properties and children; text children are text nodes, never HTML. */
export function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith("on")) el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k === "class") el.className = v;
    // Through CSSOM: the page's CSP lets no inline style attribute in.
    else if (k === "style") el.style.cssText = v;
    else if (k === "html") throw new Error("no raw HTML");
    else el.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat(Infinity)) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

const NS = "http://www.w3.org/2000/svg";
export function svg(tag, attrs = {}, ...children) {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  for (const c of children.flat(Infinity)) if (c) el.append(c);
  return el;
}

/** A QR code (qr.js) drawn as squares on white, with a quiet edge for the camera. */
export function qrCode(text, cls) {
  const m = qrMatrix(text);
  const size = m.length, q = 2;
  let d = "";
  m.forEach((row, y) => row.forEach((dark, x) => { if (dark) d += `M${x + q} ${y + q}h1v1h-1z`; }));
  return svg("svg", { viewBox: `0 0 ${size + 2 * q} ${size + 2 * q}`, class: cls, "shape-rendering": "crispEdges" },
    svg("rect", { width: size + 2 * q, height: size + 2 * q, fill: "#fff" }), svg("path", { d, fill: "#000" }));
}

/** Icons as fixed paths on a 24-point grid (lines, 1.8 wide): nothing the engine sends becomes markup. The names follow
 * the SF Symbols the Mac uses. A part adds its own with `addIcons`. */
const icons = {
  plus: "M12 5v14M5 12h14",
  minus: "M5 12h14",
  xmark: "M6 6l12 12M18 6 6 18",
  check: "M5 12.5l4.5 4.5L19 7",
  "chevron.left": "M15 5l-7 7 7 7",
  "chevron.right": "M9 5l7 7-7 7",
  "chevron.down": "M5 9l7 7 7-7",
  "chevron.up": "M5 15l7-7 7 7",
  "chevron.updown": "M8 9l4-4 4 4M8 15l4 4 4-4",
  "arrow.right": "M4 12h15M13 6l6 6-6 6",
  "arrow.up": "M12 19V5M6 11l6-6 6 6",
  "arrow.down.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7.5v9M8 12.5l4 4 4-4",
  "arrow.clockwise": "M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v4h-4",
  "sidebar.left": "M4 5h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM9.5 5v14M5.5 8.5h2M5.5 11.5h2",
  eye: "M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
  "eye.slash": "M2.5 12S6 5.5 12 5.5c1.7 0 3.2.5 4.5 1.2M21.5 12s-3.5 6.5-9.5 6.5c-1.7 0-3.2-.5-4.5-1.2M4 20 20 4M9.9 14.1a3 3 0 0 1 4.2-4.2",
  gear: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.6-2-3.4-2.4.9a7.5 7.5 0 0 0-2.6-1.5L14 2.4h-4l-.4 2.5A7.5 7.5 0 0 0 7 6.4l-2.4-.9-2 3.4 2 1.6a7.6 7.6 0 0 0 0 3l-2 1.6 2 3.4 2.4-.9a7.5 7.5 0 0 0 2.6 1.5l.4 2.5h4l.4-2.5a7.5 7.5 0 0 0 2.6-1.5l2.4.9 2-3.4z",
  clock: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3.5 2",
  star: "M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.8z",
  calendar: "M4 6h16v14H4zM4 10h16M8 3.5V7M16 3.5V7",
  "chart.pie": "M12 3v9h9a9 9 0 1 1-9-9zM15 3.5A8 8 0 0 1 20.5 9H15z",
  "rectangle.stack": "M5 8h14v11H5zM7 5h10M9 2.5h6",
  "folder.badge.plus": "M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2h8.5A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5zM12 10.5v5M9.5 13h5",
  "square.and.pencil": "M11 4.5H5.5a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1h13a1 1 0 0 0 1-1V13M17.5 3.5l3 3L12 15l-3.5.5.5-3.5z",
  "square.grid": "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
  "arrow.down.right.and.arrow.up.left": "M14 10l6-6M14 10h5M14 10V5M10 14l-6 6M10 14H5M10 14v5",
  "arrow.up.left.and.arrow.down.right": "M4 4l6 6M4 4h5M4 4v5M20 20l-6-6M20 20h-5M20 20v-5",
  copy: "M9 9h10v11H9zM5 15V4h10",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  lock: "M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3",
  "lock.open": "M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 6.8-1.2",
  globe: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c2.5 2.5 3.8 5.5 3.8 9s-1.3 6.5-3.8 9c-2.5-2.5-3.8-5.5-3.8-9S9.5 5.5 12 3z",
  mic: "M12 3.5a3 3 0 0 1 3 3V12a3 3 0 0 1-6 0V6.5a3 3 0 0 1 3-3zM6 11a6 6 0 0 0 12 0M12 17v3.5",
  "wallet.pass": "M4 7.5A1.5 1.5 0 0 1 5.5 6h13A1.5 1.5 0 0 1 20 7.5v10a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5zM4 10h16M15.5 14.5h1.5",
  "checkmark.seal": "M12 2.8l2.2 1.6 2.7-.1.8 2.6 2.2 1.6-.9 2.5.9 2.5-2.2 1.6-.8 2.6-2.7-.1L12 19.2l-2.2-1.6-2.7.1-.8-2.6-2.2-1.6.9-2.5-.9-2.5 2.2-1.6.8-2.6 2.7.1zM8.5 11l2.5 2.5 4.5-4.5",
  "exclamationmark.triangle": "M12 4l9 16H3zM12 10v4.5M12 17.5v.5",
  "questionmark.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6v.6M12 17v.5",
  ellipsis: "M6 12h.01M12 12h.01M18 12h.01",
  "macwindow": "M3.5 5h17v14h-17zM3.5 9h17M6 7h.01M8.5 7h.01",
  "window.minimize": "M6 12h12",
  "window.maximize": "M6.5 6.5h11v11h-11z",
  "window.restore": "M8.5 8.5h9v9h-9zM6.5 15.5v-9h9",
  "window.close": "M6.5 6.5l11 11M17.5 6.5l-11 11",
  doc: "M7 3.5h7l4 4V20.5H7zM14 3.5V8h4",
  person: "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4.5 20.5a7.5 7.5 0 0 1 15 0",
  trash: "M5 7h14M10 4h4M7 7l1 13h8l1-13",
  pencil: "M15.5 4.5l4 4L8 20H4v-4z",
  bolt: "M13 3L5 13.5h6L10 21l8-10.5h-6z",
  power: "M12 3v8M7 6.3a7.5 7.5 0 1 0 10 0",
};

export function addIcons(more) { Object.assign(icons, more); }

/** An icon from the set above; `size` in points (16 by default), drawn in the text's color. */
export function icon(name, size) {
  const s = svg("svg", { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": "1.8", "stroke-linecap": "round", "stroke-linejoin": "round", "aria-hidden": "true" },
    svg("path", { d: icons[name] || "" }));
  s.classList.add("ico");
  if (size) s.style.cssText = `width:${size}px;height:${size}px`;
  return s;
}

/** The mascot, drawn as on the Mac (BrandLogo, a 140-point grid): the coin with its eye-roll and the other moods. */
export function mascot(size = 34, mood = "idle") {
  const ink = "#111111", accent = "#FF7A2F";
  const c = (x, y, r, attrs) => svg("circle", { cx: x, cy: y, r, ...attrs });
  const g = svg("svg", { viewBox: "0 0 140 140", width: size, height: size, class: "mascot mood-" + mood, "aria-label": "ihatecrypto" });
  g.append(c(70, 70, 64, { fill: accent }),
    c(70, 70, 62, { fill: "none", stroke: ink, "stroke-width": 9, "stroke-dasharray": "7 7", class: "ridge" }),
    c(70, 70, 52, { fill: "none", stroke: ink, "stroke-width": 3.5 }));
  const line = (d) => svg("path", { d, fill: "none", stroke: ink, "stroke-width": 8, "stroke-linecap": "round", "stroke-linejoin": "round" });
  const eyes = (dx, dy) => [50, 90].map((x) => [c(x, 62, 17, { fill: "#fff", stroke: ink, "stroke-width": 5 }), c(x + dx, 62 + dy, 8, { fill: ink, class: "pupil" })]);
  if (mood === "idle" || mood === "waiting" || mood === "thinking") {
    const a = mood === "thinking" ? 2.25 : 1.1;
    g.append(...eyes(9 * Math.cos(a), -9 * Math.sin(a)).flat());
    g.append(line(mood === "thinking" ? "M52 98Q58 92 64 98Q70 104 76 98Q82 92 88 98" : "M54 98Q70 92 86 96"));
  } else if (mood === "tuning" || mood === "listening") {
    g.append(...eyes(mood === "tuning" ? -3 : 0, mood === "tuning" ? 6 : 0).flat());
    if (mood === "tuning") g.append(line("M34 36Q48 28 62 36M78 36Q92 28 106 36"));
    g.append(c(70, 97, 6, { fill: "none", stroke: ink, "stroke-width": 7, class: "mouth" }));
  } else if (mood === "done") {
    g.append(line("M36 64Q50 48 64 64M76 64Q90 48 104 64M50 86Q70 106 90 86"));
  } else if (mood === "asleep") {
    g.append(line("M37 62Q50 75 63 62M77 62Q90 75 103 62M60 98L80 98"));
  } else if (mood === "error") {
    g.append(line("M38 50L60 72M60 50L38 72M80 50L102 72M102 50L80 72M54 98L86 98"));
  }
  return g;
}

/** A chain's or token's logo from the bundled set (logos/), by ticker or name; a lettered disc when there's none. */
export function logo(name, size = 28, src) {
  const key = (name || "").toLowerCase().split(":").pop();
  const alias = { ubtc: "btc", ueth: "eth", usol: "sol", usdt0: "usdt", "bnb chain": "bnb", "hyperliquid": "hype", "robinhood chain": "robinhood" }[key] || key;
  const file = logoFiles.get(alias);
  const box = h("span", { class: "logo", style: `width:${size}px;height:${size}px` });
  // As the Mac's TokenLogo: Hyperliquid's mint mark sits on its dark tile, Uniswap's pink unicorn on a white disc
  // (neither has a background of its own, and on the dark theme they floated).
  if (alias === "hype") box.classList.add("logo-on-hl");
  if (alias === "uni") box.classList.add("logo-on-white");
  if (file || src) {
    const img = h("img", { src: file ? "logos/" + file : src, alt: "", draggable: "false" });
    img.addEventListener("error", () => { img.remove(); box.classList.add("letter"); box.textContent = (name || "?").slice(0, 1).toUpperCase(); }, { once: true });
    box.append(img);
  } else {
    box.classList.add("letter");
    box.style.fontSize = Math.round(size * 0.45) + "px";
    box.textContent = (name || "?").slice(0, 1).toUpperCase();
  }
  return box;
}

let toastTimer = null;
export function toast(text) {
  document.querySelector(".toast")?.remove();
  const el = h("div", { class: "toast" }, text);
  document.body.append(el);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.remove(), 6000);
}

/** Dollars as the Mac writes them: «$1,234.56»; «−$3.20» with a minus when negative. */
export const usd = (n) => {
  if (n === null || n === undefined || n === "") return "";
  const v = Number(n);
  const s = "$" + Math.abs(v).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  return v < 0 ? "−" + s : s;
};
/** «+$1.20» / «−$1.20». */
export const signedUsd = (n) => (Number(n) < 0 ? usd(n) : "+" + usd(n));
/** «+3.1%» / «−3.1%». */
export const pct = (n, digits = 1) => (n === null || n === undefined ? "" : (Number(n) < 0 ? "−" : "+") + Math.abs(Number(n)).toFixed(digits) + "%");

/** The balance hidden by the eye in the rail: amounts show as dots everywhere. */
export const masked = (text, mask = "$•••") => (store("balanceHidden") === true ? mask : text);

export async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
    toast(t("Copied.", "Скопировано."));
  } catch {
    toast(t("Couldn't copy.", "Не скопировалось."));
  }
}

export function copyButton(text, label) {
  return h("button", { class: "btn small", title: t("Copy", "Копировать"), onclick: (e) => { e.stopPropagation(); copy(text); } }, icon("copy", 13), label === undefined ? t("Copy", "Копировать") : label);
}

/** An explorer link in the default browser. */
export function link(url, text) {
  // Beside the chat, in the page panel (the Mac's WebPanel); sign-in pages still go to the person's own browser.
  return h("a", { href: "#", onclick: (e) => { e.preventDefault(); invoke("open_page", { url }).catch((x) => toast(String(x))); } }, text);
}

/** A per-viewer setting kept in this window's storage (a folded rail, the chat's size): never anything that matters. */
export function store(key, value) {
  try {
    if (value === undefined) return JSON.parse(localStorage.getItem(key) || "null");
    localStorage.setItem(key, JSON.stringify(value));
  } catch { return null; }
}

/** The chains' own colors, as the Mac's cards use them. */
export const networkColor = {
  solana: "#9945FF", hyperliquid: "#072723", base: "#0052FF", ethereum: "#627EEA", arbitrum: "#213147", polygon: "#8247E5",
  optimism: "#FF0420", bnb: "#F0B90B", robinhood: "#C6F432", tempo: "#1B1B1B", near: "#00A86B", hyperevm: "#0E3B34",
};
export function colorFor(text) {
  const s = (text || "").toLowerCase();
  for (const [k, c] of Object.entries(networkColor)) if (s.includes(k)) return c;
  return "#3a3a3a";
}

/** «23 Oct · 00:01». */
export function when(iso) {
  const d = new Date(iso), locale = ui.state?.russian ? "ru-RU" : "en-US";
  return d.toLocaleDateString(locale, { day: "numeric", month: "short" }) + " · " + d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
}

/** A menu at a point; Escape or a click outside closes it. `items`: [label, run] or null for a divider. */
export function popup(e, items, cls = "") {
  document.querySelector(".menu")?.remove();
  const menu = h("div", { class: "menu " + cls },
    items.map((it) => it === null ? h("div", { class: "menu-divider" }) : h("button", { class: "item", onclick: () => { menu.remove(); it[1](); } }, it[0])));
  document.body.append(menu);
  const w = menu.offsetWidth, hgt = menu.offsetHeight;
  menu.style.left = Math.max(8, Math.min(e.clientX, window.innerWidth - w - 8)) + "px";
  menu.style.top = Math.max(8, Math.min(e.clientY, window.innerHeight - hgt - 8)) + "px";
  setTimeout(() => document.addEventListener("click", function off(ev) { if (!menu.contains(ev.target)) { menu.remove(); document.removeEventListener("click", off); } }), 0);
  return menu;
}

/** A command a block offers goes into the chat field with its amount selected; the person sends it. */
/** A phrase into the field, never sent: the word to change selected — `select` when it's there («Name» in «save … as
 *  Name»), else the first number on its own (not the digits inside an address). */
export function suggest(text, select) {
  ui.composerText = text;
  reportComposing(text);
  render();
  const f = document.getElementById("composer");
  if (!f) return;
  f.focus();
  const at = select ? text.lastIndexOf(select) : -1;
  if (at >= 0) f.setSelectionRange(at, at + select.length);
  else {
    const m = text.match(/(?<![\w.])\d+(?:\.\d+)?(?![\w])/);
    if (m) f.setSelectionRange(m.index, m.index + m[0].length);
  }
  autosize(f);
}

/** Whether the chat's field holds text, told to the engine when that changes: a scheduled run waits while it does
 *  (the Mac's draft.isEmpty). */
let composingSaid = false;
export function reportComposing(text) {
  const now = !!(text || "").trim();
  if (now === composingSaid) return;
  composingSaid = now;
  engine("composing", { test: now }).catch(() => {});
}

export function autosize(el, max = 160) { el.style.height = "auto"; el.style.height = Math.min(el.scrollHeight, max) + "px"; }

/** Windows Hello, said the way the Mac says «Touch ID» through Presence. */
export const presence = () => "Windows Hello";
