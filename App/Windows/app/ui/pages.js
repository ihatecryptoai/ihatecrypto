// The pages that take the canvas's place beside the rail, as the Mac draws them: Settings (SettingsView.swift — a
// sidebar of tabs, each a column of sections with rows), History (HistoryView.swift) and Top up (ReceiveBoard in
// Receive.swift). Settings cover the chat while open; History and Top up keep it floating, as the canvas does.

import { ui, act, render, t, h, svg, icon, addIcons, logo, mascot, toast, copy, link, store, invoke, when, presence } from "./core.js";
import { railEdge, companyLogo } from "./rail.js";
import { chatPanel } from "./chat.js";
import { segmented, toggle, showImport, showImportKey, watchFields } from "./onboarding.js";
import { qrMatrix } from "./qr.js";

const back = () => { ui.view = "canvas"; render(); };

addIcons({
  "info.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5.5M12 7.6v.4",
  sparkles: "M10 3.5l1.6 4.4L16 9.5l-4.4 1.6L10 15.5l-1.6-4.4L4 9.5l4.4-1.6zM17.5 14l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
  creditcard: "M3.5 6h17v12h-17zM3.5 10h17M7 14.5h4",
  wand: "M4 20L15 9M13.5 4.5v2M17.5 8.5h2M16.3 5.7l1.4-1.4M19.5 4.5l-1.2 1.2M12.5 7l4.5 4.5",
  "arrow.up.right.square": "M5 5h14v14H5zM10 14l5-5M10.5 9H15v4.5",
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
  ["model", "sparkles", () => t("AI model", "Модель ИИ")],
  ["wallet", "creditcard", () => t("Wallet", "Кошелёк")],
  ["skills", "wand", () => t("Skills", "Навыки")],
];

export function settingsPage() {
  let tab = store("settingsTab");
  if (!tabs.some(([id]) => id === tab)) tab = "general";
  const s = ui.state;
  const [title, line, body] = {
    general: [t("General", "Основные"), t("How the app talks to you and keeps running.", "Как приложение говорит с тобой и работает в фоне."), general],
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
        h("div", { class: "pg-set-page" }, h("div", { class: "pg-set-page-head" }, h("h1", {}, title), h("div", { class: "secondary" }, line)), body(s)))));
}

// General: appearance, language, the chat, updates.

function general(s) {
  const theme = store("theme") || "system";
  return [
    section("eye", t("Appearance", "Оформление"), t("Light, dark, or as Windows is set.", "Светлое, тёмное или как в Windows."),
      group(row({ label: t("Theme", "Тема"), ico: "eye" }, segmented([["system", t("System", "Системная")], ["light", t("Light", "Светлая")], ["dark", t("Dark", "Тёмная")]],
        theme, (v) => { store("theme", v); applyTheme(); render(); }, "pg-set-seg")))),
    section("globe", t("Language", "Язык"), t("The app's own texts, and the chat's replies.", "Тексты приложения и ответы чата."),
      group(row({ label: t("Language", "Язык"), ico: "globe", about: t("Auto answers in the language you write in. Tickers, amounts and addresses stay as they are.", "«Как я пишу» отвечает на языке твоего сообщения. Тикеры, суммы и адреса остаются как есть.") },
        h("select", { onchange: (e) => act("language", { language: e.target.value }) },
          [["auto", t("Auto — your language", "Авто — твой язык")], ["en", "English"], ["ru", "Русский"]].map(([v, l]) => h("option", { value: v, selected: s.language === v }, l)))))),
    section("square.and.pencil", t("Chat", "Чат"), t("What this canvas's chat keeps.", "Что хранит чат этого холста."),
      group(row({ label: t("This canvas's chat", "Чат этого холста"), ico: "square.and.pencil", about: t("Clears the messages of the open canvas. Its blocks, the history and the wallet stay.", "Стирает сообщения открытого холста. Его блоки, история и кошелёк остаются.") },
        h("button", { class: "btn small", onclick: () => act("clearChat") }, t("Clear", "Очистить"))))),
    section("power", t("Background", "Фон"), t("Alerts and scheduled tasks with the window closed.", "Оповещения и задачи по расписанию при закрытом окне."),
      group(row({ label: t("Closing the window", "Закрытие окна"), ico: "power", about: t("ihatecrypto stays in the tray: price alerts, balances and scheduled tasks go on. Quit from the tray icon's menu ends it.", "ihatecrypto остаётся в трее: оповещения, балансы и задачи продолжают работать. «Выход» в меню значка в трее закрывает его совсем.") },
        h("span", { class: "secondary" }, t("Keeps it in the tray", "Оставляет в трее"))))),
    section("arrow.down.circle", t("About", "О приложении"), t("An update installs only when its signature matches ihatecrypto's key.", "Обновление ставится, только если его подпись совпадает с ключом ihatecrypto."),
      group(row({ label: t("ihatecrypto for Windows", "ihatecrypto для Windows"), ico: "arrow.down.circle" },
        h("button", { class: "btn small", onclick: checkUpdate }, t("Check for updates", "Проверить обновления"))))),
  ];
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
        row({ label: t("Ask again now", "Спрашивать снова сейчас"), ico: "lock", about: t("Ends the session now and pauses the Hyperliquid trading key until the next Windows Hello.", "Заканчивает сессию сейчас и ставит торговый ключ Hyperliquid на паузу до следующего Windows Hello.") },
          h("button", { class: "btn small", onclick: () => act("lock") }, t("Lock", "Заблокировать"))),
        s.signer.notes.map((n) => row({ label: n, ico: "checkmark.seal", cls: "wrap note" })),
        s.signer.error ? row({ label: s.signer.error, ico: "exclamationmark.triangle", cls: "wrap loss" }) : null)),
    watched ? section("eye", t("Addresses for balances", "Адреса для балансов"), t("Read-only wallets. Never paste a recovery phrase or key.", "Кошельки только для просмотра. Никогда не вставляй фразу восстановления или ключ."),
      group(h("div", { class: "pg-set-panel" }, watched.fields,
        h("div", { class: "btns" }, h("button", { class: "btn primary", onclick: watched.save }, t("Watch", "Смотреть")),
          watched.stop ? h("button", { class: "btn", onclick: watched.stop }, t("Stop watching", "Перестать смотреть")) : null)))) : null,
  ];
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

export function historyPage() {
  const s = ui.state;
  const tab = store("historyTab") === "positions" ? "positions" : "all";
  const kind = { signed: t("Signed", "Подписано"), confirmed: t("Confirmed", "Подтверждено"), settled: t("Settled", "Завершено"), failed: t("Failed", "Не вышло"), expired: t("Expired", "Истекло") };
  const glyph = (k) => k === "failed" || k === "expired" ? "xmark" : k === "settled" || k === "confirmed" ? "check" : "pencil";
  const chain = (c) => c.charAt(0).toUpperCase() + c.slice(1);
  const all = s.history.length ? s.history.map((e) => h("div", { class: "page-hist-row" },
    h("span", { class: "page-hist-at secondary num" }, when(e.at)),
    h("span", { class: "page-hist-ico " + (e.kind === "failed" ? "loss" : "secondary") }, icon(glyph(e.kind), 15)),
    h("div", { class: "page-hist-text" }, h("div", {}, `${kind[e.kind] || e.kind} · ${chain(e.chain)}: ${e.note}`)),
    e.link ? link(e.link, icon("arrow.up.right.square", 15)) : null))
    : [empty(t("Nothing signed yet. Every signature from now on is listed here.", "Пока ничего не подписано. Каждая подпись с этого момента будет здесь."))];
  const positions = s.positions.length ? s.positions.map((p) => h("div", { class: "page-story card" },
    h("div", { class: "page-story-head" }, logo(p.coin, 34),
      h("div", {}, h("b", {}, `${p.side === "long" ? t("Long", "Лонг") : t("Short", "Шорт")} ${p.coin}`), h("div", { class: "secondary small" }, `${p.size} @ ${p.entry}` + (p.liquidation ? t(` · liquidation ${p.liquidation}`, ` · ликвидация ${p.liquidation}`) : ""))),
      h("span", { class: "spacer" }), h("span", { class: "page-story-chip" }, t("open", "открыта")),
      h("b", { class: "page-story-pnl num " + (p.pnl.startsWith("-") || p.pnl.startsWith("−") ? "loss" : "profit") }, p.pnl))))
    : [empty(t("No open positions on Hyperliquid.", "Открытых позиций на Hyperliquid нет."))];
  return h("div", { class: "page-pane", style: `left:${railEdge()}px` },
    h("div", { class: "page-scroll", "data-keep": "history-" + tab },
      pageHeader(t("History", "История"), [
        segmented([["all", t("All", "Всё")], ["positions", t("Positions", "Позиции")]], tab, (v) => { store("historyTab", v); render(); }),
        h("button", { class: "icon-btn", title: t("Refresh", "Обновить"), onclick: () => act("refresh") }, icon("arrow.clockwise", 15))]),
      h("div", { class: "page-body" }, tab === "positions" ? positions : all)),
    chatPanel());
}

// MARK: Top up

/** Where money can come in, as the Mac's top-up lists them: Solana first (the cheapest, where ihatecrypto trades from),
 * then the EVM networks with Ethereum last. Hyperliquid is reached through ihatecrypto, not by sending to it. */
const topUpNetworks = [
  { key: "solana", title: "Solana", coin: "SOL", logo: "sol", usdc: "USDC" },
  { key: "base", title: "Base", coin: "ETH", logo: "base", chain: 8453 },
  { key: "arbitrum", title: "Arbitrum", coin: "ETH", logo: "arb", chain: 42161 },
  { key: "optimism", title: "Optimism", coin: "ETH", logo: "op", chain: 10 },
  { key: "polygon", title: "Polygon", coin: "POL", logo: "pol", chain: 137 },
  { key: "bnb", title: "BNB Chain", coin: "BNB", logo: "bnb", chain: 56 },
  { key: "hyperevm", title: "HyperEVM", coin: "HYPE", logo: "hyperevm", chain: 999 },
  { key: "robinhood", title: "Robinhood Chain", coin: "ETH", logo: "robinhood", chain: 4663, usdc: "USDG" },
  { key: "tempo", title: "Tempo", coin: null, logo: "tempo", chain: 4217 },
  { key: "ethereum", title: "Ethereum", coin: "ETH", logo: "eth", chain: 1, cost: () => t("fee 1–20¢ · lands in ~15 s", "комиссия 1–20¢ · дойдёт за ~15 с") },
];
const solanaUSDC = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

export function topUpPage() {
  const s = ui.state;
  const nets = topUpNetworks.filter((n) => (n.key === "solana" ? s.addresses.solana : s.addresses.evm));
  if (!nets.length) {
    return h("div", { class: "page-pane", style: `left:${railEdge()}px` }, h("div", { class: "page-scroll" },
      pageHeader(t("Top up", "Пополнить")), h("div", { class: "page-body" }, empty(t("Add a wallet first: Settings › Wallet.", "Сначала добавь кошелёк: Настройки › Кошелёк.")))));
  }
  const net = nets.find((n) => n.key === store("topUpNetwork")) || nets[0];
  const way = store("topUpWay") === "exchange" ? "exchange" : "phone";
  let coin = store("topUpCoin") || "usdc";
  if (coin === "native" && !net.coin) coin = "usdc";
  const usdcSymbol = net.usdc || "USDC", symbol = coin === "usdc" ? usdcSymbol : coin === "native" ? net.coin : null;
  const solana = net.key === "solana";
  const recipient = solana ? s.addresses.solana : s.addresses.evm;
  const cost = net.cost ? net.cost() : t("fee under 1¢ · lands in seconds", "комиссия меньше 1¢ · дойдёт за секунды");
  // What the wallet holds there now, from the portfolio's last read.
  const here = s.portfolio.networks.find((n) => n.key === net.key);
  const held = (sym) => here?.rows.find((r) => r.symbol === sym);
  const gasRow = net.coin ? held(net.coin) : null;
  const gasShort = coin === "usdc" && net.coin && (!gasRow || (gasRow.usd ?? 0) < 0.1);
  // A phone wallet's camera takes a link with the wallet and, on Solana, the coin filled in; an exchange wants the bare address.
  const qrText = way === "exchange" ? recipient : solana
    ? `solana:${recipient}?` + (coin === "usdc" ? `spl-token=${solanaUSDC}&` : "") + "label=ihatecrypto&message=Top%20up"
    : `ethereum:${recipient}@${net.chain}`;
  const pickCoin = (c) => { store("topUpCoin", c); render(); };
  const chip = (label, on, run) => h("button", { class: "page-tu-chip" + (on ? " on" : ""), onclick: run }, label);
  const tileWay = (id, ico, title, detail, price) => h("button", { class: "page-tu-way" + (way === id ? " on" : ""), onclick: () => { store("topUpWay", id); render(); } },
    h("div", { class: "page-tu-way-top" }, icon(ico, 15), h("b", {}, title)),
    h("div", { class: "secondary one-line" }, detail), h("div", { class: "one-line " + (id === "exchange" ? "secondary" : "profit") }, price));
  const [title, detail] = way === "exchange"
    ? [t(`Withdraw to this address on ${net.title}`, `Выведи на этот адрес в сети ${net.title}`),
      solana ? t("USDC, SOL or any Solana token (SPL). USDC is what ihatecrypto trades with; keep ≈ 0.02 SOL for fees.", "USDC, SOL или любой токен Solana (SPL). ihatecrypto торгует на USDC; держи ≈ 0,02 SOL на комиссии.")
        : t(`${usdcSymbol}${net.coin ? ", " + net.coin : ""} or any token on ${net.title}. The same 0x address is this wallet on every EVM network ihatecrypto uses.`,
          `${usdcSymbol}${net.coin ? ", " + net.coin : ""} или любой токен в ${net.title}. Тот же 0x-адрес — этот кошелёк во всех EVM-сетях ihatecrypto.`)]
    : [t("Scan with your phone's wallet", "Отсканируй кошельком в телефоне"),
      (solana ? "Phantom, Solflare, Backpack: " : "MetaMask, Rainbow, Coinbase Wallet: ")
        + (solana && symbol ? t(`${symbol} to this wallet on ${net.title} is filled in, the wallet asks how much.`, `${symbol} на этот кошелёк в ${net.title} уже подставлен, сколько — спросит кошелёк.`)
          : t(`this wallet on ${net.title} is filled in, you pick the coin and the amount.`, `этот кошелёк в ${net.title} уже подставлен, монету и сумму выбираешь ты.`))];
  const now = symbol && held(symbol);
  const card = h("div", { class: "page-topup card" },
    h("div", { class: "page-tu-head" }, h("span", { class: "accent-ink" }, icon("arrow.down.circle", 20)), h("b", {}, t("Top up ihatecrypto", "Пополнить ihatecrypto")),
      h("span", { class: "spacer" }), h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: back }, icon("xmark", 14))),
    h("div", { class: "page-tu-coins" },
      h("span", { class: "page-tu-symbol" }, symbol || t("Other token", "Другой токен")),
      h("span", { class: "spacer" }),
      chip(usdcSymbol, coin === "usdc", () => pickCoin("usdc")),
      net.coin ? chip(net.coin, coin === "native", () => pickCoin("native")) : null,
      chip(t("Other", "Другой"), coin === "any", () => pickCoin("any"))),
    h("div", { class: "secondary small" }, coin === "any"
      ? t(`Any token on ${net.title} lands at this wallet's address: ${usdcSymbol}, ${net.coin || "stablecoins"}, memecoins…`, `Любой токен в ${net.title} приходит на адрес этого кошелька: ${usdcSymbol}, ${net.coin || "стейблкоины"}, мемкоины…`)
      : now ? t(`${now.amount} ${symbol} on ${net.title} now`, `Сейчас ${now.amount} ${symbol} в ${net.title}`) : t(`No ${symbol} on ${net.title} yet`, `${symbol} в ${net.title} пока нет`)),
    h("div", { class: "page-tu-nets", "data-keep": "page-topup-nets" }, nets.map((n) => h("button", { class: "page-tu-net" + (n === net ? " on" : ""), title: n.key === "solana" ? t("Recommended: the cheapest and fastest to top up, and where ihatecrypto trades from", "Рекомендую: дешевле и быстрее всего, и отсюда ihatecrypto торгует") : "",
      onclick: () => { store("topUpNetwork", n.key); render(); } }, logo(n.logo, 16), n.title, n.key === "solana" ? h("span", { class: "page-tu-best" }, "BEST") : null))),
    h("div", { class: "page-tu-ways" },
      tileWay("phone", "macwindow", t("Phone wallet", "Кошелёк в телефоне"), t("scan, the wallet asks how much", "сканируй, сумму спросит кошелёк"), cost),
      tileWay("exchange", "globe", t("Exchange", "Биржа"), t("Binance, OKX, Bybit… by address", "Binance, OKX, Bybit… по адресу"), t("the exchange's withdrawal fee", "комиссия биржи за вывод"))),
    h("div", { class: "page-tu-body" },
      h("div", { class: "page-tu-qr", title: way === "exchange" ? t("Scan with the exchange's app", "Сканируй приложением биржи") : t("Scan with your phone's wallet", "Сканируй кошельком в телефоне") },
        qr(qrText), h("span", { class: "page-tu-qr-logo" }, logo(net.logo, 26))),
      h("div", { class: "page-tu-text" }, h("b", {}, title), h("div", { class: "small" }, detail), addressLines(recipient))),
    way === "exchange" ? h("div", { class: "page-tu-warn pg-jev-ink" }, icon("exclamationmark.triangle", 13),
      h("span", {}, t(`Pick the network exactly: ${solana ? "Solana (SPL)" : net.title}. Sent on another network, money can get stuck. Many exchanges withdraw USDC on Solana or Base for well under a dollar.`,
        `Выбери сеть точно: ${solana ? "Solana (SPL)" : net.title}. Отправленные в другой сети деньги могут застрять. Многие биржи выводят USDC в Solana или Base гораздо дешевле доллара.`))) : null,
    way === "exchange" && !solana ? h("div", { class: "page-tu-warn secondary" }, icon("questionmark.circle", 13),
      h("span", {}, t("Hyperliquid uses the same 0x address, but money gets there through ihatecrypto. Never send to Hyperliquid's old bridge contract.",
        "У Hyperliquid тот же 0x-адрес, но деньги туда попадают через ihatecrypto. Никогда не отправляй на старый бридж-контракт Hyperliquid."))) : null,
    gasShort ? h("div", { class: "page-tu-gas" }, h("span", { class: "accent-ink" }, icon("exclamationmark.triangle", 13)),
      h("span", {}, solana ? t("Send ≈ $1 of SOL too: every move on Solana pays its fee in SOL. Without it, from $5 of USDC the chat swaps a part for SOL, Jupiter paying that one fee.",
        "Отправь ещё ≈ $1 в SOL: каждое движение в Solana платит комиссию в SOL. Без него от $5 в USDC чат поменяет часть на SOL, и эту одну комиссию заплатит Jupiter.")
        : t(`Send ≈ $1 of ${net.coin} too: moves on ${net.title} pay their fee in ${net.coin}, so ${usdcSymbol} alone can't move there.`,
          `Отправь ещё ≈ $1 в ${net.coin}: движения в ${net.title} платят комиссию в ${net.coin}, одни ${usdcSymbol} там не сдвинутся.`)),
      h("span", { class: "spacer" }), h("button", { class: "btn small", onclick: () => pickCoin("native") }, t(`Top up ${net.coin}`, `Пополнить ${net.coin}`))) : null,
    h("div", { class: "secondary small" }, s.portfolio.updatedAt ? t("Balances read ", "Балансы прочитаны ") + when(s.portfolio.updatedAt) + " · " : "",
      h("button", { class: "link-btn", onclick: () => act("refresh") }, t("Refresh", "Обновить"))));
  return h("div", { class: "page-pane", style: `left:${railEdge()}px` }, h("div", { class: "page-scroll", "data-keep": "topup" }, h("div", { class: "page-topup-place" }, card)), chatPanel());
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

/** A QR code (qr.js) drawn as squares on white, with a quiet edge for the camera. */
function qr(text) {
  const m = qrMatrix(text);
  const size = m.length, q = 2;
  let d = "";
  m.forEach((row, y) => row.forEach((dark, x) => { if (dark) d += `M${x + q} ${y + q}h1v1h-1z`; }));
  return svg("svg", { viewBox: `0 0 ${size + 2 * q} ${size + 2 * q}`, class: "page-qr", "shape-rendering": "crispEdges" },
    svg("rect", { width: size + 2 * q, height: size + 2 * q, fill: "#fff" }), svg("path", { d, fill: "#000" }));
}
