// The window's page: it draws what the engine says (the «state» event after every change) and sends it what the person
// did. It holds no key and no money logic: a move is a card the engine made, and Confirm is the only way it runs.
//
// The layout is the Mac's: the canvases on the left rail, a canvas with the portfolio and the blocks (the newest on top,
// a move as its route's cards), and the chat floating over it. Settings replace the canvas.

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);

let state = null;
/** «canvas» or «settings». */
let view = "canvas";
/** A screen over everything else (importing a phrase or a key), kept while new states arrive. */
let overlay = null;
let toastTimer = null;
let nextId = 1;
const waiting = new Map();

// MARK: The engine

function engine(op, args = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    waiting.set(id, { resolve, reject });
    invoke("engine_send", { line: JSON.stringify({ id, op, ...args }) }).catch((e) => {
      waiting.delete(id);
      reject(new Error(String(e)));
    });
  });
}

/** An action from a button: its refusal (the signer's, Windows Hello cancelled) shows as a toast. */
async function act(op, args = {}) {
  try {
    return await engine(op, args);
  } catch (e) {
    toast(e.message);
    return null;
  }
}

tauri.event.listen("engine", (e) => {
  let msg;
  try { msg = JSON.parse(e.payload); } catch { return; }
  if (msg.event === "state") {
    state = msg.data;
    render();
  } else if (msg.event === "openURL") {
    // A sign-in page the engine asks for (ChatGPT, OpenRouter): https only, in the default browser.
    invoke("open_link", { url: msg.data.url }).catch((x) => toast(String(x)));
  } else if (msg.event === "notify") {
    toast(msg.data.body);
  } else if (msg.event === "engineStopped") {
    toast(t("The engine stopped. Restart ihatecrypto.", "Движок остановился. Перезапусти ihatecrypto."));
  } else if (typeof msg.id === "number" && waiting.has(msg.id)) {
    const w = waiting.get(msg.id);
    waiting.delete(msg.id);
    msg.ok ? w.resolve(msg.result) : w.reject(new Error(msg.error || "Failed"));
  }
});

// MARK: Little helpers

const t = (en, ru) => (state && state.russian ? ru : en);

/** An element from a tag, its properties and children; text children are text nodes, never HTML. */
function h(tag, props = {}, ...children) {
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
function svg(tag, attrs = {}, ...children) {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  for (const c of children) el.append(c);
  return el;
}

/** An icon drawn from a fixed path: nothing the engine sends becomes markup. */
function icon(name) {
  const paths = {
    plus: "M12 5v14M5 12h14",
    close: "M6 6l12 12M18 6 6 18",
    settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm8-3 2 1-2 3.5-2.2-.6a7 7 0 0 1-1.6 1L16 19h-4l-.6-2.1a7 7 0 0 1-1.6-1l-2.2.6L5.6 13l2-1a7 7 0 0 1 0-2l-2-1L7.6 5.5l2.2.6a7 7 0 0 1 1.6-1L12 3h4l.6 2.1a7 7 0 0 1 1.6 1l2.2-.6L22.4 9l-2 1a7 7 0 0 1 0 2z",
    canvas: "M4 5h16v14H4zM4 9h16M9 9v10",
    home: "M4 11l8-7 8 7M6 10v10h12V10",
    minus: "M5 12h14",
    expand: "M8 4H4v4M16 4h4v4M8 20H4v-4M16 20h4v-4",
    shrink: "M4 9h5V4M20 9h-5V4M4 15h5v5M20 15h-5v5",
    copy: "M9 9h10v11H9zM5 15V4h10",
    chat: "M4 5h16v11H8l-4 4z",
    check: "M5 12l4 4 10-10",
    more: "M6 12h.01M12 12h.01M18 12h.01",
    back: "M15 6l-6 6 6 6",
  };
  const s = svg("svg", { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": "1.8", "stroke-linecap": "round", "stroke-linejoin": "round" },
    svg("path", { d: paths[name] || "" }));
  s.classList.add("ico");
  return s;
}

function toast(text) {
  document.querySelector(".toast")?.remove();
  const el = h("div", { class: "toast" }, text);
  document.body.append(el);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.remove(), 6000);
}

const usd = (n) => (n === null || n === undefined ? "" : "$" + Number(n).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 }));

async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
    toast(t("Copied.", "Скопировано."));
  } catch {
    toast(t("Couldn't copy.", "Не скопировалось."));
  }
}

function copyButton(text) {
  return h("button", { class: "btn small-btn", onclick: (e) => { e.stopPropagation(); copy(text); } }, icon("copy"), t("Copy", "Копировать"));
}

function link(url, text) {
  return h("a", { href: "#", onclick: (e) => { e.preventDefault(); invoke("open_link", { url }).catch((x) => toast(String(x))); } }, text);
}

function store(key, value) {
  try {
    if (value === undefined) return JSON.parse(localStorage.getItem(key) || "null");
    localStorage.setItem(key, JSON.stringify(value));
  } catch { return null; }
}

/** The brand colours of the networks, as the Mac's route cards use them. */
const networkColor = {
  solana: "#9945FF", hyperliquid: "#072723", base: "#0052FF", ethereum: "#627EEA", arbitrum: "#213147", polygon: "#8247E5",
  optimism: "#FF0420", bnb: "#F0B90B", robinhood: "#1F3B0A", tempo: "#1B1B1B", near: "#00A86B", hyperevm: "#0E3B34",
};
function colorFor(text) {
  const s = (text || "").toLowerCase();
  for (const [k, c] of Object.entries(networkColor)) if (s.includes(k) || s.includes(k === "bnb" ? "bnb chain" : k)) return c;
  return "#3a3a3a";
}

// MARK: Drawing

const app = document.getElementById("app");
let composerText = "";
/** A block pointed at for the next message (right-click › Ask in chat). */
let attached = null;
/** Finished moves opened from their pill, and step cards opened to their details. */
const opened = new Set();
let chatFull = store("chatFull") === true;

let protectedOn = false;
function setProtect(on) {
  if (on === protectedOn) return;
  protectedOn = on;
  invoke("protect", { on }).catch(() => {});
}

/** While the canvas is dragged, new states wait: redrawing would drop the drag. */
let dragging = false;
let renderWaiting = false;

function render() {
  if (!state) return;
  if (dragging) { renderWaiting = true; return; }
  // What the person is in the middle of survives a new state: the field they type in, where the caret is, the scroll.
  const chatEl = document.querySelector(".chat");
  const atBottom = !chatEl || chatEl.scrollHeight - chatEl.scrollTop - chatEl.clientHeight < 40;
  const chatTop = chatEl ? chatEl.scrollTop : 0;
  const pageEl = document.querySelector(".page");
  const pageTop = pageEl ? pageEl.scrollTop : 0;
  const active = document.activeElement;
  const focused = active?.id;
  const selection = focused && "selectionStart" in active ? [active.selectionStart, active.selectionEnd] : null;
  app.replaceChildren();
  if (overlay) {
    app.style.gridTemplateColumns = "1fr";
    app.append(overlay);
    return;
  }
  if (state.phrase) {
    app.style.gridTemplateColumns = "1fr";
    app.append(phraseScreen());
    return;
  }
  setProtect(false);
  if (!state.wallets.length) {
    app.style.gridTemplateColumns = "1fr";
    app.append(welcome());
    return;
  }
  app.style.gridTemplateColumns = "";
  app.append(rail(), h("div", { class: "main" }, view === "settings" ? settings() : canvas()));
  const chatNow = document.querySelector(".chat");
  if (chatNow) chatNow.scrollTop = atBottom ? chatNow.scrollHeight : chatTop;
  const pageNow = document.querySelector(".page");
  if (pageNow) pageNow.scrollTop = pageTop;
  if (focused) {
    const el = document.getElementById(focused);
    if (el) {
      el.focus();
      if (selection && "setSelectionRange" in el) { try { el.setSelectionRange(selection[0], selection[1]); } catch {} }
    }
  }
}

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    document.querySelector(".menu")?.remove();
    if (view === "settings") { view = "canvas"; render(); }
  }
});

// MARK: The rail: canvases, the wallet, Settings

function rail() {
  const activeWallet = state.wallets.find((w) => w.id === state.active) || state.wallets[0];
  return h("nav", { class: "rail" },
    h("div", { class: "brand" }, h("img", { src: "coin.png", alt: "" }), h("span", { class: "brand-name" }, "ihatecrypto")),
    h("button", { class: "nav", onclick: async () => { view = "canvas"; await act("newCanvas"); } }, icon("plus"), h("span", { class: "label" }, t("New canvas", "Новый холст"))),
    h("div", { class: "canvas-list" }, state.canvases.map((c) => h("button", {
      class: "canvas-row" + (c.id === state.canvas && view === "canvas" ? " on" : ""),
      title: c.title,
      onclick: async () => { view = "canvas"; if (c.id !== state.canvas) await act("openCanvas", { wallet: c.id }); else render(); },
      oncontextmenu: (e) => { e.preventDefault(); canvasMenu(e, c); },
    }, icon("canvas"), h("span", { class: "label" }, h("span", { class: "canvas-title" }, c.title), h("span", { class: "canvas-at" }, when(c.at)))))),
    h("div", { class: "spacer" }),
    h("button", { class: "wallet-chip", onclick: walletMenu },
      h("span", { class: "dot" + (activeWallet?.test || activeWallet?.automation ? " test" : "") }),
      h("span", { class: "name" }, activeWallet ? activeWallet.label : t("No wallet", "Нет кошелька"))),
    h("button", { class: "nav" + (view === "settings" ? " on" : ""), onclick: () => { view = view === "settings" ? "canvas" : "settings"; render(); } },
      icon("settings"), h("span", { class: "label" }, t("Settings", "Настройки"))),
  );
}

function when(iso) {
  const d = new Date(iso);
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" }) + " · " + d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function popup(e, items) {
  document.querySelector(".menu")?.remove();
  const menu = h("div", { class: "menu", style: `left:${Math.min(e.clientX, window.innerWidth - 260)}px; top:${Math.min(e.clientY, window.innerHeight - 40 * items.length - 20)}px` },
    items.map(([label, run]) => h("button", { class: "item", onclick: () => { menu.remove(); run(); } }, label)));
  document.body.append(menu);
  setTimeout(() => document.addEventListener("click", function off(ev) { if (!menu.contains(ev.target)) { menu.remove(); document.removeEventListener("click", off); } }), 0);
}

function canvasMenu(e, c) {
  popup(e, [
    [t("Rename…", "Переименовать…"), () => renameDialog(c)],
    [t("Delete this canvas", "Удалить этот холст"), () => act("removeCanvas", { wallet: c.id })],
  ]);
}

function renameDialog(c) {
  let value = c.title;
  const done = () => { overlay = null; render(); };
  overlay = h("div", { class: "center" }, h("div", { class: "card", style: "max-width:420px;width:100%" },
    h("h1", { style: "font-size:18px" }, t("Rename the canvas", "Переименовать холст")),
    h("input", { id: "rename", style: "width:100%", value, oninput: (ev) => (value = ev.target.value),
      onkeydown: async (ev) => { if (ev.key === "Enter") { await act("renameCanvas", { wallet: c.id, label: value }); done(); } } }),
    h("div", { class: "btns", style: "margin-top:12px" },
      h("button", { class: "btn primary", onclick: async () => { await act("renameCanvas", { wallet: c.id, label: value }); done(); } }, t("Save", "Сохранить")),
      h("button", { class: "btn", onclick: done }, t("Back", "Назад")))));
  render();
  setTimeout(() => document.getElementById("rename")?.focus(), 0);
}

function walletMenu(e) {
  document.querySelector(".menu")?.remove();
  const r = e.currentTarget.getBoundingClientRect();
  const close = () => menu.remove();
  const menu = h("div", { class: "menu", style: `left:${r.left}px; bottom:${window.innerHeight - r.top + 6}px` },
    state.wallets.map((w) => h("div", { class: "item" },
      h("span", {}, w.label, w.test ? h("span", { class: "muted small" }, " · test") : null, w.automation ? h("span", { class: "muted small" }, t(" · automation", " · автоматизация")) : null),
      w.id === state.active ? h("span", { class: "check" }, "✓")
        : h("button", { class: "btn", onclick: async () => { close(); await act("switchWallet", { wallet: w.id }); } }, t("Switch", "Переключить")))),
    h("div", { class: "item" }, h("button", { class: "btn", onclick: () => { close(); view = "settings"; render(); } }, t("Wallets…", "Кошельки…"))),
  );
  document.body.append(menu);
  setTimeout(() => document.addEventListener("click", function off(ev) { if (!menu.contains(ev.target)) { menu.remove(); document.removeEventListener("click", off); } }), 0);
}

// MARK: The canvas

/** Where the canvas is looked at, per canvas: moved by dragging or the wheel, zoomed only by the person. */
let cam = null;
let camFor = null;
function camera() {
  if (camFor !== state.canvas) {
    camFor = state.canvas;
    cam = store("cam:" + state.canvas) || { x: 24, y: 72, z: 1 };
  }
  return cam;
}
function saveCam() { store("cam:" + camFor, cam); }

function canvas() {
  const c = camera();
  const world = h("div", { class: "world", style: `transform: translate(${c.x}px, ${c.y}px) scale(${c.z})` },
    portfolioFrame(),
    h("div", { class: "stack" }, state.boards.length ? state.boards.map(board) : h("div", { class: "hint" },
      t("Ask in the chat: “buy SOL for 10 USDC”, “where to keep USDC”, “chart ETH week”. What it makes appears here, the newest on top. Nothing moves until you press Confirm and Windows Hello asks for you.",
        "Спроси в чате: «купи SOL на 10 USDC», «где держать USDC», «график ETH за неделю». Что он сделает, появится здесь — новое сверху. Ничего не двигается, пока ты не нажмёшь «Подтвердить» и Windows Hello не спросит тебя."))));
  // What was on the canvas when it opened is already there; only what comes after fades in.
  setTimeout(() => { seenReady = true; }, 0);
  const surface = h("div", { class: "canvas" }, world,
    h("div", { class: "canvas-tools" },
      h("button", { class: "tool", title: t("Zoom out", "Отдалить"), onclick: () => zoom(1 / 1.15) }, icon("minus")),
      h("button", { class: "tool", title: t("Back home", "Домой"), onclick: () => { Object.assign(cam, { x: 24, y: 72, z: 1 }); saveCam(); render(); } }, icon("home")),
      h("button", { class: "tool", title: t("Zoom in", "Приблизить"), onclick: () => zoom(1.15) }, icon("plus")),
      h("button", { class: "btn add-block", onclick: blockLibrary }, icon("plus"), t("Add block", "Добавить блок"))),
    chatPanel());
  surface.addEventListener("pointerdown", (e) => {
    // Only the empty canvas drags: a block's own buttons and text stay usable.
    if (e.button !== 0 || (e.target !== surface && e.target !== world && !e.target.classList.contains("stack"))) return;
    dragging = true;
    const start = { x: e.clientX - cam.x, y: e.clientY - cam.y };
    surface.setPointerCapture(e.pointerId);
    surface.classList.add("grabbing");
    const move = (ev) => { cam.x = ev.clientX - start.x; cam.y = ev.clientY - start.y; world.style.transform = `translate(${cam.x}px, ${cam.y}px) scale(${cam.z})`; };
    const up = () => {
      surface.removeEventListener("pointermove", move);
      dragging = false;
      surface.classList.remove("grabbing");
      saveCam();
      if (renderWaiting) { renderWaiting = false; render(); }
    };
    surface.addEventListener("pointermove", move);
    surface.addEventListener("pointerup", up, { once: true });
    surface.addEventListener("pointercancel", up, { once: true });
  });
  surface.addEventListener("wheel", (e) => {
    if (e.target.closest(".chatpanel, .scroll-own")) return;
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      const r = surface.getBoundingClientRect();
      zoom(Math.exp(-e.deltaY / 400), e.clientX - r.left, e.clientY - r.top);
    } else {
      cam.x -= e.deltaX;
      cam.y -= e.deltaY;
      world.style.transform = `translate(${cam.x}px, ${cam.y}px) scale(${cam.z})`;
      saveCam();
    }
  }, { passive: false });
  return surface;
}

function zoom(f, px, py) {
  const surface = document.querySelector(".canvas");
  const r = surface?.getBoundingClientRect();
  const x = px ?? (r ? r.width / 2 : 0), y = py ?? (r ? r.height / 2 : 0);
  const z = Math.min(2.5, Math.max(0.35, cam.z * f));
  cam.x = x - ((x - cam.x) * z) / cam.z;
  cam.y = y - ((y - cam.y) * z) / cam.z;
  cam.z = z;
  saveCam();
  const world = document.querySelector(".world");
  if (world) world.style.transform = `translate(${cam.x}px, ${cam.y}px) scale(${cam.z})`;
}

function blockLibrary(e) {
  const blocks = [
    ["receive", t("Receive: your addresses", "Получить: твои адреса")],
    ["addresses", t("Address book", "Адресная книга")],
    ["history", t("History", "История")],
    ["favorites", t("Favorite tokens", "Избранные токены")],
    ["alerts", t("Price alerts", "Ценовые оповещения")],
    ["schedule", t("Schedule", "Расписание")],
    ["positions", t("Open positions", "Открытые позиции")],
    ["multisig", t("Multisig", "Мультисиг")],
  ];
  const r = e.currentTarget.getBoundingClientRect();
  popup({ clientX: r.right - 260, clientY: r.bottom + 6 }, blocks.map(([kind, label]) => [label, () => act("addBoard", { name: kind })]));
}

// MARK: The portfolio frame

function portfolioFrame() {
  const p = state.portfolio;
  const total = Number(p.total) || 0;
  const shares = p.networks.filter((n) => Number(n.usd) > 0);
  return h("div", { class: "frame portfolio" },
    h("div", { class: "eyebrow" }, t("Portfolio", "Портфель")),
    h("div", { class: "total" }, usd(p.total)),
    h("div", { class: "muted small" }, p.updatedAt ? t("Updated ", "Обновлено ") + new Date(p.updatedAt).toLocaleTimeString() : t("Reading the networks…", "Читаю сети…"),
      " · ", h("a", { href: "#", onclick: (e) => { e.preventDefault(); act("refresh"); } }, t("Refresh", "Обновить"))),
    shares.length ? h("div", { class: "shares" }, donut(shares, total),
      h("div", { class: "share-list" }, shares.map((n) => h("div", { class: "share" },
        h("span", { class: "swatch", style: `background:${networkColor[n.key] || "#888"}` }), h("span", {}, n.name),
        h("span", { class: "num muted" }, total ? Math.round((Number(n.usd) / total) * 100) + "%" : ""))))) : null,
    h("div", { class: "mini" },
      h("div", { class: "eyebrow row-head" }, t("Schedule", "Расписание"),
        h("button", { class: "link-btn", onclick: () => act("addBoard", { name: "schedule" }) }, t("All ›", "Все ›"))),
      state.tasks.length ? state.tasks.slice(0, 3).map((x) => h("div", { class: "small" }, x.text)) : h("div", { class: "muted small" }, t("Nothing scheduled yet", "Пока ничего не запланировано"))),
    p.networks.length ? p.networks.map((n) => h("div", { class: "net" },
      h("div", { class: "row" }, h("b", {}, n.name), h("b", { class: "num" }, usd(n.usd))),
      n.rows.map((r) => h("div", { class: "row" },
        h("span", { class: "sym" }, r.icon ? h("img", { src: r.icon, alt: "" }) : null, h("span", {}, r.symbol, h("span", { class: "muted small sub" }, r.amount))),
        h("span", { class: "num" }, r.usd !== undefined && r.usd !== null ? usd(r.usd) : "",
          r.pnl ? h("span", { class: "small sub " + (r.gain < 0 ? "loss" : "gain") }, r.pnl) : null)))))
      : h("div", { class: "empty" }, t("Nothing here yet. Add the Receive block to see where to send money.", "Пока пусто. Добавь блок «Получить», чтобы увидеть, куда отправить деньги.")),
    p.errors.length ? h("p", { class: "muted small" }, t("Didn't answer: ", "Не ответили: ") + p.errors.join("; ")) : null,
  );
}

function donut(shares, total) {
  const r = 26, len = 2 * Math.PI * r;
  let at = 0;
  const g = svg("svg", { viewBox: "0 0 64 64", class: "donut" });
  for (const n of shares) {
    const part = total ? (Number(n.usd) / total) * len : 0;
    g.append(svg("circle", { cx: "32", cy: "32", r: String(r), fill: "none", stroke: networkColor[n.key] || "#888", "stroke-width": "9",
      "stroke-dasharray": `${Math.max(part - 1, 0.5)} ${len}`, "stroke-dashoffset": String(-at), transform: "rotate(-90 32 32)" }));
    at += part;
  }
  return g;
}

// MARK: Blocks

/** Blocks already drawn: only a new one fades in, so a redraw never blinks the canvas. */
const seen = new Set();
function board(b) {
  const el = drawBoard(b);
  if (!seen.has(b.id)) { seen.add(b.id); if (seenReady) el.classList.add("fresh"); }
  return el;
}
let seenReady = false;
function drawBoard(b) {
  const live = ["receive", "addresses", "history", "favorites", "alerts", "schedule", "positions", "multisig"];
  if (b.kind === "move") return moveBoard(b);
  const body = live.includes(b.kind) ? liveBoard(b.kind) : b.kind === "chart" ? chartBody(b) : resultBody(b);
  return frameBoard(b, live.includes(b.kind) ? liveTitle(b.kind) : b.title, body);
}

/** A block's frame: its title, the close button, and the right-click menu that brings it into the chat. */
function frameBoard(b, title, body, extra) {
  return h("div", { class: "frame board" + (attached === b.id ? " attached" : ""), oncontextmenu: (e) => blockMenu(e, b, title) },
    h("div", { class: "board-head" }, h("b", {}, title), extra || null,
      h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => act("closeBoard", { board: b.id }) }, icon("close"))),
    body);
}

function blockMenu(e, b, title) {
  e.preventDefault();
  popup(e, [
    [t("Ask or act in chat", "Спросить или сделать в чате"), () => { attached = b.id; render(); document.getElementById("composer")?.focus(); }],
    [t("What's going on here?", "Что здесь происходит?"), () => act("send", { text: t(`What's going on in «${title}»?`, `Что происходит в «${title}»?`), board: b.id })],
  ]);
}

function liveTitle(kind) {
  return {
    receive: t("Receive", "Получить"), addresses: t("Address book", "Адресная книга"), history: t("History", "История"),
    favorites: t("Favorites", "Избранное"), alerts: t("Price alerts", "Ценовые оповещения"), schedule: t("Schedule", "Расписание"),
    positions: t("Open positions", "Открытые позиции"), multisig: t("Multisig", "Мультисиг"),
  }[kind];
}

/** A command a block offers goes into the chat field with its amount selected; the person sends it. */
function suggest(text) {
  composerText = text;
  render();
  const f = document.getElementById("composer");
  if (!f) return;
  f.focus();
  const m = text.match(/\d+(?:\.\d+)?/);
  if (m) f.setSelectionRange(m.index, m.index + m[0].length);
  autosize(f);
}

function resultBody(b) {
  return h("div", {},
    h("ul", { class: "lines" }, b.lines.map((l) => h("li", {}, l))),
    b.actions && b.actions.length ? h("div", { class: "btns" }, b.actions.map((a) => h("button", { class: "btn", onclick: () => suggest(a.command) }, a.title))) : null);
}

function chartBody(b) {
  return b.chart ? sparkline(b.chart, true) : resultBody(b);
}

/** A market's closes as a line, drawn from numbers only. */
function sparkline(c, big) {
  const w = 520, hgt = big ? 180 : 120, pts = c.closes;
  const min = Math.min(...pts), max = Math.max(...pts), span = max - min || 1;
  const line = svg("polyline", { points: pts.map((p, i) => `${(i / (pts.length - 1 || 1)) * w},${hgt - 6 - ((p - min) / span) * (hgt - 12)}`).join(" "), class: c.change.startsWith("-") ? "down" : "up" });
  const s = svg("svg", { viewBox: `0 0 ${w} ${hgt}`, class: "spark" + (big ? " big" : "") }, line);
  return h("div", { class: "chart" }, s, h("div", { class: "muted small" }, `${c.title} · ${t("low", "мин")} ${c.low} · ${t("high", "макс")} ${c.high} · ${c.change}`));
}

// A move: its route as step cards, the state on top, Confirm and Cancel while it waits.

function moveBoard(b) {
  const waitingHere = state.card && state.card.id === b.id;
  const runningHere = state.runningBoard === b.id;
  const finished = !waitingHere && !runningHere;
  const chip = { waiting: t("waiting for Confirm", "ждёт «Подтвердить»"), running: state.signer.waiting ? "Windows Hello" : t("running", "выполняется"),
    done: t("done", "готово"), failed: t("failed", "не вышло"), cancelled: t("not signed", "не подписано") }[b.state] || b.state;
  if (finished && !opened.has(b.id)) {
    return h("button", { class: "pill " + (b.state || ""), onclick: () => { opened.add(b.id); render(); }, oncontextmenu: (e) => blockMenu(e, b, b.title) },
      h("span", { class: "chip " + (b.state || "") }, chip), h("span", { class: "pill-title" }, b.title));
  }
  const steps = b.steps || [];
  const cards = [];
  steps.forEach((s, i) => {
    if (i) cards.push(h("div", { class: "arrow" }, "→"));
    cards.push(stepCard(s, b, i));
  });
  return h("div", { class: "route" + (runningHere ? " running" : "") + (attached === b.id ? " attached" : ""), oncontextmenu: (e) => blockMenu(e, b, b.title) },
    h("div", { class: "route-head" },
      h("b", {}, b.title), h("span", { class: "chip " + (b.state || "") }, chip),
      h("span", { class: "spacer" }),
      waitingHere ? [
        h("button", { class: "btn", onclick: () => act("cancel") }, t("Cancel", "Отмена")),
        h("button", { class: "btn primary", disabled: state.thinking || !!state.runningBoard, onclick: () => act("confirm") }, icon("check"), state.card.confirm),
      ] : finished ? [
        h("button", { class: "icon-btn", title: t("Fold", "Свернуть"), onclick: () => { opened.delete(b.id); render(); } }, icon("shrink")),
        h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => act("closeBoard", { board: b.id }) }, icon("close")),
      ] : null),
    h("div", { class: "steps scroll-own" }, cards),
    waitingHere && state.multisigDraft ? multisigSetup(state.multisigDraft) : null,
    waitingHere && state.card.warnings.length ? h("ul", { class: "lines warn" }, state.card.warnings.map((w) => h("li", {}, w))) : null,
    b.lines.filter((l) => !l.startsWith("⚠ ")).length ? h("ul", { class: "lines" }, b.lines.filter((l) => !l.startsWith("⚠ ")).map((l) => h("li", {}, l))) : null,
    runningHere && state.signer.waiting ? h("div", { class: "muted" }, t("Windows Hello is asking for you…", "Windows Hello спрашивает тебя…")) : null);
}

function stepCard(s, b, i) {
  const key = b.id + ":" + i;
  const open = opened.has(key);
  const state0 = b.state === "waiting" ? t("Ready", "Готово к подписи") : b.state === "running" ? (state.signer.waiting ? "Windows Hello" : t("Now", "Сейчас"))
    : b.state === "done" ? t("Done", "Готово") : b.state === "failed" ? t("Failed", "Не вышло") : t("Not signed", "Не подписано");
  const [verb, place] = s.label.split(" · ");
  return h("div", { class: "step" + (open ? " open" : "") },
    h("div", { class: "step-pic", style: `background:${colorFor(place || s.label)}` },
      h("span", { class: "chip light" }, state0), h("img", { src: "coin.png", alt: "" })),
    h("div", { class: "step-body" },
      h("div", { class: "step-label" }, verb, place ? h("span", { class: "muted" }, " · " + place) : null),
      h("div", { class: "step-title" }, s.title),
      s.money ? h("div", { class: "money" }, h("div", { class: "eyebrow" }, t("Money · now → after", "Деньги · сейчас → после")),
        h("div", { class: "money-row" }, h("span", { class: "muted" }, s.money.place), h("span", { class: "num" }, s.money.was, " → ", h("b", {}, s.money.after), " ", h("span", { class: "muted" }, s.money.symbol)))) : null,
      open ? h("ul", { class: "lines scroll-own" }, s.lines.map((l) => h("li", {}, l))) : s.lines[0] ? h("div", { class: "muted small one" }, s.lines[0]) : null,
      s.lines.length ? h("button", { class: "link-btn details", onclick: () => { open ? opened.delete(key) : opened.add(key); render(); } }, open ? t("Close ×", "Закрыть ×") : t("Details ›", "Подробнее ›")) : null));
}

const multisigNetworks = ["base", "arbitrum", "optimism", "ethereum", "polygon", "bnb", "hyperevm", "robinhood", "tempo"];

/** The set-up card's choices: the keys, how many must sign, where — each changes here before Confirm. */
function multisigSetup(d) {
  const n = d.keys.length, m = d.threshold;
  return h("div", { class: "msig" },
    h("div", { class: "eyebrow" }, t(`Keys · one on each device · ${n}`, `Ключи · по одному на устройство · ${n}`)),
    d.keys.map((k, i) => h("div", { class: "row" },
      h("span", {}, h("b", {}, k.name), h("span", { class: "muted small" }, " · " + (k.kind === "pc" ? "Windows Hello" : t("signs in Safe's app", "подписывает в приложении Safe"))),
        h("div", { class: "address small" }, k.address)),
      h("span", { class: "btns" }, h("span", { class: "chip " + (i < m ? "done" : "") }, i < m ? t("signs", "подписывает") : t("spare", "запасной")),
        k.kind === "pc" || n <= 1 ? null : h("button", { class: "icon-btn", title: t("Remove this key", "Убрать ключ"), onclick: () => act("multisigKey", { wallet: k.address }) }, icon("close"))))),
    h("div", { class: "btns" },
      state.candidates.map((c) => h("button", { class: "btn dashed", onclick: () => act("multisigKey", { address: c.address }) }, icon("plus"), c.name)),
      h("button", { class: "btn", onclick: () => suggest("save 0x… as Phone on base") }, state.candidates.length ? t("Another address…", "Другой адрес…") : t("Save a phone's or a person's address first", "Сначала сохрани адрес телефона или человека"))),
    h("div", { class: "eyebrow" }, t("How many keys move money", "Сколько ключей двигают деньги")),
    h("div", { class: "stepper" },
      h("button", { class: "btn", disabled: m <= 1, onclick: () => act("multisigThreshold", { threshold: m - 1 }) }, icon("minus")),
      h("span", { class: "big-num" }, String(m), h("span", { class: "muted" }, t(` of ${n}`, ` из ${n}`))),
      h("button", { class: "btn", disabled: m >= n, onclick: () => act("multisigThreshold", { threshold: m + 1 }) }, icon("plus"))),
    h("div", { class: "eyebrow" }, t("Networks · the same address on each", "Сети · один адрес во всех")),
    h("div", { class: "btns" }, multisigNetworks.map((net) => h("button", { class: "btn" + (d.networks.includes(net) ? " on" : ""), onclick: () => act("multisigNetwork", { network: net }) },
      d.networks.includes(net) ? icon("check") : null, net))),
    h("p", { class: "muted small" }, t("Checked before it's made: no two keys from one phrase, one key at most on this computer, every address saved with Windows Hello and not a contract.",
      "Проверяется до создания: нет двух ключей из одной фразы, не больше одного ключа на этом компьютере, каждый адрес сохранён через Windows Hello и это не контракт.")));
}

// Live blocks: what they show is read from the state each time.

const networks = ["solana", "base", "arbitrum", "ethereum", "polygon", "optimism", "bnb", "hyperliquid", "hyperevm", "robinhood", "tempo", "near",
  "bitcoin", "tron", "xrp", "dogecoin", "zcash", "sui"];
const addressForm = { name: "", network: "solana", address: "", memo: "" };

function liveBoard(kind) {
  switch (kind) {
    case "receive": return receiveBody();
    case "addresses": return addressesBody();
    case "history": return historyBody();
    case "favorites":
      return state.favorites.length ? h("div", {}, state.favorites.map((f) => h("div", { class: "row" },
        h("span", { class: "sym" }, f.image ? h("img", { src: f.image, alt: "" }) : null, f.symbol, h("span", { class: "muted small" }, " " + f.chain)),
        h("span", { class: "num" }, f.price !== undefined && f.price !== null ? "$" + Number(f.price).toPrecision(4) : "—",
          f.change24h !== undefined && f.change24h !== null ? h("span", { class: "small " + (f.change24h < 0 ? "loss" : "gain") }, `  ${f.change24h >= 0 ? "+" : ""}${Number(f.change24h).toFixed(1)}%`) : null,
          h("button", { class: "icon-btn", title: t("Remove", "Убрать"), onclick: () => act("removeFavorite", { wallet: f.chain + ":" + f.address.toLowerCase() }) }, icon("close"))))))
        : h("div", { class: "empty" }, t("No favorites yet: “add 0x… to favorites”.", "Избранного пока нет: «добавь 0x… в избранное»."));
    case "alerts":
      return state.alerts.length ? h("div", {}, state.alerts.map((a) => h("div", { class: "row" }, h("span", {}, a.text),
        h("button", { class: "icon-btn", title: t("Remove", "Убрать"), onclick: () => act("removeAlert", { wallet: a.id }) }, icon("close")))))
        : h("div", { class: "empty" }, t("No alerts: “tell me when BTC is above 120000”.", "Оповещений нет: «скажи, когда BTC выше 120000»."));
    case "schedule":
      return state.tasks.length ? h("div", {}, state.tasks.map((a) => h("div", { class: "row" }, h("span", {}, a.text),
        h("button", { class: "icon-btn", title: t("Remove", "Убрать"), onclick: () => act("removeTask", { wallet: a.id }) }, icon("close")))))
        : h("div", { class: "empty" }, t("Nothing scheduled: “every day at 9: buy SOL for 5 USDC”.", "Ничего не запланировано: «каждый день в 9: купи SOL на 5 USDC»."));
    case "positions":
      return state.positions.length ? h("div", {}, state.positions.map((x) => h("div", { class: "row" },
        h("span", { class: "sym " + (x.side === "long" ? "gain" : "loss") }, `${x.side} ${x.coin}`), h("span", { class: "num" }, `${x.size} @ ${x.entry} · ${x.pnl}`))))
        : h("div", { class: "empty" }, t("No open positions on Hyperliquid.", "Открытых позиций на Hyperliquid нет."));
    case "multisig":
      return state.multisigs.length ? h("div", {}, state.multisigs.map((m) => h("div", { class: "msig-item" },
        h("div", { class: "eyebrow" }, t(`Any ${m.threshold} of ${m.keys.length} keys · `, `Любые ${m.threshold} из ${m.keys.length} ключей · `) + m.networks.join(", ")),
        h("div", { class: "address" }, m.address), copyButton(m.address),
        m.keys.map((k) => h("div", { class: "row small" }, h("span", {}, k.name), h("span", { class: "muted" }, k.kind === "pc" ? "Windows Hello" : t("Safe's app", "приложение Safe")))),
        h("div", { class: "btns" },
          h("button", { class: "btn", onclick: () => suggest(`send 10 USDC on ${m.networks[0]} to Multisig`) }, t("Put money in", "Положить деньги")),
          h("button", { class: "btn", onclick: () => suggest(`send 1 USDC from the multisig to Alex on ${m.networks[0]}`) }, t("Pay from it", "Заплатить из него"))))))
        : h("div", {}, h("p", { class: "muted" }, t("No multisig yet. Its money moves only when enough of its keys sign: this computer's and your phone's or a person's.",
          "Мультисига пока нет. Его деньги двигаются, только когда подпишет достаточно ключей: этого компьютера и твоего телефона или другого человека.")),
          h("button", { class: "btn", onclick: () => suggest("make a multisig 2 of 2 with Phone on base") }, t("Make a multisig", "Сделать мультисиг")));
    default: return null;
  }
}

function receiveBody() {
  const a = state.addresses;
  const rows = [
    ["Solana", a.solana, t("USDC and SOL (recommended), and any Solana token", "USDC и SOL (рекомендую) и любые токены Solana")],
    ["Base · Arbitrum · Ethereum · Polygon · Optimism · BNB · Hyperliquid", a.evm, t("The same address on every EVM network: send on the network you mean", "Один адрес во всех EVM-сетях: отправляй в той сети, которую имеешь в виду")],
    ["NEAR", a.near, "NEAR"],
  ].filter((r) => r[1]);
  return h("div", {},
    h("p", { class: "muted small" }, t("ihatecrypto can't pull money in: send it here from an exchange or another wallet, on that network.",
      "ihatecrypto не может сам забрать деньги: отправь их сюда с биржи или другого кошелька, именно в этой сети.")),
    rows.map(([name, address, note]) => h("div", { class: "addr-block" },
      h("b", {}, name), h("div", { class: "muted small" }, note), h("div", { class: "address" }, address), copyButton(address))));
}

function addressesBody() {
  const form = addressForm;
  return h("div", {},
    h("p", { class: "muted small" }, t("Money goes only to saved addresses. Saving one asks Windows Hello with the whole address on its window: check every character.",
      "Деньги уходят только на сохранённые адреса. Сохранение спрашивает Windows Hello, и в его окне весь адрес — сверь каждый символ.")),
    state.book.length ? state.book.map((e) => h("div", { class: "row" },
      h("div", {}, h("b", {}, e.name), h("span", { class: "muted small" }, "  " + e.network), h("div", { class: "address small" }, e.address), e.memo ? h("div", { class: "muted small" }, "memo " + e.memo) : null),
      h("button", { class: "icon-btn", title: t("Remove", "Удалить"), onclick: () => act("removeAddress", { name: e.name, network: e.network }) }, icon("close"))))
      : h("div", { class: "empty" }, t("No saved addresses yet.", "Сохранённых адресов пока нет.")),
    h("div", { class: "eyebrow" }, t("Add an address", "Добавить адрес")),
    h("div", { class: "field" }, h("label", {}, t("Name", "Имя")), h("input", { id: "book-name", value: form.name, oninput: (e) => (form.name = e.target.value), placeholder: "Bybit" })),
    h("div", { class: "field" }, h("label", {}, t("Network", "Сеть")),
      h("select", { id: "book-network", onchange: (e) => (form.network = e.target.value) }, networks.map((n) => h("option", { value: n, selected: n === form.network }, n)))),
    h("div", { class: "field" }, h("label", {}, t("Address", "Адрес")), h("input", { id: "book-address", class: "address", value: form.address, oninput: (e) => (form.address = e.target.value.trim()) })),
    h("div", { class: "field" }, h("label", {}, t("Memo / tag (exchanges)", "Memo / тег (для бирж)")), h("input", { id: "book-memo", value: form.memo, oninput: (e) => (form.memo = e.target.value.trim()) })),
    h("button", { class: "btn primary", onclick: async () => {
      if (await act("saveAddress", { ...form }) !== null) {
        Object.assign(addressForm, { name: "", address: "", memo: "" });
        toast(t("Saved.", "Сохранено."));
        render();
      }
    } }, t("Save with Windows Hello", "Сохранить через Windows Hello")));
}

function historyBody() {
  return state.history.length ? h("div", { class: "scroll-own history" }, state.history.map((e) => h("div", { class: "row" },
    h("div", {}, h("b", {}, e.note || e.chain), h("div", { class: "muted small" }, `${new Date(e.at).toLocaleString()} · ${e.chain} · ${e.kind}`)),
    e.link ? link(e.link, t("Explorer", "Обозреватель")) : null)))
    : h("div", { class: "empty" }, t("Nothing signed yet.", "Пока ничего не подписано."));
}

// MARK: The chat, floating over the canvas

function chatPanel() {
  const s = state;
  const last = s.chat.length - 1;
  const items = s.chat.map((m, i) => h("div", { class: "msg " + (m.fromUser ? "user" : "app") },
    m.context ? h("div", { class: "context", title: m.context }, "📎 " + m.context.slice(0, 90) + (m.context.length > 90 ? "…" : "")) : null,
    h("div", { class: "bubble" }, m.text),
    m.chart ? sparkline(m.chart) : null,
    m.note ? h("span", { class: "note" }, m.note) : null,
    // Ways to go on live in the chat, under the reply that offered them.
    i === last && !m.fromUser && s.options.length && !s.thinking ? optionCards() : null));
  if (s.thinking) items.push(h("div", { class: "thinking" }, s.draft || h("span", { class: "dots" }, t("Reading", "Читаю"))));
  if (s.options.length && !s.thinking && (last < 0 || s.chat[last].fromUser)) items.push(optionCards());
  if (!s.chat.length && !s.thinking) items.push(h("div", { class: "empty" }, t("Tell me what to do. What I make goes on the canvas.", "Скажи, что сделать. Что я сделаю, появится на холсте.")));
  const field = h("textarea", {
    id: "composer", rows: "1", placeholder: t("Message", "Сообщение"),
    oninput: (e) => { composerText = e.target.value; autosize(e.target); },
    onkeydown: (e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendComposer(); } },
  });
  field.value = composerText;
  setTimeout(() => autosize(field), 0);
  const status = s.thinking ? t("Reading…", "Читаю…") : s.runningBoard ? (s.signer.waiting ? t("Windows Hello is asking for you", "Windows Hello спрашивает тебя") : t("Running the move", "Выполняю"))
    : s.card ? t("Waiting for your Confirm", "Ждёт твоего «Подтвердить»") : s.model.connected ? `${s.model.name} · ${s.model.model || ""}` : t("No model: plain commands only", "Без модели: только простые команды");
  const pointed = attached && s.boards.find((b) => b.id === attached);
  if (attached && !pointed) attached = null;
  return h("div", { class: "chatpanel" + (chatFull ? " full" : "") },
    h("div", { class: "chat-head", ondblclick: toggleChat },
      h("img", { src: "coin.png", alt: "" }),
      h("div", {}, h("b", {}, t("Assistant", "Ассистент")), h("div", { class: "muted small" }, status)),
      h("span", { class: "spacer" }),
      h("button", { class: "icon-btn", title: t("New canvas", "Новый холст"), onclick: () => act("newCanvas") }, icon("plus")),
      h("button", { class: "icon-btn", title: chatFull ? t("Smaller", "Меньше") : t("Bigger", "Больше"), onclick: toggleChat }, icon(chatFull ? "shrink" : "expand"))),
    h("div", { class: "chat" }, h("div", { class: "chat-inner" }, items)),
    h("div", { class: "composer" + (s.card ? " waiting" : "") },
      s.card ? h("div", { class: "waiting-bar" }, h("span", { class: "pill-title" }, s.card.title),
        h("button", { class: "btn", onclick: () => act("cancel") }, t("Cancel", "Отмена")),
        h("button", { class: "btn primary", disabled: s.thinking || !!s.runningBoard, onclick: () => act("confirm") }, icon("check"), s.card.confirm)) : null,
      pointed ? h("div", { class: "attach" }, "📎 " + (pointed.kind === "move" || pointed.kind === "result" || pointed.kind === "chart" ? pointed.title : liveTitle(pointed.kind)),
        h("button", { class: "icon-btn", onclick: () => { attached = null; render(); } }, icon("close"))) : null,
      h("div", { class: "composer-inner" }, field, h("button", { class: "btn primary", disabled: s.thinking, onclick: sendComposer }, t("Send", "Отправить"))),
      h("div", { class: "model-line" }, s.model.connected ? `${s.model.name} · ${s.model.model || ""}` : t("No model connected: Settings › Model.", "Модель не подключена: Настройки › Модель."))));
}

function optionCards() {
  return h("div", { class: "options" }, state.options.map((o, i) =>
    h("button", { class: "option" + (o.recommended ? " rec" : ""), onclick: () => act("pick", { index: i }) }, h("b", {}, o.title), o.comment ? h("span", {}, o.comment) : null)));
}

function toggleChat() { chatFull = !chatFull; store("chatFull", chatFull); render(); }

function autosize(el) { el.style.height = "auto"; el.style.height = Math.min(el.scrollHeight, 160) + "px"; }

function sendComposer() {
  const text = composerText.trim();
  if (!text || state.thinking) return;
  composerText = "";
  const board = attached || undefined;
  attached = null;
  act("send", { text, board });
  render();
}

// MARK: Settings

const modelForm = { provider: null, key: "", model: null };

function settings() {
  const s = state;
  const choice = modelForm;
  if (choice.provider === null) choice.provider = s.model.provider || "anthropic";
  if (choice.model === null) choice.model = s.model.model || "";
  const providers = s.providers.filter((p) => !p.unavailable);
  const chosen = () => s.providers.find((p) => p.id === choice.provider);
  const sessions = [[60, t("One move (1 min)", "Один перевод (1 мин)")], [300, "5 min"], [900, "15 min"], [1800, "30 min"]];
  return h("div", { class: "page" }, h("div", { class: "narrow" },
    h("button", { class: "link-btn back", onclick: () => { view = "canvas"; render(); } }, icon("back"), t("Back to the canvas", "Назад к холсту")),
    h("h1", {}, t("Settings", "Настройки")),

    h("h2", {}, t("Model", "Модель")),
    h("div", { class: "card" },
      h("p", { class: "muted small" }, t("The chat's model reads what you write. It never signs: a move is a card, and only Confirm runs it.",
        "Модель чата читает, что ты пишешь. Она никогда не подписывает: действие — это карточка, и запускает её только «Подтвердить».")),
      h("div", { class: "field" }, h("label", {}, t("Provider", "Провайдер")),
        h("select", { id: "model-provider", onchange: (e) => { choice.provider = e.target.value; } }, providers.map((p) =>
          h("option", { value: p.id, selected: p.id === choice.provider }, p.name + (p.connected ? " ✓" : ""))))),
      h("div", { class: "field" }, h("label", {}, t("API key (empty: a login another app already has, or a local model)", "API-ключ (пусто — вход, который уже есть у другой программы, или локальная модель)")),
        h("input", { id: "model-key", type: "password", autocomplete: "off", value: choice.key, oninput: (e) => (choice.key = e.target.value) })),
      h("div", { class: "field" }, h("label", {}, t("Model id (empty: the provider's default)", "Модель (пусто — по умолчанию у провайдера)")),
        h("input", { id: "model-id", value: choice.model, oninput: (e) => (choice.model = e.target.value.trim()) })),
      h("div", { class: "btns" },
        h("button", { class: "btn primary", onclick: async () => {
          const r = await act("connectModel", { provider: choice.provider, key: choice.key || undefined, model: choice.model || undefined });
          choice.key = "";
          if (r !== null) toast(t("Connected.", "Подключено."));
          render();
        } }, t("Connect", "Подключить")),
        h("button", { class: "btn", onclick: async () => {
          const p = chosen();
          if (!p || !p.signIn) { toast(t("This provider connects with a key.", "Этот провайдер подключается ключом.")); return; }
          toast(t("Finish the sign-in in the browser.", "Закончи вход в браузере."));
          if (await act("signIn", { provider: p.id }) !== null) toast(t("Signed in.", "Вход выполнен."));
        } }, t("Sign in with the browser", "Войти через браузер")),
        s.model.connected ? h("button", { class: "btn", onclick: () => act("disconnectModel", { provider: s.model.provider }) }, t("Disconnect", "Отключить")) : null)),

    h("h2", {}, t("Language", "Язык")),
    h("div", { class: "card" }, h("select", { onchange: (e) => act("language", { language: e.target.value }) },
      [["auto", t("As I write", "Как я пишу")], ["en", "English"], ["ru", "Русский"]].map(([v, l]) => h("option", { value: v, selected: s.language === v }, l)))),

    h("h2", {}, t("Skills", "Навыки")),
    h("div", { class: "card" },
      h("p", { class: "muted small" }, t("The assistant sees each switched-on skill's name and what it's for, and reads its whole text when your message is that work. A skill changes how it works with you, never the risk rules, the checks or your Confirm.",
        "Ассистент видит имя каждого включённого навыка и для чего он, а весь текст читает, когда сообщение про эту работу. Навык меняет то, как он работает с тобой, но не риск-правила, проверки и «Подтвердить».")),
      s.skills.map((k) => h("div", { class: "row" },
        h("label", { class: "check-row" }, h("input", { type: "checkbox", id: "skill-" + k.id, checked: k.on, onchange: (e) => act("skillOn", { wallet: k.id, test: e.target.checked }) }),
          h("span", {}, h("b", {}, k.name), k.builtIn ? h("span", { class: "muted small" }, t(" · built in", " · встроенный")) : null, h("div", { class: "muted small" }, k.about))),
        k.builtIn ? null : h("button", { class: "btn", onclick: () => act("removeSkill", { wallet: k.id }) }, t("Remove", "Удалить")))),
      h("label", { class: "btn", for: "skill-file" }, t("Add a skill (.md or .txt)…", "Добавить навык (.md или .txt)…")),
      h("input", { id: "skill-file", type: "file", accept: ".md,.txt,text/plain,text/markdown", hidden: true, onchange: async (e) => {
        for (const f of e.target.files) { await act("addSkill", { text: await f.text(), name: f.name }); }
        e.target.value = "";
      } })),

    s.memory.length ? [h("h2", {}, t("What the chat remembers", "Что помнит чат")), h("div", { class: "card" }, s.memory.map((m) =>
      h("div", { class: "row" }, h("span", {}, m), h("button", { class: "btn", onclick: () => act("forget", { text: m }) }, t("Forget", "Забыть")))))] : null,

    h("h2", {}, t("Second check", "Вторая проверка")),
    h("div", { class: "card" },
      h("label", { class: "check-row" }, h("input", { type: "checkbox", id: "second-check", checked: s.secondCheck, onchange: (e) => act("secondCheck", { test: e.target.checked }) }),
        h("span", {}, t("The model reads every command and every trade plan again before a card is drawn. On a doubt nothing runs: you pick.",
          "Модель перечитывает каждую команду и каждый план сделки, прежде чем нарисовать карточку. При сомнении ничего не запускается — выбираешь ты."))),
      h("label", { class: "check-row" }, h("input", { type: "checkbox", id: "jev-openrouter", checked: s.jevOpenRouter, disabled: !s.secondCheck, onchange: (e) => act("jevOpenRouter", { test: e.target.checked }) }),
        h("span", {}, t("Also ask TypeSafe Jev through OpenRouter (your OpenRouter key; the message goes there).", "Также спрашивать TypeSafe Jev через OpenRouter (твой ключ OpenRouter; сообщение уходит туда).")))),

    h("h2", {}, t("Signing", "Подпись")),
    h("div", { class: "card" },
      s.signer.notes.map((n) => h("p", { class: "small" }, n)),
      s.signer.error ? h("p", { class: "small", style: "color:var(--danger)" }, s.signer.error) : null,
      h("div", { class: "field" }, h("label", {}, t("One Windows Hello covers", "Одно подтверждение Windows Hello действует")),
        h("select", { onchange: (e) => act("sessionLength", { seconds: Number(e.target.value) }) },
          sessions.map(([v, l]) => h("option", { value: v, selected: s.signer.sessionLength === v }, l)))),
      h("button", { class: "btn", onclick: () => act("lock") }, t("Ask Windows Hello again now (and pause the trading key)", "Снова спрашивать Windows Hello (и поставить торговый ключ на паузу)"))),

    h("h2", {}, t("Wallets", "Кошельки")),
    h("div", { class: "card" },
      s.wallets.map((w) => h("div", { class: "row" },
        h("div", {}, h("b", {}, w.label), w.test ? h("span", { class: "muted small" }, t(" · test, signs without Windows Hello", " · тестовый, подписывает без Windows Hello")) : null,
          w.automation ? h("span", { class: "muted small" }, t(" · automation: scheduled tasks sign without Windows Hello, only to your own wallets", " · автоматизация: задачи подписывают без Windows Hello, только на твои кошельки")) : null,
          w.solana ? h("div", { class: "address small" }, "Solana " + w.solana) : null, w.evm ? h("div", { class: "address small" }, "EVM " + w.evm) : null),
        h("div", { class: "btns" },
          w.id === s.active ? h("span", { class: "check" }, "✓") : h("button", { class: "btn", onclick: () => act("switchWallet", { wallet: w.id }) }, t("Switch", "Переключить")),
          h("button", { class: "btn", onclick: () => act("removeWallet", { wallet: w.id }) }, t("Remove", "Удалить"))))),
      h("div", { class: "btns", style: "margin-top:12px" },
        h("button", { class: "btn", onclick: () => act("createWallet", { test: false }) }, t("New wallet", "Новый кошелёк")),
        h("button", { class: "btn", onclick: showImport }, t("Import a phrase", "Импортировать фразу")),
        h("button", { class: "btn", onclick: () => act("createWallet", { test: true }) }, t("New test wallet", "Новый тестовый")))),

    h("h2", {}, t("Updates", "Обновления")),
    h("div", { class: "card" },
      h("p", { class: "muted small" }, t("An update is installed only when its signature matches ihatecrypto's key.", "Обновление ставится, только если его подпись совпадает с ключом ihatecrypto.")),
      h("button", { class: "btn", onclick: checkUpdate }, t("Check for updates", "Проверить обновления"))),

    h("h2", {}, t("Chat", "Чат")),
    h("div", { class: "card" }, h("button", { class: "btn", onclick: () => act("clearChat") }, t("Clear this canvas's chat", "Очистить чат этого холста"))),
  ));
}

async function checkUpdate() {
  try {
    const version = await invoke("update_check");
    if (!version) { toast(t("ihatecrypto is up to date.", "Установлена последняя версия.")); return; }
    toast(t(`Installing ${version}: ihatecrypto restarts by itself.`, `Ставлю ${version}: ihatecrypto перезапустится сам.`));
    await invoke("update_install");
  } catch (e) {
    toast(t("Couldn't check for updates: ", "Не удалось проверить обновления: ") + e);
  }
}

// MARK: First run and the phrase

function welcome() {
  return h("div", { class: "center" }, h("div", { class: "welcome" },
    h("img", { src: "coin.png", alt: "" }),
    h("h1", {}, "ihatecrypto"),
    h("p", { class: "muted" }, t("A wallet you talk to. Keys stay on this computer, under Windows Hello: every move asks for your face, fingerprint or PIN.",
      "Кошелёк, с которым разговаривают. Ключи остаются на этом компьютере под Windows Hello: каждый перевод спрашивает лицо, отпечаток или PIN.")),
    state.signer.error ? h("div", { class: "note-box" }, state.signer.error) : null,
    state.signer.notes.length ? h("p", { class: "muted small" }, state.signer.notes.join(" ")) : null,
    h("div", { class: "btns" },
      h("button", { class: "btn primary", onclick: () => act("createWallet", { test: false }) }, t("Create a wallet", "Создать кошелёк")),
      h("button", { class: "btn", onclick: showImport }, t("I have a recovery phrase", "У меня есть фраза восстановления")),
      h("button", { class: "btn", onclick: showImportKey }, t("Import a private key", "Импортировать приватный ключ"))),
  ));
}

function showImport() { importDialog(t("Recovery phrase", "Фраза восстановления"), t("12 or 24 words, in order", "12 или 24 слова по порядку"), "importPhrase", "phrase"); }
function showImportKey() { importDialog(t("Private key", "Приватный ключ"), t("A Solana key (Phantom) or an EVM key (MetaMask)", "Ключ Solana (Phantom) или EVM (MetaMask)"), "importKey", "key"); }

function importDialog(title, hint, op, field) {
  let value = "";
  const done = () => { value = ""; overlay = null; render(); };
  overlay = h("div", { class: "center" }, h("div", { class: "card", style: "max-width:520px;width:100%" },
    h("h1", { style: "font-size:20px" }, title),
    h("p", { class: "muted small" }, hint, ". ", t("It goes straight to the signer, which keeps it under Windows Hello; nothing else keeps it.",
      "Она уходит прямо подписанту, который хранит её под Windows Hello; больше нигде она не хранится.")),
    h("textarea", { rows: "4", style: "width:100%", autocomplete: "off", spellcheck: "false", oninput: (e) => (value = e.target.value) }),
    h("div", { class: "btns", style: "margin-top:12px" },
      h("button", { class: "btn primary", onclick: async () => { const r = await act(op, { [field]: value }); if (r !== null) done(); } }, t("Import", "Импортировать")),
      h("button", { class: "btn", onclick: done }, t("Back", "Назад")))));
  render();
}

let phraseCopied = false;

function phraseScreen() {
  setProtect(true);
  const words = state.phrase;
  return h("div", { class: "center" }, h("div", { class: "card", style: "max-width:560px;width:100%" },
    h("h1", { style: "font-size:20px" }, t("Your recovery phrase", "Твоя фраза восстановления")),
    h("p", {}, t("Write these 12 words on paper, in order. They are the only way back to this wallet if this computer is lost. ihatecrypto shows them now and never again.",
      "Запиши эти 12 слов на бумагу, по порядку. Это единственный путь к кошельку, если компьютер потеряется. ihatecrypto показывает их сейчас и больше никогда.")),
    h("div", { class: "words" }, words.map((w) => h("div", {}, w))),
    h("div", { class: "note-box small" }, t("Anyone with these words takes the money. Never type them into a site or send them to anyone — not even to ihatecrypto's chat.",
      "Кто знает эти слова, забирает деньги. Никогда не вводи их на сайтах и никому не отправляй — даже в чат ihatecrypto.")),
    h("div", { class: "btns" },
      h("button", { class: "btn", disabled: phraseCopied, onclick: async (e) => {
        phraseCopied = true;
        e.currentTarget.disabled = true;
        try { await invoke("copy_secret", { text: words.join(" ") }); toast(t("Copied once; the clipboard clears in 60 s.", "Скопировано один раз; буфер очистится через 60 с.")); }
        catch { await copy(words.join(" ")); }
      } }, t("Copy all 12 words", "Скопировать все 12 слов")),
      h("button", { class: "btn primary", onclick: async () => { phraseCopied = false; await act("phraseSaved"); setProtect(false); } },
        t("I wrote them down", "Я записал")))));
}

// MARK: Start

engine("hello").catch((e) => toast(e.message));
