// The wallet rail (the Mac's StudioRail): a glass panel floating over the canvas's ground. Wide, it holds the logo, the
// balance with its wallet, Top up and the four rail buttons, the canvases and the model accounts; folded, a strip of
// round glass buttons. The collapse button, the strip's expand button or a drag of the right edge switch the two; a
// window narrower than 1140 folds it, and widening again brings back what it folded.

import { ui, act, render, t, h, svg, icon, addIcons, mascot, logo, usd, signedUsd, masked, store, popup, copyButton } from "./core.js";
// The canvas module draws the rail too; read through the namespace so neither waits on the other to load.
import * as canvas from "./canvas.js";
import { renameDialog, browserDialog } from "./onboarding.js";

addIcons({
  "bubble.left": "M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5H10l-4.5 3.5v-3.5H5A1.5 1.5 0 0 1 3.5 15V7A1.5 1.5 0 0 1 5 5.5z",
  "square.grid.2x2": "M4.5 4.5h6v6h-6zM13.5 4.5h6v6h-6zM4.5 13.5h6v6h-6zM13.5 13.5h6v6h-6z",
  flask: "M9.5 3.5h5M10.5 3.5v6L5 19a1.2 1.2 0 0 0 1 1.5h12a1.2 1.2 0 0 0 1-1.5l-5.5-9.5v-6M7.5 15h9",
  "arrow.triangle.2.circlepath": "M4.5 11a7.5 7.5 0 0 1 13-4.5M19.5 13a7.5 7.5 0 0 1-13 4.5M17.5 3v3.5H14M6.5 21v-3.5H10",
  "person.fill": "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4.5 20.5a7.5 7.5 0 0 1 15 0z",
  "arrow.left.arrow.right": "M8 4 4 8l4 4M4 8h16M16 12l4 4-4 4M20 16H4",
  "rectangle.portrait.and.arrow.right": "M14 4.5H6.5v15H14M10.5 12h10M17 8.5l3.5 3.5-3.5 3.5",
});

/** The folded strip and the wide rail (StudioRail.narrowWidth, wideWidth). */
const NARROW = 92, WIDE = 280;
/** Below this window width the wide rail and the chat leave the canvas too little room (StudioView.crampedWidth). */
const CRAMPED = 1140;

let collapsed = store("railCollapsed") ?? window.innerWidth < CRAMPED;
/** The window got narrow and folded the rail; widening it brings the rail back. */
let foldedForRoom = false;
/** The width the rail was last drawn at: a change slides from it instead of jumping. */
let drawnWidth = null;
/** Set by a fold or an unfold: the next drawing fades the new content in, once. */
let fading = false;

function setCollapsed(on) {
  if (on === collapsed) return;
  collapsed = on;
  fading = true;
  store("railCollapsed", on);
  render();
}

// Ctrl+, opens Settings, as ⌘, does on the Mac.
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.key === "," && ui.state) { e.preventDefault(); ui.view = "settings"; render(); }
});

window.addEventListener("resize", () => {
  const cramped = window.innerWidth < CRAMPED;
  if (cramped && !collapsed) { foldedForRoom = true; setCollapsed(true); }
  else if (!cramped && foldedForRoom) { foldedForRoom = false; setCollapsed(false); }
});

/** From the window's left edge to the rail's right edge: the canvas's boards start past it. */
export const railEdge = () => 10 + (collapsed ? NARROW : WIDE);

export function rail() {
  const width = collapsed ? NARROW : WIDE;
  const from = drawnWidth ?? width;
  drawnWidth = width;
  const glass = h("nav", { class: "rail glass" + (fading ? " fading" : "") }, collapsed ? narrow() : wide());
  fading = false;
  const host = h("div", { class: "rail-host", style: `width:${from}px` }, glass, edge());
  // Drawn at the old width, then moved to the new one: the CSS transition slides it.
  if (from !== width) requestAnimationFrame(() => requestAnimationFrame(() => { host.style.width = width + "px"; }));
  return host;
}

// MARK: Wide

function wide() {
  return h("div", { class: "rail-wide" },
    h("div", { class: "rail-lights" }, h("div", { class: "rail-drag", "data-tauri-drag-region": true }),
      h("button", { class: "icon-btn", title: t("Collapse the sidebar", "Свернуть панель"), onclick: () => setCollapsed(true) }, icon("sidebar.left", 17))),
    h("div", { class: "rail-brand" }, mascot(30), h("span", { class: "rail-wordmark" }, "ihatecrypto")),
    balanceBlock(),
    railActions(false),
    canvasList(),
    profileRow(),
  );
}

/** The balance, with the eye, the refresh and the wallet chip over it; both wallets' split under it. */
function balanceBlock() {
  const s = ui.state;
  const hidden = store("balanceHidden") === true;
  const fresh = h("span", { class: "rail-fresh num" });
  let timer = null;
  const tick = () => { fresh.textContent = refreshing ? t("Updating…", "Обновляю…") : freshness(); };
  const block = h("div", { class: "rail-balance",
    onmouseenter: () => { tick(); block.classList.add("hover"); timer = setInterval(tick, 1000); },
    onmouseleave: () => { block.classList.remove("hover"); clearInterval(timer); } },
    h("div", { class: "rail-balance-top" },
      h("span", { class: "secondary rail-label" }, t("Balance", "Баланс")),
      h("button", { class: "rail-hover-icon", title: hidden ? t("Show the balance", "Показать баланс") : t("Hide the balance", "Скрыть баланс"),
        onclick: () => { store("balanceHidden", !hidden); render(); } }, icon(hidden ? "eye.slash" : "eye", 14)),
      refreshButton(),
      fresh,
      h("span", { class: "spacer" }),
      walletChip(false)),
    balanceAmount("rail-amount", "$•••••", [150, 34]),
    split(),
    positionsLine("rail-positions"),
  );
  if (refreshing) block.classList.add("hover");
  tick();
  return block;
}

let refreshing = false;
async function refresh() {
  if (refreshing) return;
  refreshing = true;
  render();
  await act("refresh");
  refreshing = false;
  render();
}

function refreshButton() {
  return h("button", { class: "rail-hover-icon" + (refreshing ? " spinning" : ""), disabled: refreshing, title: t("Refresh balances", "Обновить балансы"), onclick: refresh },
    icon("arrow.triangle.2.circlepath", 14));
}

/** «Updated 12 s ago»: when the networks were last read. */
function freshness() {
  const at = ui.state.portfolio.updatedAt;
  if (!at) return t("Loading…", "Загружаю…");
  const sec = Math.max(0, Math.round((Date.now() - new Date(at)) / 1000));
  return sec < 60 ? t(`Updated ${sec} s ago`, `Обновлено ${sec} с назад`) : t(`Updated ${Math.floor(sec / 60)} min ago`, `Обновлено ${Math.floor(sec / 60)} мин назад`);
}

/** The wallet's money and the automation wallet's: both are the person's (the Mac's balance counts both). */
function totals() {
  const p = ui.state.portfolio;
  const main = Number(p.total) || 0;
  const auto = Number(ui.state.automationUSD) || 0;
  return { main, auto, all: main + auto, ready: !!p.updatedAt };
}

/** The balance, or a grey block with a light sweep until every network answered (Skeleton). */
function balanceAmount(cls, mask, [w, hgt]) {
  const { all, ready } = totals();
  if (!ready) return h("span", { class: "rail-skeleton", style: `width:${w}px;height:${hgt}px;border-radius:${hgt / 3}px` });
  return h("div", { class: cls + " num" }, masked(usd(all), mask));
}

function split() {
  const { main, auto, ready } = totals();
  if (!ready || auto < 0.01) return null;
  const share = auto / Math.max(main + auto, 0.01);
  return h("button", { class: "rail-split", title: t("The balance is both wallets: Main signs with Windows Hello, Automation signs the Schedule's tasks by itself. Click to open the Schedule.",
    "Баланс — это оба кошелька: основной подписывает через Windows Hello, кошелёк автоматизации сам подписывает задачи расписания. Нажми, чтобы открыть расписание."),
    onclick: () => act("addBoard", { name: "schedule" }) },
    h("div", { class: "rail-split-bar" }, h("span", { class: "main", style: `flex:${Math.max(0.02, 1 - share)}` }), h("span", { class: "auto", style: `flex:${Math.max(0.02, share)}` })),
    h("div", { class: "rail-split-legend num" },
      // The names give way to the amounts when the line is too long (the Mac shrinks it to 0.75).
      h("i", { class: "rail-dot main" }), h("span", { class: "one-line" }, t("Main", "Основной")), h("b", {}, masked(usd(main))),
      h("span", { class: "spacer" }),
      h("i", { class: "rail-dot auto" }), h("span", { class: "one-line" }, t("Automation", "Автоматизация")), h("b", {}, masked(usd(auto)))));
}

function positionsLine(cls) {
  const pnl = ui.state.positionsPnl;
  if (pnl === null || pnl === undefined || !ui.state.positions?.length || store("balanceHidden") === true) return null;
  return h("div", { class: cls + " num " + (pnl < 0 ? "loss" : "profit") }, signedUsd(pnl), cls === "rail-positions" ? t(" in positions", " в позициях") : "");
}

// MARK: The wallet chip and its menu

function activeWallet() {
  const s = ui.state;
  return s.wallets.find((w) => w.id === s.active) || s.wallets.find((w) => !w.automation) || null;
}

function walletChip(compact) {
  const w = activeWallet();
  const watching = !w && ui.state.watching;
  if (!w && !watching) return null;
  const test = w?.test === true;
  const label = watching ? t("Watching", "Просмотр") : w.label;
  return h("button", { class: "rail-wallet-chip" + (compact ? " compact" : "") + (test ? " test" : ""),
    title: watching ? t("These addresses are only watched: nothing signs for them.", "Эти адреса только для просмотра: за них ничего не подписывается.")
      : test ? t(`«${w.label}» signs without Windows Hello — a test wallet. Click to switch or rename.`, `«${w.label}» подписывает без Windows Hello — тестовый кошелёк. Нажми, чтобы переключить или переименовать.`)
      : t(`«${w.label}» signs, with Windows Hello. Click to switch or rename.`, `«${w.label}» подписывает через Windows Hello. Нажми, чтобы переключить или переименовать.`),
    onclick: (e) => { e.stopPropagation(); walletMenu(e.currentTarget, compact); } },
    icon(watching ? "eye" : test ? "flask" : "lock", compact ? 9 : 10),
    h("span", { class: "one-line" }, label),
    compact ? null : icon("chevron.down", 8));
}

/** A panel by a button (the Mac's popovers): Escape, a click outside or a pick closes it. It lives on the body, so the
 * window's redraws leave it where it is. */
function popover(anchor, side, content, cls = "") {
  document.querySelector(".menu")?.remove();
  const panel = h("div", { class: "menu rail-popover " + cls, onclick: (e) => e.stopPropagation() }, content);
  document.body.append(panel);
  const r = anchor.getBoundingClientRect(), w = panel.offsetWidth, hgt = panel.offsetHeight;
  let x, y;
  if (side === "right") { x = r.right + 10; y = Math.min(r.top - 8, window.innerHeight - hgt - 10); }
  else if (side === "top") { x = r.left; y = r.top - hgt - 8; }
  else { x = r.left; y = r.bottom + 8; }
  panel.style.left = Math.max(10, Math.min(x, window.innerWidth - w - 10)) + "px";
  panel.style.top = Math.max(10, Math.min(y, window.innerHeight - hgt - 10)) + "px";
  setTimeout(() => document.addEventListener("click", function off(ev) {
    if (!panel.isConnected) { document.removeEventListener("click", off); return; }
    if (!panel.contains(ev.target)) { panel.remove(); document.removeEventListener("click", off); }
  }), 0);
  return panel;
}
const closePopover = () => document.querySelector(".rail-popover")?.remove();

/** The wallet menu (W04): the wallets on the left; the one picked on the right, its address on every network with
 * Copy and «Switch to…». The automation wallet lives in the Schedule, not here. */
function walletMenu(anchor, compact) {
  let picked = ui.state.active;
  let renaming = false;
  const body = h("div", { class: "rail-wallet-menu" });
  const draw = () => {
    const s = ui.state;
    const wallets = s.wallets.filter((w) => !w.automation);
    const shown = wallets.find((w) => w.id === picked) || wallets[0];
    const list = h("div", { class: "rail-wm-list" },
      h("div", { class: "rail-wm-head" }, h("b", {}, t("Wallets", "Кошельки")), h("span", { class: "spacer" }),
        h("button", { class: "rail-hover-icon", title: t("Manage wallets: create, import, remove", "Управление кошельками: создать, импортировать, удалить"),
          onclick: () => { closePopover(); ui.view = "settings"; render(); } }, icon("gear", 15))),
      wallets.map((w) => h("button", { class: "rail-wm-row" + (shown && w.id === shown.id ? " on" : ""), onclick: () => { picked = w.id; renaming = false; draw(); } },
        h("span", { class: "rail-wm-icon" + (w.test ? " test" : "") }, icon(w.kind === "ledger" ? "bolt" : w.test ? "flask" : "lock", 14)),
        h("span", { class: "rail-wm-text" }, h("b", { class: "one-line" }, w.label), h("span", { class: "secondary one-line" }, listLine(w))),
        w.id === s.active ? h("span", { class: "accent-ink" }, icon("check", 13)) : null)),
      !wallets.length && s.watching ? h("div", { class: "secondary small rail-wm-empty" }, t("Only watching addresses: add a wallet in Settings.", "Только просмотр адресов: добавь кошелёк в настройках.")) : null,
      h("div", { class: "spacer" }),
      h("div", { class: "menu-divider" }),
      s.wallets.some((w) => w.kind === "ledger") ? null : h("button", { class: "rail-wm-action", onclick: () => { closePopover(); act("addBoard", { name: "ledger" }); } }, icon("bolt", 14), t("Connect Ledger…", "Подключить Ledger…")),
      h("button", { class: "rail-wm-action", onclick: () => { closePopover(); ui.view = "settings"; render(); } }, icon("plus", 14), t("New wallet…", "Новый кошелёк…")),
      h("button", { class: "rail-wm-action", onclick: () => { closePopover(); act("addBoard", { name: "multisig" }); } }, icon("lock", 14), t("New multisig…", "Новый мультисиг…")));
    body.replaceChildren(list, shown ? detail(shown) : h("div", { class: "rail-wm-detail" }));
  };
  const detail = (w) => {
    const current = w.id === ui.state.active;
    const how = w.kind === "ledger" ? t("on the device, you approve each move on it", "на устройстве, каждое действие подтверждается на нём")
      : w.test ? t("signs without Windows Hello, keep only a few dollars here", "подписывает без Windows Hello, держи тут лишь несколько долларов")
      : t("signs each move with Windows Hello", "подписывает каждое действие через Windows Hello");
    let name = w.label;
    const title = renaming
      ? h("input", { class: "rail-wm-rename", value: w.label, oninput: (e) => (name = e.target.value),
        onkeydown: async (e) => { if (e.key === "Enter") { renaming = false; await act("renameWallet", { wallet: w.id, label: name }); draw(); } else if (e.key === "Escape") { e.stopPropagation(); renaming = false; draw(); } } })
      : h("div", { class: "rail-wm-title" }, h("b", {}, w.label),
        h("button", { class: "rail-hover-icon", title: t("Rename", "Переименовать"), onclick: () => { renaming = true; draw(); setTimeout(() => body.querySelector(".rail-wm-rename")?.focus(), 0); } }, icon("pencil", 13)));
    const nets = [
      ["Solana", "", "sol", w.solana],
      ["EVM", t("every EVM network", "все EVM-сети"), "eth", w.evm],
      ["Hyperliquid", t("same as EVM", "тот же, что EVM"), "hype", w.evm],
      ["NEAR", "", "near", w.near],
    ];
    return h("div", { class: "rail-wm-detail" },
      h("div", { class: "rail-wm-detail-head" },
        h("div", {}, title, h("div", { class: "secondary rail-wm-how" }, current ? t("Current wallet · ", "Текущий кошелёк · ") + how : how[0].toUpperCase() + how.slice(1))),
        h("span", { class: "spacer" }),
        current ? null : h("button", { class: "btn primary", title: t("Switching asks for Windows Hello.", "Переключение спросит Windows Hello."),
          onclick: () => { closePopover(); act("switchWallet", { wallet: w.id }); } }, t(`Switch to ${w.label}`, `Переключить на «${w.label}»`))),
      nets.map(([n, note, lg, address]) => h("div", { class: "rail-wm-net" + (address ? "" : " missing") },
        h("span", { class: "rail-wm-net-name" }, logo(lg, 20), h("span", {}, h("b", {}, n), note ? h("span", { class: "secondary small" }, note) : null)),
        address ? [h("span", { class: "address rail-wm-address" }, address), copyButton(address)]
          : h("span", { class: "secondary" }, n === "NEAR" && w.kind === "phrase" ? t("Shown after its first NEAR move.", "Появится после первого действия в NEAR.") : t(`No ${n} key in this wallet.`, `В этом кошельке нет ключа ${n}.`)))));
  };
  draw();
  popover(anchor, compact ? "right" : "bottom", body, "rail-wallet-pop");
}

function listLine(w) {
  const n = [w.solana, w.evm, w.evm, w.near].filter(Boolean).length;
  const how = w.kind === "ledger" ? t("On the device", "На устройстве") : w.test ? t("No Windows Hello", "Без Windows Hello") : "Windows Hello";
  return `${how} · ${n} ${t(n === 1 ? "network" : "networks", n === 1 ? "сеть" : n < 5 ? "сети" : "сетей")}`;
}

// MARK: The rail's buttons

/** Top up, Portfolio, History, Favorites, Schedule: glass capsules in a grid, or the strip's round buttons. */
function railActions(compact) {
  const tiles = [
    ["topup", t("Top up", "Пополнить"), "arrow.down.circle.fill", ui.view === "topup", true, t("Your addresses: where to send money into ihatecrypto", "Твои адреса: куда отправить деньги в ihatecrypto"),
      () => { ui.view = ui.view === "topup" ? "canvas" : "topup"; render(); }],
    ["portfolio", t("Portfolio", "Портфель"), "chart.pie.fill", false, false, t("The portfolio on the canvas", "Портфель на холсте"),
      () => { if (ui.view !== "canvas") { ui.view = "canvas"; render(); } canvas.goHome?.(); }],
    ["history", t("History", "История"), "clock.arrow.circlepath", ui.view === "history", false, t("What ihatecrypto did lately", "Что ihatecrypto делал недавно"),
      () => { ui.view = ui.view === "history" ? "canvas" : "history"; render(); }],
    ["favorites", t("Favorites", "Избранное"), "star.fill", hasBoard("favorites"), false, t("Favorite tokens, saved addresses, your wallets", "Избранные токены, сохранённые адреса, твои кошельки"),
      () => toggleBoard("favorites")],
    ["schedule", t("Schedule", "Расписание"), "calendar", hasBoard("schedule"), false, t("What runs on its own, its budget and when it ran", "Что запускается само, его бюджет и когда оно шло"),
      () => toggleBoard("schedule")],
  ];
  if (compact) {
    return h("div", { class: "rail-round-stack" }, tiles.map(([id, title, glyph, on, accent, help, run]) =>
      roundButton(glyph, title, { on, accent, help: `${title}: ${help}`, run })));
  }
  return h("div", { class: "rail-actions" }, tiles.map(([id, title, glyph, on, accent, help, run]) =>
    h("button", { class: "rail-tile" + (accent ? " accent" : "") + (on ? " on" : "") + (id === "topup" ? " wide" : ""), title: help, onclick: run },
      glyph2(glyph, 15), h("span", { class: "one-line" }, title))));
}

function hasBoard(kind) { return ui.state.boards.some((b) => b.kind === kind); }
/** A rail button shows its block on the canvas, or takes it away when it's there (the Mac's toggles). */
function toggleBoard(kind) {
  if (ui.view !== "canvas") { ui.view = "canvas"; render(); }
  const b = ui.state.boards.find((x) => x.kind === kind);
  act(b ? "closeBoard" : "addBoard", b ? { board: b.id } : { name: kind });
}

/** The rail's filled glyphs (SF Symbols' «.fill»), drawn here: the line icons in core are strokes only. */
function glyph2(name, size) {
  const box = (...kids) => {
    const s = svg("svg", { viewBox: "0 0 24 24", width: size, height: size, "aria-hidden": "true", class: "ico" }, ...kids);
    s.style.cssText = `width:${size}px;height:${size}px`;
    return s;
  };
  const line = { fill: "none", stroke: "currentColor", "stroke-width": "2", "stroke-linecap": "round", "stroke-linejoin": "round" };
  switch (name) {
    case "arrow.down.circle.fill":
      return box(svg("circle", { cx: 12, cy: 12, r: 10, fill: "currentColor" }), svg("path", { d: "M12 7v9.5M8 12.5l4 4 4-4", ...line, stroke: "var(--accent)", "stroke-width": "2.4" }));
    case "chart.pie.fill":
      return box(svg("path", { d: "M11 3.1A9 9 0 1 0 20.9 13H11z", fill: "currentColor" }), svg("path", { d: "M13 2.1v8.9h8.9A9 9 0 0 0 13 2.1z", fill: "currentColor" }));
    case "star.fill":
      return box(svg("path", { d: "M12 2.8l2.8 5.8 6.3.9-4.6 4.4 1.1 6.3L12 17.2l-5.6 3 1.1-6.3-4.6-4.4 6.3-.9z", fill: "currentColor", stroke: "currentColor", "stroke-width": "1", "stroke-linejoin": "round" }));
    case "clock.arrow.circlepath":
      return box(svg("path", { d: "M4 12a8 8 0 1 0 2.4-5.7M3.5 4v4h4M12 8v4.5l3 1.8", ...line }));
    case "calendar":
      return box(svg("path", { d: "M4.5 6.5h15v13h-15zM4.5 10.5h15M8.5 4v4M15.5 4v4", ...line }));
    case "rectangle.stack":
      return box(svg("path", { d: "M5 9h14v10.5H5zM7 6h10M9 3.5h6", ...line }));
    default:
      return icon(name, size);
  }
}

/** RailRound: a glass circle with its name under it; orange for Top up, dashed for a new canvas, bare until hovered for
 * the expand button. Only the edge and the fill change under the pointer. */
function roundButton(glyph, title, { on, accent, dashed, plain, help, run } = {}) {
  return h("button", { class: "rail-round" + (on ? " on" : "") + (accent ? " accent" : "") + (dashed ? " dashed" : "") + (plain ? " plain" : ""),
    title: help || title, onclick: run },
    h("span", { class: "disc" }, glyph2(glyph, 19)), title ? h("span", { class: "rail-round-title one-line" }, title) : null);
}

// MARK: Canvases

function canvasList(close) {
  const s = ui.state;
  return h("div", { class: "rail-canvases" },
    h("div", { class: "rail-canvases-head" },
      h("span", { class: "secondary rail-label" }, t("Canvases", "Холсты")), h("span", { class: "spacer" }),
      h("button", { class: "rail-hover-icon new", title: t("New canvas", "Новый холст"), onclick: () => { close?.(); ui.view = "canvas"; act("newCanvas"); } },
        icon("square.and.pencil", 12), t("New", "Новый"))),
    s.canvases.length ? null : h("div", { class: "tertiary small" }, t("Each conversation gets its own canvas and is kept here.", "У каждого разговора свой холст, и он хранится здесь.")),
    h("div", { class: "rail-canvas-rows", "data-keep": close ? "rail-canvases-panel" : "canvases" },
      [...s.canvases].sort((a, b) => new Date(b.at) - new Date(a.at)).map((c) => canvasRow(c, close))));
}

function canvasRow(c, close) {
  const s = ui.state;
  const current = c.id === s.canvas && ui.view === "canvas";
  const menu = (e) => {
    e.preventDefault();
    e.stopPropagation();
    popup(e, [
      [t("Rename…", "Переименовать…"), () => renameDialog(c)],
      null,
      [t("Delete canvas", "Удалить холст"), () => act("removeCanvas", { wallet: c.id })],
    ]);
  };
  return h("div", { class: "rail-crow" + (current ? " current" : "") + (current && s.thinking ? " pulsing" : ""), title: c.title, oncontextmenu: menu },
    h("button", { class: "rail-crow-open", onclick: async () => { close?.(); ui.view = "canvas"; if (c.id !== s.canvas) await act("openCanvas", { wallet: c.id }); else render(); } },
      h("span", { class: "rail-crow-icon" }, icon(c.id === s.canvas && s.boards.length ? "square.grid.2x2" : "bubble.left", 12)),
      h("span", { class: "rail-crow-text" }, h("span", { class: "rail-crow-title one-line" }, c.title), h("span", { class: "rail-crow-at secondary one-line" }, date(c.at)))),
    h("button", { class: "rail-crow-more", title: t("Rename, delete", "Переименовать, удалить"), onclick: menu }, icon("ellipsis", 16)));
}

/** «Oct 22 at 10:38 PM», or the time alone for today (Fmt.date). */
function date(iso) {
  const d = new Date(iso), now = new Date();
  const lang = ui.state.russian ? "ru-RU" : "en-US";
  const time = d.toLocaleTimeString(lang, { hour: "numeric", minute: "2-digit" });
  if (d.toDateString() === now.toDateString()) return time;
  return d.toLocaleDateString(lang, { day: "numeric", month: "short", year: d.getFullYear() === now.getFullYear() ? undefined : "numeric" }) + t(" at ", " в ") + time;
}

// MARK: The profile: model accounts, Settings, the browser

/** The companies' logos on their brand tiles (ModelCompany): one-color logos are tinted. */
const companies = [
  { name: "OpenAI", logo: "llm-openai", tile: "#000000", ink: "#ffffff", ids: ["chatgpt", "openai", "codex"] },
  { name: "Anthropic", logo: "llm-claude", tile: "#F4F3EE", ids: ["claude-code", "anthropic"] },
  { name: "Google", logo: "llm-gemini", tile: "#FFFFFF", ids: ["gemini-subscription", "gemini"] },
  { name: "xAI", logo: "llm-grok", tile: "#000000", ink: "#ffffff", ids: ["grok-subscription", "xai"] },
  { name: "DeepSeek", logo: "llm-deepseek", tile: "#FFFFFF", ids: ["deepseek"] },
  { name: "OpenRouter", logo: "llm-openrouter", tile: "#6566F1", ink: "#ffffff", ids: ["openrouter"] },
  { name: "Ollama", logo: "llm-ollama", tile: "#FFFFFF", ink: "#000000", ids: ["ollama"] },
  { name: "LM Studio", logo: "llm-lmstudio", tile: "#4338CA", ink: "#ffffff", ids: ["lmstudio"] },
];

/** A model provider's company logo as a rounded tile, like an app icon. */
export function companyLogo(providerId, size = 28, name = "") {
  const c = companies.find((x) => x.ids.includes(providerId));
  const tile = h("span", { class: "rail-company", style: `width:${size}px;height:${size}px;border-radius:${size * 0.26}px;background:${c ? c.tile : "#8E8E93"}` });
  if (!c) { tile.textContent = (name || providerId || "?").slice(0, 1).toUpperCase(); tile.style.fontSize = size * 0.5 + "px"; return tile; }
  const g = size * 0.64;
  if (c.ink) tile.append(h("span", { class: "rail-company-glyph", style: `width:${g}px;height:${g}px;background:${c.ink};-webkit-mask-image:url(logos/${c.logo}.png);mask-image:url(logos/${c.logo}.png)` }));
  else tile.append(h("img", { src: `logos/${c.logo}.png`, alt: "", style: `width:${g}px;height:${g}px`, draggable: "false" }));
  return tile;
}

/** «Claude», «ChatGPT», «Ollama»: the provider's name without what's in brackets, « API» or « on this Mac». */
function shortName(p) {
  let s = p.name.split(" (")[0];
  for (const tail of [" API", " on this Mac", " on this PC"]) if (s.endsWith(tail)) s = s.slice(0, -tail.length);
  return s;
}
const kindName = (p) => p.kind === "subscription" ? t("Subscription", "Подписка") : p.kind === "local" ? t("On this PC · free", "На этом ПК · бесплатно") : t("API key", "API-ключ");

/** The accounts the chat can use: logins and keys kept, and the local model in use. */
function accounts() {
  const s = ui.state;
  const list = s.providers.filter((p) => !p.unavailable && p.connected && (p.kind !== "local" || p.id === s.model.provider));
  const current = list.find((p) => p.id === s.model.provider);
  return current ? [current, ...list.filter((p) => p !== current)] : list;
}

function profileRow() {
  const list = accounts();
  const current = list.find((p) => p.id === ui.state.model.provider);
  const count = t(`${list.length} ${list.length === 1 ? "account" : "accounts"}`, `${list.length} ${list.length === 1 ? "аккаунт" : list.length < 5 ? "аккаунта" : "аккаунтов"}`);
  const wallet = ui.state.wallets.length ? t("wallet on this PC", "кошелёк на этом ПК") : t("no wallet yet", "кошелька пока нет");
  return h("button", { class: "rail-profile", title: t("Chat accounts, Settings, the browser", "Аккаунты чата, настройки, браузер"),
    onclick: (e) => { e.stopPropagation(); profileMenu(e.currentTarget, "top"); } },
    list.length ? h("span", { class: "rail-profile-logos" }, list.slice(0, 3).map((p) => companyLogo(p.id, 28, p.name)))
      : h("span", { class: "rail-profile-none" }, icon("person.fill", 13)),
    h("span", { class: "rail-profile-text" },
      h("b", { class: "one-line" }, current ? shortName(current) : t("Accounts", "Аккаунты")),
      h("span", { class: "secondary one-line" }, list.length ? (list.length < 3 ? `${count} · ${wallet}` : count) : t("no model connected", "модель не подключена"))),
    icon("chevron.updown", 12));
}

function profileMenu(anchor, side) {
  const go = (run) => () => { closePopover(); run(); };
  const body = h("div", { class: "rail-profile-menu" });
  const draw = (signingOut, open) => {
    const s = ui.state;
    if (signingOut) {
      body.replaceChildren(h("b", { class: "rail-pm-title" }, t(`Sign out of ${shortName(signingOut)}?`, `Выйти из ${shortName(signingOut)}?`)),
        h("p", { class: "secondary rail-pm-note" }, t("The chat stops using the model until you sign in again. Your wallet key stays under Windows Hello.",
          "Чат перестанет пользоваться этой моделью, пока ты снова не войдёшь. Ключ кошелька остаётся под Windows Hello.")
          + (signingOut.id === "openrouter" ? t(" The second check (Jev) uses this login too.", " Вторая проверка (Jev) тоже пользуется этим входом.") : "")),
        h("div", { class: "btns" },
          h("button", { class: "btn primary rail-danger", onclick: go(() => act("disconnectModel", { provider: signingOut.id })) }, t("Sign out", "Выйти")),
          h("button", { class: "btn", onclick: () => draw() }, t("Cancel", "Отмена"))));
      return;
    }
    const list = accounts();
    body.replaceChildren(
      h("b", { class: "rail-pm-title" }, t("Accounts", "Аккаунты")),
      ...list.map((p) => {
        const inUse = p.id === s.model.provider;
        return h("div", { class: "rail-pm-account" + (inUse ? " in-use" : "") },
          h("button", { class: "rail-pm-use", disabled: inUse, title: inUse ? t(`The chat uses ${shortName(p)}`, `Чат пользуется ${shortName(p)}`) : t(`The chat goes on with ${shortName(p)}`, `Чат продолжит с ${shortName(p)}`),
            onclick: go(() => act("connectModel", { provider: p.id })) },
            companyLogo(p.id, 32, p.name),
            h("span", { class: "rail-pm-text" }, h("b", { class: "one-line" }, shortName(p)), h("span", { class: "secondary one-line" }, kindName(p))),
            inUse ? h("span", { class: "rail-pm-check" }, icon("check", 14)) : null),
          h("button", { class: "rail-pm-more", title: t("Add another account, sign out", "Добавить аккаунт, выйти"),
            onclick: () => draw(null, open === p.id ? null : p.id) }, icon("ellipsis", 16)),
          open === p.id ? h("div", { class: "rail-pm-actions" },
            inUse ? null : h("button", { onclick: go(() => act("connectModel", { provider: p.id })) }, icon("arrow.left.arrow.right", 13), t(`Switch to ${shortName(p)}`, `Переключиться на ${shortName(p)}`)),
            h("button", { onclick: go(() => { ui.view = "settings"; render(); }) }, icon("plus", 13), t("Add another account…", "Добавить другой аккаунт…")),
            p.kind === "local" ? null : h("button", { onclick: () => draw(p) }, icon("rectangle.portrait.and.arrow.right", 13), t(`Sign out of ${shortName(p)}…`, `Выйти из ${shortName(p)}…`))) : null);
      }),
      h("button", { class: "rail-pm-add", onclick: go(() => { ui.view = "settings"; render(); }) },
        h("span", { class: "rail-pm-add-box" }, icon("plus", 14)), h("b", { class: "secondary" }, t("Add an account", "Добавить аккаунт"))),
      h("div", { class: "menu-divider" }),
      h("div", { class: "rail-pm-apps" },
        h("button", { class: "rail-pm-app", title: t("Settings · Ctrl+,", "Настройки · Ctrl+,"), onclick: go(() => { ui.view = "settings"; render(); }) }, icon("gear", 15), t("Settings", "Настройки")),
        h("button", { class: "rail-pm-app", title: t("The dApp browser, with your wallet", "Браузер для dApp, с твоим кошельком"), onclick: go(browserDialog) }, icon("globe", 15), t("Browser", "Браузер"))));
  };
  draw();
  popover(anchor, side, body, "rail-profile-pop");
}

// MARK: Folded

function narrow() {
  return h("div", { class: "rail-narrow" },
    h("div", { class: "rail-drag rail-lights", "data-tauri-drag-region": true }),
    h("button", { class: "rail-coin" + (refreshing ? " refreshing" : ""), disabled: refreshing,
      title: refreshing ? t("Updating the balance…", "Обновляю баланс…") : t("Balance — click to update it", "Баланс — нажми, чтобы обновить"), onclick: refresh },
      mascot(34, refreshing ? "thinking" : "idle"),
      h("span", { class: "rail-coin-amount" }, balanceAmount("rail-mini", "$•••", [44, 11]), positionsLine("rail-mini-pnl"))),
    walletChip(true),
    h("div", { class: "rail-divider" }),
    railActions(true),
    h("div", { class: "rail-divider" }),
    roundButton("rectangle.stack", t("Canvases", "Холсты"), { help: t("Canvases", "Холсты"), run: (e) => {
      e.stopPropagation();
      const anchor = e.currentTarget;
      popover(anchor, "right", h("div", { class: "rail-canvases-panel" }, canvasList(closePopover)), "rail-canvases-pop");
    } }),
    roundButton("plus", t("New", "Новый"), { dashed: true, help: t("New canvas", "Новый холст"), run: () => { ui.view = "canvas"; act("newCanvas"); } }),
    h("div", { class: "spacer" }),
    roundButton("sidebar.left", null, { plain: true, help: t("Expand the sidebar", "Развернуть панель"), run: () => { foldedForRoom = false; setCollapsed(false); } }),
    roundButton("person.fill", t("Profile", "Профиль"), { help: t("Chat accounts, Settings, the browser", "Аккаунты чата, настройки, браузер"),
      run: (e) => { e.stopPropagation(); profileMenu(e.currentTarget, "right"); } }),
  );
}

// MARK: The edge

/** The rail's right edge: drag to fold or unfold; it settles on the nearer width. The accent capsule shows under the
 * pointer. While dragging, new states wait (ui.dragging), as they do for the canvas. */
function edge() {
  const handle = h("div", { class: "rail-edge", title: t("Drag to collapse or expand the wallet", "Потяни, чтобы свернуть или развернуть кошелёк") }, h("span", {}));
  handle.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const host = handle.parentElement;
    const base = collapsed ? NARROW : WIDE, x0 = e.clientX;
    let last = [{ x: e.clientX, at: performance.now() }];
    handle.setPointerCapture(e.pointerId);
    handle.classList.add("dragging");
    host.classList.add("no-slide");
    ui.dragging = true;
    const move = (ev) => {
      host.style.width = Math.min(340, Math.max(NARROW, base + ev.clientX - x0)) + "px";
      last = [...last.slice(-4), { x: ev.clientX, at: performance.now() }];
    };
    const up = (ev) => {
      handle.removeEventListener("pointermove", move);
      handle.classList.remove("dragging");
      host.classList.remove("no-slide");
      // Where it would land with the pull's speed, as the Mac's predictedEndTranslation.
      const a = last[0], b = last[last.length - 1];
      const speed = b.at > a.at ? (b.x - a.x) / (b.at - a.at) : 0;
      const landing = base + (ev.clientX - x0) + speed * 150;
      drawnWidth = parseFloat(host.style.width);
      ui.dragging = false;
      const fold = landing < (NARROW + WIDE) / 2;
      if (fold !== collapsed) { foldedForRoom = false; setCollapsed(fold); }
      else render();
      if (ui.renderWaiting) { ui.renderWaiting = false; render(); }
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up, { once: true });
    handle.addEventListener("pointercancel", up, { once: true });
  });
  return handle;
}
