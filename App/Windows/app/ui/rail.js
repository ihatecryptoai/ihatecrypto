// The wallet rail (the Mac's StudioRail): a glass panel floating over the canvas's ground. Wide, it holds the logo, the
// balance with its wallet, Top up and the four rail buttons, the canvases and the model accounts; folded, a strip of
// round glass buttons. The collapse button, the strip's expand button or a drag of the right edge switch the two; a
// window narrower than 1140 folds it, and widening again brings back what it folded.

import { ui, act, render, t, h, svg, icon, addIcons, mascot, logo, usd, signedUsd, masked, store, popup, copyButton, suggest, toast } from "./core.js";
import { logoFiles } from "./logos.js";
// The canvas module draws the rail too; read through the namespace so neither waits on the other to load.
import * as canvas from "./canvas.js";
import { renameDialog, browserDialog } from "./onboarding.js";
import { openWhatsNew, whatsNewUnseen } from "./whatsnew.js";

addIcons({
  "bubble.left": "M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5H10l-4.5 3.5v-3.5H5A1.5 1.5 0 0 1 3.5 15V7A1.5 1.5 0 0 1 5 5.5z",
  "square.grid.2x2": "M4.5 4.5h6v6h-6zM13.5 4.5h6v6h-6zM4.5 13.5h6v6h-6zM13.5 13.5h6v6h-6z",
  "note.text": "M6 3.5h9l3.5 3.5v13.5H6zM15 3.5V7h3.5M9 11h6M9 14h6M9 17h4",
  flask: "M9.5 3.5h5M10.5 3.5v6L5 19a1.2 1.2 0 0 0 1 1.5h12a1.2 1.2 0 0 0 1-1.5l-5.5-9.5v-6M7.5 15h9",
  "arrow.triangle.2.circlepath": "M4.5 11a7.5 7.5 0 0 1 13-4.5M19.5 13a7.5 7.5 0 0 1-13 4.5M17.5 3v3.5H14M6.5 21v-3.5H10",
  "person.fill": "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4.5 20.5a7.5 7.5 0 0 1 15 0z",
  "arrow.left.arrow.right": "M8 4 4 8l4 4M4 8h16M16 12l4 4-4 4M20 16H4",
  "rectangle.portrait.and.arrow.right": "M14 4.5H6.5v15H14M10.5 12h10M17 8.5l3.5 3.5-3.5 3.5",
  pin: "M9 4h6l-1 5 3 3H7l3-3zM12 12v8",
  "pin.slash": "M9 4h6l-1 5 3 3H7l3-3zM12 12v8M4 4l16 16",
  "face.smiling": "M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17zM8.5 14a4.5 4.5 0 0 0 7 0M9 9.5h.01M15 9.5h.01",
  folder: "M3.5 7A1.5 1.5 0 0 1 5 5.5h4l2 2h8A1.5 1.5 0 0 1 20.5 9v8.5A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z",
  "folder.fill": "M3.5 7A1.5 1.5 0 0 1 5 5.5h4l2 2h8A1.5 1.5 0 0 1 20.5 9v8.5A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5zM3.5 10.5h17",
  "folder.badge.minus": "M3.5 7A1.5 1.5 0 0 1 5 5.5h4l2 2h8A1.5 1.5 0 0 1 20.5 9v8.5A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5zM9 13.5h6",
  photo: "M4 5.5h16v13H4zM4 15.5l4.5-4.5 4 4 2.5-2.5 5 5M15.5 9.5h.01",
  "doc.on.clipboard": "M8.5 5.5H6.5v15h11v-15h-2M9 4h6v3H9z",
  magnifyingglass: "M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5l4.5 4.5",
  "circle.circle": "M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17zM12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z",
  "rail.shield": "M12 3.5 5 6.5v5c0 4.4 3 7.6 7 9 4-1.4 7-4.6 7-9v-5zM9.5 12.5v-1a2.5 2.5 0 0 1 5 0v1M9 12.5h6v3.5H9z",
  "rail.pc": "M3.5 5.5h17v10.5h-17zM9 19.5h6M12 16v3.5",
  "rail.phone": "M8 3.5h8v17H8zM11 17.5h2",
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
    h("button", { class: "rail-brand" + (whatsNewUnseen() ? " rail-unseen" : ""), title: t("What's new", "Что нового"), onclick: () => openWhatsNew() },
      mascot(30), h("span", { class: "rail-wordmark" }, "ihatecrypto")),
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
  // The last full balance, dimmed, until every network answers (the Mac's lastBalance); a skeleton only at the very first.
  const last = ui.state.lastBalance;
  if (!ready && last != null) return h("div", { class: cls + " num rail-stale" }, masked(usd(Number(last)), mask));
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
function walletMenu(anchor, compact, start) {
  let picked = start || ui.state.active;
  let renaming = false;
  const body = h("div", { class: "rail-wallet-menu" });
  const draw = () => {
    const s = ui.state;
    const wallets = s.wallets.filter((w) => !w.automation);
    const shown = wallets.find((w) => w.id === picked) || wallets[0];
    const msShown = picked === "new-ms" || (picked || "").startsWith("ms:");
    const multisigs = s.multisigs || [];
    const list = h("div", { class: "rail-wm-list" },
      h("div", { class: "rail-wm-head" }, h("b", {}, t("Wallets", "Кошельки")), h("span", { class: "spacer" }),
        h("button", { class: "rail-hover-icon", title: t("Manage wallets: create, import, remove", "Управление кошельками: создать, импортировать, удалить"),
          onclick: () => { closePopover(); store("settingsTab", "wallet"); ui.view = "settings"; render(); } }, icon("gear", 15))),
      wallets.map((w) => h("button", { class: "rail-wm-row" + (shown && w.id === shown.id && !msShown ? " on" : ""), onclick: () => { picked = w.id; renaming = false; draw(); } },
        h("span", { class: "rail-wm-icon" + (w.test ? " test" : "") }, icon(w.kind === "ledger" ? "bolt" : w.test ? "flask" : "lock", 14)),
        h("span", { class: "rail-wm-text" }, h("b", { class: "one-line" }, w.label), h("span", { class: "secondary one-line" }, listLine(w))),
        w.id === s.active ? h("span", { class: "accent-ink" }, icon("check", 13)) : null)),
      !wallets.length && s.watching ? h("div", { class: "secondary small rail-wm-empty" }, t("Only watching addresses: add a wallet in Settings.", "Только просмотр адресов: добавь кошелёк в настройках.")) : null,
      multisigs.length ? h("div", { class: "eyebrow rail-wm-section" }, t("MULTISIG", "МУЛЬТИСИГ")) : null,
      multisigs.map((m) => h("button", { class: "rail-wm-row" + (picked === "ms:" + m.address ? " on" : ""), onclick: () => { picked = "ms:" + m.address; renaming = false; draw(); } },
        h("span", { class: "rail-wm-icon" }, icon("rail.shield", 14)),
        h("span", { class: "rail-wm-text" }, h("b", { class: "one-line" }, t("Multisig", "Мультисиг")),
          h("span", { class: "secondary one-line" }, t(`${m.threshold} of ${m.keys.length} keys · `, `${m.threshold} из ${m.keys.length} ключей · `) + m.networks.map(netName).join(", "))))),
      h("div", { class: "spacer" }),
      h("div", { class: "menu-divider" }),
      s.wallets.some((w) => w.kind === "ledger") ? null : h("button", { class: "rail-wm-action", onclick: () => { closePopover(); act("addBoard", { name: "ledger" }); } }, icon("bolt", 14), t("Connect Ledger…", "Подключить Ledger…")),
      h("button", { class: "rail-wm-action", onclick: () => { closePopover(); store("settingsTab", "wallet"); ui.view = "settings"; render(); } }, icon("plus", 14), t("New wallet…", "Новый кошелёк…")),
      h("button", { class: "rail-wm-action" + (picked === "new-ms" ? " on" : ""), title: t("What a multisig is, and how to make one", "Что такое мультисиг и как его сделать"),
        onclick: () => { picked = "new-ms"; renaming = false; draw(); } }, icon("rail.shield", 14), t("New multisig…", "Новый мультисиг…")));
    const ms = multisigs.find((m) => picked === "ms:" + m.address);
    body.replaceChildren(list, picked === "new-ms" ? newMultisig() : ms ? multisigDetail(ms) : shown ? detail(shown) : h("div", { class: "rail-wm-detail" }));
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

/** A multisig on the right: its networks, its whole address, who signs; «Open» puts its block on the canvas. */
function multisigDetail(m) {
  const nets = m.networks.map(netName);
  const only = nets.length < 2 ? nets.join("") : nets.slice(0, -1).join(", ") + t(" or ", " или ") + nets[nets.length - 1];
  const how = (k) => k.kind === "pc" ? t("Windows Hello on this PC", "Windows Hello на этом ПК") : k.kind === "ledger" ? t("Approve on the device", "Подтверждение на устройстве") : t("Signs in the Safe app", "Подписывает в приложении Safe");
  return h("div", { class: "rail-wm-detail rail-ms" },
    h("div", { class: "rail-wm-detail-head" },
      h("div", {}, h("b", { class: "rail-ms-title" }, t("Multisig", "Мультисиг")),
        h("div", { class: "secondary rail-wm-how" }, t(`Safe · money leaves when any ${m.threshold} of its ${m.keys.length} keys sign`, `Safe · деньги уходят, когда подписывают любые ${m.threshold} из ${m.keys.length} ключей`))),
      h("span", { class: "spacer" }),
      h("button", { class: "btn primary", title: t("Its money and payments, on the canvas", "Его деньги и платежи — на холсте"), onclick: () => { closePopover(); act("addBoard", { name: "multisig" }); } }, t("Open", "Открыть"))),
    h("div", { class: "rail-wm-net" },
      h("span", { class: "rail-wm-net-name" }, h("span", { class: "rail-ms-logos" }, m.networks.slice(0, 3).map((n) => logo({ arbitrum: "arb", ethereum: "eth", polygon: "pol", optimism: "op" }[n] || n, 20))),
        h("span", {}, h("b", {}, t("Address", "Адрес")), h("span", { class: "secondary small" }, nets.join(", ")))),
      h("span", { class: "address rail-wm-address" }, m.address), copyButton(m.address)),
    h("div", { class: "rail-ms-note" }, icon("questionmark.circle", 14),
      h("span", {}, t(`Send to it only on ${only}. It isn't a multisig on other networks.`, `Отправляй на него только в ${only}. В других сетях это не мультисиг.`))),
    h("div", { class: "eyebrow rail-ms-keys" }, t(`KEYS · ANY ${m.threshold} OF ${m.keys.length} SIGN`, `КЛЮЧИ · ПОДПИСЫВАЮТ ЛЮБЫЕ ${m.threshold} ИЗ ${m.keys.length}`)),
    m.keys.map((k) => h("div", { class: "rail-ms-key" },
      h("span", { class: "rail-wm-icon" }, icon(k.kind === "ledger" ? "bolt" : k.kind === "pc" ? "rail.pc" : "rail.phone", 14)),
      h("span", { class: "rail-wm-text" }, h("b", { class: "one-line" }, k.name), h("span", { class: "secondary one-line" }, how(k))),
      k.kind !== "address" ? h("span", { class: "secondary small" }, t("signs here", "подписывает здесь")) : null)),
    h("div", { class: "secondary small rail-ms-foot" }, t("A multisig lives on EVM networks only: no Solana, Hyperliquid or NEAR.", "Мультисиг бывает только в EVM-сетях: не в Solana, Hyperliquid или NEAR.")));
}

/** What a multisig is before making one; «Make a multisig» goes to the chat, nothing is made until Confirm. */
function newMultisig() {
  const step = (n, title, text) => h("div", { class: "rail-ms-step" }, h("span", { class: "rail-ms-n num" }, String(n)),
    h("span", {}, h("b", {}, title), h("span", { class: "secondary" }, text)));
  return h("div", { class: "rail-wm-detail rail-ms" },
    h("div", {}, h("b", { class: "rail-ms-title" }, t("New multisig", "Новый мультисиг")),
      h("div", { class: "secondary rail-wm-how" }, t("A shared safe on EVM networks. Money leaves it only when enough of its keys sign, so one lost or stolen device can't empty it.",
        "Общий сейф в EVM-сетях. Деньги уходят из него, только когда подписывает достаточно ключей, поэтому одно потерянное или украденное устройство его не опустошит."))),
    h("div", { class: "rail-ms-steps" },
      step(1, t("Keys", "Ключи"), t("This PC, your Ledger, a phone with the Safe app. As many as you want.", "Этот ПК, твой Ledger, телефон с приложением Safe. Сколько угодно.")),
      step(2, t("How many must sign", "Сколько подписей нужно"), t("More than half is safest: 2 of 3, 3 of 5.", "Надёжнее всего больше половины: 2 из 3, 3 из 5.")),
      step(3, t("Networks", "Сети"), t("Base, Arbitrum or another EVM network. The address is the same on each.", "Base, Arbitrum или другая EVM-сеть. Адрес в каждой один и тот же."))),
    h("div", { class: "rail-ms-make" },
      h("button", { class: "btn primary", onclick: () => { closePopover(); ui.view = "canvas"; suggest("make a multisig"); } }, icon("plus", 13), t("Make a multisig", "Сделать мультисиг")),
      h("span", { class: "secondary small" }, t("It goes to the chat; nothing is made until you confirm.", "Это уйдёт в чат; ничего не создаётся, пока ты не подтвердишь."))));
}

/** An EVM network's name by its key. */
const netName = (k) => ({ base: "Base", arbitrum: "Arbitrum", ethereum: "Ethereum", polygon: "Polygon", optimism: "Optimism", bnb: "BNB Chain", hyperevm: "HyperEVM",
  robinhood: "Robinhood", tempo: "Tempo", avalanche: "Avalanche", monad: "Monad" })[k] || k[0].toUpperCase() + k.slice(1);

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
    ["history", t("History", "История"), "clock.arrow.circlepath", hasBoard("history"), false, t("What ihatecrypto did lately", "Что ihatecrypto делал недавно"),
      () => toggleBoard("history")],
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

/** Folders folded in the list, kept per viewer (the Mac's foldedCanvasFolders). */
const foldedFolders = () => new Set(store("foldedFolders") || []);
const maxPinned = 5;

/** Pinned canvases first (in the order they were pinned), then folders (open or folded), then the canvases in no folder;
 * newest first inside each. A pinned canvas shows only at the top, not again in its folder. The open canvas with nothing
 * on it yet is the new canvas on top, kept once written on. */
function canvasList(close) {
  const s = ui.state;
  const all = [...s.canvases].sort((a, b) => new Date(b.at) - new Date(a.at));
  const fresh = all.find((c) => c.id === s.canvas && c.empty) || null;
  const rest = all.filter((c) => c !== fresh);
  const pinned = rest.filter((c) => c.pinnedAt).sort((a, b) => new Date(a.pinnedAt) - new Date(b.pinnedAt));
  const items = rest.filter((c) => !c.pinnedAt);
  const folded = foldedFolders();
  const head = h("div", { class: "rail-canvases-head" },
    h("span", { class: "secondary rail-label" }, t("Canvases", "Холсты")), h("span", { class: "spacer" }),
    h("button", { class: "rail-hover-icon", title: t("New folder — drag canvases into it", "Новая папка — перетащи в неё холсты"),
      onclick: (e) => { e.stopPropagation(); askName(e.currentTarget, t("New folder", "Новая папка"), "", t("Drag canvases into it, or start one there from its menu.", "Перетащи в неё холсты или начни новый из её меню."),
        t("Create", "Создать"), (name) => act("createFolder", { name })); } }, icon("folder.badge.plus", 13)),
    h("button", { class: "rail-hover-icon new", disabled: !canSwitch(), title: canSwitch() ? t("New canvas", "Новый холст") : busyTip(), onclick: () => { close?.(); ui.view = "canvas"; act("newCanvas"); } },
      icon("square.and.pencil", 12), t("New", "Новый")));
  const rows = [];
  if (fresh) rows.push(newCanvasRow(fresh));
  rows.push(...pinned.map((c) => canvasRow(c, close)));
  if (pinned.length && items.length) rows.push(h("div", { class: "rail-crow-divider" }));
  for (const f of s.folders || []) {
    const inside = items.filter((c) => c.folder === f.name);
    const shut = folded.has(f.name);
    rows.push(h("div", { class: "rail-folder", "data-folder": f.name },
      folderRow(f, inside.length, shut), shut ? null : inside.map((c) => canvasRow(c, close, true))));
  }
  rows.push(...items.filter((c) => !c.folder).map((c) => canvasRow(c, close)));
  // A canvas dropped anywhere but on a folder leaves its folder.
  return h("div", { class: "rail-canvases" }, head,
    all.length ? null : h("div", { class: "tertiary small" }, t("Each conversation gets its own canvas and is kept here.", "У каждого разговора свой холст, и он хранится здесь.")),
    h("div", { class: "rail-canvas-rows", "data-folder": "", "data-keep": close ? "rail-canvases-panel" : "canvases" }, rows));
}

/** The open canvas before anything is on it: its edge breathes while it waits for the first message and while the first
 * answer is written; with that it becomes a row of its own. */
function newCanvasRow(c) {
  const s = ui.state;
  const line = s.thinking ? t("answering…", "отвечаю…")
    : c.folder ? t(`goes to «${c.folder}» · write below`, `попадёт в «${c.folder}» · пиши ниже`) : t("write below to keep it", "напиши ниже, чтобы сохранить");
  return h("div", { class: "rail-crow current rail-new", title: t("A new canvas: it's kept here once you write something", "Новый холст: он сохранится здесь, как только ты что-нибудь напишешь") },
    h("span", { class: "rail-crow-open" },
      h("span", { class: "rail-crow-icon" }, icon("bubble.left", 12)),
      h("span", { class: "rail-crow-text" },
        h("span", { class: "rail-crow-title one-line" }, (s.chat || []).find((m) => m.fromUser)?.text || t("New canvas", "Новый холст")),
        h("span", { class: "rail-crow-at secondary one-line" }, line))));
}

function canvasRow(c, close, inFolder) {
  const s = ui.state;
  const current = c.id === s.canvas && ui.view === "canvas";
  const provider = s.providers.find((p) => p.id === c.provider);
  const tip = [c.title, provider ? shortName(provider) : c.provider, c.model].filter(Boolean).join(" · ");
  const menu = (e) => { e.preventDefault(); e.stopPropagation(); canvasMenu(e, c, row); };
  const title = h("span", { class: "rail-crow-title one-line" }, c.title);
  let stop = null;
  const open = h("button", { class: "rail-crow-open", disabled: c.id !== s.canvas && !canSwitch(), title: c.id !== s.canvas && !canSwitch() ? busyTip() : null,
    onclick: async () => { close?.(); ui.view = "canvas"; if (c.id !== s.canvas) await act("openCanvas", { wallet: c.id }); else render(); } },
    canvasGlyph(c),
    h("span", { class: "rail-crow-text" },
      h("span", { class: "rail-crow-line" }, title, c.pinnedAt ? h("span", { class: "rail-crow-pin", title: t("Pinned", "Закреплён") }, icon("pin", 10)) : null),
      h("span", { class: "rail-crow-at secondary one-line" }, date(c.at))));
  open.addEventListener("pointerdown", (e) => dragCanvas(e, c, open));
  const row = h("div", { class: "rail-crow" + (current ? " current" : "") + (current && s.thinking ? " pulsing" : "") + (inFolder ? " in-folder" : ""), title: tip, oncontextmenu: menu,
    // A title cut short runs past like a ticker while the pointer is on the row.
    onmouseenter: () => { stop = ticker(title, c.title); }, onmouseleave: () => { stop?.(); stop = null; } },
    open,
    h("button", { class: "rail-crow-more", title: t("Pin, rename, move to a folder, delete", "Закрепить, переименовать, перенести в папку, удалить"), onclick: menu }, icon("ellipsis", 16)));
  return row;
}

/** The canvas's picture, or the sign of what it holds; the model that talked on it as its company's round logo at the
 * corner, never its name (that's in the tooltip). */
function canvasGlyph(c) {
  // As the Mac's row: its picture, else its trade plan's coin, else a sign of what it holds (a note, blocks, a chat).
  const base = c.icon ? pickedIcon(c.icon, 26, c.title) : c.coin ? coinBadge(c.coin)
    : h("span", { class: "rail-crow-icon" }, icon(c.note ? "note.text" : c.blocks ? "square.grid.2x2" : "bubble.left", 12));
  const wrap = h("span", { class: "rail-crow-glyph" }, base);
  if (c.provider) {
    const badge = companyLogo(c.provider, 12);
    badge.classList.add("rail-crow-badge");
    wrap.append(badge);
  }
  return wrap;
}

/** A plan's coin on its canvas's row (AssetBadge, dark): its logo, or a chart sign for a market without one (xyz:TSLA). */
function coinBadge(coin) {
  const el = logo(coin.split(":").pop(), 26);
  el.classList.add("rail-crow-coin");
  if (el.classList.contains("letter")) { el.replaceChildren(icon("chart.line", 12)); el.style.fontSize = ""; }
  return el;
}

/** Each action with its icon, as in the Mac's menus; a greyed item says why. */
function canvasMenu(e, c, anchor) {
  const s = ui.state;
  const full = !c.pinnedAt && s.canvases.filter((x) => x.pinnedAt).length >= maxPinned;
  const items = [
    c.pinnedAt ? [[icon("pin.slash", 14), t("Unpin", "Открепить")], () => act("pinCanvas", { wallet: c.id, test: false })]
      : [[icon("pin", 14), full ? t(`Pin — ${maxPinned} already pinned, unpin one first`, `Закрепить — уже ${maxPinned}, сначала открепи один`) : t("Pin to the top", "Закрепить наверху")],
        () => act("pinCanvas", { wallet: c.id, test: true }), full],
    null,
    [[icon("pencil", 14), t("Rename", "Переименовать")], () => askName(anchor, t("Rename the canvas", "Переименовать холст"), c.title, null, t("Rename", "Переименовать"),
      (label) => act("renameCanvas", { wallet: c.id, label }))],
    [[icon("face.smiling", 14), t("Change icon…", "Сменить иконку…")], () => iconPicker(anchor, c.icon, c.title, { wallet: c.id })],
    [[icon("folder", 14), t("Move to folder…", "Перенести в папку…")], () => folderMenu(e, c, anchor)],
    null,
    [[icon("trash", 14), t("Delete canvas", "Удалить холст")], () => act("removeCanvas", { wallet: c.id }), c.id === s.canvas && !canSwitch()],
  ];
  menuWith(e, items);
}

/** The Mac's «Move to folder» submenu, as a menu of its own where the first one was. */
function folderMenu(e, c, anchor) {
  const others = (ui.state.folders || []).map((f) => f.name).filter((f) => f !== c.folder);
  menuWith(e, [
    ...others.map((f) => [[icon("folder", 14), f], () => act("moveCanvas", { wallet: c.id, name: f })]),
    others.length ? null : undefined,
    [[icon("folder.badge.plus", 14), t("New folder…", "Новая папка…")], () => askName(anchor, t("New folder", "Новая папка"), "", t(`“${c.title}” goes into it.`, `«${c.title}» попадёт в неё.`),
      t("Create", "Создать"), (name) => act("moveCanvas", { wallet: c.id, name }))],
    c.folder ? [[icon("folder.badge.minus", 14), t(`Remove from “${c.folder}”`, `Убрать из «${c.folder}»`)], () => act("moveCanvas", { wallet: c.id, name: "" })] : undefined,
  ].filter((x) => x !== undefined));
}

/** A folder of canvases: a click folds or unfolds it; canvases are dragged onto it. */
function folderRow(f, count, shut) {
  const toggle = () => {
    const set = foldedFolders();
    if (set.has(f.name)) set.delete(f.name); else set.add(f.name);
    store("foldedFolders", [...set].sort());
    render();
  };
  let row;
  const menu = (e) => {
    e.preventDefault();
    e.stopPropagation();
    menuWith(e, [
      [[icon("square.and.pencil", 14), t("New canvas here", "Новый холст здесь")], () => { ui.view = "canvas"; act("newCanvasIn", { name: f.name }); }, !!ui.state.running],
      [[icon("pencil", 14), t("Rename", "Переименовать")], () => askName(row, t("Rename folder", "Переименовать папку"), f.name, null, t("Rename", "Переименовать"),
        (name) => act("renameFolder", { label: f.name, name }))],
      [[icon("face.smiling", 14), t("Change icon…", "Сменить иконку…")], () => iconPicker(row, f.icon, f.name, { name: f.name })],
      null,
      [[icon("trash", 14), t("Delete folder (canvases stay)", "Удалить папку (холсты останутся)")], () => act("deleteFolder", { name: f.name })],
    ]);
  };
  row = h("div", { class: "rail-frow", oncontextmenu: menu },
    h("button", { class: "rail-frow-open", title: shut ? t(`Show the canvases in ${f.name}`, `Показать холсты в «${f.name}»`) : t(`Fold ${f.name}`, `Свернуть «${f.name}»`), onclick: toggle },
      h("span", { class: "rail-frow-chevron" + (shut ? "" : " open") }, icon("chevron.right", 9)),
      f.icon ? pickedIcon(f.icon, 18, f.name) : h("span", { class: "rail-frow-icon accent-ink" }, icon(shut ? "folder" : "folder.fill", 13)),
      h("b", { class: "one-line" }, f.name), h("span", { class: "spacer" }), h("span", { class: "tertiary num rail-frow-count" }, String(count))),
    h("button", { class: "rail-crow-more", title: t("New canvas here, rename, change icon, delete", "Новый холст здесь, переименовать, сменить иконку, удалить"), onclick: menu }, icon("ellipsis", 16)));
  return row;
}

/** A row dragged onto a folder moves into it; anywhere else in the list, out of its folder. A short press stays a click. */
function dragCanvas(e, c, el) {
  if (e.button !== 0) return;
  const x0 = e.clientX, y0 = e.clientY;
  let ghost = null, target = null;
  const move = (ev) => {
    if (!ghost && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
    if (!ghost) { ghost = h("div", { class: "rail-drag-ghost" }, c.title); document.body.append(ghost); ui.dragging = true; }
    ghost.style.left = ev.clientX + 12 + "px";
    ghost.style.top = ev.clientY + 6 + "px";
    const over = document.elementFromPoint(ev.clientX, ev.clientY)?.closest("[data-folder]") || null;
    if (over !== target) { target?.classList.remove("drop"); target = over; target?.classList.add("drop"); }
  };
  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    if (!ghost) return;
    ghost.remove();
    target?.classList.remove("drop");
    // The click that ends a drag doesn't open the canvas.
    el.addEventListener("click", (ev) => { ev.stopImmediatePropagation(); ev.preventDefault(); }, { capture: true, once: true });
    ui.dragging = false;
    const to = target?.dataset.folder;
    const waited = ui.renderWaiting;
    ui.renderWaiting = false;
    if (to !== undefined && (c.folder || "") !== to) act("moveCanvas", { wallet: c.id, name: to });
    else if (waited) render();
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}

/** Still for a moment, then left at 30 px a second with a second copy following, so the loop never jumps (the Mac's
 * Ticker). Returns what puts the title back. */
function ticker(el, text) {
  if (el.scrollWidth <= el.clientWidth + 1 || matchMedia("(prefers-reduced-motion: reduce)").matches) return null;
  const step = el.scrollWidth + 32, pause = 0.8, run = step / 30;
  const strip = h("span", { class: "rail-ticker" }, h("span", {}, text), h("span", { style: "margin-left:32px" }, text));
  el.replaceChildren(strip);
  el.classList.add("ticking");
  const anim = strip.animate([{ transform: "translateX(0)" }, { transform: "translateX(0)", offset: pause / (pause + run) }, { transform: `translateX(${-step}px)` }],
    { duration: (pause + run) * 1000, iterations: Infinity });
  return () => { anim.cancel(); el.classList.remove("ticking"); el.replaceChildren(text); };
}

/** popup() with icons, greyed items (a third value true) and no leading or doubled dividers. */
function menuWith(e, items) {
  const clean = items.filter((it, i) => it !== null || (i > 0 && items[i - 1] !== null && i < items.length - 1));
  const off = [];
  const list = clean.map((it, i) => it === null ? null : (it[2] ? off.push(i) : 0, [h("span", { class: "rail-menu-item" }, ...it[0]), it[1]]));
  const m = popup(e, list, "rail-menu");
  for (const i of off) { const b = m.children[i]; b.disabled = true; b.classList.add("disabled"); }
  return m;
}

/** A name asked in place, by the row (the Mac's alerts): Enter saves, Escape or a click outside leaves it. */
function askName(anchor, title, value, hint, save, done) {
  let text = value;
  const go = () => { const v = text.trim(); if (!v) return; closePopover(); done(v); };
  const field = h("input", { id: "rail-ask", value, oninput: (e) => (text = e.target.value),
    onkeydown: (e) => { if (e.key === "Enter") go(); else if (e.key === "Escape") { e.stopPropagation(); closePopover(); } } });
  popover(anchor, "right", h("div", { class: "rail-ask" },
    h("b", {}, title), hint ? h("span", { class: "secondary small" }, hint) : null, field,
    h("div", { class: "btns" }, h("button", { class: "btn small", onclick: closePopover }, t("Cancel", "Отмена")),
      h("button", { class: "btn small primary", onclick: go }, save))), "rail-ask-pop");
  setTimeout(() => { field.focus(); field.select(); }, 0);
}

/** For the preview (`&call=rail.show&panel=picker|menu|folder|wallet-ms|new-ms`): opens one of the rail's panels. */
export function show() {
  const panel = new URLSearchParams(location.search).get("panel");
  const row = document.querySelector(".rail-crow:not(.rail-new)"), c = ui.state.canvases.find((x) => !x.empty);
  const r = row?.getBoundingClientRect() || { right: 200, top: 300 };
  const at = { clientX: r.right - 20, clientY: r.top + 20, preventDefault() {}, stopPropagation() {} };
  if (panel === "picker") iconPicker(row, c.icon, c.title, { wallet: c.id });
  else if (panel === "menu") canvasMenu(at, c, row);
  else if (panel === "folder") folderMenu(at, c, row);
  else if (panel?.endsWith("ms")) walletMenu(document.querySelector(".rail-wallet-chip"), false, panel === "new-ms" ? "new-ms" : "ms:" + ui.state.multisigs[0]?.address);
}

// MARK: The icon library (the Mac's IconPicker)

/** The library's glyphs, drawn here (24-point strokes). */
const pickSymbols = {
  "building.columns": "M3.5 9.5 12 4l8.5 5.5M5 9.5v8M9.5 9.5v8M14.5 9.5v8M19 9.5v8M3.5 20h17",
  "wallet.pass": "M4 7.5A2.5 2.5 0 0 1 6.5 5h11A2.5 2.5 0 0 1 20 7.5v9a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5zM4 10h16M15 14.5h2",
  person: "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4.5 20.5a7.5 7.5 0 0 1 15 0",
  "person.2": "M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM2.5 20a6.5 6.5 0 0 1 13 0M16 4.3a3.5 3.5 0 0 1 0 6.4M17.5 13.5a6.5 6.5 0 0 1 4 6.5",
  creditcard: "M3 6.5h18v11H3zM3 10h18M6.5 14.5h4",
  banknote: "M2.5 7h19v10h-19zM12 14.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5zM5.5 10v4M18.5 10v4",
  bag: "M5 8h14l-1 12H6zM9 8V6.5a3 3 0 0 1 6 0V8",
  cart: "M3 4h2.5l2 11h10.5l2-7.5H6.5M9.5 20a1 1 0 1 0 0-2 1 1 0 0 0 0 2zM17 20a1 1 0 1 0 0-2 1 1 0 0 0 0 2z",
  briefcase: "M3.5 8h17v11h-17zM9 8V5.5h6V8M3.5 13h17",
  gift: "M4 9h16v4H4zM5.5 13v7.5h13V13M12 9v11.5M12 9c-1.5-3-5-3.5-5-1.2C7 9 12 9 12 9zM12 9c1.5-3 5-3.5 5-1.2C17 9 12 9 12 9z",
  heart: "M12 20s-7.5-4.6-7.5-10.2A4.3 4.3 0 0 1 12 7.2a4.3 4.3 0 0 1 7.5 2.6C19.5 15.4 12 20 12 20z",
  star: "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z",
  house: "M4 11 12 4.5 20 11M6 9.5v10h12v-10M10 19.5v-5h4v5",
  lock: "M6.5 11h11v9h-11zM8.5 11V8a3.5 3.5 0 0 1 7 0v3",
  bolt: "M13 3 5.5 13.5H12L11 21l7.5-10.5H12z",
  flame: "M12 21c-3.6 0-6-2.5-6-5.8 0-3.6 3.2-5.5 4-9.2 2.6 1.6 3.9 4 3.6 6.2 1-.6 1.7-1.6 1.9-2.8 1.6 1.6 2.5 3.6 2.5 5.8 0 3.3-2.4 5.8-6 5.8z",
};
/** Each with the words a search finds it by. */
const pickEmoji = [["🏦", "bank банк"], ["💼", "briefcase work работа"], ["🐋", "whale кит"], ["🚀", "rocket ракета"], ["💎", "gem diamond алмаз"], ["🔥", "fire огонь"],
  ["🐸", "frog лягушка"], ["🦄", "unicorn единорог"], ["🍀", "clover luck удача"], ["👑", "crown king корона"], ["🧊", "ice cube лёд"], ["🌙", "moon night луна"]];

function symbolGlyph(name, size) {
  const s = svg("svg", { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": "1.8", "stroke-linecap": "round", "stroke-linejoin": "round", "aria-hidden": "true" },
    svg("path", { d: pickSymbols[name] || "" }));
  s.classList.add("ico");
  s.style.cssText = `width:${size}px;height:${size}px`;
  return s;
}

/** A picture from the library: a logo, a glyph or an emoji on a soft disc, the person's own picture in a circle, or the
 * name's first letter. */
export function pickedIcon(ic, size, letter = "") {
  const disc = (kids, cls = "") => h("span", { class: "rail-picked " + cls, style: `width:${size}px;height:${size}px;font-size:${size * 0.56}px` }, kids);
  switch (ic?.kind) {
    case "logo": return logo(ic.value, size);
    case "symbol": return pickSymbols[ic.value] ? disc(symbolGlyph(ic.value, size * 0.5), "symbol") : disc(letter.slice(0, 1).toUpperCase(), "letter");
    case "emoji": return disc(ic.value, "emoji");
    case "image": return h("img", { class: "rail-picked image", src: ic.value, alt: "", draggable: "false", style: `width:${size}px;height:${size}px` });
    default: return disc(letter.slice(0, 1).toUpperCase(), "letter");
  }
}

/** Logos, glyphs and emoji in sections with a search and jumps, and the person's own picture: a file, a pasted image or a
 * site's icon. A pick applies at once; «Default» goes back to the sign of what it holds. `target`: {wallet} or {name}. */
export function iconPicker(anchor, current, name, target, op = "setIcon") {
  let chosen = current || null, query = "", error = "";
  const body = h("div", { class: "rail-picker" });
  const pick = async (ic) => {
    error = "";
    if (await act(op, { ...target, text: ic ? JSON.stringify(ic) : "" }) !== null) chosen = ic;
    draw();
  };
  const kind = (ic) => !ic ? t("Default", "По умолчанию") : ic.kind === "logo" ? t("Logo · ", "Логотип · ") + ic.value.toUpperCase()
    : ic.kind === "symbol" ? t("Symbol", "Символ") : ic.kind === "emoji" ? "Emoji" : t("Your picture", "Твоя картинка");
  const fromImage = (src) => new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      if (Math.max(img.naturalWidth, img.naturalHeight) < 32) { resolve(null); return; }
      // Square, cut from the middle, 128 px (smaller if a photo would make the PNG too big to keep).
      for (const side of [128, 96, 64]) {
        const c = document.createElement("canvas");
        c.width = c.height = side;
        const cut = Math.min(img.naturalWidth, img.naturalHeight);
        c.getContext("2d").drawImage(img, (img.naturalWidth - cut) / 2, (img.naturalHeight - cut) / 2, cut, cut, 0, 0, side, side);
        const url = c.toDataURL("image/png");
        if (url.length <= 80_000) { resolve(url); return; }
      }
      resolve(null);
    };
    img.onerror = () => resolve(null);
    img.src = src;
  });
  const usePicture = async (src) => {
    const url = await fromImage(src);
    if (!url) { error = t("That picture can't be used: it's too small or not an image.", "Эту картинку не поставить: она слишком маленькая или это не картинка."); draw(); return; }
    pick({ kind: "image", value: url });
  };
  const readFile = (file) => { if (!file) return; const r = new FileReader(); r.onload = () => usePicture(r.result); r.readAsDataURL(file); };
  const grid = (title, items) => items.length ? h("div", { class: "rail-pk-section" },
    h("div", { class: "rail-pk-label" }, h("span", { class: "eyebrow" }, title), h("span", { class: "tertiary small num" }, String(items.length))),
    h("div", { class: "rail-pk-grid" }, items)) : null;
  const cell = (ic, glyph, tip) => h("button", { class: "rail-pk-cell" + (chosen && chosen.kind === ic.kind && chosen.value === ic.value ? " on" : ""), title: tip || "", onclick: () => pick(ic) }, glyph);
  const sections = {};
  const draw = () => {
    const q = query.trim().toLowerCase();
    const logos = [...logoFiles.keys()].filter((k) => !k.startsWith("llm-") && (!q || k.includes(q)));
    const symbols = Object.keys(pickSymbols).filter((k) => !q || k.includes(q));
    const emoji = pickEmoji.filter(([, words]) => !q || words.includes(q));
    let site = "";
    const siteField = h("input", { class: "rail-pk-site", placeholder: t("website, e.g. bybit.com", "сайт, например bybit.com"), oninput: (e) => (site = e.target.value.trim()),
      onkeydown: (e) => { if (e.key === "Enter") getSite(); } });
    const getSite = async () => {
      if (!site.includes(".")) return;
      const r = await act("siteIcon", { address: site });
      if (r?.image) usePicture(r.image);
    };
    sections.logos = grid(t("TOKENS AND NETWORKS", "ТОКЕНЫ И СЕТИ"), logos.map((k) => cell({ kind: "logo", value: k }, logo(k, 24), k.toUpperCase())));
    sections.symbols = grid(t("SYMBOLS", "СИМВОЛЫ"), symbols.map((k) => cell({ kind: "symbol", value: k }, symbolGlyph(k, 16))));
    sections.emoji = grid("EMOJI", emoji.map(([e]) => cell({ kind: "emoji", value: e }, h("span", { class: "rail-pk-emoji" }, e))));
    const file = h("input", { type: "file", accept: "image/png,image/jpeg,image/svg+xml,image/webp,image/gif", hidden: true, onchange: (e) => readFile(e.target.files[0]) });
    sections.picture = h("div", { class: "rail-pk-section" },
      h("div", { class: "rail-pk-label" }, h("span", { class: "eyebrow" }, t("YOUR PICTURE", "ТВОЯ КАРТИНКА"))),
      file,
      way(t("Choose a file", "Выбрать файл"), t("PNG, JPG or SVG from this PC", "PNG, JPG или SVG с этого ПК"), "photo", () => file.click()),
      way(t("Paste an image", "Вставить картинку"), t("A copied picture or a screenshot · Ctrl+V", "Скопированная картинка или скриншот · Ctrl+V"), "doc.on.clipboard", async () => {
        try {
          for (const item of await navigator.clipboard.read()) {
            const type = item.types.find((x) => x.startsWith("image/"));
            if (type) { const r = new FileReader(); r.onload = () => usePicture(r.result); r.readAsDataURL(await item.getType(type)); return; }
          }
          error = t("No image on the clipboard.", "В буфере обмена нет картинки.");
        } catch { error = t("Press Ctrl+V here to paste the picture.", "Нажми Ctrl+V здесь, чтобы вставить картинку."); }
        draw();
      }),
      h("div", { class: "rail-pk-way static" },
        h("span", { class: "rail-pk-way-icon" }, icon("globe", 15)),
        h("span", { class: "rail-pk-way-text" }, h("b", {}, t("Logo from a website", "Логотип с сайта")), h("span", { class: "secondary small" }, t("The site's own icon", "Иконка самого сайта"))),
        h("div", { class: "rail-pk-site-row" }, siteField, h("button", { class: "btn small primary", onclick: getSite }, t("Get logo", "Взять логотип"))),
        h("span", { class: "secondary small" }, t("Asked of the site itself: nothing goes to an icon service.", "Иконку отдаёт сам сайт: никакой сервис иконок ничего не узнаёт."))),
      error ? h("div", { class: "loss small" }, error) : null);
    const scroller = h("div", { class: "rail-pk-scroll" }, sections.logos, sections.symbols, sections.emoji, sections.picture);
    const jump = (key, label, glyph) => h("button", { class: "rail-pk-jump", onclick: () => sections[key]?.scrollIntoView({ block: "start" }) }, icon(glyph, 13), label);
    const keepScroll = body.querySelector(".rail-pk-scroll")?.scrollTop || 0;
    body.replaceChildren(
      h("div", { class: "rail-pk-head" },
        h("span", { class: "rail-pk-now" }, chosen ? pickedIcon(chosen, 30, name) : h("span", { class: "rail-pk-none" })),
        h("span", { class: "rail-pk-title" }, h("b", { class: "one-line" }, name || t("Choose a picture", "Выбери картинку")), h("span", { class: "secondary small" }, kind(chosen))),
        h("button", { class: "btn small", disabled: !chosen, onclick: () => pick(null) }, t("Default", "По умолчанию"))),
      h("div", { class: "rail-pk-search" }, icon("magnifyingglass", 13),
        h("input", { id: "rail-pk-q", value: query, placeholder: t("Search logos, symbols, emoji", "Поиск: логотипы, символы, эмодзи"),
          oninput: (e) => { query = e.target.value; draw(); document.getElementById("rail-pk-q")?.focus(); } })),
      h("div", { class: "rail-pk-jumps" }, jump("logos", t("Logos", "Логотипы"), "circle.circle"), jump("symbols", t("Symbols", "Символы"), "star"),
        jump("emoji", "Emoji", "face.smiling"), jump("picture", t("Picture", "Картинка"), "photo")),
      scroller);
    scroller.scrollTop = keepScroll;
  };
  const way = (title, hint, glyph, run) => h("button", { class: "rail-pk-way", onclick: run },
    h("span", { class: "rail-pk-way-icon" }, icon(glyph, 15)),
    h("span", { class: "rail-pk-way-text" }, h("b", {}, title), h("span", { class: "secondary small" }, hint)), icon("chevron.right", 11));
  body.addEventListener("paste", (e) => {
    const file = [...(e.clipboardData?.files || [])].find((f) => f.type.startsWith("image/"));
    if (file) { e.preventDefault(); readFile(file); }
  });
  draw();
  popover(anchor, "right", body, "rail-picker-pop");
  body.tabIndex = -1;
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
            onclick: go(() => act("useAccount", { provider: p.id })) },
            companyLogo(p.id, 32, p.name),
            h("span", { class: "rail-pm-text" }, h("b", { class: "one-line" }, shortName(p)), h("span", { class: "secondary one-line" }, kindName(p))),
            inUse ? h("span", { class: "rail-pm-check" }, icon("check", 14)) : null),
          h("button", { class: "rail-pm-more", title: t("Add another account, sign out", "Добавить аккаунт, выйти"),
            onclick: () => draw(null, open === p.id ? null : p.id) }, icon("ellipsis", 16)),
          open === p.id ? h("div", { class: "rail-pm-actions" },
            inUse ? null : h("button", { onclick: go(() => act("useAccount", { provider: p.id })) }, icon("arrow.left.arrow.right", 13), t(`Switch to ${shortName(p)}`, `Переключиться на ${shortName(p)}`)),
            h("button", { onclick: go(() => { store("settingsTab", "model"); ui.view = "settings"; render(); }) }, icon("plus", 13), t("Add another account…", "Добавить другой аккаунт…")),
            p.kind === "local" ? null : h("button", { onclick: () => draw(p) }, icon("rectangle.portrait.and.arrow.right", 13), t(`Sign out of ${shortName(p)}…`, `Выйти из ${shortName(p)}…`))) : null);
      }),
      h("button", { class: "rail-pm-add", onclick: go(() => { store("settingsTab", "model"); ui.view = "settings"; render(); }) },
        h("span", { class: "rail-pm-add-box" }, icon("plus", 14)), h("b", { class: "secondary" }, t("Add an account", "Добавить аккаунт"))),
      h("div", { class: "menu-divider" }),
      h("div", { class: "rail-pm-apps" },
        h("button", { class: "rail-pm-app", title: t("Settings · Ctrl+,", "Настройки · Ctrl+,"), onclick: go(() => { ui.view = "settings"; render(); }) }, icon("gear", 15), t("Settings", "Настройки")),
        h("button", { class: "rail-pm-app", title: t("The dApp browser, with your wallet", "Браузер для dApp, с твоим кошельком"), onclick: go(browserDialog) }, icon("globe", 15), t("Browser", "Браузер")),
        h("button", { class: "rail-pm-app", title: t("What's new in ihatecrypto", "Что нового в ihatecrypto"), onclick: go(() => openWhatsNew()) },
          icon("sparkles", 15), t("What's new", "Что нового"), whatsNewUnseen() ? h("i", { class: "rail-new-dot" }) : null)));
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
    roundButton("plus", t("New", "Новый"), { dashed: true, help: t("New canvas", "Новый холст"), run: () => { if (!canSwitch()) { toast(busyTip()); return; } ui.view = "canvas"; act("newCanvas"); } }),
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


/** Another canvas may open: nothing is being answered or carried out (the Mac's canSwitchCanvas), so a reply or a move's
 *  outcome lands on the canvas it was asked on. */
function canSwitch() { const s = ui.state; return !s.thinking && !s.runningBoard; }
function busyTip() {
  return ui.state.runningBoard ? t("A move is running: wait for it to finish.", "Идёт перевод: дождись, пока он закончится.")
    : t("The chat is still answering: wait for it, or press Stop.", "Чат ещё отвечает: подожди или нажми «Стоп».");
}
