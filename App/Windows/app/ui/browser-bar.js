// The browser's toolbar (the Mac's BrowserView): it moves the site through the app's own commands and shows what the
// engine says about the site. It never sees the page's contents, and the page never sees it.

import { h, icon, t, ui, addIcons } from "./core.js";
import { loadError } from "./browser-words.js";
import { follow, grip } from "./panel-grip.js";

addIcons({
  "exclamationmark.octagon": "M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3zM12 7.5v5.5M12 16v.5",
  "hand.raised": "M8 13V5.5a1.5 1.5 0 0 1 3 0V11M11 10.5V4a1.5 1.5 0 0 1 3 0v6.5M14 10.5V5.5a1.5 1.5 0 0 1 3 0V14a7 7 0 0 1-7 7 6 6 0 0 1-5-3l-2-3.5a1.5 1.5 0 0 1 2.5-1.6L8 15",
  "macwindow.on.rectangle": "M7.5 8.5h13v11h-13zM7.5 11.5h13M3.5 4.5h13v4M3.5 4.5v11h4",
});

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
const bar = document.getElementById("bar");
// Beside the chat (the Mac's WebPanel with the wallet): a row of the panel's own over the toolbar, which moves it, and
// its left edge, which sizes it. In its own window neither shows.
const head = document.getElementById("panel-head");
const edge = document.getElementById("edge");

/** The toolbar's last answer from the app, and the address being typed (kept while the field has focus). */
let s = null;
let error = "";

const field = h("input", {
  id: "address", spellcheck: "false", autocomplete: "off",
  onkeydown: (e) => {
    if (e.key === "Enter" && field.value.trim()) go("open", field.value.trim());
    if (e.key === "Escape") { field.value = s?.url || ""; field.blur(); }
  },
  // A redraw puts the focus back (below): only a real click selects the whole address.
  onfocus: () => { if (!refocusing) field.select(); },
});
let refocusing = false;

async function go(action, url) {
  error = "";
  try {
    await invoke("browser_nav", { action, url });
    if (action === "open") field.blur();
  } catch (e) {
    error = String(e);
  }
  draw();
}

function badge(check, real) {
  const [name, cls, tip] = {
    known: ["checkmark.seal", "known", t("Known site", "Знакомый сайт")],
    lookalike: ["exclamationmark.octagon", "lookalike", t(`Looks like ${real || "a known site"}, but it's a different address`, `Похож на ${real || "знакомый сайт"}, но адрес другой`)],
    local: ["doc", "local", ""],
  }[check] || ["questionmark.circle", "unknown", t("Unfamiliar site: check the address", "Незнакомый сайт: проверь адрес")];
  return h("span", { class: "badge " + cls, title: tip }, icon(name, 15));
}

function draw() {
  // The field moves into the new row below, and moving it out of the page drops its focus: a redraw while typing (a
  // site's loads and titles come often) would lose what's typed. Read it before anything moves, put it back after.
  const typing = document.activeElement === field, from = field.selectionStart, to = field.selectionEnd;
  const host = s?.blockedHost || (() => { try { return new URL(s.url).host; } catch { return ""; } })();
  if (s && document.activeElement !== field) field.value = s.url === "about:blank" ? "" : s.url;
  field.placeholder = t("Site address", "Адрес сайта");
  const row = h("div", { class: "bar-row" },
    h("button", { class: "icon-btn", title: t("Back", "Назад"), disabled: s ? !s.back : true, onclick: () => go("back") }, icon("chevron.left")),
    h("button", { class: "icon-btn", title: t("Forward", "Вперёд"), disabled: s ? !s.forward : true, onclick: () => go("forward") }, icon("chevron.right")),
    s?.loading
      ? h("button", { class: "icon-btn", title: t("Stop", "Остановить"), onclick: () => go("stop") }, icon("xmark"))
      : h("button", { class: "icon-btn", title: t("Reload", "Обновить"), onclick: () => go("reload") }, icon("arrow.clockwise")),
    h("label", { class: "address-pill", for: "address" }, badge(s?.check, s?.real), field),
    s?.connected ? h("button", { class: "connected", onclick: () => go("disconnect"),
      title: t("The site sees your wallet's address this session. Disconnect takes it away and reloads the page.",
        "Сайт видит адрес твоего кошелька в этом сеансе. «Отключить» забирает его и перезагружает страницу.") },
      icon("link", 13), t("Connected · Disconnect", "Подключён · Отключить")) : null,
    h("div", { class: "with-wallet", title: t("Every signature the site asks for comes to the canvas as a card first: nothing is signed without Confirm and Windows Hello.",
      "Каждая подпись, которую просит сайт, сначала приходит на холст карточкой: без «Подтвердить» и Windows Hello ничего не подписывается.") },
      icon("wallet.pass", 15),
      s?.wallet
        ? h("div", { class: "one-line" }, h("b", {}, t("With your wallet", "С твоим кошельком")),
          h("span", { class: "one-line" }, t("wallet ", "кошелёк ") + s.wallet + " · " + t("every signature is shown first", "каждую подпись сначала видишь ты")))
        : h("div", { class: "one-line" }, h("b", {}, t("No wallet here", "Без кошелька")),
          h("span", { class: "one-line" }, t("only watching: nothing here can sign", "только просмотр: подписывать нечем")))),
  );
  const blocked = s?.blocked ? h("div", { class: "blocked" }, icon("hand.raised", 15),
    t(`${host} is on MetaMask's list of phishing sites, so ihatecrypto didn't open it and offers it no wallet. If you typed it, check the address letter by letter.`,
      `${host} в списке фишинговых сайтов MetaMask, поэтому ihatecrypto его не открыл и не даёт ему кошелёк. Если ты вводил адрес сам, сверь его по буквам.`)) : null;
  const problem = error || (s?.blocked ? "" : loadError(s?.error));
  bar.replaceChildren(...[row, blocked, problem ? h("div", { class: "load-error" }, problem) : null].filter(Boolean));
  const panel = !!s?.panel;
  document.body.classList.toggle("in-panel", panel);
  head.hidden = edge.hidden = !panel;
  if (panel) {
    edge.title = t("Drag to resize the page · double-click: its own width", "Потяни, чтобы изменить ширину · двойной щелчок: обычная ширина");
    head.title = t("Drag to move · double-click: back beside the chat", "Потяни, чтобы сдвинуть · двойной щелчок: обратно к чату");
    head.replaceChildren(
      icon("wallet.pass", 14),
      s.wallet ? h("b", {}, t("With your wallet", "С твоим кошельком")) : h("b", {}, t("No wallet here", "Без кошелька")),
      h("span", { class: "one-line" }, s.wallet
        ? `· ${t("wallet", "кошелёк")} ${s.wallet} · ${t("every signature is shown first", "каждую подпись сначала видишь ты")}`
        : `· ${t("only watching: nothing here can sign", "только просмотр: подписывать нечем")}`),
      h("span", { class: "spacer" }),
      h("button", { class: "icon-btn", title: t("Move to its own window", "В отдельное окно"), onclick: () => go("window") }, icon("macwindow.on.rectangle", 15)),
      h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => go("close") }, icon("xmark")),
    );
  }
  if (typing && document.activeElement !== field) {
    refocusing = true;
    field.focus({ preventScroll: true });
    try { field.setSelectionRange(from, to); } catch {}
    refocusing = false;
  }
}

let timer = null;
async function refresh() {
  try {
    s = await invoke("browser_state");
    error = "";
    ui.state = { russian: s.russian };
    document.documentElement.lang = s.russian ? "ru" : "en";
  } catch {
    // The toolbar can load before the site's webview exists: the page's first load asks again.
  }
  draw();
}
/** Page loads, title changes and every state of the engine say «look again»; a burst of them is one look. */
tauri.event.listen("browser", () => { clearTimeout(timer); timer = setTimeout(refresh, 150); });

// The toolbar's height follows what it shows (the panel's row, the phishing banner, an error line): the site gets the rest.
const top = document.getElementById("top");
new ResizeObserver(() => invoke("browser_layout", { height: Math.ceil(top.getBoundingClientRect().height) }).catch(() => {})).observe(top);

// Beside the chat it follows the rail and the chat (this page shares the main page's storage) and moves by its row.
follow();
grip(head, edge);
draw();
refresh();
