// The pages that take the canvas's place beside the rail, as the Mac draws them: Settings (SettingsView.swift — a
// sidebar of tabs, each a column of sections with rows), History (HistoryView.swift) and Top up (ReceiveBoard in
// Receive.swift). Settings cover the chat while open; History and Top up keep it floating, as the canvas does.

import { ui, act, engine, render, t, h, svg, icon, addIcons, logo, mascot, toast, copy, link, store, invoke, when, presence, qrCode, usd } from "./core.js";
import { railEdge, companyLogo } from "./rail.js";
import { chatPanel } from "./chat.js";
import { segmented, toggle, showImport, showImportKey, watchFields } from "./onboarding.js";
import { codeMaker, lockAfterPicker } from "./lock.js";

const back = () => { ui.view = "canvas"; render(); };

addIcons({
  "info.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5.5M12 7.6v.4",
  sparkles: "M10 3.5l1.6 4.4L16 9.5l-4.4 1.6L10 15.5l-1.6-4.4L4 9.5l4.4-1.6zM17.5 14l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
  creditcard: "M3.5 6h17v12h-17zM3.5 10h17M7 14.5h4",
  wand: "M4 20L15 9M13.5 4.5v2M17.5 8.5h2M16.3 5.7l1.4-1.4M19.5 4.5l-1.2 1.2M12.5 7l4.5 4.5",
  "arrow.up.right.square": "M5 5h14v14H5zM10 14l5-5M10.5 9H15v4.5",
  key: "M8 15a4 4 0 1 1 3.5-6H21v3h-2v2h-2v-2h-5.5A4 4 0 0 1 8 15zM7 11h.01",
  server: "M4 4h16v6H4zM4 14h16v6H4zM8 7h.01M8 17h.01",
  circle: "M12 20a8 8 0 1 0 0-16 8 8 0 0 0 0 16z",
  dial: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 12l4-4M12 5.5v1.5M5.5 12H7M17 12h1.5",
  "gauge.0": "M4 16a8 8 0 0 1 16 0M12 16l-5.2-3",
  "gauge.50": "M4 16a8 8 0 0 1 16 0M12 16V9.5",
  "gauge.100": "M4 16a8 8 0 0 1 16 0M12 16l5.2-3",
  shield: "M12 3l7 3v5c0 4.5-3 8.5-7 10-4-1.5-7-5.5-7-10V6zM12 3v18",
  book: "M5 4.5A1.5 1.5 0 0 1 6.5 3H19v16H6.5A1.5 1.5 0 0 0 5 20.5zM5 20.5A1.5 1.5 0 0 0 6.5 22H19",
  "chart.bar": "M5 20v-8M10 20V5M15 20v-6M20 20V9",
  "square.stack": "M4 9l8-4 8 4-8 4zM4 13l8 4 8-4M4 17l8 4 8-4",
  "arrow.up.right": "M7 17L17 7M9 7h8v8",
  multiply: "M7 7l10 10M17 7L7 17",
  flag: "M5 21V4M5 4h11l-2 4 2 4H5",
  "hand.raised": "M8 13V6.5a1.5 1.5 0 0 1 3 0V11M11 11V4.5a1.5 1.5 0 0 1 3 0V11M14 11V6.5a1.5 1.5 0 0 1 3 0V14c0 4-2.5 7-6 7-2.8 0-4.3-1.7-5.6-4L3.6 13a1.5 1.5 0 0 1 2.5-1.6L8 13",
  percent: "M19 5L5 19M7 9a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM17 19a2 2 0 1 0 0-4 2 2 0 0 0 0 4z",
  "arrow.left.arrow.right": "M4 8h14M14 4l4 4-4 4M20 16H6M10 12l-4 4 4 4",
  "chart.line": "M3 17l6-6 4 4 8-8M15 7h6v6",
  bridge: "M5 8a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM19 20a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM7 6c9 0 1 12 10 12",
});

// MARK: Appearance — kept in this window's storage, applied as the page loads

/** System follows Windows (the default); Light and Dark fix it. */
function applyTheme() {
  const theme = store("theme");
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
  else if (theme === "system") delete document.documentElement.dataset.theme;
}
applyTheme();

// MARK: Settings' building blocks (SettingsSection, SettingsGroup, SettingsRow, SettingsPill, SettingsInfo)

const tile = (name, size = 26, active = true) => h("span", { class: "pg-set-icon" + (active ? "" : " off"), style: `width:${size}px;height:${size}px;border-radius:${size * 0.28}px` }, icon(name, Math.round(size * 0.52)));

function section(iconName, title, line, ...content) {
  return h("section", { class: "pg-set-section" },
    h("div", { class: "pg-set-section-head" }, tile(iconName, 34), h("div", {}, h("h2", {}, title), line ? h("div", { class: "secondary" }, line) : null)),
    content);
}
const group = (...rows) => h("div", { class: "pg-set-group" }, rows);

/** The ⓘ next to a row's name: its longer text opens under the row. */
const openInfo = new Set();
function info(key, text) {
  return h("button", { class: "pg-set-info" + (openInfo.has(key) ? " on" : ""), title: t("About this", "Подробнее"),
    onclick: () => { openInfo.has(key) ? openInfo.delete(key) : openInfo.add(key); render(); } }, icon("info.circle", 13));
}

/** One row: an icon tile (or a logo), the name, its ⓘ, and its controls on the right. */
function row({ label, ico = "gear", mark, about, cls = "" }, ...controls) {
  const key = typeof label === "string" ? label : "";
  return h("div", { class: "pg-set-row " + cls },
    h("div", { class: "pg-set-row-line" }, mark || tile(ico), h("span", { class: "pg-set-label" }, label), about ? info(key, about) : null,
      h("span", { class: "spacer" }), h("span", { class: "pg-set-controls" }, controls)),
    about && openInfo.has(key) ? h("div", { class: "pg-set-info-text selectable" }, about) : null);
}
const pill = (text, good) => h("span", { class: "pg-set-pill" + (good ? " good" : "") }, h("i", {}), text);
const letterTile = (label, size, active) => h("span", { class: "pg-letter" + (active ? " on" : ""), style: `width:${size}px;height:${size}px;font-size:${Math.round(size * 0.42)}px` }, (label || "?").slice(0, 1).toUpperCase());
const short = (a) => (a && a.length > 16 ? a.slice(0, 6) + "…" + a.slice(-6) : a || "");
const copyShort = (address, what) => h("button", { class: "btn small", title: t(`Copy the ${what} address`, `Скопировать адрес ${what}`), onclick: () => copy(address) }, icon("copy", 12), h("span", { class: "pg-address-inline" }, short(address)));

// MARK: Settings

const tabs = [
  ["general", "gear", () => t("General", "Основные")],
  ["agent", "chart.line", () => t("Trading agent", "Торговый агент")],
  ["model", "sparkles", () => t("AI model", "Модель ИИ")],
  ["wallet", "creditcard", () => t("Wallet", "Кошелёк")],
  ["modules", "square.grid", () => t("Modules", "Модули")],
  ["skills", "wand", () => t("Skills", "Навыки")],
];

export function settingsPage() {
  let tab = store("settingsTab");
  if (!tabs.some(([id]) => id === tab)) tab = "general";
  const s = ui.state;
  const [title, line, body] = {
    general: [t("General", "Основные"), t("How the app talks to you and keeps running.", "Как приложение говорит с тобой и работает в фоне."), general],
    agent: [t("Trading agent", "Торговый агент"), t("How the assistant trades for you. Code checks every plan against these rules.", "Как ассистент торгует за тебя. Код проверяет каждый план по этим правилам."), agentTab],
    modules: [t("Modules", "Модули"), t("What the assistant may use. Off means it never plans a move there. Money already there still shows on the canvas.", "Чем ассистенту можно пользоваться. Выключено — он никогда не планирует там движений. Деньги, которые там уже есть, по-прежнему видны на холсте."), modulesTab],
    model: [t("AI model", "Модель ИИ"), t("What reads your chat: a subscription, an API key, or a free model on this PC.", "Что читает твой чат: подписка, API-ключ или бесплатная модель на этом компьютере."), modelTab],
    wallet: [t("Wallet", "Кошелёк"), t(`Your wallets, the key that signs and ${presence()}.`, `Твои кошельки, ключ, который подписывает, и ${presence()}.`), walletTab],
    skills: [t("Skills", "Навыки"), t("Instructions the assistant follows for one kind of work. Switch them on and off, or add your own from a file.", "Инструкции, которым ассистент следует в одном виде работы. Включай и выключай их или добавь свой из файла."), skillsTab],
  }[tab];
  return h("div", { class: "pg-settings", style: `left:${railEdge() + 10}px` },
    h("div", { class: "pg-settings-head" }, h("b", {}, t("Settings", "Настройки")),
      h("button", { class: "icon-btn pg-settings-close", title: t("Close Settings (Esc)", "Закрыть настройки (Esc)"), onclick: back }, icon("xmark", 14))),
    h("div", { class: "pg-settings-body" },
      h("nav", { class: "pg-settings-side" },
        h("button", { class: "pg-settings-assistant", title: t("Back to the canvas (Esc)", "Назад к холсту (Esc)"), onclick: back }, mascot(28, "tuning"),
          h("span", {}, h("b", {}, t("Assistant", "Ассистент")), h("span", { class: "secondary" }, t("In Settings", "В настройках")))),
        tabs.map(([id, ico, name]) => h("button", { class: "pg-settings-tab" + (id === tab ? " on" : ""), onclick: () => { store("settingsTab", id); render(); } },
          tile(ico, 24, id === tab), name()))),
      h("div", { class: "pg-settings-scroll", "data-keep": "settings-" + tab },
        h("div", { class: "pg-set-page" + (tab === "modules" ? " pg-set-wide" : "") }, h("div", { class: "pg-set-page-head" }, h("h1", {}, title), h("div", { class: "secondary" }, line)), body(s)))));
}

// General: appearance, language, the chat, updates.

function general(s) {
  const theme = store("theme") || "system";
  return [
    section("eye", t("Appearance", "Оформление"), t("Light, dark, or as Windows is set.", "Светлое, тёмное или как в Windows."),
      group(row({ label: t("Theme", "Тема"), ico: "eye" }, segmented([["system", t("System", "Системная")], ["light", t("Light", "Светлая")], ["dark", t("Dark", "Тёмная")]],
        theme, (v) => { store("theme", v); applyTheme(); render(); }, "pg-set-seg")))),
    section("globe", t("Language", "Язык"), t("The app's own texts, and the chat's replies.", "Тексты приложения и ответы чата."),
      group(
        row({ label: t("App language", "Язык приложения"), ico: "character.book", about: t("Menus, buttons and the app's texts. System follows Windows' language. Many texts are still English only; more are translated with updates.",
          "Меню, кнопки и тексты приложения. «Системный» — как язык Windows. Часть текстов пока только на английском; переводы добавляются с обновлениями.") },
          h("select", { id: "set-app-language", onchange: (e) => act("appLanguage", { language: e.target.value }) },
            [["system", t("System", "Системный")], ["en", "English"], ["ru", "Русский"]].map(([v, l]) => h("option", { value: v, selected: (s.appLanguage || "system") === v }, l)))),
        row({ label: t("Reply language", "Язык ответов"), ico: "text.bubble", about: t("Auto answers in the language you write in. Tickers, amounts and addresses stay as they are.", "«Авто» отвечает на языке твоего сообщения. Тикеры, суммы и адреса остаются как есть.") },
          h("select", { id: "set-reply-language", onchange: (e) => act("language", { language: e.target.value }) },
            [["auto", t("Auto — your language", "Авто — твой язык")], ["en", "English"], ["ru", "Русский"]].map(([v, l]) => h("option", { value: v, selected: s.language === v }, l)))))),
    section("square.and.pencil", t("Chat", "Чат"), t("What this canvas's chat keeps.", "Что хранит чат этого холста."),
      group(row({ label: t("This canvas's chat", "Чат этого холста"), ico: "square.and.pencil", about: t("Clears the messages of the open canvas. Its blocks, the history and the wallet stay.", "Стирает сообщения открытого холста. Его блоки, история и кошелёк остаются.") },
        h("button", { class: "btn small", onclick: () => act("clearChat") }, t("Clear", "Очистить"))))),
    noticeSection(s),
    section("power", t("Background", "Фон"), t("Alerts and scheduled tasks with the window closed.", "Оповещения и задачи по расписанию при закрытом окне."),
      group(loginRow(s), row({ label: t("Closing the window", "Закрытие окна"), ico: "power", about: t("ihatecrypto stays in the tray: price alerts, balances and scheduled tasks go on. Quit from the tray icon's menu ends it.", "ihatecrypto остаётся в трее: оповещения, балансы и задачи продолжают работать. «Выход» в меню значка в трее закрывает его совсем.") },
        h("span", { class: "secondary" }, t("Keeps it in the tray", "Оставляет в трее"))))),
    s.nodes ? nodesSection(s.nodes) : null,
    s.nodes ? oneClickSection(s.nodes) : null,
    section("arrow.down.circle", t("About", "О приложении"), t("An update installs only when its signature matches ihatecrypto's key.", "Обновление ставится, только если его подпись совпадает с ключом ihatecrypto."),
      group(row({ label: t("ihatecrypto for Windows", "ihatecrypto для Windows"), ico: "arrow.down.circle" },
        h("button", { class: "btn small", onclick: checkUpdate }, t("Check for updates", "Проверить обновления"))))),
  ];
}

// Notifications (the Mac's NoticeSettings) and Open at login: the system part's rows (Notices.swift, system.rs).

const NOTICE_ICONS = { incoming: "arrow.down.circle", positions: "chart.line", orders: "checkmark.circle", alerts: "bell", moves: "arrow.left.arrow.right",
  hlp: "lock.open", signature: "lock", confirm: "hand.raised", update: "arrow.clockwise" };
const noticeList = { open: false };
function noticeSection(s) {
  const list = s.notices || [];
  const on = list.filter((n) => n.on).length;
  return section("bell", t("Notifications", "Уведомления"), t("When the assistant taps you, even with the window closed.", "Когда ассистент зовёт тебя, даже при закрытом окне."),
    group(
      row({ label: t("Notifications", "Уведомления"), ico: "bell", about: t("They show while the window is in the tray or behind others; click one to come back to it. Windows decides whether they can appear at all: Settings › System › Notifications › ihatecrypto.",
          "Они появляются, когда окно в трее или за другими; нажми на уведомление, чтобы вернуться в окно. Показывать ли их вообще, решает Windows: «Параметры › Система › Уведомления › ihatecrypto».") },
        h("button", { class: "btn small", onclick: () => act("noticeTest") }, t("Send a test", "Отправить тест")),
        h("button", { class: "btn small", onclick: () => invoke("open_notification_settings").catch((x) => toast(String(x))) }, t("Windows settings…", "Настройки Windows…"))),
      row({ label: t("Which events", "Какие события"), ico: "bell" },
        h("button", { class: "btn small", onclick: () => { noticeList.open = !noticeList.open; render(); } }, t(`${on} of ${list.length}`, `${on} из ${list.length}`), icon(noticeList.open ? "chevron.up" : "chevron.down", 10))),
      noticeList.open ? list.map((n) => row({ label: n.title, ico: NOTICE_ICONS[n.id] || "bell" }, toggle("notice-" + n.id, n.on, (v) => act("notice", { name: n.id, test: v })))) : null));
}

/** Open at login (the engine's `openAtLogin`; main.rs sets the registry's Run key and says back how Windows has it). */
function loginRow(s) {
  return row({ label: t("Open at login", "Открывать при входе"), ico: "power", about: t("Starts ihatecrypto in the tray when you log in to Windows, its window closed: the schedule, alerts and stops go on. Stop and take-profit orders already on the exchange keep working either way.",
      "Запускает ihatecrypto в трее при входе в Windows, с закрытым окном: расписание, оповещения и стопы продолжают работать. Стоп и тейк, уже стоящие на бирже, работают в любом случае.") },
    toggle("open-at-login", s.openAtLogin === true, (v) => act("openAtLogin", { test: v }), s.openAtLogin === undefined));
}

// Your own nodes and the NEAR Intents key (Nodes.swift): what's typed is checked by the engine before it's kept; only a
// node's host comes back.

const secretForm = { open: null, text: "", problem: null, checking: false };
function openSecret(id) { Object.assign(secretForm, { open: secretForm.open === id ? null : id, text: "", problem: null, checking: false }); render(); }
async function saveSecret(op, args) {
  secretForm.checking = true; secretForm.problem = null; render();
  try { await engine(op, args); Object.assign(secretForm, { open: null, text: "", checking: false }); }
  catch (e) { secretForm.problem = e.message; secretForm.checking = false; }
  render();
}
function secretPanel(id, why, steps, site, placeholder, save, password) {
  return h("div", { class: "pg-set-panel" },
    why.map((x) => h("div", { class: "secondary small" }, x)),
    h("ol", { class: "pg-node-steps small" }, steps.map((x) => h("li", { class: "selectable" }, x))),
    site ? h("div", { class: "small" }, link(site[0], site[1])) : null,
    h("input", { id: "secret-" + id, class: "pg-set-input pg-node-input address", type: password ? "password" : "text", placeholder, value: secretForm.text, autocomplete: "off", spellcheck: "false",
      oninput: (e) => (secretForm.text = e.target.value), onkeydown: (e) => { if (e.key === "Enter") save(); if (e.key === "Escape") openSecret(null); } }),
    secretForm.problem ? h("div", { class: "small loss" }, icon("exclamationmark.triangle", 12), " ", secretForm.problem) : null,
    h("div", { class: "btns" }, h("button", { class: "btn", onclick: () => openSecret(null) }, t("Cancel", "Отмена")),
      h("button", { class: "btn primary", disabled: secretForm.checking || !secretForm.text.trim(), onclick: save }, secretForm.checking ? t("Checking…", "Проверяю…") : t("Check and save", "Проверить и сохранить"))));
}

function nodesSection(v) {
  return section("server", t("Your own nodes", "Свои ноды"), t("Where the app reads each network and sends its transactions.", "Где приложение читает каждую сеть и отправляет её транзакции."),
    group(v.nodes.map((n) => {
      const sol = n.id === "solana";
      const label = n.host ? `${n.title}: ${n.host}` : `${n.title}: ${sol ? t("Solana's public node", "публичная нода Solana") : t("built-in free nodes", "встроенные бесплатные ноды")}`;
      const about = n.host
        ? t(`Your node is asked first; the ${sol ? "public node takes" : "built-in nodes take"} over when yours doesn't answer, refuses its key or lacks a call.`,
          `Сначала спрашивается твоя нода; ${sol ? "публичная подхватывает" : "встроенные подхватывают"}, если твоя не отвечает, отказывает по ключу или не умеет нужный вызов.`)
        : t(`Free public nodes are shared by everyone: they limit how often anyone may ask, so at a busy moment a balance or a move can fail${sol ? ", and Solana's has no free backup" : ""}. A node of your own (free at ${n.provider}) takes that away.`,
          `Бесплатные публичные ноды общие для всех: они ограничивают, как часто можно спрашивать, поэтому в час пик баланс или перевод могут не пройти${sol ? ", а у Solana нет бесплатной запасной" : ""}. Своя нода (бесплатно у ${n.provider}) это убирает.`);
      const save = () => saveSecret("node", { network: n.id, address: secretForm.text });
      return [row({ label, mark: logo(n.logo, 26), about },
        n.host ? h("button", { class: "btn small", onclick: () => saveSecret("node", { network: n.id, address: "" }) }, t("Remove", "Убрать")) : null,
        h("button", { class: "btn small", onclick: () => openSecret("node-" + n.id) }, n.host ? t("Change…", "Изменить…") : t("Add your own…", "Добавить свою…"))),
        secretForm.open === "node-" + n.id ? secretPanel("node-" + n.id,
          [t(`A node only reads the chain and passes on transactions already signed on this PC: it never sees your keys and can't move money. It does see your addresses. Its address carries your key, so it's kept encrypted for your Windows user and shown only by its host.${n.id === "ethereum" ? " Ethereum's transactions still go out through MEV Blocker, out of the public mempool." : ""}`,
            `Нода только читает сеть и передаёт транзакции, уже подписанные на этом компьютере: ключей она не видит и двигать деньги не может. Твои адреса она видит. В её адресе лежит твой ключ, поэтому он хранится зашифрованным для твоего пользователя Windows и показывается только хостом.${n.id === "ethereum" ? " Транзакции Ethereum по-прежнему уходят через MEV Blocker, мимо публичного мемпула." : ""}`)],
          sol ? [t("Open helius.dev and sign up — the free plan, no card.", "Открой helius.dev и зарегистрируйся — бесплатный план, без карты."),
            t(`In the dashboard, open RPCs (Endpoints) and copy the Mainnet URL: ${n.example}`, `В панели открой RPCs (Endpoints) и скопируй адрес Mainnet: ${n.example}`),
            t("Paste it below and press Check and save.", "Вставь его ниже и нажми «Проверить и сохранить».")]
            : [t("Open alchemy.com and sign up — the free plan, no card.", "Открой alchemy.com и зарегистрируйся — бесплатный план, без карты."),
              t(`Create an app, pick ${n.title} Mainnet, and copy its HTTPS URL: ${n.example}`, `Создай приложение, выбери ${n.title} Mainnet и скопируй его адрес HTTPS: ${n.example}`),
              t("Paste it below and press Check and save.", "Вставь его ниже и нажми «Проверить и сохранить».")],
          [n.site, t(`Open ${n.site.replace("https://www.", "")}`, `Открыть ${n.site.replace("https://www.", "")}`)], n.example, save, false) : null];
    })));
}

function oneClickSection(v) {
  const save = () => saveSecret("oneClickKey", { key: secretForm.text });
  return section("key", t("NEAR Intents key", "Ключ NEAR Intents"), t("Lower fees on NEAR and cross-chain swaps through 1Click.", "Меньше комиссия на NEAR и обменах между сетями через 1Click."),
    group(row({ label: v.oneClickKey ? t("NEAR Intents: your partner key", "NEAR Intents: твой партнёрский ключ") : t("NEAR Intents: no key — 0.25% more per swap", "NEAR Intents: ключа нет — +0,25% к каждому обмену"), mark: logo("near", 26),
      about: t("1Click, NEAR Intents' swap service, takes an extra 0.25% from every quote asked without a key. A free partner key at partners.near-intents.org switches it to the partner fees: 0.20%, and 0.01% on stablecoins and the same coin between networks.",
        "1Click, сервис обменов NEAR Intents, берёт лишние 0,25% с каждой котировки без ключа. Бесплатный партнёрский ключ на partners.near-intents.org переключает его на партнёрские комиссии: 0,20%, а на стейблкоинах и той же монете между сетями — 0,01%.") },
      v.oneClickKey ? h("button", { class: "btn small", onclick: () => saveSecret("oneClickKey", { key: "" }) }, t("Remove", "Убрать")) : null,
      h("button", { class: "btn small", onclick: () => openSecret("1click") }, v.oneClickKey ? t("Change…", "Изменить…") : t("Add your key…", "Добавить ключ…"))),
      secretForm.open === "1click" ? secretPanel("1click",
        [t("Buying and selling NEAR, and the moves between networks that go through NEAR Intents, are quoted by its 1Click service. Without a key it adds 0.25% to every quote. With a free partner key it charges its partner fees instead: 0.20%, and 0.01% on stablecoins and the same coin between networks.",
          "Покупку и продажу NEAR и переводы между сетями через NEAR Intents оценивает его сервис 1Click. Без ключа он добавляет 0,25% к каждой котировке. С бесплатным партнёрским ключом берёт партнёрские комиссии: 0,20%, а на стейблкоинах и той же монете между сетями — 0,01%."),
         t("The key only identifies you to 1Click: it can't move money or sign anything. It's kept encrypted for your Windows user.", "Ключ только представляет тебя 1Click: двигать деньги или что-то подписывать он не может. Он хранится зашифрованным для твоего пользователя Windows.")],
        [t("Open partners.near-intents.org and sign in with your email — free.", "Открой partners.near-intents.org и войди по почте — бесплатно."),
         t("In the Partner Dashboard, create an API key and copy the token (a long JWT: eyJ…).", "В Partner Dashboard создай API-ключ и скопируй токен (длинный JWT: eyJ…)."),
         t("Paste it below and press Check and save.", "Вставь его ниже и нажми «Проверить и сохранить».")],
        ["https://partners.near-intents.org", t("Open partners.near-intents.org", "Открыть partners.near-intents.org")], "eyJ…", save, true) : null));
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

// AI model: the chat's account, the second check, safety, what the chat remembers.

/** The account whose sign-in shows below the list (nil: the chat's), and what's typed for it; kept across redraws.
 * Picking a row never switches the chat: «Switch» does. */
const modelForm = { shown: null, key: "", model: null, modelFor: null };
const ways = [["claude-code", () => t("Claude subscription", "Подписка Claude"), () => t("Uses Claude Code on this PC", "Через Claude Code на этом компьютере")],
  ["chatgpt", () => t("ChatGPT subscription", "Подписка ChatGPT"), () => t("Plus or Pro · sign in in the browser", "Plus или Pro · вход через браузер")],
  ["openrouter", () => "OpenRouter", () => t("One key for many models", "Один ключ на много моделей")]];

function modelTab(s) {
  const providers = s.providers;
  const shown = providers.find((p) => p.id === (modelForm.shown || s.model.provider)) || providers.find((p) => !p.unavailable) || providers[0];
  if (!shown) return [];
  if (modelForm.modelFor !== shown.id) {
    modelForm.modelFor = shown.id;
    modelForm.key = "";
    modelForm.model = shown.id === s.model.provider ? s.model.model || "" : "";
  }
  const state = (p) => p.connected ? (p.id === s.model.provider ? chatsModel() : pill(t("Connected", "Подключено"), true)) : null;
  const pick = (id) => { modelForm.shown = id; render(); };
  const wayRow = (p, title, line) => h("button", { class: "pg-set-way" + (p.id === shown.id ? " on" : ""), onclick: () => pick(p.id) },
    companyLogo(p.id, 28, p.name), h("span", { class: "pg-set-way-text" }, h("b", {}, title), h("span", { class: "secondary" }, line)), h("span", { class: "spacer" }), state(p));
  const known = ways.map(([id, title, line]) => [providers.find((p) => p.id === id), title, line]).filter(([p]) => p && !p.unavailable);
  const others = providers.filter((p) => !p.unavailable && !ways.some(([id]) => id === p.id));
  const otherShown = others.includes(shown);
  const other = h("div", { class: "pg-set-way" + (otherShown ? " on" : "") },
    h("span", { class: "pg-set-icon pg-key-tile" }, icon("lock", 15)),
    h("span", { class: "pg-set-way-text" }, h("b", {}, t("API key or a model on this PC", "API-ключ или модель на этом компьютере")),
      h("span", { class: "secondary" }, otherShown ? shown.name : t("Choose a provider or a local model", "Выбери провайдера или локальную модель"))),
    h("span", { class: "spacer" }), otherShown ? state(shown) : null,
    h("select", { class: "pg-set-choose", onchange: (e) => e.target.value && pick(e.target.value) },
      h("option", { value: "" }, t("Choose…", "Выбрать…")),
      [["subscription", t("Subscription", "Подписка")], ["api", t("API key or credits", "API-ключ или кредиты")], ["local", t("On this PC", "На этом компьютере")]].map(([kind, name]) => {
        const list = others.filter((p) => (p.kind === kind) || (kind === "api" && !["subscription", "local"].includes(p.kind)));
        return list.length ? h("optgroup", { label: name }, list.map((p) => h("option", { value: p.id, selected: otherShown && p.id === shown.id }, p.name))) : null;
      })));
  const isChat = shown.connected && shown.id === s.model.provider;
  const connect = async (extra = {}) => {
    const r = await act("connectModel", { provider: shown.id, key: modelForm.key || undefined, model: modelForm.model || undefined, ...extra });
    modelForm.key = "";
    if (r !== null) toast(t("Connected.", "Подключено."));
    render();
  };
  const jevReady = providers.some((p) => p.id === "openrouter" && p.connected);
  return [
    section("sparkles", t("Chat model", "Модель чата"), t("Lets you write freely, in any language. Connect one or more; the chat uses one at a time.", "Позволяет писать свободно, на любом языке. Подключи одну или несколько; чат пользуется одной."),
      group(known.map(([p, title, line]) => wayRow(p, title(), line())), others.length ? other : null),
      group(
        row({ label: shown.name, ico: "info.circle", about: shown.note || null },
          shown.unavailable ? h("span", { class: "accent-ink" }, shown.unavailable)
            : isChat ? chatsModel()
            : [pill(shown.connected ? t("Connected", "Подключено") : t("Not connected", "Не подключено"), shown.connected),
              shown.connected ? h("button", { class: "btn small", title: t(`The chat goes on with ${shown.name}`, `Чат продолжит с ${shown.name}`), onclick: () => connect() }, t("Switch", "Переключить")) : null]),
        row({ label: t("Sign-in", "Вход"), ico: "person", about: shown.connected ? t("Connected: the key is kept with Windows' data protection (DPAPI).", "Подключено: ключ хранится под защитой данных Windows (DPAPI).") : t("Not connected.", "Не подключено.") },
          shown.existingLogin ? h("button", { class: "btn small", onclick: () => connect() }, t(`Use ${shown.existingLogin}`, `Взять вход ${shown.existingLogin}`)) : null,
          shown.signIn ? h("button", { class: "btn small", onclick: async () => {
            toast(t("Finish the sign-in in the browser.", "Закончи вход в браузере."));
            if (await act("signIn", { provider: shown.id }) !== null) toast(t("Signed in.", "Вход выполнен."));
          } }, t("Sign in with the browser", "Войти через браузер")) : null,
          !shown.existingLogin && !shown.signIn ? h("span", { class: "secondary" }, shown.kind === "local" ? t("No key needed", "Ключ не нужен") : t("Use an API key below", "Используй API-ключ ниже")) : null),
        shown.takesKey ? row({ label: t("API key", "API-ключ"), ico: "lock" },
          h("input", { id: "model-key", type: "password", autocomplete: "off", class: "pg-set-input", placeholder: t("API key", "API-ключ"), value: modelForm.key, oninput: (e) => (modelForm.key = e.target.value) }),
          h("button", { class: "btn small", onclick: () => modelForm.key ? connect() : toast(t("Paste the key first.", "Сначала вставь ключ.")) }, t("Save", "Сохранить"))) : null,
        row({ label: t("Model", "Модель"), ico: "bolt", about: t("Empty: the provider's default.", "Пусто — модель провайдера по умолчанию.") },
          h("input", { id: "model-id", class: "pg-set-input", list: "model-list", placeholder: t("Default", "По умолчанию"), value: modelForm.model, oninput: (e) => (modelForm.model = e.target.value.trim()) }),
          isChat && s.model.models.length ? h("datalist", { id: "model-list" }, s.model.models.map((m) => h("option", { value: m }))) : null,
          isChat ? h("button", { class: "btn small", onclick: () => act("setModel", { model: modelForm.model || undefined }) }, t("Use", "Выбрать"))
            : !shown.connected && (shown.kind === "local") ? h("button", { class: "btn small", onclick: () => connect() }, t("Connect", "Подключить")) : null),
        shown.connected ? row({ label: t("Connection", "Подключение"), ico: "link" },
          h("button", { class: "btn small", onclick: () => act("disconnectModel", { provider: shown.id }) }, t("Disconnect", "Отключить"))) : null)),
    section("checkmark.seal", t("Second check", "Вторая проверка"), t("A second reader. It can only stop, never approve.", "Второй читатель. Он может только остановить, но не одобрить."),
      group(
        row({ label: t("Check commands and plans twice", "Проверять команды и планы дважды"), ico: "checkmark.seal",
          about: t("The model reads every command and every trade plan again before a card is drawn. On a doubt nothing runs: you pick.", "Модель перечитывает каждую команду и каждый план сделки, прежде чем нарисовать карточку. При сомнении ничего не запускается — выбираешь ты.") },
          toggle("second-check", s.secondCheck, (on) => act("secondCheck", { test: on }))),
        row({ label: "OpenRouter", mark: companyLogo("openrouter", 26), about: t("Also asks TypeSafe Jev through OpenRouter with your OpenRouter key: the message goes there.", "Также спрашивает TypeSafe Jev через OpenRouter твоим ключом OpenRouter: сообщение уходит туда.") },
          jevReady ? toggle("jev-openrouter", s.jevOpenRouter, (on) => act("jevOpenRouter", { test: on }), !s.secondCheck)
            : [pill(t("Not connected", "Не подключено"), false), h("button", { class: "btn small", onclick: () => pick("openrouter") }, t("Connect", "Подключить"))]))),
    section("lock", t("Safety", "Безопасность"), t("A model reads your words. It cannot approve a move.", "Модель читает твои слова. Одобрить движение она не может."),
      group(
        row({ label: t(`Money moves only after Confirm + ${presence()}`, `Деньги двигаются только после «Подтвердить» и ${presence()}`), ico: "person",
          about: t(`You approve money moves in the app. One ${presence()} can cover one move or a few minutes, as set in Wallet.`, `Движения денег одобряешь ты в приложении. Одно подтверждение ${presence()} действует на одно движение или несколько минут — как задано во вкладке «Кошелёк».`) }),
        row({ label: t("Keys stay on this PC", "Ключи остаются на этом компьютере"), ico: "eye.slash",
          about: t("The chat model receives your messages, your balances, open positions and what it looks up. It never receives keys or the recovery phrase.", "Модель чата получает твои сообщения, балансы, открытые позиции и то, что она ищет. Ключи и фразу восстановления она не получает никогда.") }))),
    s.memory.length ? section("doc", t("What the chat remembers", "Что помнит чат"), t("Facts it keeps across canvases. Forget any of them.", "Факты, которые он помнит между холстами. Любой можно забыть."),
      group(s.memory.map((m) => row({ label: m, ico: "doc", cls: "wrap" }, h("button", { class: "btn small", onclick: () => act("forget", { text: m }) }, t("Forget", "Забыть")))))) : null,
  ];
}

const chatsModel = () => h("span", { class: "pg-set-chat-model profit", title: t("The chat answers with this account", "Чат отвечает этим аккаунтом") }, icon("check", 13), t("Chat model", "Модель чата"));

// Wallet: the wallet that signs, the wallets, adding one, Windows Hello, addresses watched.

function kindName(w) {
  const base = w.kind === "ledger" ? "Ledger" : w.kind === "solana" ? t("Private key · Solana only", "Приватный ключ · только Solana")
    : w.kind === "evm" ? t("Private key · EVM only", "Приватный ключ · только EVM") : t("Recovery phrase · every network", "Фраза восстановления · все сети");
  return base + (w.test ? t(" · test, signs without Windows Hello", " · тестовый, подписывает без Windows Hello") : "")
    + (w.automation ? t(" · automation: scheduled tasks sign without Windows Hello, only to your own wallets", " · автоматизация: задачи подписывают без Windows Hello, только на твои кошельки") : "");
}

/** The wallet being renamed and what's typed for it, kept across the states that arrive meanwhile. */
let renaming = null, renameDraft = "";
let removing = null;
let creating = { open: false, test: false };

function walletTab(s) {
  const w = s.wallets.find((x) => x.id === s.active);
  const sessions = [[60, t("One move", "Один перевод")], [300, t("5 min", "5 мин")], [900, t("15 min", "15 мин")], [1800, t("30 min", "30 мин")]];
  const way = (ico, title, line, run, open) => h("button", { class: "pg-set-way", onclick: run }, tile(ico),
    h("span", { class: "pg-set-way-text" }, h("b", {}, title), h("span", { class: "secondary" }, line)), h("span", { class: "spacer" }),
    icon(open ? "chevron.down" : "chevron.right", 12));
  const watched = s.wallets.length ? null : watchFields();
  return [
    section("creditcard", t("Wallet that signs", "Кошелёк, который подписывает"),
      w ? t("The active wallet. Switch in «Your wallets» below.", "Активный кошелёк. Переключить — в «Твоих кошельках» ниже.") : t("No key on this PC yet: add a wallet below.", "На этом компьютере пока нет ключа: добавь кошелёк ниже."),
      group(
        w ? h("div", { class: "pg-set-signer" }, letterTile(w.label, 44, true), h("div", {}, h("b", {}, w.label), h("div", { class: "secondary" }, kindName(w)))) : null,
        w ? [["Solana", "sol", w.solana], ["EVM", "eth", w.evm], ["NEAR", "near", w.near]].filter((x) => x[2]).map(([name, l, a]) =>
          row({ label: name, mark: logo(l, 26) }, copyShort(a, name))) : null,
        row({ label: t("Signing key", "Ключ подписи"), ico: "lock", about: t("A separate process without network access keeps the key, sealed by Windows Hello and this PC's TPM, and signs approved moves.", "Ключ хранит отдельный процесс без доступа к сети, запечатанный Windows Hello и TPM этого компьютера; он подписывает одобренные движения.") },
          pill(w ? t("Windows Hello + TPM", "Windows Hello + TPM") : t("No key on this PC", "Ключа нет"), !!w && s.signer.ready)),
        w?.kind === "phrase" ? row({ label: t("Recovery phrase", "Фраза восстановления"), ico: "doc", about: t("ihatecrypto keeps only the key made from your words, sealed in a separate process — never the words themselves, so they can't be shown again. Lost them? Create a new wallet below, write its words down and move the money there.", "ihatecrypto хранит только ключ, сделанный из твоих слов, запечатанный в отдельном процессе, — но не сами слова, поэтому показать их снова нельзя. Потерял? Создай новый кошелёк ниже, запиши его слова и переведи деньги туда.") },
          h("span", { class: "secondary" }, t("Not kept on this PC", "На этом компьютере не хранится"))) : null)),
    s.wallets.length ? section("wallet.pass", t("Your wallets", "Твои кошельки"), t(`One signs at a time. Switching or removing one asks for ${presence()}.`, `Подписывает один за раз. Переключение или удаление спрашивает ${presence()}.`),
      group(s.wallets.map((x) => walletRow(x, s)))) : null,
    section("plus", t("Add a wallet", "Добавить кошелёк"), t("Make a new one here, or bring a phrase or a key from another app.", "Создай новый здесь или принеси фразу или ключ из другого приложения."),
      group(
        way("bolt", t("Create a new wallet", "Создать новый кошелёк"), t("12 new words, made inside the signer", "12 новых слов, созданных внутри подписанта"), () => { creating.open = !creating.open; render(); }, creating.open),
        creating.open ? h("div", { class: "pg-set-panel" },
          h("div", { class: "secondary small" }, t(`ihatecrypto makes a new 12-word recovery phrase inside the signer and keeps the key sealed by Windows Hello and the TPM. Every signature asks for ${presence()}.`,
            `ihatecrypto создаёт новую фразу из 12 слов внутри подписанта и хранит ключ, запечатанный Windows Hello и TPM. Каждая подпись спрашивает ${presence()}.`) + (s.wallets.length ? t(" It is added next to your other wallets.", " Он добавится рядом с другими кошельками.") : "")),
          h("div", { class: "pg-set-option" }, h("span", {}, t(`Test wallet: signs without ${presence()}`, `Тестовый кошелёк: подписывает без ${presence()}`)), h("span", { class: "spacer" }),
            toggle("new-test", creating.test, (on) => { creating.test = on; render(); })),
          creating.test ? h("div", { class: "small pg-jev-ink" }, t("For trying things out: keep only a few dollars on it.", "Для проб: держи на нём только несколько долларов.")) : null,
          h("div", {}, h("button", { class: "btn primary", onclick: async () => { if (await act("createWallet", { test: creating.test }) !== null) creating = { open: false, test: false }; } },
            creating.test ? t("Create a test wallet", "Создать тестовый") : t("Create a new wallet", "Создать новый кошелёк")))) : null,
        way("doc", t("Import a recovery phrase", "Импортировать фразу восстановления"), t("12 or 24 words from MetaMask, Phantom or another wallet", "12 или 24 слова из MetaMask, Phantom или другого кошелька"), showImport),
        way("lock", t("Import a private key", "Импортировать приватный ключ"), t("One key from Phantom (Solana) or MetaMask (EVM)", "Один ключ из Phantom (Solana) или MetaMask (EVM)"), showImportKey),
        s.automation ? null : way("calendar", t("Automation wallet", "Кошелёк автоматизации"), t("A small budget scheduled tasks spend without Windows Hello", "Небольшой бюджет, который задачи тратят без Windows Hello"), () => act("createAutomationWallet")),
        way("link", t("Connect a Ledger", "Подключить Ledger"), t("USB or Bluetooth: every move shows on its screen", "USB или Bluetooth: каждое движение — на его экране"), () => { ui.view = "canvas"; act("addBoard", { name: "ledger" }); }))),
    section("person", presence(), t("One Windows Hello covers one move, or a few minutes.", "Одно подтверждение Windows Hello действует на одно движение или несколько минут."),
      group(
        row({ label: t("Stays valid for", "Действует"), ico: "clock", about: t("One move covers that move's signatures for at most a minute. With 5–30 minutes, signatures need no new Windows Hello during that time; each still passes the guard and the second check. Locking the PC ends it.", "Один перевод покрывает подписи этого перевода не дольше минуты. С 5–30 минутами новые подписи в это время не спрашивают Windows Hello; каждая всё равно проходит проверки. Блокировка компьютера заканчивает сессию.") },
          segmented(sessions, s.signer.sessionLength, (v) => act("sessionLength", { seconds: v }), "pg-set-seg")),
        s.trading ? row({ label: t("One transaction for approve and swap", "Одна транзакция для approve и обмена"), ico: "square.stack",
          about: t("On Base, Arbitrum, Ethereum, Polygon and Robinhood, an approve and the swap, deposit or bridge that spends it go out as one transaction under one Windows Hello (EIP-7702). For that, from the first such move on a network your wallet there runs the code of an audited account contract (eth-infinitism's Simple7702Account): it takes coins and tokens as before, only you can make it act, and every call in it is checked like a single one. Off: two transactions, two confirmations, as before; a network already switched stays so.",
            "В Base, Arbitrum, Ethereum, Polygon и Robinhood approve и обмен, депозит или мост, который его тратит, уходят одной транзакцией под одним Windows Hello (EIP-7702). Для этого с первого такого движения в сети твой кошелёк там исполняет код проверенного аудитом контракта-аккаунта (Simple7702Account от eth-infinitism): он принимает монеты и токены как раньше, заставить его действовать можешь только ты, и каждый вызов в нём проверяется как отдельный. Выключено — две транзакции и два подтверждения, как раньше; сеть, где уже включено, такой и остаётся.") },
          toggle("pg-batches", s.trading.batches, (on) => act("batches", { test: on }))) : null,
        s.trading ? row({ label: t("Cap for unknown protocols", "Лимит для незнакомых протоколов"), ico: "shield",
          about: t("A contract the chat calls outside ihatecrypto's own list (Aave, Morpho, Uniswap, Relay…) may put at most this much at risk in one call — what leaves the wallet, its approve and the coins sent with it. It must also have verified code, be a month old, hold $1M or more by DefiLlama and be on no scam list. Every check can be fooled; this caps what that costs.",
            "Контракт, который чат вызывает вне собственного списка ihatecrypto (Aave, Morpho, Uniswap, Relay…), может рисковать за один вызов не больше этой суммы — что уходит из кошелька, его approve и монеты, отправленные с ним. Ещё у него должен быть проверенный код, возраст от месяца, от $1M по DefiLlama и ни одного списка мошенников. Любую проверку можно обмануть — лимит ограничивает, во что это обойдётся.") },
          h("select", { id: "pg-contract-cap", onchange: (e) => act("contractCap", { value: Number(e.target.value) }) },
            [10, 25, 50, 100, 250, 500, 1000].map((v) => h("option", { value: v, selected: s.trading.contractCap === v }, "$" + v.toLocaleString("en-US"))))) : null,
        row({ label: t("Ask again now", "Спрашивать снова сейчас"), ico: "lock", about: t("Ends the session now and pauses the Hyperliquid trading key until the next Windows Hello.", "Заканчивает сессию сейчас и ставит торговый ключ Hyperliquid на паузу до следующего Windows Hello.") },
          h("button", { class: "btn small", onclick: () => act("lock") }, t("Lock", "Заблокировать"))),
        s.signer.notes.map((n) => row({ label: n, ico: "checkmark.seal", cls: "wrap note" })),
        s.signer.error ? row({ label: s.signer.error, ico: "exclamationmark.triangle", cls: "wrap loss" }) : null)),
    s.lock ? passcodeSection(s.lock) : null,
    s.trading?.key?.possible ? hyperliquidSection(s) : null,
    watched ? section("eye", t("Addresses for balances", "Адреса для балансов"), t("Read-only wallets. Never paste a recovery phrase or key.", "Кошельки только для просмотра. Никогда не вставляй фразу восстановления или ключ."),
      group(h("div", { class: "pg-set-panel" }, watched.fields,
        h("div", { class: "btns" }, h("button", { class: "btn primary", onclick: watched.save }, t("Watch", "Смотреть")),
          watched.stop ? h("button", { class: "btn", onclick: watched.stop }, t("Stop watching", "Перестать смотреть")) : null)))) : null,
  ];
}

/** Settings › Wallet › Passcode (the Mac's PasscodeSettings): make, change or turn off the code; how soon it locks; lock now. */
const passcode = { making: false, offered: false };
function passcodeSection(l) {
  if (l.offerNewCode && !passcode.offered) Object.assign(passcode, { making: true, offered: true });
  const made = () => { passcode.making = false; render(); };
  return section("lock", t("Passcode", "Код"), t("Opens ihatecrypto: balances, chat and history stay hidden until then.", "Открывает ihatecrypto: балансы, чат и история скрыты до этого."),
    group(
      row({ label: t("Passcode", "Код"), ico: "lock", about: t("6 digits, checked by the separate signer: five wrong in a row turn it off, and then only Windows Hello opens the app. Windows Hello opens it too; moves still sign with Windows Hello.",
        "6 цифр, их проверяет отдельный подписант: после пяти неверных подряд код выключается, и тогда приложение открывает только Windows Hello. Windows Hello открывает и так; движения по-прежнему подписываются Windows Hello.") },
        l.codeSet ? h("span", { class: "secondary" }, t("On", "Включён")) : null,
        h("button", { class: "btn small", onclick: () => { passcode.making = !passcode.making; render(); } },
          passcode.making ? t("Cancel", "Отмена") : l.codeSet ? t("Change…", "Изменить…") : t("Make a code…", "Задать код…")),
        l.codeSet ? h("button", { class: "btn small", onclick: () => act("clearCode") }, t("Turn off", "Выключить")) : null),
      passcode.making ? codeMaker(l, made) : null,
      l.codeSet ? row({ label: t("Lock after", "Блокировать через"), ico: "clock", about: t("Minutes without touching the PC's keyboard or mouse. Sleep and a locked screen lock it at once.",
        "Минуты, когда никто не трогает клавиатуру и мышь компьютера. Сон и блокировка экрана закрывают сразу.") }, lockAfterPicker(l)) : null,
      l.codeSet ? row({ label: t("Lock the app now", "Заблокировать сейчас"), ico: "lock", about: t("Hides everything until the code or Windows Hello, and ends the signing session. Ctrl+L does the same.",
        "Скрывает всё до кода или Windows Hello и заканчивает сессию подписи. То же делает Ctrl+L.") },
        h("button", { class: "btn small", onclick: () => act("lockApp") }, t("Lock now", "Заблокировать"))) : null));
}

function walletRow(w, s) {
  const active = w.id === s.active;
  const save = async () => {
    const name = renameDraft.trim();
    renaming = null;
    if (name && name !== w.label) await act("renameWallet", { wallet: w.id, label: name });
    render();
  };
  return h("div", { class: "pg-set-row" }, h("div", { class: "pg-set-row-line" },
    letterTile(w.label, 26, active),
    h("div", { class: "pg-set-wallet" },
      renaming === w.id ? h("input", { id: "rename-wallet", class: "pg-set-input", value: renameDraft, oninput: (e) => (renameDraft = e.target.value), onkeydown: (e) => { if (e.key === "Enter") save(); if (e.key === "Escape") { renaming = null; render(); } } })
        : h("b", {}, w.label),
      h("span", { class: "secondary small one-line selectable" }, [w.solana && "Solana " + short(w.solana), w.evm && "EVM " + short(w.evm)].filter(Boolean).join(" · ") || kindName(w))),
    h("span", { class: "spacer" }),
    h("span", { class: "pg-set-controls" },
      active ? pill(t("Signs now", "Подписывает"), true) : h("button", { class: "btn small", onclick: () => act("switchWallet", { wallet: w.id }) }, t("Use", "Выбрать")),
      renaming === w.id ? h("button", { class: "btn small", onclick: save }, t("Save", "Сохранить"))
        : h("button", { class: "icon-btn", title: t("Rename", "Переименовать"), onclick: () => { renaming = w.id; renameDraft = w.label; render(); setTimeout(() => document.getElementById("rename-wallet")?.select(), 0); } }, icon("pencil", 14)),
      removing === w.id
        ? h("button", { class: "btn small pg-danger", title: t("Only if its recovery phrase or key is written down: the money itself stays where it is.", "Только если фраза или ключ записаны: сами деньги остаются на месте."),
          onclick: async () => { removing = null; await act("removeWallet", { wallet: w.id }); } }, t("Remove?", "Удалить?"))
        : h("button", { class: "icon-btn", title: t("Remove this wallet", "Удалить этот кошелёк"), onclick: () => { removing = w.id; render(); setTimeout(() => { if (removing === w.id) { removing = null; render(); } }, 4000); } }, icon("trash", 14)))));
}

/** Settings › Wallet › Hyperliquid: the trading key (made and approved once, paused by «lock») and ihatecrypto's fee. */
let feeAskedFor = null;
function hyperliquidSection(s) {
  const k = s.trading.key;
  const owner = s.addresses.evm;
  if (k.feeApproved == null && owner && feeAskedFor !== owner) { feeAskedFor = owner; act("readFee"); }
  const confirm = k.ledger ? t("the Ledger's buttons", "кнопок Ledger") : "Windows Hello";
  return section("bolt", "Hyperliquid", t("A trading key places orders. It cannot withdraw money.", "Торговый ключ ставит ордера. Вывести деньги он не может."),
    group(
      row({ label: t("Trading key", "Торговый ключ"), mark: logo("hype", 26),
        about: k.active ? t("Opens positions without Windows Hello for 7 days after your last confirmation for it; stops and closes go on after that. It cannot withdraw money.",
          "Открывает позиции без Windows Hello 7 дней после последнего подтверждения для него; стопы и закрытия работают и после. Вывести деньги он не может.")
          : t(`Without a trading key, every order asks for ${k.ledger ? "the Ledger's buttons" : "Windows Hello"}. Hyperliquid accepts the key after the account has had a deposit.`,
            `Без торгового ключа каждый ордер спрашивает ${k.ledger ? "кнопки Ledger" : "Windows Hello"}. Hyperliquid принимает ключ, когда на аккаунт уже был депозит.`) },
        pill(k.active ? t("On", "Включён") : t("Off", "Выключен"), k.active),
        !k.active && !k.paused ? h("button", { class: "btn small", onclick: () => act("tradingKey") }, k.ledger ? t("Approve on Ledger…", "Одобрить на Ledger…") : t("Approve…", "Одобрить…")) : null),
      k.paused ? row({ label: t("Paused by «lock»", "На паузе после «Заблокировать»"), ico: "lock",
        about: t(`Orders ask for ${k.ledger ? "the Ledger's buttons" : "Windows Hello"} until it resumes. Resuming asks for Windows Hello once.`, `Ордера спрашивают ${confirm}, пока ключ не возобновлён. Возобновление один раз спросит Windows Hello.`) },
        h("button", { class: "btn small", onclick: () => act("tradingKey") }, t("Resume…", "Возобновить…"))) : null,
      row({ label: t("ihatecrypto's fee", "Комиссия ihatecrypto"), ico: "percent",
        about: t(`${k.feeRate} of each Hyperliquid trade placed in ihatecrypto, on top of the exchange's own fee: perps both ways, spot only when selling. Hyperliquid takes it, and only once you approve it: the first trade you confirm asks. «Revoke» takes the approval back, and the next trade asks again. The referral code ${k.referral} gives the account 4% off Hyperliquid's fees.`,
          `${k.feeRate} с каждой сделки на Hyperliquid, сделанной в ihatecrypto, сверх комиссии самой биржи: перпы в обе стороны, спот только при продаже. Её берёт Hyperliquid, и только после твоего одобрения: спросит первая сделка, которую ты подтвердишь. «Отозвать» забирает одобрение, и следующая сделка спросит снова. Реферальный код ${k.referral} даёт аккаунту скидку 4% на комиссии Hyperliquid.`) },
        pill(k.feeApproved === true ? `${k.feeRate} · ${t("Approved", "Одобрена")}` : k.feeApproved === false ? `${k.feeRate} · ${t("Not yet", "Ещё нет")}` : k.feeRate, k.feeApproved === true),
        k.feeApproved === true ? h("button", { class: "btn small", onclick: () => act("revokeFee") }, t("Revoke…", "Отозвать…")) : null)));
}

// Trading agent: a style that sets every rule at once, the limits, «by the book», Safe search (SettingsView.agent).

const styleWords = {
  careful: ["gauge.0", () => t("Careful", "Осторожный"), () => t("Small bets, little leverage, wide stops.", "Маленькие ставки, мало плеча, широкие стопы.")],
  balanced: ["gauge.50", () => t("Balanced", "Сбалансированный"), () => t("The book: 1% risk, a 2R take.", "По учебнику: риск 1%, тейк 2R.")],
  bold: ["gauge.100", () => t("Bold", "Смелый"), () => t("Bigger bets and more leverage.", "Ставки крупнее и больше плеча.")],
};
const ruleKeys = ["maxRiskPct", "defaultRiskPct", "minRewardRisk", "defaultRewardRisk", "maxLeverage", "atrStopMultiple", "maxOpenRiskPct"];
const samePolicy = (a, b) => ruleKeys.every((k) => Math.abs(a[k] - b[k]) < 1e-9);
const n2 = (v) => String(Number(Number(v).toFixed(2)));
const pctText = (v) => n2(v) + "%";

/** A rule changed by a step: the ones that depend on each other kept in line after it (the Mac's update). */
function changeRule(change) {
  const p = { ...ui.state.trading.policy };
  change(p);
  for (const k of ruleKeys) p[k] = Math.round(p[k] * 100) / 100;
  p.defaultRiskPct = Math.min(p.defaultRiskPct, p.maxRiskPct);
  p.maxOpenRiskPct = Math.max(p.maxOpenRiskPct, p.maxRiskPct);
  p.defaultRewardRisk = Math.max(p.defaultRewardRisk, p.minRewardRisk);
  act("policy", { policy: p });
}

/** − value +. */
function stepper(value, what, down, up, change) {
  const step = (sym, help, enabled, d) => h("button", { class: "pg-agent-step", title: help, "aria-label": help, disabled: !enabled, onclick: () => changeRule((p) => change(p, d)) }, icon(sym, 11));
  return h("span", { class: "pg-agent-stepper" }, step("minus", t("Less ", "Меньше: ") + what, down, -1), h("b", { class: "num" }, value), step("plus", t("More ", "Больше: ") + what, up, 1));
}

function agentTab(s) {
  if (!s.trading) return [];
  const tr = s.trading, p = tr.policy;
  const current = tr.styles.find((x) => samePolicy(x.policy, p));
  const card = (x) => {
    const [ico, name, line] = styleWords[x.id];
    const on = x === current;
    return h("button", { class: "pg-agent-style" + (on ? " on" : ""), "aria-pressed": String(on), title: on ? t(`${name()} — in use`, `${name()} — сейчас`) : t(`Use ${name()}: sets all the rules below`, `Выбрать «${name()}»: задаёт все правила ниже`),
      onclick: () => act("policy", { policy: x.policy }) },
      h("div", { class: "pg-agent-style-top" }, tile(ico, 34), h("span", { class: "spacer" }), icon(on ? "check" : "circle", 15)),
      h("div", {}, h("b", {}, name()), h("div", { class: "secondary small" }, line())),
      h("div", { class: "pg-agent-figures" },
        [[t("Risk", "Риск"), pctText(x.policy.maxRiskPct)], [t("Leverage", "Плечо"), n2(x.policy.maxLeverage) + "×"], [t("Take", "Тейк"), n2(x.policy.defaultRewardRisk) + "R"]]
          .map(([l, v]) => h("span", {}, h("span", { class: "secondary" }, l), h("b", { class: "num" }, v)))));
  };
  return [
    section("dial", t("Style", "Стиль"), current ? t("Sets every rule below at once. Change any number after.", "Задаёт все правила ниже сразу. Любое число можно поменять потом.")
      : t("Custom: your own numbers below. A style resets them.", "Свои: твои числа ниже. Стиль их сбросит."),
      h("div", { class: "pg-agent-styles" }, tr.styles.map(card))),
    section("shield", t("Limits", "Лимиты"), t("A plan that breaks one is not executed. Crypto only.", "План, который нарушает хоть один, не исполняется. Только крипта."),
      group(
        row({ label: t("Risk per trade", "Риск на сделку"), ico: "chart.bar", about: t("The most one trade may lose at its stop, as a share of your capital.", "Сколько одна сделка может потерять на стопе — в доле от капитала.") },
          h("span", { class: "pg-agent-bar", "aria-hidden": "true" }, h("i", { style: `width:${Math.min(100, Math.max(0, p.maxRiskPct * 10))}%` })),
          stepper(pctText(p.maxRiskPct), t("risk per trade", "риск на сделку"), p.maxRiskPct > 0.5, p.maxRiskPct < 10, (q, d) => (q.maxRiskPct += d * 0.5))),
        row({ label: t("All open positions together", "Все открытые позиции вместе"), ico: "square.stack", about: t("Their combined loss at the stops can't go over this share of capital. It's never below the risk per trade.", "Их общий убыток на стопах не может быть больше этой доли капитала. Он никогда не меньше риска на сделку.") },
          stepper(pctText(p.maxOpenRiskPct), t("for all positions", "для всех позиций"), p.maxOpenRiskPct > p.maxRiskPct, p.maxOpenRiskPct < 30, (q, d) => (q.maxOpenRiskPct += d))),
        row({ label: t("Reward / risk at least", "Прибыль к риску не меньше"), ico: "arrow.up.right", about: t("The planned profit must be at least this many times the planned loss.", "Плановая прибыль должна быть хотя бы во столько раз больше планового убытка.") },
          stepper(n2(p.minRewardRisk), t("reward per risk", "прибыль к риску"), p.minRewardRisk > 1, p.minRewardRisk < 5, (q, d) => (q.minRewardRisk += d * 0.1))),
        row({ label: t("Leverage up to", "Плечо до"), ico: "multiply", about: t("Borrowed exposure makes gains and losses larger. The assistant never trades above this.", "Заёмные деньги увеличивают и прибыль, и убыток. Выше этого ассистент не торгует.") },
          stepper(n2(p.maxLeverage) + "×", t("leverage", "плечо"), p.maxLeverage > 1, p.maxLeverage < 20, (q, d) => (q.maxLeverage += d))))),
    section("book", t("By the book", "По правилам"), t("What «by the book» uses when you name no numbers.", "Что берут «по правилам», если ты не назвал чисел."),
      group(
        row({ label: t("Risk", "Риск"), ico: "chart.bar", about: t("The share of capital a trade risks at its stop when you don't say how much. Never above the risk per trade.", "Доля капитала, которой сделка рискует на стопе, если ты не назвал сумму. Никогда не больше риска на сделку.") },
          stepper(pctText(p.defaultRiskPct), t("default risk", "риск по умолчанию"), p.defaultRiskPct > 0.25, p.defaultRiskPct < p.maxRiskPct, (q, d) => (q.defaultRiskPct += d * 0.25))),
        row({ label: t("Take-profit", "Тейк-профит"), ico: "flag", about: t("How far the take is, as a multiple of the stop's distance: 2R takes twice what the stop would lose. Never below the reward / risk limit.", "Как далеко тейк — в кратных расстоянию до стопа: 2R забирает вдвое больше, чем потерял бы стоп. Никогда не меньше лимита прибыли к риску.") },
          stepper(n2(p.defaultRewardRisk) + "R", t("take-profit", "тейк-профит"), p.defaultRewardRisk > p.minRewardRisk, p.defaultRewardRisk < 10, (q, d) => (q.defaultRewardRisk += d * 0.5))),
        row({ label: t("Stop", "Стоп"), ico: "hand.raised", about: t("How far the stop is, in 4-hour ATRs — the coin's usual move over four hours. Wider stops are hit less often but lose more when they are.", "Как далеко стоп — в 4-часовых ATR, обычном движении монеты за четыре часа. Широкий стоп срабатывает реже, но теряет больше.") },
          stepper(n2(p.atrStopMultiple) + " ATR", t("stop distance", "расстояние до стопа"), p.atrStopMultiple > 0.5, p.atrStopMultiple < 5, (q, d) => (q.atrStopMultiple += d * 0.5))))),
    section("checkmark.seal", t("What it may buy", "Что можно покупать"), t("Tokens on Solana.", "Токены на Solana."),
      group(row({ label: t("Safe search", "Безопасный поиск"), ico: "checkmark.seal",
        about: tr.safeSearch ? t("Only verified coins on Solana: SOL, BTC, ETH, JUP and JitoSOL. Look-alikes of famous tokens cannot slip in.", "Только проверенные монеты на Solana: SOL, BTC, ETH, JUP и JitoSOL. Подделки известных токенов не проскочат.")
          : t("Any Solana token can be bought by its mint through Jupiter. Before you confirm, the assistant explains risks such as a creator who can freeze or print tokens, a thin pool, or no verification. Confirm and simulation still apply.",
            "Любой токен Solana можно купить по mint-адресу через Jupiter. Перед подтверждением ассистент объясняет риски: создатель может заморозить или напечатать токены, пул тонкий или проверки нет. «Подтвердить» и симуляция остаются.") },
        toggle("pg-safe-search", tr.safeSearch, (on) => act("safeSearch", { test: on }))))),
  ];
}

// Modules: the networks and each kind of move on the left, the switches of the one picked on the right (ModulesSettings).

const kindIcon = { swaps: "arrow.left.arrow.right", earn: "percent", perps: "chart.line", stocks: "chart.bar", bridges: "bridge", payments: "creditcard" };
const kindWords = {
  swaps: () => [t("Swaps", "Обмены"), t("Trade one token for another", "Один токен на другой")],
  earn: () => [t("Earn · DeFi", "Доход · DeFi"), t("Lend and stake idle money", "Давать в долг и стейкать свободные деньги")],
  perps: () => [t("Perps", "Перпы"), t("Long or short with leverage", "Лонг или шорт с плечом")],
  stocks: () => [t("Stocks & oil", "Акции и нефть"), t("Commodities and tokenized shares", "Сырьё и токенизированные акции")],
  bridges: () => [t("Bridges", "Мосты"), t("Move money between networks", "Перевод денег между сетями")],
  payments: () => [t("Payments", "Платежи"), t("Pay for services per request", "Оплата сервисов за запрос")],
};

function modulesTab(s) {
  const m = s.modules;
  if (!m) return [];
  let pick = store("modulesPick") || "networks";
  const kind = m.kinds.find((k) => k.key === pick);
  if (!kind) pick = "networks";
  const count = (k) => [k.on ? k.protocols.filter((p) => p.on).length : 0, k.protocols.length];
  const netsOn = m.networks.filter((n) => n.on).length;
  const words = (k) => (kindWords[k.key] ? kindWords[k.key]() : [k.name, k.line]);
  const side = (id, name, ico, on, of) => h("button", { class: "pg-mod-side-row" + (pick === id ? " on" : ""), "aria-pressed": String(pick === id), onclick: () => { store("modulesPick", id); render(); } },
    icon(ico, 16), h("span", { class: "one-line" }, name), h("span", { class: "spacer" }), h("span", { class: "pg-mod-count" + (on === of ? "" : " some") }, `${on}/${of}`));
  const header = (title, line, ico, on, of, master) => h("div", { class: "pg-mod-head" }, tile(ico, 44),
    h("div", { class: "pg-mod-head-text" }, h("b", {}, title), h("span", { class: "secondary" }, line)), h("span", { class: "spacer" }),
    h("span", { class: "secondary small" }, t(`${on} of ${of} on`, `включено ${on} из ${of}`)), master || null);
  const item = (logoName, name, sub, control, { dim, faded, warn } = {}) => h("div", { class: "pg-mod-row" + (faded ? " faded" : "") },
    h("span", { class: "pg-mod-logo" + (dim ? " dim" : "") }, logo(logoName, 34)),
    h("div", { class: "pg-mod-row-text" }, h("b", { class: dim ? "secondary" : "" }, name), h("span", { class: warn ? "accent-ink" : "secondary" }, sub)),
    h("span", { class: "spacer" }), control);
  let detail;
  if (kind) {
    const [on, of] = count(kind);
    const [name, line] = words(kind);
    detail = [header(name, line, kindIcon[kind.key] || "square.grid", on, of, toggle("mod-" + kind.id, kind.on, (v) => act("module", { name: kind.id, test: v }))),
      h("div", { class: "pg-mod-box" }, kind.protocols.map((p) => item(p.logo, p.name, p.line,
        toggle("mod-" + p.id, p.on, (v) => act("module", { name: p.id, test: v }), !kind.on), { faded: !p.allowed, warn: p.networksOff && p.on })))];
  } else {
    detail = [header(t("Networks", "Сети"), t("Where the assistant may move money", "Где ассистенту можно двигать деньги"), "globe", netsOn, m.networks.length),
      h("div", { class: "pg-mod-box" }, m.networks.map((n) => item(n.logo, n.name, n.users,
        toggle("mod-" + n.id, n.on, (v) => act("module", { name: n.id, test: v })), { dim: !n.on })))];
  }
  return [h("div", { class: "pg-mod" },
    h("nav", { class: "pg-mod-side" },
      side("networks", t("Networks", "Сети"), "globe", netsOn, m.networks.length),
      h("div", { class: "menu-divider" }),
      m.kinds.map((k) => { const [on, of] = count(k); return side(k.key, words(k)[0], kindIcon[k.key] || "square.grid", on, of); }),
      h("div", { class: "pg-mod-side-row soon" }, icon("questionmark.circle", 16), h("span", {}, t("Predictions", "Прогнозы")), h("span", { class: "spacer" }), h("span", { class: "small" }, t("Soon", "Скоро")))),
    h("div", { class: "pg-mod-detail" }, detail))];
}

// Skills: a card each, its switch, its text on demand, and adding one from a file.

let reading = null;
function skillsTab(s) {
  return [
    h("div", { class: "pg-set-skills" },
      s.skills.map((k) => h("div", { class: "pg-skill-card" },
        h("div", { class: "pg-skill-top" }, tile(k.builtIn ? "chart.pie" : "doc", 34, k.on),
          h("div", { class: "pg-skill-text" }, h("div", {}, h("b", {}, k.name), k.builtIn ? h("span", { class: "pg-skill-chip" }, t("Built in", "Встроенный")) : null),
            h("div", { class: "secondary small" }, k.about)),
          h("span", { class: "spacer" }), toggle("skill-" + k.id, k.on, (on) => act("skillOn", { wallet: k.id, test: on }))),
        h("div", { class: "pg-skill-actions" },
          k.text ? h("button", { class: "link-btn accent-ink", onclick: () => { reading = reading === k.id ? null : k.id; render(); } }, reading === k.id ? t("Hide the text", "Скрыть текст") : t("Read the text", "Читать текст")) : null,
          k.builtIn ? null : h("button", { class: "link-btn loss", onclick: () => act("removeSkill", { wallet: k.id }) }, t("Remove", "Удалить"))),
        reading === k.id ? h("pre", { class: "pg-skill-body selectable", "data-keep": "skill-" + k.id }, k.text) : null)),
      h("div", { class: "btns" },
        h("label", { class: "btn", for: "pg-skill-file" }, icon("plus", 14), t("Add skill…", "Добавить навык…")),
        h("input", { id: "pg-skill-file", type: "file", accept: ".md,.txt,text/plain,text/markdown", multiple: true, hidden: true, onchange: async (e) => {
          for (const f of e.target.files) await act("addSkill", { text: await f.text(), name: f.name });
          e.target.value = "";
        } })),
      h("div", { class: "secondary small" }, t("The assistant sees each switched-on skill's name and what it's for, and reads its whole text when your message is that work. A skill changes how it works with you, never the risk rules, the checks or your Confirm.",
        "Ассистент видит имя каждого включённого навыка и для чего он, а весь текст читает, когда сообщение про эту работу. Навык меняет то, как он работает с тобой, но не риск-правила, проверки и «Подтвердить»."))),
  ];
}

// MARK: History

/** A page's top line: its name, its controls, and the way back (PageHeader). */
function pageHeader(title, controls) {
  return h("div", { class: "page-head" }, h("h1", {}, title), controls, h("span", { class: "spacer" }),
    h("button", { class: "icon-btn", title: t("Back to the canvas", "Назад к холсту"), onclick: back }, icon("xmark", 15)));
}
const empty = (text) => h("div", { class: "page-empty" }, text);

// MARK: History — the Mac's HistoryView: Agent, Positions, All (HistoryTabs.swift reads them when the page opens)

addIcons({
  "arrow.up.right.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 15l6-6M10 9h5v5",
  "arrow.down.left.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM15 9l-6 6M14 15H9v-5",
  "arrow.up.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 16.5v-9M8.5 11 12 7.5l3.5 3.5",
  "xmark.octagon": "M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3zM9 9l6 6M15 9l-6 6",
  hourglass: "M6.5 3h11M6.5 21h11M7.5 3c0 5 9 5 9 9s-9 4-9 9M16.5 3c0 5-9 5-9 9s9 4 9 9",
  "character.bubble": "M4 5h16v11H9l-5 4zM9.5 13l2.5-6 2.5 6M10.3 11.2h3.4",
  "arrow.up.forward.app": "M10 4H5.5A1.5 1.5 0 0 0 4 5.5v13A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5V14M14 4h6v6M20 4l-9 9",
  "text.bubble": "M4 5h16v11H9l-5 4zM8 9h8M8 12.5h5",
  "character.book": "M5 4.5A1.5 1.5 0 0 1 6.5 3H19v16H6.5A1.5 1.5 0 0 0 5 20.5zM9.5 14l2.5-7 2.5 7M10.3 12h3.4",
  qrcode: "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h2v2h-2zM18 14h2v2M14 18h2v2M18 18h2v2h-2zM6.5 6.5h1v1h-1zM16.5 6.5h1v1h-1zM6.5 16.5h1v1h-1z",
});

/** A page's data read again when it opens: `leave` runs once the window shows something else. */
const shownPoll = {};
function whileShown(view, leave) {
  if (shownPoll[view]) return;
  shownPoll[view] = setInterval(() => {
    if (ui.view === view) return;
    clearInterval(shownPoll[view]);
    delete shownPoll[view];
    leave();
  }, 800);
}

const history = { data: null, loading: false, stale: true, open: new Set() };
async function loadHistory() {
  if (history.loading) return;
  history.loading = true;
  history.stale = false;
  render();
  const r = await act("historyTabs");
  history.loading = false;
  if (r && r.json) { try { history.data = JSON.parse(r.json); } catch {} }
  render();
}

const toneClass = { accent: "accent-ink", profit: "profit", loss: "loss", secondary: "secondary" };
const signedMoney = (v) => (Number(v) < 0 ? "−" + usd(Math.abs(Number(v))) : "+" + usd(Number(v)));
const dayTime = (iso) => new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
const sameDay = (a, b) => new Date(a).toDateString() === new Date(b).toDateString();

/** A card of the canvas frozen in time: its label, title and two lines. */
function historyBubble(b) {
  return h("div", { class: "page-hb page-hb-" + b.status },
    h("div", { class: "eyebrow one-line" }, b.label),
    h("b", { class: "page-hb-title" }, b.title),
    (b.lines || []).slice(0, 2).map((l) => h("div", { class: "page-hb-line secondary" }, l)));
}

function activityCard(a, key) {
  const open = history.open.has(key);
  const ico = a.kind === "execution" ? "signature" : a.kind === "automatic" ? "gear" : "text.bubble";
  const lines = open ? a.lines : a.lines.slice(-2);
  return h("div", { class: "page-act card" },
    h("div", { class: "page-act-head" }, h("span", { class: a.failed ? "loss" : "accent-ink" }, icon(ico, 16)),
      h("b", { class: "one-line" }, a.title || (a.kind === "request" ? t("Request", "Запрос") : "ihatecrypto")),
      h("span", { class: "spacer" }), h("span", { class: "secondary small num" }, when(a.at))),
    a.request ? h("div", { class: "page-act-ask selectable" }, a.request) : null,
    a.understood ? h("div", { class: "page-act-read secondary" }, icon("character.bubble", 12), t(`understood as “${a.understood}”`, `понял как «${a.understood}»`)) : null,
    a.bubbles.length ? h("div", { class: "page-act-bubbles" }, a.bubbles.map(historyBubble)) : null,
    lines.map((l) => h("div", { class: "page-act-line secondary selectable" }, l)),
    a.lines.length > 2 ? h("button", { class: "link-btn", onclick: () => { open ? history.open.delete(key) : history.open.add(key); render(); } },
      open ? t("Less", "Меньше") : t(`All ${a.lines.length} lines`, `Все строки: ${a.lines.length}`)) : null);
}

function storyCard(st) {
  const span = st.closed ? `${when(st.opened)} → ${sameDay(st.opened, st.closed) ? dayTime(st.closed) : when(st.closed)}` : `${when(st.opened)} · ${t("open", "открыта")}`;
  return h("div", { class: "page-story card" },
    h("div", { class: "page-story-head" }, logo(st.coin, 34),
      h("div", { class: "page-story-name" }, h("b", {}, `${st.long ? t("Long", "Лонг") : t("Short", "Шорт")} ${st.coin}`), h("div", { class: "secondary small" }, span)),
      h("span", { class: "spacer" }),
      st.open ? h("span", { class: "page-story-chip" }, t("open", "открыта"))
        : h("b", { class: "page-story-pnl num " + (Number(st.realized) < 0 ? "loss" : "profit") }, signedMoney(st.realized))),
    h("div", { class: "page-story-events" }, st.events.map((e, i) => [
      i > 0 ? h("span", { class: "page-story-step tertiary" }, icon("chevron.right", 12)) : null,
      h("div", { class: "page-hb page-hb-" + e.kind }, h("div", { class: "eyebrow one-line" }, e.label), h("b", { class: "page-hb-title" }, e.title),
        h("div", { class: "page-hb-line secondary" }, when(e.at)), (e.lines || []).map((l) => h("div", { class: "page-hb-line secondary" }, l)))])),
    st.open ? null : h("div", { class: "secondary small" },
      t(`Realized ${signedMoney(st.realized)} after ${usd(st.fees)} in fees · peak size ${st.peak}`,
        `Итог ${signedMoney(st.realized)} после комиссий ${usd(st.fees)} · наибольший размер ${st.peak}`)));
}

export function historyPage() {
  const tab = ["agent", "positions", "all"].includes(store("historyTab")) ? store("historyTab") : "agent";
  whileShown("history", () => { history.stale = true; });
  if (history.stale && !history.loading) setTimeout(loadHistory);
  const d = history.data;
  let body;
  if (!d) {
    body = [empty(t("Reading the history…", "Читаю историю…"))];
  } else if (tab === "positions") {
    body = d.stories.length ? d.stories.map(storyCard)
      : [empty(!d.hyperliquid ? t("No Hyperliquid address yet.", "Адреса Hyperliquid пока нет.") : t("No trades on Hyperliquid yet.", "Сделок на Hyperliquid пока нет."))];
  } else if (tab === "all") {
    body = d.rows.length ? d.rows.map((r) => h("div", { class: "page-hist-row" },
      h("span", { class: "page-hist-at secondary num" }, when(r.at)),
      h("span", { class: "page-hist-ico " + (toneClass[r.tone] || "secondary") }, icon(r.icon, 15)),
      h("div", { class: "page-hist-text selectable" }, h("div", {}, r.text), r.detail ? h("div", { class: "secondary small" }, r.detail) : null),
      r.link ? link(r.link, icon("arrow.up.right.square", 15)) : null))
      : [empty(t("Nothing yet.", "Пока ничего."))];
  } else {
    body = [
      !d.agent.length && !d.earlier.length ? empty(t("Nothing yet. Every request from now on is kept here with the cards it had on the canvas.",
        "Пока ничего. Каждый запрос с этого момента сохраняется здесь вместе с карточками, которые были на холсте.")) : null,
      d.agent.map((a, i) => activityCard(a, "a" + i + a.at)),
      d.earlier.length ? h("div", { class: "page-act-earlier secondary small" }, icon("clock.arrow", 12),
        t("Earlier — from the signature journal. The chat of that time wasn’t kept; from now on every request is.",
          "Раньше — из журнала подписей. Чат того времени не сохранился; теперь сохраняется каждый запрос.")) : null,
      d.earlier.map((a, i) => activityCard(a, "e" + i + a.at)),
    ];
  }
  return h("div", { class: "page-pane", style: `left:${railEdge()}px` },
    h("div", { class: "page-scroll", "data-keep": "history-" + tab },
      pageHeader(t("History", "История"), [
        segmented([["agent", t("Agent", "Агент")], ["positions", t("Positions", "Позиции")], ["all", t("All", "Всё")]], tab, (v) => { store("historyTab", v); render(); }),
        history.loading ? h("span", { class: "page-spin" }) : null,
        h("button", { class: "icon-btn", title: t("Refresh", "Обновить"), onclick: loadHistory }, icon("arrow.clockwise", 15))]),
      d && d.error ? h("div", { class: "page-hist-error loss small" }, d.error) : null,
      h("div", { class: "page-body" }, body)),
    chatPanel());
}

// MARK: Top up — the Mac's ReceiveBoard: what lands, on which network, three ways to send it, and the money watched for

const topUpLogos = { solana: "sol", base: "base", arbitrum: "arb", optimism: "op", polygon: "pol", bnb: "bnb", hyperevm: "hyperevm",
  robinhood: "robinhood", tempo: "tempo", ethereum: "eth" };
/** Quick amounts in a coin (nil: the network's dollar): a few dollars' worth of gas, more of the cheaper coins. */
const presets = (coin) => ({ USDC: ["20", "50", "100", "500"], SOL: ["0.02", "0.1", "0.5"], HYPE: ["0.02", "0.1", "0.5"], ETH: ["0.002", "0.01", "0.05"],
  BNB: ["0.005", "0.02", "0.1"], POL: ["5", "20", "100"] })[coin || "USDC"] || [];
const solanaFeeTopUp = "0.01";
const gaslessSwapUSD = 5;
const gasUSD = (key) => (key === "ethereum" ? 3 : 1);

/** The order as TopUpOrder has it: the coin's symbol and digits, its smallest units (null: any coin or amount, or a
 * typo) and the link a phone wallet's camera takes (Solana Pay or EIP-681). */
function topUpOrder(net, coin, amount) {
  const typed = (amount || "").replace(",", ".").trim();
  const symbol = coin === "usdc" ? net.usdcSymbol : coin === "native" ? net.coin : t("any coin", "любая монета");
  const decimals = coin === "usdc" ? net.usdcDecimals : net.chainId == null ? 9 : 18;
  let units = null;
  const m = /^(\d*)(?:\.(\d*))?$/.exec(typed);
  if (coin !== "any" && typed && m && (m[1] || m[2]) && (m[2] || "").length <= decimals) {
    const v = BigInt((m[1] || "0") + (m[2] || "").padEnd(decimals, "0"));
    if (v > 0n && v <= 1000000n * 10n ** BigInt(decimals)) units = v.toString();
  }
  const shown = typed.startsWith(".") ? "0" + typed : typed;
  let payLink = null;
  if (coin === "any" || !typed || units) {
    if (net.chainId == null) payLink = `solana:${net.recipient}?` + (units ? `amount=${shown}&` : "") + (coin === "usdc" ? `spl-token=${net.usdc}&` : "") + "label=ihatecrypto&message=Top%20up";
    else if (coin === "usdc") payLink = `ethereum:${net.usdc}@${net.chainId}/transfer?address=${net.recipient}` + (units ? `&uint256=${units}` : "");
    else if (coin === "native") payLink = `ethereum:${net.recipient}@${net.chainId}` + (units ? `?value=${units}` : "");
    else payLink = `ethereum:${net.recipient}@${net.chainId}`;
  }
  return { symbol, decimals, units, payLink, amount: shown, typed };
}

let topUpAsked = "";
let topUpOpening = false;
let topUpProblem = null;
let topUpHeard = null;
let topUpPoll = null;
const num = (v, digits) => Number(v).toLocaleString(ui.state.russian ? "ru-RU" : "en-US", { maximumFractionDigits: digits });

/** The Top up page: the panel in its card, with its title and close. */
export function topUpPage() {
  return h("div", { class: "page-pane", style: `left:${railEdge()}px` },
    h("div", { class: "page-scroll", "data-keep": "topup" }, h("div", { class: "page-topup-place" }, h("div", { class: "page-topup card" },
      h("div", { class: "page-tu-head" }, h("span", { class: "accent-ink" }, icon("arrow.down.circle", 20)), h("b", {}, t("Top up ihatecrypto", "Пополнить ihatecrypto")),
        h("span", { class: "spacer" }), h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: back }, icon("xmark", 14))),
      topUpPanel()))), chatPanel());
}

/** Top up's panel (ReceiveBoard): the amount, the network, the three ways, the QR and address, the gas line and the
 * money watched for. `embedded`: inside another card (the onboarding's Top up step), with no frame of its own;
 * `landed(text)` hears once what arrived. While the panel is on screen the engine watches; once it's gone, it stops. */
export function topUpPanel({ embedded = false, landed } = {}) {
  const s = ui.state;
  const v = s.topUp;
  const way = ["browser", "phone", "exchange"].includes(store("topUpWay")) ? store("topUpWay") : "browser";
  const nets = v ? v.networks : [];
  const wanted = store("topUpNetwork") || "solana";
  const net = nets.find((n) => n.key === wanted) || nets[0];
  let coin = store("topUpCoin") || "usdc";
  if (net && coin === "native" && !net.coin) coin = "usdc";
  // The engine reads what's there and, once a way is used (anything but the browser's page), watches for it.
  const watch = way !== "browser" || (v && v.waiting && v.network === net?.key && v.coin === coin);
  const key = `${net ? net.key : wanted}|${coin}|${watch}`;
  if (topUpAsked !== key) { topUpAsked = key; setTimeout(() => act("topUp", { network: net ? net.key : wanted, text: coin, test: watch })); }
  // Gone from the window (another page, the onboarding's next step): the engine stops watching and closes the page.
  if (!topUpPoll) {
    topUpPoll = setInterval(() => {
      if (document.querySelector(".page-tu-live")) return;
      clearInterval(topUpPoll);
      topUpPoll = null;
      topUpAsked = "";
      topUpProblem = null;
      engine("topUpStop").catch(() => {});
    }, 800);
  }
  const frame = (...content) => h("div", { class: "page-tu-panel page-tu-live" + (embedded ? " embedded" : "") }, ...content);
  if (!s.addresses.solana && !s.addresses.evm) return frame(empty(t("Add a wallet first: Settings › Wallet.", "Сначала добавь кошелёк: Настройки › Кошелёк.")));
  if (!net) return frame(empty(t("Reading the networks…", "Читаю сети…")));
  const amount = coin === "any" ? "" : (store("topUpAmount") ?? "20");
  const o = topUpOrder(net, coin, amount);
  const solana = net.chainId == null;
  const here = v && v.network === net.key && v.coin === coin ? v : null;
  const digits = coin === "usdc" ? 2 : 6;
  const setAmount = (a) => { store("topUpAmount", a); render(); };
  const pickCoin = (c) => {
    store("topUpCoin", c);
    if (c === "usdc") store("topUpAmount", "20");
    else if (c === "native") store("topUpAmount", presets(net.coin)[0] || "");
    render();
  };
  const pickNet = (n) => {
    store("topUpNetwork", n.key);
    if (coin === "native") { if (n.coin) store("topUpAmount", presets(n.coin)[0] || ""); else { store("topUpCoin", "usdc"); store("topUpAmount", "20"); } }
    render();
  };
  const chip = (label, on, run, title) => h("button", { class: "page-tu-chip" + (on ? " on" : ""), title: title || "", onclick: run }, label);
  const best = () => h("span", { class: "page-tu-best" }, "BEST");

  // The amount: a field (or «Other token»), the coin's chips, quick amounts and the balance now → after.
  const quick = presets(coin === "usdc" ? null : net.coin);
  const now = here && here.held[o.symbol];
  const amountRow = h("div", { class: "page-tu-amount" },
    h("div", { class: "page-tu-coins" },
      coin === "any" ? h("span", { class: "page-tu-symbol" }, t("Other token", "Другой токен"))
        : [h("input", { id: "topup-amount", class: "page-tu-field num", inputmode: "decimal", autocomplete: "off", value: amount, placeholder: "0", "aria-label": t("Amount", "Сумма"),
            oninput: (e) => { store("topUpAmount", e.target.value); render(); } }),
          h("span", { class: "page-tu-unit" }, o.symbol)],
      h("span", { class: "spacer" }),
      chip(net.usdcSymbol, coin === "usdc", () => pickCoin("usdc")),
      net.coin ? chip(net.coin, coin === "native", () => pickCoin("native")) : null,
      chip(t("Other", "Другой"), coin === "any", () => pickCoin("any"), t("Other token", "Другой токен"))),
    coin === "any"
      ? h("div", { class: "secondary small" }, t(`Any token on ${net.title} lands at this wallet's address: ${net.usdcSymbol}, ${net.coin || "stablecoins"}, memecoins…`,
          `Любой токен в ${net.title} приходит на адрес этого кошелька: ${net.usdcSymbol}, ${net.coin || "стейблкоины"}, мемкоины…`))
      : h("div", { class: "page-tu-presets" },
          quick.map((p) => chip(p, amount === p, () => setAmount(p))),
          chip(t("Custom", "Своя"), !quick.includes(amount), () => { store("topUpAmount", ""); render(); setTimeout(() => document.getElementById("topup-amount")?.focus()); }, t("Custom amount", "Своя сумма")),
          h("span", { class: "spacer" }),
          now != null ? h("span", { class: "secondary small num", title: t(`What this wallet holds on ${net.title} now → after the top-up`, `Сколько на этом кошельке в ${net.title} сейчас → после пополнения`) },
            o.units ? `${num(now, digits)} → ${num(Number(now) + Number(o.amount), digits)} ${o.symbol}` : t(`${num(now, digits)} ${o.symbol} now`, `сейчас ${num(now, digits)} ${o.symbol}`)) : null),
    coin !== "any" && o.typed && !o.units
      ? h("div", { class: "loss small" }, t(`Type an amount above zero, with at most ${o.decimals} digits after the point.`, `Введи сумму больше нуля, не больше ${o.decimals} знаков после точки.`)) : null);

  const netRow = h("div", { class: "page-tu-nets", "data-keep": "page-topup-nets" }, nets.map((n) => h("button", { class: "page-tu-net" + (n === net ? " on" : ""),
    title: n.chainId == null ? t("Recommended: the cheapest and fastest to top up, and where ihatecrypto trades from", "Рекомендую: дешевле и быстрее всего, и отсюда ihatecrypto торгует") : "",
    onclick: () => pickNet(n) }, logo(topUpLogos[n.key] || n.key, 16), n.title, n.chainId == null ? best() : null)));

  // The three ways, each saying what it costs.
  const cost = net.key === "ethereum" ? t("fee 1–20¢ · lands in ~15 s", "комиссия 1–20¢ · дойдёт за ~15 с") : t("fee under 1¢ · lands in seconds", "комиссия меньше 1¢ · дойдёт за секунды");
  const browserNeeds = coin === "any" ? t(`pick ${net.usdcSymbol}${net.coin ? " or " + net.coin : ""} above`, `выбери ${net.usdcSymbol}${net.coin ? " или " + net.coin : ""} выше`)
    : !o.typed ? t("pick an amount above", "выбери сумму выше") : null;
  const tile = (id, ico, title, detail, price) => h("button", { class: "page-tu-way" + (way === id ? " on" : ""), onclick: () => { store("topUpWay", id); topUpProblem = null; render(); } },
    h("div", { class: "page-tu-way-top" }, icon(ico, 15), h("b", {}, title), h("span", { class: "spacer" }), id === "browser" ? best() : null),
    h("div", { class: "secondary one-line" }, detail), h("div", { class: "one-line " + (id === "exchange" ? "secondary" : "profit") }, price));
  const ways = h("div", { class: "page-tu-ways3" },
    tile("browser", "macwindow", t("Browser wallet", "Кошелёк в браузере"),
      browserNeeds ? t(`Needs a coin and an amount: ${browserNeeds}`, `Нужны монета и сумма: ${browserNeeds}`) : solana ? "Phantom, Solflare, Backpack" : "MetaMask, Rabby, Phantom", cost),
    h("div", { class: "page-tu-ways" },
      tile("phone", "qrcode", t("Phone wallet", "Кошелёк в телефоне"), o.units ? t("scan, the amount is filled in", "сканируй, сумма уже подставлена") : t("scan, the wallet asks how much", "сканируй, сумму спросит кошелёк"), cost),
      tile("exchange", "building.columns", t("Exchange", "Биржа"), t("Binance, OKX, Bybit… by address", "Binance, OKX, Bybit… по адресу"), t("the exchange's withdrawal fee", "комиссия биржи за вывод"))));

  // One block for all three: the QR, what this way does, and the whole address with Copy.
  const qrText = way === "exchange" ? net.recipient : o.payLink || net.recipient;
  const [title, detail] = way === "browser"
    ? [t("Approve in your browser wallet", "Подтверди в кошельке браузера"),
      browserNeeds ? t(`A browser wallet is asked for one transfer: ${browserNeeds}. For any coin or amount, scan with your phone or send from an exchange.`,
          `Кошелёк в браузере просят об одном переводе: ${browserNeeds}. Для любой монеты или суммы отсканируй телефоном или отправь с биржи.`)
        : t(`Your browser opens a page served by this PC. Pick your wallet there and approve ${o.amount} ${o.symbol} on ${net.title}: the address is put in by ihatecrypto, so no website can swap it.`,
          `Браузер откроет страницу, которую отдаёт этот компьютер. Выбери там свой кошелёк и подтверди ${o.amount} ${o.symbol} в ${net.title}: адрес подставляет ihatecrypto, поэтому никакой сайт его не подменит.`)]
    : way === "phone"
      ? [t("Scan with your phone's wallet", "Отсканируй кошельком в телефоне"),
        (solana ? "Phantom, Solflare, Backpack: " : "MetaMask, Rainbow, Coinbase Wallet: ")
          + (coin === "any" ? t(`this wallet on ${net.title} is filled in, you pick the coin and the amount.`, `этот кошелёк в ${net.title} уже подставлен, монету и сумму выбираешь ты.`)
            : o.units ? t(`${o.amount} ${o.symbol} to this wallet on ${net.title} is filled in, you only approve.`, `${o.amount} ${o.symbol} на этот кошелёк в ${net.title} уже подставлено, остаётся подтвердить.`)
            : t(`${o.symbol} to this wallet on ${net.title} is filled in, the wallet asks how much.`, `${o.symbol} на этот кошелёк в ${net.title} уже подставлен, сколько — спросит кошелёк.`))]
      : [t(`Withdraw to this address on ${net.title}`, `Выведи на этот адрес в сети ${net.title}`),
        solana ? t("USDC, SOL or any Solana token (SPL). USDC is what ihatecrypto trades with; keep ≈ 0.02 SOL for fees.", "USDC, SOL или любой токен Solana (SPL). ihatecrypto торгует на USDC; держи ≈ 0,02 SOL на комиссии.")
          : t(`${net.usdcSymbol}${net.coin ? ", " + net.coin : ""} or any token on ${net.title}. The same 0x address is this wallet on every EVM network ihatecrypto uses.`,
            `${net.usdcSymbol}${net.coin ? ", " + net.coin : ""} или любой токен в ${net.title}. Тот же 0x-адрес — этот кошелёк во всех EVM-сетях ihatecrypto.`)];
  const openPage = async () => {
    topUpOpening = true;
    topUpProblem = null;
    render();
    try {
      const r = await engine("topUpOpen", { network: net.key, text: coin, label: o.amount });
      await invoke("open_link", { url: r.url });
    } catch (e) {
      topUpProblem = t("The page didn't open: ", "Страница не открылась: ") + (e.message || String(e));
    }
    topUpOpening = false;
    render();
  };
  const wayBody = h("div", { class: "page-tu-wayblock" },
    h("div", { class: "page-tu-body" },
      h("div", { class: "page-tu-qr", title: way === "exchange" ? t("Scan with the exchange's app", "Сканируй приложением биржи") : t("Scan with your phone's wallet", "Сканируй кошельком в телефоне"),
        "aria-label": qrText !== net.recipient && o.units ? t(`QR code to top up ${o.amount} ${o.symbol}`, `QR-код, чтобы пополнить ${o.amount} ${o.symbol}`) : t(`QR code of this wallet on ${net.title}`, `QR-код этого кошелька в ${net.title}`) },
        qrCode(qrText, "page-qr"), h("span", { class: "page-tu-qr-logo" }, logo(topUpLogos[net.key] || net.key, 26))),
      h("div", { class: "page-tu-text" }, h("b", {}, title), h("div", { class: "small" }, detail), addressLines(net.recipient))),
    way === "browser" ? h("button", { class: "btn primary page-tu-go", disabled: !o.units || topUpOpening, onclick: openPage }, icon("arrow.up.forward.app", 15),
      topUpOpening ? t("Opening…", "Открываю…") : !browserNeeds ? t(`Top up ${o.amount} ${o.symbol} from a browser wallet`, `Пополнить ${o.amount} ${o.symbol} из кошелька в браузере`)
        : t("Top up from a browser wallet", "Пополнить из кошелька в браузере")) : null,
    way === "exchange" ? h("div", { class: "page-tu-warn pg-jev-ink" }, icon("exclamationmark.triangle", 13),
      h("span", {}, t(`Pick the network exactly: ${solana ? "Solana (SPL)" : net.title}. Sent on another network, money can get stuck. Many exchanges withdraw USDC on Solana or Base for well under a dollar.`,
        `Выбери сеть точно: ${solana ? "Solana (SPL)" : net.title}. Отправленные в другой сети деньги могут застрять. Многие биржи выводят USDC в Solana или Base гораздо дешевле доллара.`))) : null,
    way === "exchange" && !solana ? h("div", { class: "page-tu-warn secondary" }, icon("questionmark.circle", 13),
      h("span", {}, t("Hyperliquid uses the same 0x address, but money gets there through ihatecrypto. Never send to Hyperliquid's old bridge contract.",
        "У Hyperliquid тот же 0x-адрес, но деньги туда попадают через ihatecrypto. Никогда не отправляй на старый бридж-контракт Hyperliquid."))) : null);

  // What's happening with the money: sent from the page, landed, or watched for.
  // Each landing heard once, by its words.
  if (landed && here && here.arrived && topUpHeard !== here.arrived) { topUpHeard = here.arrived; setTimeout(() => landed(here.arrived)); }
  const status = here && here.sent && !here.arrived
    ? h("div", { class: "page-tu-status profit" }, icon("paperplane", 13), h("span", {}, t("Sent from your wallet — it shows here the moment it lands.", "Отправлено из кошелька — появится здесь, как только дойдёт.")),
        here.sent.link ? link(here.sent.link, t("See it", "Посмотреть")) : null)
    : here && here.arrived ? h("div", { class: "page-tu-status profit" }, icon("checkmark.circle", 13), h("span", {}, here.arrived))
    : here && here.waiting ? h("div", { class: "page-tu-status secondary" }, icon("hourglass", 13),
        h("span", {}, t(`Waiting for ${o.units ? `${o.amount} ${o.symbol}` : coin === "any" ? "money" : o.symbol} on ${net.title}…`,
          `Жду ${o.units ? `${o.amount} ${o.symbol}` : coin === "any" ? "деньги" : o.symbol} в ${net.title}…`)))
    : null;

  // Dollars alone are a dead end where fees are paid in another coin: said before they're sent. A browser wallet on
  // Solana brings the SOL along in the same approval; the other ways get a button for the coin.
  const gas = coin === "usdc" && here && here.gasShort && here.gasShort === net.coin ? here.gasShort : null;
  const bundled = solana && way === "browser";
  const gasLine = gas ? h("div", { class: "page-tu-gas" }, h("span", { class: bundled ? "profit" : "accent-ink" }, icon(bundled ? "fuelpump" : "exclamationmark.triangle", 13)),
    h("span", {}, bundled ? t(`+ ${solanaFeeTopUp} SOL for fees goes along, in the same approval: on Solana, USDC alone can't move.`, `+ ${solanaFeeTopUp} SOL на комиссии уйдёт вместе, в том же подтверждении: в Solana одни USDC не сдвинутся.`)
      : solana ? t(`Send ≈ $${gasUSD(net.key)} of SOL too: every move on Solana pays its fee in SOL. Without it, from $${gaslessSwapUSD} of USDC the chat swaps a part for SOL, Jupiter paying that one fee.`,
          `Отправь ещё ≈ $${gasUSD(net.key)} в SOL: каждое движение в Solana платит комиссию в SOL. Без него от $${gaslessSwapUSD} в USDC чат поменяет часть на SOL, и эту одну комиссию заплатит Jupiter.`)
      : net.bySignature ? t(`Send ≈ $${gasUSD(net.key)} of ${gas} too: moves on ${net.title} pay their fee in ${gas}. Without it the ${net.usdcSymbol} still goes to another network by signature, Relay paying the gas.`,
          `Отправь ещё ≈ $${gasUSD(net.key)} в ${gas}: движения в ${net.title} платят комиссию в ${gas}. Без него ${net.usdcSymbol} всё равно уйдут в другую сеть по подписи, газ заплатит Relay.`)
      : t(`Send ≈ $${gasUSD(net.key)} of ${gas} too: every move on ${net.title} pays its fee in ${gas}, so ${net.usdcSymbol} alone can't move there.`,
          `Отправь ещё ≈ $${gasUSD(net.key)} в ${gas}: каждое движение в ${net.title} платит комиссию в ${gas}, одни ${net.usdcSymbol} там не сдвинутся.`)),
    h("span", { class: "spacer" }), bundled ? null : h("button", { class: "btn small", onclick: () => pickCoin("native") }, t(`Top up ${gas}`, `Пополнить ${gas}`))) : null;

  return frame(amountRow, netRow, ways, wayBody, status, gasLine,
    topUpProblem ? h("div", { class: "loss small" }, topUpProblem) : null,
    h("div", { class: "secondary small" }, s.portfolio.updatedAt ? t("Balances read ", "Балансы прочитаны ") + when(s.portfolio.updatedAt) + " · " : "",
      h("button", { class: "link-btn", onclick: () => act("refresh") }, t("Refresh", "Обновить"))));
}

/** An address as it's checked by eye: the whole of it as an even block (lines of the same length, never a half that
 * wraps on its own), its ends in bold, and Copy for all of it (AddressLines). */
function addressLines(address, perLine = 22) {
  const n = [2, 3, 4, 5, 6].sort((a, b) => ((address.length % a ? 1 : 0) - (address.length % b ? 1 : 0)) || a - b)
    .find((k) => Math.ceil(address.length / k) <= perLine) || 6;
  const head = Math.min(8, Math.floor(address.length / 2)), tail = address.length - head;
  const starts = [...Array(n + 1).keys()].map((i) => i * Math.floor(address.length / n) + Math.min(i, address.length % n));
  const part = (a, b) => (a < b ? address.slice(a, b) : "");
  return h("div", { class: "page-tu-addr" },
    h("div", { class: "page-tu-addr-block", "aria-label": address }, [...Array(n).keys()].map((i) => {
      const a = starts[i], b = starts[i + 1];
      return h("div", {}, h("b", {}, part(a, Math.min(b, head))), h("span", { class: "secondary" }, part(Math.max(a, head), Math.min(b, tail))), h("b", {}, part(Math.max(a, tail), b)));
    })),
    h("button", { class: "btn small", onclick: () => copy(address) }, icon("copy", 12), t("Copy address", "Скопировать адрес")));
}

