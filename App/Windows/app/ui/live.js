// The live blocks on the canvas: what they show is read from the state each time. They follow the Mac's boards:
// Receive's address cards (Receive.swift), Favorites with Tokens | Wallets (AddressBook.swift, TokensUI.swift), the
// History board (HistoryView.swift), price alerts as a chain (Alerts.swift), the Schedule frame with its date tiles
// and the automation wallet (ScheduleFrame.swift), positions (Studio.swift PositionBoard), the multisig
// (Multisig.swift) and the Ledger's connect board (Ledger.swift). A button here never moves money: it puts the command
// in the chat field (`suggest`), and the chat's card waits for Confirm.

import { ui, act, engine, render, t, h, svg, icon, addIcons, logo, usd, signedUsd, masked, popup, suggest, store, presence, invoke, toast, qrCode } from "./core.js";
import { blockFrame, tradePhrases } from "./boards.js";
// The picture picker and its drawing are the rail's (wallets and canvases use them too); read when drawn.
import * as rail from "./rail.js";

addIcons({
  bell: "M6 16v-5a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0",
  "arrow.down": "M12 5v14M6 13l6 6 6-6",
  paperplane: "M21 3 3 10.5l7 3 3 7zM10 13.5 21 3",
  bubble: "M4 5h16v11H9l-5 4z",
  "shield.check": "M12 3l7.5 3v5.5c0 4.5-3.2 8.2-7.5 9.5-4.3-1.3-7.5-5-7.5-9.5V6zM8.8 12l2.2 2.2 4.4-4.4",
  "shield.exclam": "M12 3l7.5 3v5.5c0 4.5-3.2 8.2-7.5 9.5-4.3-1.3-7.5-5-7.5-9.5V6zM12 8v5M12 16v.5",
  "hand.tap": "M9 11V5.5a1.5 1.5 0 0 1 3 0V11M12 10.5V9a1.5 1.5 0 0 1 3 0v2M15 10.5a1.5 1.5 0 0 1 3 0V15a6 6 0 0 1-6 6h-.5a6 6 0 0 1-4.6-2.2l-2.4-3.3a1.5 1.5 0 0 1 2.3-1.9L9 16",
  "checkmark.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 12.5l3 3 5-6",
  "circle.dotted": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z",
  laptop: "M5 6h14v9H5zM3 18h18",
  iphone: "M8 3h8a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM11 18h2",
  drive: "M3 9h18v6H3zM6.5 12h5M16.5 12h.01",
  fuelpump: "M5 20V5a1 1 0 0 1 1-1h7a1 1 0 0 1 1 1v15M4 20h11M5 10h9M14 8l3 2.5V17a1.5 1.5 0 0 0 3 0V9l-2.5-2.5",
  "arrow.up.right.square": "M11 5H5v14h14v-6M13 5h6v6M19 5l-8 8",
  "arrow.left.arrow.right": "M4 8h14M14 4l4 4-4 4M20 16H6M10 12l-4 4 4 4",
  "arrow.up.right": "M7 17 17 7M9 7h8v8",
  "arrow.down.left": "M17 7 7 17M15 17H7V9",
  "arrow.up": "M12 19V5M6 11l6-6 6 6",
  play: "M8 5l11 7-11 7z",
  pause: "M8 5v14M16 5v14",
  signature: "M3 17c3-1 5-9 7-9s-1 9 1 9 3-4 4-4 1 3 3 3M3 20.5h18",
  key: "M15 3.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11zM11 13 3.5 20.5M6.5 17.5 9 20",
  "chart.line": "M4 18l5-6 4 3 7-8",
  building: "M4 20h16M5 10h14L12 4zM7 10v8M12 10v8M17 10v8",
  wrench: "M14.5 5.5a4 4 0 0 0 4.8 5.2L12 18l-2.5.5L10 16l7.3-7.3a4 4 0 0 0-2.8-3.2z",
  "clock.arrow": "M4.5 12a7.5 7.5 0 1 0 2.2-5.3M4.5 4.5v4h4M12 8v4.5l3 2",
  "xmark.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 9l6 6M15 9l-6 6",
  "minus.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 12h8",
  "xmark.octagon": "M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3zM9.5 9.5l5 5M14.5 9.5l-5 5",
  "person.crop.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7.5a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM6.5 18.2c1.2-2 3.2-3 5.5-3s4.3 1 5.5 3",
  pencil: "M15 4.5l4.5 4.5L8.5 20H4v-4.5zM13 6.5l4.5 4.5",
  photo: "M4 5h16v14H4zM4 16l5-5 4 4 2.5-2.5L20 17M15.5 9a1.5 1.5 0 1 0 0-.01",
  repeat: "M17 3l4 4-4 4M3 11V10a3 3 0 0 1 3-3h15M7 21l-4-4 4-4M21 13v1a3 3 0 0 1-3 3H3",
});

export const liveKinds = ["receive", "addresses", "history", "favorites", "alerts", "schedule", "positions", "multisig", "ledger", "scheduleItem"];

export function liveTitle(kind) {
  return {
    receive: t("Receive", "Получить"), addresses: t("Address book", "Адресная книга"), history: t("History · latest", "История · последнее"),
    favorites: t("Favorites", "Избранное"), alerts: t("Price alerts", "Ценовые оповещения"), schedule: t("Schedule", "Расписание"),
    positions: t("Open positions", "Открытые позиции"), multisig: t("Multisig", "Мультисиг"), ledger: "Ledger",
    scheduleItem: t("Scheduled task", "Задача по расписанию"),
  }[kind] || kind;
}

/** Each block's width and header icon, as the Mac's boards have them (470, the Schedule 320, the multisig 492). */
const frames = {
  receive: { width: 470, icon: "arrow.down.circle" }, addresses: { width: 470, icon: "person" }, history: { width: 470, icon: "clock.arrow" },
  favorites: { width: 470, icon: "star" }, alerts: { width: 470, icon: "bell" }, schedule: { width: 320, icon: "calendar" },
  positions: { width: 492, icon: "chart.line" }, multisig: { width: 492, icon: "key" }, ledger: { width: 470, icon: "drive" },
  scheduleItem: { width: 500, icon: "calendar" },
};

export function liveBlock(b) {
  const body = { receive: receiveBody, addresses: addressesBody, history: historyBody, favorites: favoritesBody, alerts: alertsBody,
    schedule: scheduleBody, positions: positionsBody, multisig: multisigBody, ledger: ledgerBody, scheduleItem: scheduleItemBody }[b.kind];
  // The Ledger's frame names the model it found and spins while it connects, as the Mac's board header does.
  const c = b.kind === "ledger" ? ui.state.ledgerConnect : null;
  const item = b.kind === "scheduleItem" ? scheduleItemHead(b.title) : null;
  const title = c?.model ? "Ledger · " + c.model : item ? item.title : liveTitle(b.kind);
  const extra = c?.busy ? h("span", { class: "live-spinner", "aria-label": t("Connecting", "Подключаю") }) : item ? item.extra : undefined;
  const el = blockFrame(b, title, h("div", { class: "live live-" + b.kind }, body ? body(b) : null), { ...(frames[b.kind] || {}), extra });
  el.classList.add("live-frame", "live-frame-" + b.kind);
  return el;
}

// MARK: Pieces the blocks share

/** A network's logo file and name. */
const nets = {
  solana: ["sol", "Solana"], base: ["base", "Base"], arbitrum: ["arb", "Arbitrum"], ethereum: ["eth", "Ethereum"], polygon: ["pol", "Polygon"],
  optimism: ["op", "Optimism"], bnb: ["bnb", "BNB Chain"], hyperliquid: ["hype", "Hyperliquid"], hyperevm: ["hyperevm", "HyperEVM"],
  robinhood: ["robinhood", "Robinhood Chain"], tempo: ["tempo", "Tempo"], near: ["near", "NEAR"], bitcoin: ["btc", "Bitcoin"],
  tron: ["trx", "Tron"], xrp: ["xrp", "XRP"], dogecoin: ["doge", "Dogecoin"], zcash: ["zec", "Zcash"], sui: ["sui", "Sui"],
};
const netLogo = (key, size) => logo(nets[key]?.[0] || key, size);
const netName = (key) => nets[key]?.[1] || key;

/** An address as it's checked by eye (AddressLines): the whole of it in even lines, its ends in bold. */
function addressLines(address, maxChars = 30) {
  const counts = [2, 3, 4, 5, 6].sort((a, b) => (address.length % a ? 1 : 0) - (address.length % b ? 1 : 0) || a - b);
  const n = address.length <= maxChars ? 1 : counts.find((c) => Math.ceil(address.length / c) <= maxChars) || 6;
  const head = Math.min(8, Math.floor(address.length / 2)), tail = address.length - head;
  const starts = Array.from({ length: n + 1 }, (_, i) => i * Math.floor(address.length / n) + Math.min(i, address.length % n));
  const part = (a, b) => (a < b ? address.slice(a, b) : "");
  return h("div", { class: "live-addr-lines selectable", title: address },
    starts.slice(0, n).map((a, i) => {
      const b = starts[i + 1];
      return h("div", {}, h("b", {}, part(a, Math.min(b, head))), h("span", { class: "secondary" }, part(Math.max(a, head), Math.min(b, tail))), h("b", {}, part(Math.max(a, tail), b)));
    }));
}

/** «Copy address»: the label turns to «Copied» for a moment, in place (CopyButton). */
function copyAddress(text, label = t("Copy address", "Скопировать адрес")) {
  const face = (ic, words) => [icon(ic, 12), h("span", {}, words)];
  const btn = h("button", { class: "live-copy-btn", title: t("Copy the whole address", "Скопировать весь адрес") }, face("copy", label));
  btn.addEventListener("click", async (e) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(text);
      btn.classList.add("copied");
      btn.replaceChildren(...face("check", t("Copied", "Скопировано")));
      setTimeout(() => { btn.classList.remove("copied"); btn.replaceChildren(...face("copy", label)); }, 1500);
    } catch { toast(t("Couldn't copy.", "Не скопировалось.")); }
  });
  return btn;
}

/** The empty state: a quiet line in a dashed frame. */
const note = (text) => h("div", { class: "live-note" }, text);
const section = (text, trailing) => h("div", { class: "live-section" }, h("span", {}, text), trailing || null);
const hint = (text, ic) => h("div", { class: "live-hint" }, ic ? icon(ic, 12) : null, h("span", {}, text));

/** The next message is about this block: it goes with the message (right-click › Ask in chat does the same). */
function askAbout(b) {
  ui.attached = b.id;
  render();
  document.getElementById("composer")?.focus();
}

/** A pasted-in URL opens in the user's browser. */
const openLink = (url) => invoke("open_page", { url }).catch((x) => toast(String(x)));

/** A compact dollar amount: $18.4M, $51K. */
function compact(n) {
  const v = Number(n);
  if (!isFinite(v)) return "—";
  if (v >= 1e9) return "$" + (v / 1e9).toFixed(1) + "B";
  if (v >= 1e6) return "$" + (v / 1e6).toFixed(1) + "M";
  if (v >= 1e3) return "$" + Math.round(v / 1e3) + "K";
  return usd(v);
}
/** A token's price: cents above a dollar, three significant digits under it. */
const price = (p) => (p === null || p === undefined ? "—" : Number(p) >= 1 ? usd(p) : "$" + Number(p).toLocaleString("en-US", { maximumSignificantDigits: 3 }));
const signedPct = (n) => (Number(n) < 0 ? "−" : "+") + Math.abs(Number(n)).toFixed(1) + "%";
const negative = (text) => /^\s*[−-]/.test(text || "");

// MARK: Receive

function receiveBody() {
  const a = ui.state.addresses;
  const cards = [
    a.solana && { logos: ["solana"], title: "Solana", badge: t("Recommended", "Рекомендую"), address: a.solana, tint: "solana",
      what: t("USDC and SOL, and any Solana token. Every move starts here.", "USDC и SOL и любые токены Solana. С них начинается каждый перевод.") },
    a.evm && { logos: ["base", "arbitrum", "ethereum", "polygon", "hyperliquid"], title: t("EVM networks", "EVM-сети"), address: a.evm, tint: "evm",
      what: t("Base · Arbitrum · Ethereum · Polygon · Optimism · BNB · Hyperliquid: the same address on each. Send on the network you mean.",
        "Base · Arbitrum · Ethereum · Polygon · Optimism · BNB · Hyperliquid: один адрес во всех. Отправляй в той сети, которую имеешь в виду.") },
    a.near && { logos: ["near"], title: "NEAR", address: a.near, tint: "plain", what: t("Native NEAR on the NEAR chain.", "NEAR в сети NEAR.") },
  ].filter(Boolean);
  return [
    h("p", { class: "live-lead" }, t("ihatecrypto can't pull money in: send it here from an exchange or another wallet, on that network.",
      "ihatecrypto не может сам забрать деньги: отправь их сюда с биржи или другого кошелька, именно в этой сети.")),
    cards.length ? cards.map((c) => h("div", { class: "live-receive-card tint-" + c.tint },
      h("div", { class: "live-receive-qr", title: t("Scan with the exchange or wallet app", "Сканируй приложением биржи или кошелька") },
        qrCode(c.address, "live-qr"), h("span", { class: "live-receive-qr-logo" }, netLogo(c.logos[0], 26))),
      h("div", { class: "live-receive-text" },
        h("div", { class: "live-receive-head" },
          h("span", { class: "live-logo-stack" }, c.logos.map((k) => netLogo(k, 20))),
          h("b", {}, c.title),
          c.badge ? h("span", { class: "live-badge" }, c.badge) : null),
        h("div", { class: "live-receive-what" }, c.what),
        addressLines(c.address, 26),
        copyAddress(c.address))))
      : note(t("No wallet on this computer yet: add one in Settings › Wallets.", "На этом компьютере пока нет кошелька: добавь его в Настройках › Кошельки.")),
    h("button", { class: "link-btn live-more", onclick: () => { ui.view = "topup"; render(); } }, icon("arrow.down.circle", 12), t("Top up: amounts, a wallet app or an exchange", "Пополнить: сумма, кошелёк или биржа")),
  ];
}

// MARK: Saved addresses (the address book and the Wallets tab)

/** Rows opened in place, by block, name and network. */
const openRows = new Set();
const bookKey = (e) => e.network + ":" + e.address.toLowerCase();

/** What the book keeps beside a saved address (its picture, category, note): the engine's address-extras. */
const extraOf = (e) => ui.state.addressExtras?.[e.network + ":" + e.name.toLowerCase()] || {};

/** A saved address's picture: the one picked (drawn as the rail draws canvas pictures), else its first letter. */
function addressPic(e, size) {
  const ic = extraOf(e).icon;
  if (ic && rail.pickedIcon) return rail.pickedIcon(ic, size, e.name);
  if (ic?.kind === "logo") return logo(ic.value, size);
  if (ic?.kind === "image") return h("img", { class: "live-addr-img", src: ic.value, alt: "", draggable: "false", style: `width:${size}px;height:${size}px` });
  if (ic?.kind === "emoji") return h("span", { class: "live-letter-disc", style: `font-size:${Math.round(size * 0.56)}px` }, ic.value);
  return h("span", { class: "live-letter-disc" }, (e.name || "?").slice(0, 1).toUpperCase());
}

/** Opens the rail's picture picker beside `anchor` for a saved address; a pick is kept at once. */
function pickPicture(anchor, e) {
  if (!rail.iconPicker) return toast(t("Pictures for addresses need the newer rail.", "Картинки для адресов появятся с новой панелью."));
  rail.iconPicker(anchor, extraOf(e).icon, e.name, { name: e.name, network: e.network }, "addressIcon");
}

/** An address as a row (AddressCard): its picture, name and network; a click opens the whole address with Copy and
 * what to do with it. `own`: one of the wallet's own addresses, read-only. */
function addressCard(e, b, { own = false } = {}) {
  const key = b.id + ":" + (own ? "own:" : "") + e.network + ":" + e.name;
  const open = openRows.has(key);
  const starred = !own && ui.state.starred.includes(bookKey(e));
  const extra = own ? {} : extraOf(e);
  const toggle = () => { open ? openRows.delete(key) : openRows.add(key); render(); };
  const sendPhrase = `send 10 USDC to ${e.name}` + (e.address.startsWith("0x") ? ` on ${e.network}` : "");
  const menu = (ev) => {
    ev.preventDefault();
    const row = ev.currentTarget;
    popup(ev, [
      ...(own ? [] : [[t("Send…", "Отправить…"), () => suggest(sendPhrase)]]),
      [t("Ask in chat", "Спросить в чате"), () => askAbout(b)],
      [t("Copy address", "Скопировать адрес"), () => navigator.clipboard.writeText(e.address).catch(() => {})],
      ...(own ? [] : [null,
        [starred ? t("Unpin from the top", "Открепить") : t("Pin to the top", "Закрепить наверху"), () => act("star", { name: e.name, network: e.network, test: !starred })],
        [t("Edit…", "Изменить…"), () => editAddress(e)],
        [t("Change picture…", "Сменить картинку…"), () => pickPicture(row, e)],
        [t("Delete", "Удалить"), () => act("removeAddress", { name: e.name, network: e.network })]]),
    ]);
  };
  return h("div", { class: "live-addr-card" + (open ? " open" : ""), oncontextmenu: menu },
    h("div", { class: "live-addr-row" },
      h("button", { class: "live-addr-main", onclick: toggle, "aria-expanded": open ? "true" : "false" },
        h("span", { class: "live-addr-pic" },
          own ? netLogo(e.network, 30) : addressPic(e, 30),
          own ? null : h("span", { class: "live-addr-net" }, netLogo(e.network, 14))),
        h("span", { class: "live-addr-text" },
          h("span", { class: "live-addr-name one-line" }, e.name),
          h("span", { class: "live-addr-sub one-line" }, own ? t("Your wallet", "Твой кошелёк") + " · " + netName(e.network)
            : [extra.category, netName(e.network), e.memo ? "memo" : null].filter(Boolean).join(" · "))),
        own ? null : h("span", { class: "live-addr-shield", title: t(`Saved with ${presence()}`, `Сохранён через ${presence()}`) }, icon("shield.check", 13)),
        h("span", { class: "live-addr-chevron" }, icon(open ? "chevron.up" : "chevron.down", 11))),
      own ? null : h("button", { class: "icon-btn live-star-btn" + (starred ? " on" : ""), title: starred ? t("Unpin from the top", "Открепить") : t("Pin to the top", "Закрепить наверху"),
        onclick: () => act("star", { name: e.name, network: e.network, test: !starred }) }, icon("star", 15))),
    open ? h("div", { class: "live-addr-details" },
      addressLines(e.address, 32),
      copyAddress(e.address),
      e.memo ? h("div", { class: "live-addr-memo" }, "# ", t("Memo / tag: ", "Memo / тег: "), h("span", { class: "selectable" }, e.memo)) : null,
      extra.note ? h("div", { class: "live-addr-note secondary" }, extra.note) : null,
      own ? null : h("div", { class: "live-addr-ok" }, icon("shield.check", 12),
        t(`Saved with ${presence()}: money goes only to this address`, `Сохранён через ${presence()}: деньги уходят только на этот адрес`)),
      h("div", { class: "btns" },
        own ? h("button", { class: "btn primary small", onclick: () => { ui.view = "topup"; render(); } }, icon("arrow.down.circle", 13), t("Top up", "Пополнить"))
          : h("button", { class: "btn primary small", title: t(`Puts «${sendPhrase}» in the chat, the amount selected`, `Кладёт «${sendPhrase}» в чат, сумма выделена`), onclick: () => suggest(sendPhrase) },
            icon("paperplane", 13), t("Send", "Отправить")),
        h("button", { class: "btn small", onclick: () => askAbout(b) }, icon("bubble", 13), t("Ask in chat", "Спросить в чате")),
        own ? null : h("button", { class: "btn small", onclick: () => editAddress(e) }, icon("pencil", 13), t("Edit", "Изменить")),
        own ? null : h("button", { class: "btn small live-danger", onclick: () => act("removeAddress", { name: e.name, network: e.network }) }, icon("trash", 13), t("Delete", "Удалить")))) : null);
}

/** Rows in one frame, a line between them. */
const list = (rows) => h("div", { class: "live-list" }, rows);

const networks = ["solana", "base", "arbitrum", "ethereum", "polygon", "optimism", "bnb", "hyperliquid", "hyperevm", "robinhood", "tempo", "near",
  "bitcoin", "tron", "xrp", "dogecoin", "zcash", "sui"];
const categories = [["Exchange", "Биржа"], ["Wallet", "Кошелёк"], ["Friend", "Друг"], ["DeFi", "DeFi"], ["Work", "Работа"]];
/** Kept across redraws while the person types; cleared after a save. `editing`: the saved entry being changed. */
const blankForm = { name: "", network: "solana", address: "", memo: "", category: "", note: "", favorite: false, editing: null, check: null };
const addressForm = { ...blankForm };

/** The form takes a saved address to change (the Mac's Edit…): every field filled, the address checked again. */
function editAddress(e) {
  const x = extraOf(e);
  Object.assign(addressForm, { ...blankForm, name: e.name, network: e.network, address: e.address, memo: e.memo || "", category: x.category || "", note: x.note || "",
    favorite: ui.state.starred.includes(bookKey(e)), editing: { name: e.name, network: e.network } });
  render();
  checkForm(0);
  const field = document.getElementById("book-name");
  field?.scrollIntoView({ block: "center", behavior: "smooth" });
  field?.focus();
}

/** The line under the address as it's typed (AddressForm.status): what's wrong with it, or its ends to compare with the
 * source, a duplicate, the wallet's own, and an exchange's network warning. Never corrects the address. */
function addressStatus() {
  const f = addressForm, c = f.check;
  const line = (ic, cls, text) => h("div", { class: "live-book-line " + cls }, icon(ic, 12), h("span", {}, text));
  if (!f.address.trim()) return [line("doc.on.clipboard", "secondary", t("Copy the address from the exchange or wallet itself, then press Paste.", "Скопируй адрес в самой бирже или кошельке, потом нажми «Вставить»."))];
  if (!c) return [];
  if (c.problem) return [line("xmark.octagon", "loss", c.problem)];
  return [
    line("checkmark.seal", "profit", t(`Valid ${netName(f.network)} address. Compare it with the source: starts ${c.start}, ends ${c.end}.`,
      `Адрес ${netName(f.network)} верный. Сверь его с источником: начало ${c.start}, конец ${c.end}.`)),
    c.duplicate ? line("exclamationmark.triangle", "live-warn", t(`Already saved as «${c.duplicate}».`, `Уже сохранён как «${c.duplicate}».`)) : null,
    c.own ? line("person.crop.circle", "secondary", t("This is your own wallet.", "Это твой собственный кошелёк.")) : null,
    /exchange|бирж/i.test(f.category) ? line("exclamationmark.triangle", "secondary", t("An exchange credits a deposit only on the network it shows. Sent on another network, the money can be lost.",
      "Биржа зачисляет пополнение только в той сети, которую показывает. Отправленные в другой сети деньги могут пропасть.")) : null,
  ].filter(Boolean);
}

const canSave = () => !!addressForm.name.trim() && !!addressForm.check && !addressForm.check.problem && !addressForm.check.duplicate;

/** The status line and Save follow the typing in place: a redraw would take the focus from the field. */
function drawStatus() {
  document.getElementById("book-status")?.replaceChildren(...addressStatus());
  const save = document.getElementById("book-save");
  if (save) save.disabled = !canSave();
}

let checkTimer = 0, checkSeq = 0;
/** The engine checks the address for its network, its duplicates and the wallet's own (reads only). */
function checkForm(delay = 200) {
  clearTimeout(checkTimer);
  const seq = ++checkSeq, f = addressForm;
  if (!f.address.trim()) { f.check = null; drawStatus(); return; }
  checkTimer = setTimeout(async () => {
    let r = null;
    try { r = await engine("checkAddress", { address: f.address.trim(), network: f.network, label: f.editing?.name }); } catch { r = null; }
    if (seq !== checkSeq) return;
    f.check = r;
    drawStatus();
  }, delay);
}

/** A new entry, or a changed name, address, network or memo, goes into the signer's book first (Windows Hello, the
 * whole address on its window); the category, the note and the star change at once. */
async function saveForm() {
  const f = addressForm, entry = { name: f.name.trim(), network: f.network, address: f.address.trim(), memo: f.memo.trim() };
  const done = f.editing ? await act("editAddress", { ...entry, label: f.editing.name, text: f.editing.network }) : await act("saveAddress", entry);
  if (done === null) return;
  await act("addressExtra", { name: entry.name, network: entry.network, label: f.category.trim(), text: f.note.trim() });
  if (f.favorite !== ui.state.starred.includes(bookKey(entry))) await act("star", { name: entry.name, network: entry.network, test: f.favorite });
  Object.assign(addressForm, { ...blankForm, network: f.network });
  toast(t("Saved.", "Сохранено."));
  render();
}

async function pasteAddress() {
  const field = document.getElementById("book-address");
  try { addressForm.address = (await navigator.clipboard.readText()).trim(); } catch { field?.focus(); toast(t("Press Ctrl+V in the field.", "Нажми Ctrl+V в поле.")); return; }
  if (field) field.value = addressForm.address;
  checkForm(0);
}

function addressesBody(b) {
  const form = addressForm;
  const starred = new Set(ui.state.starred);
  const saved = [...ui.state.book].sort((x, y) => (starred.has(bookKey(y)) - starred.has(bookKey(x))) || x.name.localeCompare(y.name));
  const exchange = /exchange|бирж/i.test(form.category);
  const editing = form.editing && ui.state.book.find((e) => e.name.toLowerCase() === form.editing.name.toLowerCase() && e.network === form.editing.network);
  // The check runs again after a redraw with an address typed but not checked yet (a new state, a reopened block).
  if (form.address.trim() && !form.check) queueMicrotask(() => checkForm(0));
  return [
    h("p", { class: "live-lead" }, t(`Money goes only to saved addresses. Saving one asks ${presence()} with the whole address on its window: check every character.`,
      `Деньги уходят только на сохранённые адреса. Сохранение спрашивает ${presence()}, и в его окне весь адрес — сверь каждый символ.`)),
    saved.length ? list(saved.map((e) => addressCard(e, b)))
      : note(t("No saved addresses yet. Save an exchange's deposit address, another wallet or a friend's — with a name, so you never paste a wrong one.",
        "Сохранённых адресов пока нет. Сохрани адрес пополнения биржи, другой кошелёк или адрес друга — с именем, чтобы никогда не вставить чужой.")),
    section(editing ? t(`Edit «${editing.name}»`, `Изменить «${editing.name}»`) : t("Add an address", "Добавить адрес"),
      editing ? h("button", { class: "live-book-pic", title: t("Choose a picture: a logo, an emoji or your own image", "Выбрать картинку: логотип, эмодзи или своё изображение"),
        onclick: (ev) => pickPicture(ev.currentTarget, editing) }, addressPic(editing, 26), h("span", { class: "live-book-pen" }, icon("pencil", 9))) : null),
    h("div", { class: "live-addr-form" },
      h("div", { class: "live-field-row" },
        h("div", { class: "field" }, h("label", { for: "book-name" }, t("Name", "Имя")),
          h("input", { id: "book-name", value: form.name, placeholder: t("e.g. Bybit · USDC deposit", "например, Bybit · пополнение USDC"), oninput: (e) => { form.name = e.target.value; drawStatus(); } })),
        h("div", { class: "field" }, h("label", { for: "book-network" }, t("Network", "Сеть")),
          h("select", { id: "book-network", onchange: (e) => { form.network = e.target.value; checkForm(0); } }, networks.map((n) => h("option", { value: n, selected: n === form.network }, netName(n)))))),
      h("div", { class: "field" }, h("label", { for: "book-category" }, t("Category", "Категория")),
        h("div", { class: "live-book-cats" },
          h("input", { id: "book-category", value: form.category, placeholder: t("optional", "необязательно"), oninput: (e) => { form.category = e.target.value; drawStatus(); } }),
          categories.map(([en, ru]) => { const c = t(en, ru); return h("button", { class: "live-cat" + (form.category === c ? " on" : ""), onclick: () => { form.category = form.category === c ? "" : c; render(); } }, c); }))),
      h("div", { class: "field" }, h("label", { for: "book-address" }, t("Address", "Адрес")),
        h("div", { class: "live-book-paste" },
          h("input", { id: "book-address", class: "address", value: form.address, autocomplete: "off", spellcheck: "false", oninput: (e) => { form.address = e.target.value; checkForm(); } }),
          h("button", { class: "btn small", onclick: pasteAddress }, t("Paste", "Вставить")))),
      h("div", { id: "book-status", class: "live-book-status" }, addressStatus()),
      h("div", { class: "field" }, h("label", { for: "book-memo" }, t("Memo / tag", "Memo / тег")),
        h("input", { id: "book-memo", value: form.memo, placeholder: exchange ? t("only if the exchange shows one", "только если биржа его показывает") : t("optional", "необязательно"),
          oninput: (e) => (form.memo = e.target.value) })),
      h("div", { class: "field" }, h("label", { for: "book-note" }, t("Note", "Заметка")),
        h("input", { id: "book-note", value: form.note, placeholder: t("optional", "необязательно"), oninput: (e) => (form.note = e.target.value) })),
      h("label", { class: "live-book-fav" }, h("input", { type: "checkbox", checked: form.favorite, onchange: (e) => (form.favorite = e.target.checked) }), t("Pin to the top", "Закрепить наверху")),
      h("div", { class: "btns" },
        form.editing || form.name || form.address ? h("button", { class: "btn", onclick: () => { Object.assign(addressForm, { ...blankForm }); render(); } }, t("Cancel", "Отмена")) : null,
        h("button", { id: "book-save", class: "btn primary", disabled: !canSave(), onclick: saveForm }, icon("shield.check", 14),
          form.editing ? t("Save changes", "Сохранить изменения") : t(`Save with ${presence()}`, `Сохранить через ${presence()}`)))),
  ];
}

// MARK: History

const kindIcon = (k) => ({ transfer: "arrow.up.right", send: "arrow.up.right", receive: "arrow.down.left", swap: "arrow.left.arrow.right", buy: "arrow.left.arrow.right",
  sell: "arrow.left.arrow.right", bridge: "arrow.left.arrow.right", deposit: "building", withdraw: "arrow.down.left", order: "chart.line", sign: "signature" }[k] || "signature");

function dayName(d) {
  const today = new Date(), y = new Date(today); y.setDate(today.getDate() - 1);
  const same = (a, b) => a.toDateString() === b.toDateString();
  return same(d, today) ? t("Today", "Сегодня") : same(d, y) ? t("Yesterday", "Вчера") : d.toLocaleDateString(dateLocale(), { day: "numeric", month: "short" });
}

/** The History block (the Mac's HistoryBoard): what the latest day did to the money with a question for the chat, then
 * each move as one row, by day; a row opens to the request it came from. Nothing moved yet: the signatures as they were. */
const openMoves = new Set();
const moveIcon = (k) => ({ sent: "arrow.up.right", received: "arrow.down.left", swapped: "arrow.left.arrow.right", bought: "arrow.left.arrow.right",
  sold: "arrow.left.arrow.right", moved: "arrow.right", deposited: "building", withdrew: "arrow.down.left", closed: "xmark" }[k] || "signature");

/** «Closed · 7 days» (the Mac's ClosedTrades): Hyperliquid's trades closed this week and what they made; a row opens
 * History › Positions. */
let closedAll = false;
function closedTrades() {
  const list = ui.state.recentlyClosed || [];
  if (!list.length) return null;
  const total = list.reduce((sum, x) => sum + Number(x.realized), 0);
  const when = (iso) => {
    const d = new Date(iso), l = dateLocale();
    return d.toLocaleDateString(l, { month: "short", day: "numeric" }) + (ui.state.russian ? " в " : " at ") + d.toLocaleTimeString(l, { hour: "numeric", minute: "2-digit" });
  };
  return h("div", { class: "live-closed" },
    h("div", { class: "live-closed-head" }, h("span", { class: "secondary" }, t("Closed · 7 days", "Закрыто · 7 дней")), h("span", { class: "spacer" }),
      h("b", { class: "num " + (total < 0 ? "loss" : "profit") }, masked(signedUsd(total)))),
    (closedAll ? list : list.slice(0, 4)).map((x) => h("button", { class: "live-closed-row", title: t("Open in History → Positions", "Открыть в Истории → Позиции"),
      onclick: () => { store("historyTab", "positions"); ui.view = "history"; render(); } },
      assetBadge(x.coin, 20),
      h("span", { class: "live-closed-text" }, h("b", { class: "one-line" }, `${x.long ? t("Long", "Лонг") : t("Short", "Шорт")} ${x.name}`), h("span", { class: "secondary" }, when(x.closed))),
      h("span", { class: "spacer" }),
      h("b", { class: "num " + (Number(x.realized) < 0 ? "loss" : "profit") }, masked(signedUsd(x.realized))))),
    list.length > 4 ? h("button", { class: "live-closed-more", onclick: () => { closedAll = !closedAll; render(); } }, icon(closedAll ? "chevron.up" : "chevron.down", 10),
      closedAll ? t("Show less", "Свернуть") : t(`Show ${list.length - 4} more`, `Показать ещё ${list.length - 4}`)) : null);
}

function historyBody() {
  const s = ui.state, moves = s.moves || [], day = s.movesDay;
  const time = (d) => d.toLocaleTimeString(dateLocale(), { hour: "2-digit", minute: "2-digit" });
  const out = [];
  const closed = closedTrades();
  if (closed) out.push(closed);
  if (day) {
    const d = new Date(day.day), today = new Date(), y = new Date(today); y.setDate(today.getDate() - 1);
    const when = d.toDateString() === today.toDateString() ? t("today", "сегодня") : d.toDateString() === y.toDateString() ? t("yesterday", "вчера") : dayName(d);
    out.push(h("div", { class: "live-hist-summary" },
      h("span", { class: "live-hist-day" }, dayName(d)),
      day.lines.map((l) => h("b", { class: "num" }, masked(l))),
      day.own.length ? h("span", { class: "secondary small" }, t(`${day.own.join(", ")} ${day.own.length === 1 ? "is" : "are"} your own wallet: what went there is still yours, outside the balance.`,
        `${day.own.join(", ")} — ${day.own.length === 1 ? "твой кошелёк" : "твои кошельки"}: то, что ушло туда, по-прежнему твоё, просто вне баланса.`)) : null,
      h("button", { class: "live-chip-btn live-hist-ask", disabled: s.thinking, onclick: () => act("send", { text: t(`Where did my money go ${when}?`, `Куда ушли деньги ${when}?`) }) },
        icon("bubble", 12), t("Ask the chat where the money went", "Спросить в чате, куда ушли деньги"))));
  }
  if (moves.length) {
    const rows = [];
    moves.forEach((m, i) => {
      const d = new Date(m.at), key = m.at + m.text;
      if (i === 0 || new Date(moves[i - 1].at).toDateString() !== d.toDateString()) rows.push(h("div", { class: "live-day-head" }, dayName(d).toUpperCase()));
      const amount = m.amount === null || m.amount === undefined ? null
        : m.kind === "sent" ? ["−", m.own ? "secondary" : "loss"] : m.kind === "received" ? ["+", "profit"] : null;
      const sub = [time(d), m.own ? t("your wallet", "твой кошелёк") : null, m.request && m.request.length > 6 ? `“${m.request}”` : null].filter(Boolean).join(" · ");
      const open = openMoves.has(key);
      rows.push(h("div", { class: "live-hist-row" },
        h("button", { class: "live-hist-open", title: t("Open the request it came from", "Открыть запрос, из которого это"), onclick: () => { open ? openMoves.delete(key) : openMoves.add(key); render(); } },
          h("span", { class: "live-hist-icon" }, icon(moveIcon(m.kind), 13)),
          h("span", { class: "live-hist-text" }, h("span", { class: "live-hist-note one-line" }, m.text), h("span", { class: "live-hist-sub one-line" }, sub)),
          amount ? h("b", { class: "live-hist-amount num " + amount[1] }, masked(`${amount[0]}${Number(m.amount).toLocaleString("en-US", { maximumFractionDigits: 6 })} ${m.symbol || ""}`)) : null),
        m.link ? h("button", { class: "icon-btn", title: t("See it on the explorer", "Открыть в обозревателе"), onclick: () => openLink(m.link) }, icon("arrow.up.right.square", 15)) : null));
      // The request as it was: what was asked, what ihatecrypto said, and its canvas.
      if (open) rows.push(h("div", { class: "live-hist-request" },
        m.request ? h("div", { class: "live-hist-asked" }, m.request) : null,
        h("div", { class: "live-hist-reply secondary small selectable" }, m.reply),
        m.canvas !== ui.state.canvas ? h("button", { class: "link-btn", onclick: () => act("openCanvas", { wallet: m.canvas }) }, icon("arrow.up.right", 11), t("Open its canvas", "Открыть его холст")) : null));
    });
    out.push(h("div", { class: "live-hist-card scroll-own", "data-keep": "live-history" }, rows));
  } else {
    // Nothing moved yet: what was signed, from the journal.
    const lines = s.history.slice(0, 14), rows = [];
    lines.forEach((e, i) => {
      const d = new Date(e.at);
      if (i === 0 || new Date(lines[i - 1].at).toDateString() !== d.toDateString()) rows.push(h("div", { class: "live-day-head" }, dayName(d).toUpperCase()));
      rows.push(h("div", { class: "live-hist-row" },
        h("span", { class: "live-hist-icon" }, icon(kindIcon(e.kind), 13)),
        h("span", { class: "live-hist-text" },
          h("span", { class: "live-hist-note one-line" }, e.note || netName(e.chain)),
          h("span", { class: "live-hist-sub one-line" }, time(d) + " · " + netName(e.chain) + " · " + e.kind)),
        e.link ? h("button", { class: "icon-btn", title: t("See it on the explorer", "Открыть в обозревателе"), onclick: () => openLink(e.link) }, icon("arrow.up.right.square", 15)) : null));
    });
    out.push(rows.length ? h("div", { class: "live-hist-card scroll-own", "data-keep": "live-history" }, rows)
      : note(t("Nothing yet. Every move from now on is listed here, with the request it came from.", "Пока ничего. Каждое движение отсюда и дальше будет здесь, с запросом, из которого оно.")));
  }
  out.push(h("button", { class: "link-btn live-more", onclick: () => { ui.view = "history"; render(); } }, icon("clock", 12), t("All history: requests, positions, transfers, signatures", "Вся история: запросы, позиции, переводы, подписи")));
  return out;
}

// MARK: Favorites

let favTab = store("favTab") || "tokens";
let favSearch = "";
let tokenSort = store("tokenSort") || "change";
let tokenUp = store("tokenSortUp") === true;

function favoritesBody(b) {
  const tab = (id, ic, label) => h("button", { class: "live-seg" + (favTab === id ? " on" : ""), onclick: () => { favTab = id; store("favTab", id); render(); } }, icon(ic, 13), label);
  return [
    h("div", { class: "live-fav-bar" },
      h("div", { class: "live-segmented" }, tab("tokens", "star", t("Tokens", "Токены")), tab("wallets", "wallet.pass", t("Wallets", "Кошельки"))),
      h("input", { id: "fav-search", class: "live-search", value: favSearch, placeholder: favTab === "wallets" ? t("Search wallets", "Поиск кошельков") : t("Search tokens", "Поиск токенов"),
        oninput: (e) => { favSearch = e.target.value; render(); } }),
      h("button", { class: "btn primary small live-fav-plus", title: t("Add a token or a wallet", "Добавить токен или кошелёк"), "aria-label": t("Add to favorites", "Добавить в избранное"),
        onclick: (e) => { e.stopPropagation(); openFavAdd(e.currentTarget); } }, icon("plus", 14))),
    favTab === "wallets" ? walletsTab(b) : tokensTab(b),
  ];
}

function tokensTab(b) {
  const q = favSearch.trim().toLowerCase();
  const held = (f) => {
    const n = ui.state.portfolio.networks.find((x) => x.key === f.chain);
    const r = n?.rows.find((x) => x.symbol.toUpperCase() === f.symbol.toUpperCase());
    return r && r.usd !== null && r.usd !== undefined ? Number(r.usd) : null;
  };
  const rows = ui.state.favorites.filter((f) => !q || [f.name, f.symbol, f.address, netName(f.chain)].some((x) => (x || "").toLowerCase().includes(q)))
    .map((f) => ({ f, held: held(f) }));
  const keyOf = (r) => ({ price: r.f.price, liquidity: r.f.liquidity, held: r.held, change: r.f.change24h }[tokenSort]);
  if (tokenSort === "name") rows.sort((a, c) => (a.f.symbol.localeCompare(c.f.symbol)) * (tokenUp ? 1 : -1));
  else rows.sort((a, c) => {
    const x = keyOf(a), y = keyOf(c);
    if (x === null || x === undefined) return 1;
    if (y === null || y === undefined) return -1;
    return tokenUp ? x - y : y - x;
  });
  if (!rows.length) {
    return note(q ? t(`Nothing matches “${favSearch}”.`, `Ничего не нашлось по «${favSearch}».`)
      : t("No tokens yet. Tell the chat «add 0x… to favorites»: each shows its price, its day and what you hold.",
        "Токенов пока нет. Скажи в чате «добавь 0x… в избранное»: у каждого будет цена, изменение за день и сколько у тебя."));
  }
  const col = (title, key, cls) => h("button", { class: "live-col " + cls + (tokenSort === key ? " on" : ""), onclick: () => {
    if (tokenSort === key) tokenUp = !tokenUp; else { tokenSort = key; tokenUp = key === "name"; }
    store("tokenSort", tokenSort); store("tokenSortUp", tokenUp); render();
  } }, title, tokenSort === key ? icon(tokenUp ? "arrow.up" : "arrow.down", 8) : null);
  const heldSum = rows.map((r) => r.held).filter((x) => x > 0);
  // As the Mac's favorite row: a click opens the token's card; its menu asks with the card attached, then Buy and Sell
  // (by ticker for a Solana token the wallet holds and a list vouches for, else by contract).
  const heldVerified = (f) => (ui.state.portfolio?.networks || []).find((n) => n.key === f.chain)?.rows
    .find((r) => r.contract?.toLowerCase() === f.address.toLowerCase())?.verified === true;
  const open = (f) => act("openToken", { address: f.address, network: f.chain });
  const ask = async (f) => {
    const a = await open(f);
    if (!a?.board) return;
    ui.attached = a.board;
    render();
    document.getElementById("composer")?.focus();
  };
  const actions = (ev, f) => popup(ev, [
    [[icon("info.circle", 14), t("Details", "Подробности")], () => open(f)],
    [[icon("bubble.left", 14), t("Ask in chat", "Спросить в чате")], () => ask(f)],
    ...tradePhrases(f.chain, f.address, f.symbol, { verified: heldVerified(f) }).map(([title, glyph, phrase]) => [[icon(glyph, 14), title], () => suggest(phrase)]),
    null,
    [t("Copy contract", "Скопировать контракт"), () => navigator.clipboard.writeText(f.address).catch(() => {})],
    [t("Remove from favorites", "Убрать из избранного"), () => act("removeFavorite", { wallet: f.chain + ":" + f.address.toLowerCase() })],
  ]);
  return [
    h("div", { class: "live-token-table" },
      h("div", { class: "live-token-head" }, col(t("TOKEN", "ТОКЕН"), "name", "live-c-name"), col(t("PRICE", "ЦЕНА"), "price", "live-c-price"), col(t("24 H", "24 Ч"), "change", "live-c-change"),
        col(t("LIQUIDITY", "ЛИКВ."), "liquidity", "live-c-liq"), col(t("YOU HOLD", "У ТЕБЯ"), "held", "live-c-held")),
      rows.map(({ f, held }) => h("button", { class: "live-token-row", title: t(`${f.name} on ${netName(f.chain)}: chart, buy, sell`, `${f.name} в ${netName(f.chain)}: график, покупка, продажа`),
        onclick: () => open(f), oncontextmenu: (ev) => { ev.preventDefault(); actions(ev, f); } },
        h("span", { class: "live-c-name" }, logo(f.symbol, 26, f.image || undefined),
          h("span", { class: "live-tok-text" }, h("b", { class: "one-line" }, f.symbol), h("span", { class: "secondary small one-line" }, netName(f.chain)))),
        h("span", { class: "live-c-price num" }, price(f.price)),
        h("span", { class: "live-c-change num " + (f.change24h === null || f.change24h === undefined ? "secondary" : f.change24h < 0 ? "loss" : "profit") },
          f.change24h === null || f.change24h === undefined ? "—" : signedPct(f.change24h)),
        h("span", { class: "live-c-liq num secondary" }, f.liquidity ? compact(f.liquidity) : "—"),
        h("span", { class: "live-c-held num" + (held > 0 ? "" : " tertiary") }, held > 0 ? masked(usd(held)) : "—"))),
      heldSum.length ? h("div", { class: "live-token-foot" }, h("span", { class: "secondary" }, t(`Held in ${heldSum.length} of ${rows.length}`, `Есть ${heldSum.length} из ${rows.length}`)),
        h("b", { class: "num" }, masked(usd(heldSum.reduce((a, c) => a + c, 0))))) : null),
    hint(t("A row: Buy, Sell, Ask in chat, Copy contract, Remove.", "Строка: купить, продать, спросить в чате, скопировать контракт, убрать."), "hand.tap"),
  ];
}

function walletsTab(b) {
  const a = ui.state.addresses;
  const q = favSearch.trim().toLowerCase();
  const starred = new Set(ui.state.starred);
  const saved = ui.state.book.filter((e) => !q || [e.name, e.address, netName(e.network), e.memo].some((x) => (x || "").toLowerCase().includes(q)))
    .sort((x, y) => (starred.has(bookKey(y)) - starred.has(bookKey(x))) || x.name.localeCompare(y.name));
  const own = [
    a.solana && { name: t("Solana · USDC and SOL", "Solana · USDC и SOL"), network: "solana", address: a.solana },
    a.evm && { name: t("Base · USDC and ETH (same 0x as Hyperliquid)", "Base · USDC и ETH (тот же 0x, что у Hyperliquid)"), network: "base", address: a.evm },
  ].filter(Boolean);
  return [
    own.length && !q ? [section(t("Your wallets · send money here to top up", "Твои кошельки · сюда отправлять пополнение")), list(own.map((e) => addressCard(e, b, { own: true })))] : null,
    section(t("Saved addresses", "Сохранённые адреса")),
    saved.length ? list(saved.map((e) => addressCard(e, b)))
      : note(q ? t(`Nothing matches “${favSearch}”.`, `Ничего не нашлось по «${favSearch}».`)
        : t("Save an exchange's deposit address, another wallet or a friend's — with a name, so you never paste a wrong one: the Address book block, or «save <address> as Bybit on base» in the chat.",
          "Сохрани адрес пополнения биржи, другой кошелёк или адрес друга — с именем, чтобы никогда не вставить чужой: блок «Адресная книга» или «сохрани <адрес> как Bybit на base» в чате.")),
    hint(t(`Every saved address is confirmed with ${presence()}, the whole address on the prompt, and money goes to a name only at that address — never to an address from a chat, a site or a model.`,
      `Каждый сохранённый адрес подтверждается через ${presence()}, весь адрес в окне, и деньги уходят по имени только на этот адрес — никогда на адрес из чата, сайта или от модели.`), "lock"),
  ];
}

// MARK: Favorites «+»

/** The board's one «+» (the Mac's AddFavoriteButton): paste anything and it says what it is — a token DexScreener prices
 * (added to Tokens at once) or a wallet address (named and saved under Windows Hello). Under the field, the tokens the
 * wallet holds and doesn't watch yet, a click each. The panel lives on the body, so it keeps its field while the window
 * redraws; it redraws its own parts. */
const favAdd = { text: "", found: null, seq: 0, naming: false, name: "", network: "", busy: false };
let favPanel = null;
const dollars = new Set(["USDC", "USDT", "USDT0", "USD₮0", "USDS", "USDE", "DAI", "USDG", "PYUSD", "FDUSD", "USD1"]);

export function openFavAdd(anchor = document.querySelector(".live-fav-plus")) {
  document.querySelector(".menu")?.remove();
  Object.assign(favAdd, { text: "", found: null, naming: false, name: "", busy: false });
  const field = h("input", { id: "fav-add-field", class: "address live-fav-field", placeholder: t("A token's contract or a wallet address", "Контракт токена или адрес кошелька"),
    autocomplete: "off", spellcheck: "false", oninput: (e) => look(e.target.value) });
  const paste = h("button", { class: "btn small", onclick: async () => {
    try { field.value = (await navigator.clipboard.readText()).trim(); look(field.value); } catch { field.focus(); toast(t("Press Ctrl+V in the field.", "Нажми Ctrl+V в поле.")); }
  } }, t("Paste", "Вставить"));
  favPanel = h("div", { class: "menu live-fav-pop", onclick: (e) => e.stopPropagation() },
    h("b", { class: "live-fav-pop-title" }, t("Add to favorites", "Добавить в избранное")),
    h("div", { class: "live-fav-field-row" }, field, paste),
    h("div", { class: "live-fav-result" }),
    h("div", { class: "live-fav-held" }),
    hint(t("Or in the chat: «add 0x… to favorites», «save 0x… as Masha on Base».", "Или в чате: «добавь 0x… в избранное», «сохрани 0x… как Маша на Base»."), "bubble"));
  document.body.append(favPanel);
  drawFavAdd();
  const r = anchor?.getBoundingClientRect() || { right: window.innerWidth / 2, bottom: 80 };
  favPanel.style.left = Math.max(8, Math.min(r.right - favPanel.offsetWidth, window.innerWidth - favPanel.offsetWidth - 8)) + "px";
  favPanel.style.top = Math.max(8, Math.min(r.bottom + 8, window.innerHeight - favPanel.offsetHeight - 8)) + "px";
  setTimeout(() => document.addEventListener("click", function off(ev) {
    if (favPanel?.isConnected && favPanel.contains(ev.target)) return;
    closeFavAdd(); document.removeEventListener("click", off);
  }), 0);
  field.focus();
}

function closeFavAdd() { favPanel?.remove(); favPanel = null; }

/** A token if a pool trades it, else an address: the engine says, a moment after the typing stops. */
function look(text) {
  favAdd.text = text.trim();
  favAdd.found = null;
  favAdd.naming = false;
  const seq = ++favAdd.seq;
  drawFavAdd();
  if (!favAdd.text) return;
  setTimeout(async () => {
    if (seq !== favAdd.seq) return;
    let found;
    try { found = await engine("lookToken", { address: favAdd.text }); } catch {}
    found ||= { kind: "unknown", address: favAdd.text, network: "solana" };
    if (seq !== favAdd.seq) return;
    favAdd.found = found;
    favAdd.network = found.network || "solana";
    drawFavAdd();
  }, 250);
}

/** One of the wallet's own addresses, or the automation wallet's. */
function ownAddress(a) {
  const s = ui.state, mine = [...Object.values(s.addresses || {}), ...(s.wallets || []).flatMap((w) => [w.solana, w.evm]), s.automation?.solana, s.automation?.evm];
  return mine.some((x) => x && x.toLowerCase() === a.toLowerCase());
}

function drawFavAdd() {
  if (!favPanel) return;
  const result = favPanel.querySelector(".live-fav-result"), held = favPanel.querySelector(".live-fav-held");
  const f = favAdd.found;
  const kind = (title, sub, cls) => h("div", { class: "live-fav-kind" }, h("span", { class: "live-fav-chip " + cls }, title), h("span", { class: "secondary small one-line" }, sub));
  const saveLink = (a) => h("button", { class: "link-btn", onclick: () => { favAdd.found = { kind: "address", address: a, network: a.startsWith("0x") ? "base" : favAdd.network }; favAdd.network = favAdd.found.network; favAdd.naming = true; drawFavAdd(); } },
    t("Save it as an address instead", "Сохранить как адрес"));
  let body = null;
  if (!favAdd.text) body = null;
  else if (!f) body = h("div", { class: "live-fav-looking secondary small" }, h("span", { class: "live-spinner" }), t("Looking it up on DexScreener…", "Ищу на DexScreener…"));
  else if (f.kind === "invalid") body = hint(t("That isn't an address: paste a 0x…, a Solana key, bc1…, T… or a .near name.", "Это не адрес: вставь 0x…, ключ Solana, bc1…, T… или имя .near."), "questionmark.circle");
  else if (f.kind === "token") {
    const have = ui.state.favorites.some((x) => x.chain === f.chain && x.address.toLowerCase() === f.address.toLowerCase());
    const change = f.change24h === undefined ? null : Number(f.change24h);
    body = [
      h("div", { class: "live-fav-card" },
        kind(t("TOKEN", "ТОКЕН"), t("found on DexScreener", "найден на DexScreener"), "token"),
        h("div", { class: "live-fav-token" }, logo(f.symbol, 32, f.image || undefined),
          h("span", { class: "live-tok-text" }, h("b", { class: "one-line" }, f.name), h("span", { class: "secondary small one-line" }, `${f.symbol} · ${netName(f.chain)}`)),
          h("span", { class: "live-fav-price" }, h("b", { class: "num" }, f.price === undefined ? t("no price", "нет цены") : price(f.price)),
            change === null ? null : h("span", { class: "small num " + (change < 0 ? "loss" : "profit") }, signedPct(change)))),
        h("div", { class: "secondary small" }, t("A contract to watch, not an address to send money to.", "Контракт, за которым следят, а не адрес для перевода денег."))),
      h("div", { class: "live-fav-actions" }, saveLink(f.address), h("span", { class: "spacer" }),
        h("button", { class: "btn primary small", disabled: have || favAdd.busy, onclick: async () => {
          favAdd.busy = true; drawFavAdd();
          await act("addFavorite", { address: f.address, network: f.chain });
          closeFavAdd();
        } }, icon("star", 13), have ? t("In your tokens", "Уже в токенах") : t("Add token", "Добавить токен")))];
  } else if (f.kind === "address" || f.kind === "unknown") {
    const own = ownAddress(f.address);
    const same = ui.state.book.find((e) => e.address.toLowerCase() === f.address.toLowerCase());
    if (f.kind === "unknown" && !favAdd.naming) {
      body = [hint(t("DexScreener didn't answer, so it isn't known yet whether this is a token.", "DexScreener не ответил, поэтому пока непонятно, токен ли это."), "exclamationmark.triangle"),
        h("div", { class: "live-fav-actions" }, h("button", { class: "btn small", onclick: () => look(favAdd.text) }, t("Try again", "Ещё раз")), h("span", { class: "spacer" }),
          h("button", { class: "btn small", onclick: () => { favAdd.naming = true; drawFavAdd(); } }, t("Save it as an address…", "Сохранить как адрес…")))];
    } else {
      const evmOrSol = f.address.startsWith("0x") || favAdd.network === "solana";
      body = [
        h("div", { class: "live-fav-card" },
          kind(t("WALLET ADDRESS", "АДРЕС КОШЕЛЬКА"), netName(favAdd.network) + (evmOrSol ? t(" · no token trades at it", " · ни один токен им не торгуется") : ""), "wallet"),
          addressLines(f.address, 34),
          own ? hint(t("This is your own wallet: it's under Wallets already.", "Это твой кошелёк: он уже во вкладке «Кошельки»."), "checkmark.circle")
            : same ? hint(t(`Already saved as “${same.name}”.`, `Уже сохранён как «${same.name}».`), "checkmark.circle") : null),
        favAdd.naming && !own && !same ? h("div", { class: "live-fav-name" },
          h("div", { class: "live-field-row" },
            h("div", { class: "field" }, h("label", { for: "fav-add-name" }, t("Name", "Имя")),
              h("input", { id: "fav-add-name", value: favAdd.name, placeholder: "Bybit", oninput: (e) => (favAdd.name = e.target.value) })),
            h("div", { class: "field" }, h("label", { for: "fav-add-network" }, t("Network", "Сеть")),
              h("select", { id: "fav-add-network", onchange: (e) => (favAdd.network = e.target.value) }, networks.map((n) => h("option", { value: n, selected: n === favAdd.network }, netName(n)))))),
          h("div", { class: "live-fav-actions" }, h("span", { class: "spacer" }),
            h("button", { class: "btn primary small", disabled: favAdd.busy, onclick: async () => {
              if (!favAdd.name.trim()) { toast(t("Give it a name first.", "Сначала дай ему имя.")); return; }
              favAdd.busy = true; drawFavAdd();
              const saved = await act("saveAddress", { name: favAdd.name.trim(), network: favAdd.network, address: f.address, memo: "" });
              favAdd.busy = false;
              if (saved === null) { drawFavAdd(); return; }
              favTab = "wallets"; store("favTab", "wallets");
              closeFavAdd(); toast(t("Saved.", "Сохранено.")); render();
            } }, icon("shield.check", 13), t(`Save with ${presence()}`, `Сохранить через ${presence()}`))))
          : h("div", { class: "live-fav-actions" }, h("span", { class: "spacer" }),
            h("button", { class: "btn primary small", disabled: own || !!same, title: t(`A name, its network; the ${presence()} prompt shows the whole address before it's saved`, `Имя и сеть; окно ${presence()} покажет весь адрес перед сохранением`),
              onclick: () => { favAdd.naming = true; drawFavAdd(); document.getElementById("fav-add-name")?.focus(); } },
              icon("shield.check", 13), t("Name it and save…", "Назвать и сохранить…")))];
    }
  }
  result.replaceChildren(...[body].flat(2).filter(Boolean));
  // The tokens the wallet holds that aren't watched: dollars and gas coins aren't offered.
  const watched = new Set(ui.state.favorites.map((x) => x.chain + ":" + x.address.toLowerCase()));
  const list = favAdd.text ? [] : (ui.state.portfolio?.networks || []).filter((n) => !["hyperliquid", "near"].includes(n.key))
    .flatMap((n) => n.rows.filter((r) => r.contract && r.section !== "defi" && r.verified !== false && !dollars.has(r.symbol.toUpperCase())
      && !watched.has(n.key + ":" + r.contract.toLowerCase())).map((r) => ({ n, r })))
    .sort((a, c) => (Number(c.r.usd) || 0) - (Number(a.r.usd) || 0)).slice(0, 5);
  held.replaceChildren(...(list.length ? [h("div", { class: "menu-divider" }), h("div", { class: "eyebrow" }, t("Tokens you hold", "Токены, которые у тебя есть")),
    ...list.map(({ n, r }) => h("button", { class: "live-fav-held-row", disabled: favAdd.busy, title: t(`Watch ${r.symbol}`, `Следить за ${r.symbol}`), onclick: async () => {
      favAdd.busy = true; drawFavAdd();
      await act("addFavorite", { address: r.contract, network: n.key });
      favAdd.busy = false; drawFavAdd();
    } }, logo(r.symbol, 24, r.icon || undefined),
      h("span", { class: "live-tok-text" }, h("b", { class: "one-line" }, r.symbol), h("span", { class: "secondary small one-line" }, n.name)),
      h("span", { class: "spacer" }), h("span", { class: "secondary small num" }, r.usd === null || r.usd === undefined ? "" : masked(usd(r.usd))),
      h("span", { class: "live-fav-star" }, icon("star", 13))))] : []));
}

// MARK: Price alerts

function alertsBody() {
  const alerts = ui.state.alerts;
  if (!alerts.length) return note(t("No alerts: «tell me when BTC is above 120000».", "Оповещений нет: «скажи, когда BTC выше 120000»."));
  const arrow = () => h("div", { class: "live-alert-arrow" }, icon("arrow.down", 13));
  return [
    alerts.map((a) => {
      const coin = a.coin || (a.text.match(/^[A-Za-z0-9:]+/) || [""])[0];
      const steps = a.steps || [];
      return h("div", { class: "live-alert-chain" },
        h("div", { class: "live-alert-step first" },
          h("span", { class: "live-alert-badge" }, coin ? logo(coin, 32) : icon("bell", 15)),
          h("span", { class: "live-alert-text" }, h("span", { class: "live-alert-label" }, t("WHEN · PRICE", "КОГДА · ЦЕНА")), h("b", {}, a.when || a.text),
            a.now ? h("span", { class: "secondary small num" }, a.now) : null),
          h("button", { class: "icon-btn", title: t("Remove the alert", "Убрать оповещение"), onclick: () => act("removeAlert", { wallet: a.id }) }, icon("trash", 14))),
        // What it does when the price gets there, each step its own block.
        (a.blocks || steps.map((title) => ({ plan: false, title }))).map((x) => [arrow(), h("div", { class: "live-alert-step" },
          x.plan ? h("span", { class: "live-alert-badge" }, assetBadge(x.coin, 32)) : h("span", { class: "live-alert-badge plain" }, icon("bolt", 15)),
          h("span", { class: "live-alert-text" }, h("span", { class: "live-alert-label" }, x.plan ? t("THEN · PLAN ON HYPERLIQUID", "ЗАТЕМ · ПЛАН НА HYPERLIQUID") : t("THEN", "ЗАТЕМ")),
            h("b", {}, x.title), x.line ? h("span", { class: "secondary small num" }, x.line) : null))]),
        arrow(),
        h("div", { class: "live-alert-step" },
          h("span", { class: "live-alert-badge plain" }, icon("bell", 15)),
          h("span", { class: "live-alert-text" }, h("span", { class: "live-alert-label" }, t("YOU · NOTIFICATION", "ТЫ · УВЕДОМЛЕНИЕ")),
            h("b", {}, steps.length ? t("A Windows notification, then your Confirm", "Уведомление Windows, потом твоё «Подтвердить»") : t("A Windows notification", "Уведомление Windows")),
            steps.length ? h("span", { class: "secondary small" }, t(`Prepared when it fires; nothing moves without ${presence()}.`, `Готовится, когда сработает; без ${presence()} ничего не двигается.`)) : null)));
    }),
    h("div", { class: "live-hint tertiary" }, t("Checked every 10 s while ihatecrypto is open or in the tray. Nothing moves until you confirm it.",
      "Проверяется каждые 10 с, пока ihatecrypto открыт или в трее. Ничего не двигается, пока ты не подтвердишь.")),
  ];
}

// MARK: Schedule

let scheduleTab = store("scheduleTab") || "tasks";
let showPaused = false;
let walletNet = "solana";

/** The interface's language for dates, not Windows' (a Russian window on an English Windows says «окт», «Сегодня 13:40»). */
const dateLocale = () => (ui.state?.russian ? "ru-RU" : "en-US");

/** A run's day as a calendar leaf (DateTile): the month on a strip, orange when it's today, the day under it. */
function dateTile(iso) {
  if (!iso) return h("span", { class: "live-date-tile" }, h("span", { class: "live-date-icon" }, icon("calendar", 13)));
  const d = new Date(iso), now = new Date();
  const today = d <= now || d.toDateString() === now.toDateString();
  return h("span", { class: "live-date-tile" },
    h("span", { class: "live-date-month" + (today ? " today" : "") }, d.toLocaleDateString(dateLocale(), { month: "short" }).replace(".", "").toUpperCase()),
    h("span", { class: "live-date-day num" }, String(d.getDate())));
}

/** When it runs next, and how long until it within a day (NextLine). */
function nextLine(x) {
  if (x.paused) return t("paused", "на паузе");
  if (!x.next) return "";
  const d = new Date(x.next), now = new Date();
  const time = d.toLocaleTimeString(dateLocale(), { hour: "2-digit", minute: "2-digit" });
  const tomorrow = new Date(now); tomorrow.setDate(now.getDate() + 1);
  const day = d.toDateString() === now.toDateString() ? t("Today", "Сегодня") : d.toDateString() === tomorrow.toDateString() ? t("Tomorrow", "Завтра")
    : d.toLocaleDateString(dateLocale(), { weekday: "short", day: "numeric", month: "short" });
  const left = d - now;
  const tail = left <= 0 ? t("any moment", "вот-вот") : left < 86_400_000
    ? (left < 3_600_000 ? t(`in ${Math.max(1, Math.round(left / 60_000))} min`, `через ${Math.max(1, Math.round(left / 60_000))} мин`)
      : t(`in ${Math.round(left / 3_600_000)} h`, `через ${Math.round(left / 3_600_000)} ч`)) : "";
  return [h("b", {}, `${day} ${time}`), tail ? " · " + tail : ""];
}

/** A switch, as the Mac's mini toggle: a click only — a task that pays by itself never starts from a chat message. */
function toggle(id, on, label, change) {
  return h("label", { class: "live-switch", title: label },
    h("input", { type: "checkbox", id, checked: on, "aria-label": label, onchange: (e) => change(e.target.checked) }), h("span", { class: "live-switch-track" }));
}

function taskRow(x, a) {
  const title = x.text.replace(/ · (paused|на паузе)$/, "");
  const mark = x.automatic
    ? h("span", { class: "live-signer-mark", title: t("Runs by itself from ", "Идёт сама с ") + (a ? a.label : "") + t(`, no ${presence()}`, `, без ${presence()}`) }, icon("bolt", 14))
    : h("span", { class: "live-signer-mark ask", title: t("A money move waits for Confirm", "Перевод ждёт «Подтвердить»") }, icon("hand.tap", 14));
  const key = "task:" + x.id;
  return h("div", { class: "live-task-row" + (x.paused ? " paused" : "") + (ui.state.scheduleKey === key ? " lit" : ""),
    title: t("Open it on its own canvas", "Открыть на отдельном холсте"), onclick: (e) => { if (!e.target.closest("button, label")) act("openSchedule", { name: key }); } },
    dateTile(x.paused ? null : x.next),
    h("span", { class: "live-task-text" }, h("span", { class: "live-task-title" }, title), h("span", { class: "live-task-next" }, nextLine(x))),
    // Its mark, or under the pointer (always on a paused one) Delete, in the same place: nothing shifts.
    h("span", { class: "live-task-mark" }, mark,
      h("button", { class: "icon-btn live-task-delete", title: t("Delete it: off the schedule for good", "Удалить: из расписания насовсем"), onclick: () => act("removeTask", { wallet: x.id }) }, icon("trash", 14))),
    toggle("task-" + x.id, !x.paused, x.paused ? t("Paused: switch on to run it again", "На паузе: включи, чтобы снова шла") : t("Switch off to pause it", "Выключи, чтобы поставить на паузу"),
      (on) => act("taskOn", { wallet: x.id, test: on })));
}

function scheduleBody() {
  const a = ui.state.automation;
  const tasks = ui.state.tasks;
  const running = tasks.filter((x) => !x.paused), paused = tasks.filter((x) => x.paused);
  const tab = (id, label) => h("button", { class: "live-seg" + (scheduleTab === id ? " on" : ""), onclick: () => { scheduleTab = id; store("scheduleTab", id); render(); } }, label);
  const bud = ui.state.automationBudget;
  const num = (x, max = 6) => Number(x).toLocaleString("en-US", { maximumFractionDigits: max });
  const weeks = bud?.weeks;
  const budget = a
    ? h("div", { class: "live-budget" },
      h("div", { class: "live-budget-line" }, h("span", { class: "live-budget-usd num" }, ui.state.automationUSD === null || ui.state.automationUSD === undefined ? "…" : masked(usd(ui.state.automationUSD))),
        h("span", { class: "secondary small" }, t("budget", "бюджет"))),
      bud?.tokens?.length ? h("div", { class: "secondary small" }, bud.tokens.map((c) => masked(`${num(c.amount)} ${c.symbol}`)).join(", ") + t(" on Solana · ", " в Solana · ") + masked(`${num(bud.total, 2)} USDC`)) : null,
      // How long the budget lasts at the tasks' buys: four marks, one a week.
      weeks === null || weeks === undefined ? null : [
        h("div", { class: "small live-budget-weeks " + (weeks === 0 ? "loss" : "profit") }, weeks >= 4 ? t("covers the next 4 weeks", "хватит на 4 недели вперёд")
          : weeks === 0 ? t(`${num(bud.shortForWeek || 0)} USDC short for the week's buys`, `не хватает ${num(bud.shortForWeek || 0)} USDC на покупки недели`)
          : t(`covers ${weeks} week${weeks === 1 ? "" : "s"}`, `хватит на ${weeks} ${weeks === 1 ? "неделю" : "недели"}`)),
        h("div", { class: "live-budget-bar" }, [0, 1, 2, 3].map((i) => h("span", { class: i < weeks ? "on" : "" })))],
      // USDC on Solana but next to no SOL: every move there fails on the fee or a new account's rent.
      bud && bud.sol !== null && bud.sol !== undefined && Number(bud.sol) < 0.005 && (Number(bud.solana) > 0 || bud.tokens.length)
        ? h("div", { class: "live-budget-sol" }, h("span", { class: "loss small" }, t(`${num(bud.sol)} SOL for Solana's fees: too little, its tasks there fail.`, `${num(bud.sol)} SOL на комиссии Solana: мало, задачи там не пройдут.`)),
          h("button", { class: "btn small", onclick: () => suggestGas(a, bud) }, icon("fuelpump", 12), t("Top up gas", "Пополнить газ"))) : null,
      // Short for the week, or nothing on it: the top-up goes into the chat, the user sends it.
      bud && (bud.shortForWeek || Number(bud.total) === 0) ? h("button", { class: "live-wide-btn accent", title: t("Puts it in the chat with the amount to change; nothing goes until you send it", "Кладёт в чат, сумму можно поменять; ничего не уходит, пока не отправишь"),
        onclick: () => suggestTopUp(a, bud, bud.shortForWeek || 20) }, icon("plus", 13), bud.shortForWeek ? t(`Top up ${num(bud.shortForWeek)} USDC for the week`, `Пополнить на ${num(bud.shortForWeek)} USDC на неделю`) : t("Top up 20 USDC", "Пополнить на 20 USDC")) : null,
      h("div", { class: "secondary small" }, t(`${a.label} · tasks you switch on spend from it without ${presence()}, only to your own wallets and saved addresses.`,
        `${a.label} · включённые задачи тратят с него без ${presence()}, только на твои кошельки и сохранённые адреса.`)))
    : h("div", { class: "live-budget-ask" },
      h("b", {}, t(`Run tasks by themselves, without ${presence()}?`, `Запускать задачи самим, без ${presence()}?`)),
      h("span", { class: "secondary small" }, t(`An automation wallet holds the Schedule's budget and signs its tasks. Your main wallet still asks for ${presence()}.`,
        `Кошелёк автоматизации держит бюджет расписания и подписывает его задачи. Основной кошелёк по-прежнему спрашивает ${presence()}.`)),
      h("button", { class: "btn primary small", onclick: () => act("createAutomationWallet") }, icon("plus", 13), t("Create an automation wallet", "Создать кошелёк автоматизации")));
  const held = ui.state.schedulePaused === true;
  const tasksTab = [
    running.length ? h("button", { class: "live-wide-btn" + (held ? " live-held" : ""), onclick: () => act("schedulePause", { test: !held }),
      title: held ? t("Everything scheduled runs again", "Всё запланированное снова пойдёт") : t("Nothing scheduled runs until you resume; stops and takes stay on the exchange", "Ничего запланированного не запустится, пока не возобновишь; стопы и тейки остаются на бирже") },
      icon(held ? "play" : "pause", 13), held ? t("Resume schedule", "Возобновить расписание") : t("Pause schedule", "Приостановить расписание")) : null,
    running.map((x) => taskRow(x, a)),
    ui.state.positions.map(positionRow),
    !running.length ? h("div", { class: "secondary small" }, paused.length ? t("Nothing running now.", "Сейчас ничего не идёт.") : t("Nothing scheduled. Say it in the chat, in any words:", "Ничего не запланировано. Скажи в чате любыми словами:")) : null,
    h("button", { class: "live-wide-btn", title: t("Puts a start in the chat: say what to do and when", "Кладёт начало в чат: скажи, что и когда делать"), onclick: () => suggest(t("every day at 09:00: ", "каждый день в 09:00: ")) },
      icon("plus", 13), t("New schedule", "Новое расписание")),
    gasButton(a, bud),
    paused.length ? [
      h("button", { class: "live-paused-head", onclick: () => { showPaused = !showPaused; render(); } },
        h("span", {}, t(`PAUSED · ${paused.length}`, `НА ПАУЗЕ · ${paused.length}`)), icon(showPaused ? "chevron.up" : "chevron.down", 11)),
      showPaused ? paused.map((x) => taskRow(x, a)) : null] : null,
    h("div", { class: "live-chips" }, [t("every week: rebalance USDC", "каждую неделю: перераспредели USDC"), t("every day at 09:00: what's moving", "каждый день в 09:00: что растёт")]
      .map((e) => h("button", { class: "live-chip-btn", title: t("Put it in the chat", "Положить в чат"), onclick: () => suggest(e) }, icon("bubble", 11), e))),
  ];
  return [
    h("div", { class: "live-sched-status small " + (held ? "loss" : "secondary") }, held ? t("paused", "на паузе") : t("while ihatecrypto runs, in the tray too", "пока ihatecrypto работает, в трее тоже")),
    budget,
    h("div", { class: "live-segmented full" }, tab("tasks", t("Tasks", "Задачи")), tab("history", t("History", "История")), tab("wallet", t("Wallet", "Кошелёк"))),
    scheduleTab === "wallet" ? automationTab(a) : scheduleTab === "history" ? runsTab()
      : h("div", { class: "live-sched-tasks" + (held ? " held" : "") }, tasksTab),
  ];
}

/** An open position in the Schedule (the Mac's «perp:» entry): its stop and take are on the exchange; it opens its canvas. */
function positionRow(p) {
  const key = "perp:" + p.coin;
  return h("button", { class: "live-task-row live-pos-row" + (ui.state.scheduleKey === key ? " lit" : ""), onclick: () => act("openSchedule", { name: key }) },
    h("span", { class: "live-date-tile" }, h("span", { class: "live-date-icon" }, icon("chart.line", 13))),
    h("span", { class: "live-task-text" }, h("span", { class: "live-task-title" }, (p.side === "long" ? t("Long ", "Лонг ") : t("Short ", "Шорт ")) + p.coin.split(":").pop()),
      h("span", { class: "live-task-next" }, t("stop and take on the exchange", "стоп и тейк на бирже"))),
    h("span", { class: "live-pos-pnl small num " + (negative(p.pnl) ? "loss" : "profit") }, masked(p.pnl)));
}

/** What each run did, by day (the Mac's History tab): its time, how it ended, what it was, and its canvas. */
function runsTab() {
  const runs = ui.state.scheduleRuns || [];
  if (!runs.length) return h("div", { class: "secondary small" }, t("Nothing ran yet. Each run shows here with its time.", "Пока ничего не запускалось. Каждый запуск появится здесь со временем."));
  const rows = [];
  runs.forEach((r, i) => {
    const d = new Date(r.at);
    if (i === 0 || new Date(runs[i - 1].at).toDateString() !== d.toDateString()) rows.push(h("div", { class: "live-day-head" }, dayName(d).toUpperCase()));
    const [ic, cls, word] = r.result === "failed" ? ["xmark.circle", "loss", t("Failed", "Не вышло")] : r.result === "nothing" ? ["minus.circle", "secondary", t("Nothing to do", "Нечего делать")]
      : ["checkmark.circle", "profit", t("Done", "Готово")];
    const canvas = r.canvas && ui.state.canvases.find((c) => c.id === r.canvas);
    rows.push(h("div", { class: "live-run-row" },
      h("span", { class: "live-run-time secondary small num" }, d.toLocaleTimeString(dateLocale(), { hour: "2-digit", minute: "2-digit" })),
      h("span", { class: "live-run-icon " + cls, title: word }, icon(ic, 14)),
      h("span", { class: "live-run-title one-line", title: r.line || r.title }, r.title),
      canvas ? h("button", { class: "icon-btn", title: t("Open the canvas it ran on", "Открыть холст, где это шло"), onclick: () => act("openCanvas", { wallet: canvas.id }) }, icon("arrow.up.right", 13)) : null));
  });
  return h("div", { class: "live-runs" }, rows);
}

/** «Top up»: the send typed into the chat, its amount selected to type over; short of SOL, its fees go in the same chain. */
function suggestTopUp(a, bud, amount) {
  const sol = bud?.gas?.find((g) => g.network === "solana");
  suggest(`send ${amount} USDC to ${a.label} on Solana` + (sol ? `, then send ${sol.amount} SOL to ${a.label} on Solana` : ""));
}

/** «Top up gas»: what it lacks, network by network, as sends from the wallet that's on; with enough, a spare 0.005 SOL. */
function suggestGas(a, bud) {
  const short = bud?.gas?.length ? bud.gas : [{ network: "solana", coin: "SOL", amount: 0.005 }];
  suggest(short.map((g) => `send ${g.amount} ${g.coin} on ${g.network} to ${a.label}`).join(", then "));
}

function gasButton(a, bud) {
  if (!a) return null;
  const short = bud?.gas || [];
  return h("button", { class: "live-gas-btn" + (short.length ? " short" : ""), onclick: () => suggestGas(a, bud),
    title: t("Puts the send from your wallet in the chat, the amount worked out for the next 4 weeks of tasks; nothing goes until you send it",
      "Кладёт перевод с твоего кошелька в чат, сумма посчитана на 4 недели задач; ничего не уходит, пока не отправишь") },
    h("span", { class: "live-gas-icon" }, icon("fuelpump", 14)),
    h("span", { class: "live-gas-text" }, h("b", {}, t("Top up gas", "Пополнить газ")),
      h("span", { class: "secondary small" }, !bud ? "…" : short.length ? short.map((g) => `${g.amount} ${g.coin} ${t("on", "в")} ${netName(g.network)}`).join("\n")
        : t("Enough for now · adds 0.005 SOL", "Пока хватает · добавит 0.005 SOL"))));
}

function automationTab(a) {
  if (!a) return h("div", { class: "secondary small" }, t("No automation wallet yet: create it above, then its addresses show here.", "Кошелька автоматизации пока нет: создай его выше, и здесь появятся адреса."));
  const b = ui.state.automationBudget, num = (x, max = 6) => Number(x).toLocaleString("en-US", { maximumFractionDigits: max });
  // What the dollars on Solana are: its USDC, its other coins and the SOL its fees are paid with.
  const solanaNote = !b ? t("USDC and a little SOL for fees", "USDC и немного SOL на комиссии")
    : [Number(b.solana) > 0 ? masked(`${num(b.solana)} USDC`) : null, ...(b.tokens || []).map((c) => masked(`${num(c.amount)} ${c.symbol}`)),
      b.sol !== null && b.sol !== undefined ? masked(`${num(b.sol, 4)} SOL ${t("for fees", "на комиссии")}`) : null].filter(Boolean).join(" · ");
  const places = [
    a.solana && { key: "solana", logos: ["solana"], title: "Solana", note: solanaNote, address: a.solana },
    a.evm && { key: "evm", logos: ["base", "arbitrum", "ethereum", "polygon"], title: t("EVM networks", "EVM-сети"), note: t("9 networks, one address", "9 сетей, один адрес"), address: a.evm },
    a.near && { key: "near", logos: ["near"], title: "NEAR", note: "", address: a.near },
  ].filter(Boolean);
  const chosen = places.find((p) => p.key === walletNet) || places[0];
  return [
    h("div", { class: "secondary small" }, t("Scheduled tasks pay from this wallet. Send it USDC on any of these networks:", "Задачи по расписанию платят с этого кошелька. Отправь на него USDC в любой из этих сетей:")),
    places.map((p) => {
      const bud = ui.state.automationBudget, money = !bud ? null : p.key === "evm" ? bud.evmUSD : p.key === "near" ? bud.near : bud.solanaUSD;
      return h("button", { class: "live-place-row" + (chosen === p ? " on" : ""), onclick: () => { walletNet = p.key; render(); } },
        h("span", { class: "live-logo-stack" }, p.logos.map((k) => netLogo(k, 18))),
        h("span", { class: "live-place-text" }, h("b", {}, p.title), p.note ? h("span", { class: "secondary small" }, p.note) : null),
        h("b", { class: "live-place-usd num" }, money === null || money === undefined ? "…" : masked(usd(money))));
    }),
    chosen ? h("div", { class: "live-place-pay" },
      h("span", { class: "live-place-qr", title: t("Scan with the exchange or wallet app", "Сканируй приложением биржи или кошелька") }, qrCode(chosen.address, "live-qr-small")),
      h("div", { class: "live-place-address" }, h("b", { class: "small" }, chosen.key === "evm" ? t("USDC on any EVM network", "USDC в любой EVM-сети") : t(`USDC on ${chosen.title}`, `USDC в ${chosen.title}`)),
        addressLines(chosen.address, 22), copyAddress(chosen.address))) : null,
    chosen?.key === "evm" ? h("div", { class: "secondary small" }, t("The same address on Base, Arbitrum, Ethereum, Polygon, Optimism, BNB Chain, HyperEVM, Robinhood Chain and Tempo. On Robinhood Chain it counts USDG.",
      "Один адрес в Base, Arbitrum, Ethereum, Polygon, Optimism, BNB Chain, HyperEVM, Robinhood Chain и Tempo. В Robinhood Chain считается USDG.")) : null,
    h("button", { class: "live-wide-btn accent", onclick: () => suggestTopUp(a, ui.state.automationBudget, 10) }, icon("plus", 13), t("Top it up", "Пополнить")),
    gasButton(a, ui.state.automationBudget),
    h("div", { class: "live-hint" }, t("It signs scheduled tasks only, with the same checks; its key stays under this computer's Windows Hello vault.",
      "Он подписывает только задачи по расписанию, с теми же проверками; его ключ остаётся в хранилище Windows Hello этого компьютера.")),
  ];
}

// MARK: A scheduled item's own canvas

/** «every day», «каждые 3 дн.»: the engine's words for how often (ScheduledTask.Every.words), said in the window's language. */
function everyWords(w) {
  if (!w) return t("once", "один раз");
  const ru = { hour: "каждый час", day: "каждый день", week: "каждую неделю", month: "каждый месяц" }[w];
  const m = w.match(/^(\d+) (hours|days|weeks|months)$/);
  return t("every " + w, ru || (m ? `каждые ${m[1]} ${{ hours: "ч", days: "дн.", weeks: "нед.", months: "мес." }[m[2]]}` : w));
}

/** The item's frame: «SCHEDULED · EVERY 3 DAYS», and Close back to the canvas it was opened from. */
function scheduleItemHead(key) {
  const x = ui.state.tasks.find((k) => "task:" + k.id === key);
  const every = key.startsWith("perp:") || !x ? null : everyWords(x.every);
  return {
    title: [t("SCHEDULED", "ПО РАСПИСАНИЮ"), every ? every.toUpperCase() : null].filter(Boolean).join(" · "),
    extra: ui.state.scheduleKey === key ? h("button", { class: "btn small", title: t("Back to the canvas you came from", "Назад на холст, с которого пришёл"), onclick: () => act("closeSchedule") },
      icon("xmark", 12), t("Close", "Закрыть")) : null,
  };
}

/** The Mac's ScheduleItemBoard: the task with its switch and Run now, its next run as a date, who pays and who signs,
 * its steps, its last runs; Ask in chat and Delete. An open position shows its own card: its stop and take are on the exchange. */
function scheduleItemBody(b) {
  const key = b.title, s = ui.state;
  if (key.startsWith("perp:")) {
    const p = s.positions.find((x) => x.coin === key.slice(5));
    return p ? [positionCard(p), h("div", { class: "secondary small" }, t("Its stop and take sit on the exchange: they work with ihatecrypto closed.", "Стоп и тейк стоят на бирже: работают, даже когда ihatecrypto закрыт."))]
      : note(t("Closed — nothing open on this market now.", "Закрыта — на этом рынке сейчас ничего не открыто."));
  }
  const x = s.tasks.find((k) => "task:" + k.id === key);
  const runs = (s.scheduleRuns || []).filter((r) => r.task && "task:" + r.task === key);
  const runRows = runs.length ? runs.map((r) => {
    const [ic, cls, word] = r.result === "failed" ? ["xmark.circle", "loss", t("Failed", "Не вышло")] : r.result === "nothing" ? ["minus.circle", "secondary", t("Nothing to do", "Нечего делать")]
      : ["checkmark.circle", "profit", t("Done", "Готово")];
    return h("div", { class: "live-run-row" },
      h("span", { class: "live-run-time secondary small num" }, `${dayName(new Date(r.at))} ${new Date(r.at).toLocaleTimeString(dateLocale(), { hour: "2-digit", minute: "2-digit" })}`),
      h("span", { class: "live-run-icon " + cls, title: word }, icon(ic, 14)),
      h("span", { class: "live-run-title one-line", title: r.line || r.title }, r.line || r.title));
  }) : [];
  const head = h("div", { class: "live-section" }, h("span", {}, runs.length ? t(`LAST RUNS · ${runs.length}`, `ПОСЛЕДНИЕ ЗАПУСКИ · ${runs.length}`) : t("HASN'T RUN YET", "ЕЩЁ НЕ ЗАПУСКАЛАСЬ")));
  if (!x) return [h("b", { class: "secondary" }, t("No longer scheduled — what it did is below.", "Больше не в расписании — что она сделала, ниже.")), head, h("div", { class: "live-item-runs" }, runRows)];
  const held = s.schedulePaused === true, a = s.automation;
  const wallet = s.wallets.find((w) => w.id === s.active)?.label || t("Your wallet", "Твой кошелёк");
  const fact = (label, value, detail) => h("div", { class: "live-item-fact" }, h("span", { class: "live-alert-label" }, label), h("b", {}, value), detail ? h("span", { class: "secondary small" }, detail) : null);
  const next = x.next ? new Date(x.next) : null;
  return [
    h("div", { class: "live-item-head" },
      h("span", { class: "live-item-icon" }, icon(x.every ? "repeat" : "calendar", 16)),
      h("b", { class: "live-item-title" }, x.steps.length ? x.steps.join(" → ") : x.text),
      toggle("item-" + x.id, !x.paused, x.paused ? t("Paused: switch on to run it again", "На паузе: включи, чтобы снова шла") : t("Switch off to pause it", "Выключи, чтобы поставить на паузу"),
        (on) => act("taskOn", { wallet: x.id, test: on })),
      h("button", { class: "btn small", disabled: held || x.paused, title: t("Runs within the minute, here", "Запустится в течение минуты, здесь"), onclick: () => act("runNow", { name: key }) },
        icon("play", 12), t("Run now", "Запустить"))),
    next ? h("div", { class: "live-item-next" + (x.paused || held ? " dim" : "") },
      dateTile(x.next),
      h("span", { class: "live-task-text" }, h("span", { class: "live-alert-label" }, t("NEXT RUN", "СЛЕДУЮЩИЙ ЗАПУСК")),
        h("span", { class: "live-item-when" }, nextLine(x)),
        h("span", { class: "secondary small" }, x.paused ? t("paused — switch it on to run", "на паузе — включи, чтобы шла") : held ? t("the whole schedule is paused", "всё расписание на паузе")
          : x.every ? t("then again ", "потом снова ") + everyWords(x.every) : t("once, then it's off the schedule", "один раз, потом уходит из расписания")))) : null,
    h("div", { class: "live-item-facts" },
      // With an automation wallet a money task picks where it pays from, on the tile itself (the Mac's PAYS FROM menu).
      x.pick && a ? h("div", { class: "live-item-fact" }, h("span", { class: "live-alert-label" }, t("PAYS FROM", "ПЛАТИТ С")),
        h("button", { class: "live-pays-pick", title: x.automatic ? t(`Pay from «${wallet}» instead: each run waits for Confirm and ${presence()}`, `Платить с «${wallet}»: каждый запуск ждёт «Подтвердить» и ${presence()}`)
          : t(`Pay from «${a.label}» instead: runs by itself`, `Платить с «${a.label}»: пойдёт сама`),
          onclick: (e) => popup(e, [
            [t(`«${a.label}» · by itself, no ${presence()}`, `«${a.label}» · сама, без ${presence()}`), () => act("taskPays", { wallet: x.id, test: false })],
            [t(`«${wallet}» · Confirm and ${presence()} each run`, `«${wallet}» · «Подтвердить» и ${presence()} каждый запуск`), () => act("taskPays", { wallet: x.id, test: true })],
          ]) }, h("b", {}, x.automatic ? a.label : wallet), icon("chevron.updown", 10)),
        x.automatic ? h("span", { class: "secondary small" }, t("the automation wallet's budget", "бюджет кошелька автоматизации")) : null)
        : fact(t("PAYS FROM", "ПЛАТИТ С"), x.automatic && a ? a.label : wallet, x.automatic ? t("the automation wallet's budget", "бюджет кошелька автоматизации") : null),
      x.automatic ? fact(t("SIGNS", "ПОДПИСЫВАЕТ"), t("By itself", "Сама"), t(`no ${presence()}`, `без ${presence()}`))
        : fact(t("SIGNS", "ПОДПИСЫВАЕТ"), t("Your Confirm", "Твоё «Подтвердить»"), t("each move waits on its canvas", "каждое движение ждёт на холсте"))),
    x.steps.length > 1 ? h("div", { class: "live-item-steps" }, x.steps.map((st, i) => h("div", { class: "small" }, h("b", { class: "num" }, `${i + 1}. `), st))) : null,
    head, h("div", { class: "live-item-runs" }, runRows),
    h("div", { class: "live-item-foot" },
      h("button", { class: "btn small", title: t("Attaches this task to your next message: ask about it or change it in any words", "Прикрепляет задачу к следующему сообщению: спроси о ней или измени любыми словами"),
        onclick: () => askAbout(b) }, icon("bubble", 12), t("Ask in chat", "Спросить в чате")),
      h("span", { class: "spacer" }),
      h("button", { class: "link-btn loss", onclick: () => act("removeTask", { wallet: x.id }) }, icon("trash", 12), t("Delete", "Удалить"))),
  ];
}

// MARK: Positions

/** The asset's logo, or a drawn sign for markets without one (AssetBadge). */
function assetBadge(coin, size) {
  const ticker = coin.split(":").pop().toLowerCase();
  const el = logo(ticker, size);
  if (el.classList.contains("letter")) {
    el.className = "logo live-asset-sign";
    el.style.cssText = `width:${size}px;height:${size}px`;
    el.replaceChildren(icon("chart.line", Math.round(size * 0.5)));
  }
  return el;
}

function positionsBody() {
  const list = ui.state.positions;
  if (!list.length) return note(t("No open positions on Hyperliquid.", "Открытых позиций на Hyperliquid нет."));
  return list.map(positionCard);
}

/** The close the board asks about («Close SOL at market?»): the coin and whether it's half; one at a time. */
let closeAsk = null;

function positionCard(x) {
    const name = x.coin.split(":").pop();
    const long = x.side === "long";
    const lost = negative(x.pnl);
    const r = x.ruler;
    const chip = (label, ic, phrase, cls = "") => h("button", { class: "live-pos-chip " + cls, title: t("Puts it in the chat; the chat's card waits for Confirm", "Кладёт в чат; карточка ждёт «Подтвердить»"),
      onclick: () => suggest(phrase) }, icon(ic, 12), label);
    // A close is asked here first, as the Mac's dialog; then it goes in the chat, and its card waits for Confirm.
    const ask = (half) => h("button", { class: "live-pos-chip" + (half ? "" : " close"), onclick: () => { closeAsk = { coin: x.coin, half }; render(); } },
      icon(half ? "minus" : "xmark", 12), half ? t("Close ½", "Закрыть ½") : t("Close", "Закрыть"));
    const asking = closeAsk?.coin === x.coin ? closeAsk : null;
    const actions = asking
      ? h("div", { class: "live-pos-actions plan-close-ask" },
          h("span", { class: "plan-close-q" }, asking.half ? t(`Close half of ${name} at market?`, `Закрыть половину ${name} по рынку?`) : t(`Close ${name} at market?`, `Закрыть ${name} по рынку?`)),
          h("button", { class: "live-pos-chip", onclick: () => { closeAsk = null; render(); } }, t("Cancel", "Отмена")),
          h("button", { class: "live-pos-chip plan-close-yes", title: t("Puts it in the chat; the chat's card waits for Confirm", "Кладёт в чат; карточка ждёт «Подтвердить»"),
            onclick: () => { closeAsk = null; suggest(asking.half ? `close half ${name}` : `close ${name}`); } },
            asking.half ? t("Close half", "Закрыть половину") : t("Close position", "Закрыть позицию")))
      : h("div", { class: "live-pos-actions" },
          chip(t("Stop to breakeven", "Стоп в безубыток"), "shield.check", `move stop to breakeven on ${name}`, "lead"),
          chip(t("Trailing 2%", "Трейлинг 2%"), "arrow.up.right", `trail 2% on ${name}`),
          ask(true), ask(false));
    return h("div", { class: "live-position" },
      actions,
      h("div", { class: "live-pos-card plan-pos-card" },
        h("div", { class: "plan-pos-head" },
          assetBadge(x.coin, 40),
          h("span", { class: "live-pos-text" },
            h("b", {}, (long ? t("Long ", "Лонг ") : t("Short ", "Шорт ")) + name),
            h("span", { class: "live-pos-sub" }, `${masked(x.size, "•••")} ${t("at", "по")} ${x.entry}` + (r?.nowText ? ` · ${t("now", "сейчас")} ${r.nowText}` : "")
              + (x.liquidation ? ` · ${t("liq.", "ликв.")} ${x.liquidation}` : ""))),
          h("span", { class: "live-pos-pnl num " + (lost ? "loss" : "profit") }, masked(x.pnl))),
        r ? priceRuler(r) : null,
        r?.guardLine ? h("div", { class: "plan-pos-guard" }, icon("shield.check", 11), r.guardLine) : null));
}

/** The preview's `&call=live.askClose`: the first position's close question. */
export function askClose() { const p = ui.state.positions[0]; if (p) { closeAsk = { coin: p.coin, half: false }; render(); } }

/** Take, entry, now and stop on one line (the Mac's PriceRuler): red between entry and stop, green to the farthest take,
 * labels in the first row where they don't overlap and never past either end. */
const RULER_W = 428;
let rulerMeasure;
function rulerTextWidth(text) {
  rulerMeasure ??= document.createElement("canvas").getContext("2d");
  rulerMeasure.font = "600 10px Onest, sans-serif";
  return Math.ceil(rulerMeasure.measureText(text).width);
}

function priceRuler(r) {
  const values = r.points.map((p) => p.at).concat(r.now != null ? [r.now] : []);
  const low = Math.min(...values), span = Math.max(Math.max(...values) - low, 0.0001);
  const x = (v) => (v - low) / span * (RULER_W - 24) + 12;
  const pct = (px) => (px / RULER_W * 100).toFixed(2) + "%";
  const entry = r.points.find((p) => p.kind === "entry"), stop = r.points.find((p) => p.kind === "stop");
  const far = r.points.filter((p) => p.kind === "take").reduce((a, b) => !a || Math.abs(b.at - entry.at) > Math.abs(a.at - entry.at) ? b : a, null);
  const seg = (to, cls) => h("span", { class: "plan-ruler-seg " + cls, style: `left:${pct(Math.min(x(entry.at), x(to.at)))};width:${pct(Math.abs(x(to.at) - x(entry.at)))}` });
  const ends = [];
  const labels = r.points.map((p) => ({ p, at: x(p.at) })).sort((a, b) => a.at - b.at).map(({ p, at }) => {
    const w = rulerTextWidth(p.text);
    const start = Math.min(Math.max(0, at - w / 2), Math.max(0, RULER_W - w));
    let row = ends.findIndex((e) => e + 6 < start);
    if (row < 0) { row = ends.length; ends.push(start + w); } else ends[row] = start + w;
    const edge = start + w >= RULER_W ? "right:0" : `left:${pct(start)}`;
    return h("span", { class: "plan-ruler-label " + p.kind, style: `${edge};top:${30 + row * 14}px` }, p.text);
  });
  return h("div", { class: "plan-ruler", style: `height:${Math.max(76, 44 + ends.length * 14)}px`, "aria-hidden": "true" },
    h("span", { class: "plan-ruler-track" }),
    stop ? seg(stop, "loss") : null,
    far ? seg(far, "profit") : null,
    r.now != null ? h("span", { class: "plan-ruler-now", style: `left:calc(${pct(x(r.now))} - 2px)` }) : null,
    labels);
}

// MARK: Multisig

/** The keyholes picture (KeyholesPicture): a lock with a hole per key, the ones that must sign lit. */
function keyholes(total, lit, chip, done) {
  const n = Math.max(1, Math.min(total, 8)), sp = n <= 4 ? 36 : 28, w = sp * n + 36, hgt = 66, x = 480 - w - 26, y = (100 - hgt) / 2 + 6;
  const holes = Array.from({ length: n }, (_, i) => {
    const cx = x + 18 + sp / 2 + sp * i;
    const d = `M${cx - 6} ${y + 22}a6 6 0 1 0 12 0a6 6 0 1 0 -12 0M${cx - 4} ${y + 25}L${cx + 4} ${y + 25}L${cx + 6} ${y + 44}L${cx - 6} ${y + 44}Z`;
    return svg("path", i < lit ? { d, fill: "#12FF80" } : { d, fill: "none", stroke: "#fff", "stroke-width": "1.8" });
  });
  return h("div", { class: "live-keyholes" },
    svg("svg", { viewBox: "0 0 480 100", preserveAspectRatio: "xMaxYMid meet", "aria-hidden": "true" },
      svg("rect", { x, y, width: w, height: hgt, rx: 10, fill: "#1E211F", stroke: "#fff", "stroke-width": "2.5" }), holes),
    h("span", { class: "live-keyholes-chip" + (done ? " done" : "") }, chip));
}

const safePrefix = (k) => ({ base: "base", arbitrum: "arb1", optimism: "oeth", ethereum: "eth", polygon: "matic", bnb: "bnb", hyperevm: "hyper", robinhood: "robinhood", tempo: "tempo" }[k] || k);
const explorers = { base: "https://basescan.org", arbitrum: "https://arbiscan.io", ethereum: "https://etherscan.io", optimism: "https://optimistic.etherscan.io",
  polygon: "https://polygonscan.com", bnb: "https://bscscan.com" };

function keyRow(k) {
  const where = k.kind === "pc" ? presence() : k.kind === "ledger" ? t("the Ledger's buttons", "кнопки Ledger") : t("signs in Safe's app", "подписывает в приложении Safe");
  const here = k.kind !== "address";
  return h("div", { class: "live-key-row" },
    h("span", { class: "live-key-icon" }, icon(k.kind === "pc" ? "laptop" : k.kind === "ledger" ? "drive" : "iphone", 15)),
    h("span", { class: "live-key-text" }, h("b", { class: "one-line" }, k.name), h("span", { class: "secondary small one-line" }, `${k.address.slice(0, 6)}…${k.address.slice(-4)} · ${where}`)),
    h("span", { class: "live-key-role" + (here ? " here" : "") }, here ? t("signs here", "подписывает здесь") : t("elsewhere", "в другом месте")));
}

function multisigBody() {
  const all = ui.state.multisigs;
  if (!all.length) {
    return [
      keyholes(2, 0, t("No multisig yet", "Мультисига пока нет"), false),
      h("p", { class: "live-lead" }, t("Its money moves only when enough of its keys sign: this computer's and your phone's or a person's.",
        "Его деньги двигаются, только когда подпишет достаточно ключей: этого компьютера и твоего телефона или другого человека.")),
      h("button", { class: "btn primary", onclick: () => suggest("make a multisig 2 of 2 with Phone on base") }, icon("plus", 13), t("Make a multisig", "Сделать мультисиг")),
    ];
  }
  return all.map((m) => {
    const net = m.networks[0] || "base";
    // Its money on each network (the Mac's «By network»): «…» until the node answers, «empty» when there's nothing.
    const held = ui.state.multisigHeld?.[m.address];
    const byNet = new Map((held || []).map((x) => [x.network, x]));
    const known = (held || []).filter((x) => x.usd !== null && x.usd !== undefined);
    return h("div", { class: "live-msig" },
      keyholes(m.keys.length, m.threshold, t("Ready", "Готов"), true),
      h("div", { class: "live-msig-body" },
        h("div", { class: "live-msig-heading" }, h("span", { class: "live-verb" }, "MULTISIG"), h("span", { class: "live-dot" }), h("span", { class: "live-safe-s" }, "S"),
          h("b", {}, t(`Safe · any ${m.threshold} of ${m.keys.length} keys`, `Safe · любые ${m.threshold} из ${m.keys.length} ключей`))),
        h("div", { class: "live-msig-total num" }, known.length ? masked(usd(known.reduce((a, x) => a + Number(x.usd), 0))) : "…"),
        addressLines(m.address, 22), copyAddress(m.address),
        section(t("By network · same address", "По сетям · один адрес")),
        m.networks.map((k) => {
          const x = byNet.get(k), read = x && x.coins !== null && x.coins !== undefined;
          return h("div", { class: "live-net-row" }, netLogo(k, 22), h("b", {}, netName(k)), h("span", { class: "spacer" }),
            h("span", { class: "live-net-coins secondary small one-line", title: read ? x.coins : "" }, !read ? "…" : x.coins || t("empty", "пусто")),
            h("b", { class: "live-net-usd num" }, read ? masked(usd(x.usd)) : ""),
            explorers[k] ? h("button", { class: "icon-btn", title: t("Its history on the explorer", "История в обозревателе"), onclick: () => openLink(`${explorers[k]}/address/${m.address}`) }, icon("arrow.up.right.square", 14)) : null);
        }),
        section(t(`Keys · any ${m.threshold} of ${m.keys.length} move money`, `Ключи · любые ${m.threshold} из ${m.keys.length} двигают деньги`)),
        m.keys.map(keyRow),
        h("div", { class: "btns" },
          h("button", { class: "btn live-dark", onclick: () => suggest(`send 1 USDC from the multisig to Alex on ${net}`) }, icon("arrow.up", 13), t("Send", "Отправить")),
          h("button", { class: "btn", onclick: () => suggest(`send 10 USDC on ${net} to Multisig`) }, icon("arrow.down", 13), t("Put money in", "Положить деньги")),
          h("button", { class: "btn", onclick: () => openLink(`https://app.safe.global/home?safe=${safePrefix(net)}:${m.address}`) }, icon("arrow.up.right.square", 13), t("Open in Safe", "Открыть в Safe"))),
        h("div", { class: "live-hint" }, t("Its keys and how many must sign change only with that many signatures, in Safe's own app for now.",
          "Ключи и сколько должно подписать меняются только таким же числом подписей, пока в приложении Safe."))));
  });
}

// MARK: Ledger

/** The Ledger drawn small (LedgerGlyph). */
function ledgerGlyph(size = 30) {
  return svg("svg", { viewBox: "0 0 30 15", width: size, height: size / 2, class: "live-ledger-glyph", "aria-hidden": "true" },
    svg("rect", { x: 0, y: 1.2, width: 30, height: 12.6, rx: 4.8, fill: "currentColor" }),
    svg("rect", { x: 3, y: 4.5, width: 12.6, height: 6, rx: 1.5, fill: "#FF7A2F" }),
    svg("circle", { cx: 23.7, cy: 7.5, r: 4.4, fill: "none", stroke: "var(--card)", "stroke-opacity": "0.5", "stroke-width": "1.5" }));
}

function ledgerBody() {
  const c = ui.state.ledgerConnect;
  const w = ui.state.wallets.find((x) => x.kind === "ledger");
  const on = ui.state.active === "ledger";
  const busy = c?.busy === true;
  const evm = c?.evm || w?.evm, solana = c?.solana || w?.solana;
  const step = (label, value, done, solanaApp) => h("div", { class: "live-ledger-step" + (done ? " done" : "") },
    icon(done ? "checkmark.circle" : "circle.dotted", 17),
    h("span", { class: "live-ledger-step-text" }, h("span", { class: "live-alert-label" }, label),
      value ? (solanaApp === undefined ? h("b", {}, value) : addressLines(value, 26)) : h("span", { class: "secondary" }, "—"),
      done && solanaApp !== undefined && !busy ? h("div", { class: "btns" }, copyAddress(value),
        h("button", { class: "btn small", title: t("The Ledger shows this address on its own screen, to compare", "Ledger покажет этот адрес на своём экране — сверь"),
          onclick: () => act("showOnLedger", { test: solanaApp }) }, t("Show on Ledger", "Показать на Ledger"))) : null));
  if (!c && !w) {
    return [
      h("div", { class: "live-ledger-title" }, ledgerGlyph(34)),
      h("p", { class: "live-lead" }, t("Plug it in over USB or turn on Bluetooth, and unlock it. ihatecrypto reads one address from its Ethereum app and one from its Solana app; every move is then shown on its screen and approved with its buttons.",
        "Подключи его по USB или включи Bluetooth и разблокируй. ihatecrypto прочитает по адресу из приложений Ethereum и Solana; дальше каждое движение показывается на его экране и подтверждается его кнопками.")),
      h("button", { class: "btn primary", onclick: () => act("connectLedger") }, t("Connect a Ledger", "Подключить Ledger")),
      ledgerFoot(),
    ];
  }
  return [
    step(t("USB OR BLUETOOTH", "USB ИЛИ BLUETOOTH"), c?.model ? t(`${c.model} found`, `${c.model} найден`) : w ? t("Saved", "Сохранён") : null, !!(c?.model || w)),
    step(t("ETHEREUM APP · EVM", "ПРИЛОЖЕНИЕ ETHEREUM · EVM"), evm, !!evm, false),
    step(t("SOLANA APP", "ПРИЛОЖЕНИЕ SOLANA"), solana, !!solana, true),
    c ? h("div", { class: "live-ledger-status" + (busy ? "" : " done") }, icon(busy ? "hand.tap" : "checkmark.seal", 14), h("span", {}, ui.state.signer.ledger || c.status)) : null,
    c?.problem ? h("div", { class: "live-ledger-problem" }, icon("exclamationmark.triangle", 14), h("span", {}, c.problem),
      h("button", { class: "btn small", onclick: () => act("connectLedger") }, t("Try again", "Ещё раз"))) : null,
    (c?.notes || []).map((n) => h("div", { class: "live-ledger-note" }, icon("gear", 13), h("span", {}, n))),
    w && !busy ? h("div", { class: "btns" },
      on ? h("span", { class: "live-ledger-on" }, icon("check", 13), t("It signs every move now", "Подписывает каждое движение"))
        : h("button", { class: "btn primary", onclick: () => act("switchWallet", { wallet: "ledger" }) }, icon("signature", 14), t("Sign with Ledger", "Подписывать через Ledger")),
      h("button", { class: "btn", onclick: () => act("connectLedger") }, t("Connect again", "Подключить снова")),
      h("button", { class: "btn", onclick: () => act("removeWallet", { wallet: "ledger" }) }, t("Forget", "Забыть"))) : null,
    ledgerFoot(),
  ];
}

const ledgerFoot = () => h("div", { class: "live-hint" }, t("The keys never leave the Ledger. ihatecrypto checks every move first — only your own addresses, no unlimited approvals — then the Ledger shows it, and nothing is signed until you approve it on the device.",
  "Ключи никогда не покидают Ledger. ihatecrypto сначала проверяет каждое движение — только твои адреса, никаких безлимитных разрешений, — потом Ledger показывает его, и ничего не подписывается, пока ты не подтвердишь на устройстве."));
