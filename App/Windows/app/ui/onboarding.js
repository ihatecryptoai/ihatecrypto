// The first run and the sheets over the dotted ground, as the Mac draws them (Welcome.swift, Onboarding.swift): the
// first screen (C01: the logo, the slogan, a request and the route it becomes), the onboarding card every step sits in,
// the recovery words with their check, and the small dialogs — importing, watching, renaming, opening a site.

import { ui, act, render, t, h, icon, mascot, logo, toast, invoke } from "./core.js";

let protectedOn = false;
/** While the recovery words are on screen the window is left out of screenshots and screen sharing. */
export function setProtect(on) {
  if (on === protectedOn) return;
  protectedOn = on;
  invoke("protect", { on }).catch(() => {});
}

// MARK: Pieces Settings uses too

/** Drawn segments with the accent on the chosen one, as the Mac's theme and Touch ID pickers. */
export function segmented(options, value, pick, cls = "") {
  return h("div", { class: "ob-seg " + cls, role: "radiogroup" }, options.map(([v, label]) =>
    h("button", { class: v === value ? "on" : "", role: "radio", "aria-checked": String(v === value), onclick: () => pick(v) }, label)));
}

/** The accent switch (AccentSwitch). */
export function toggle(id, on, change, disabled) {
  return h("label", { class: "ob-switch" + (disabled ? " off" : "") },
    h("input", { type: "checkbox", id, checked: on, disabled, onchange: (e) => change(e.target.checked) }), h("span", {}));
}

/** One step's card: the coin with the step's face, a title and a line, what the step asks, then a quiet way back on the
 * left and the step's one orange button on the right. */
export function stepCard({ mood = "idle", title, subtitle, content, primary, onPrimary, enabled = true, quiet }) {
  return h("div", { class: "ob-card" },
    h("div", { class: "ob-head" }, mascot(56, mood),
      h("div", {}, h("div", { class: "ob-title" }, title), subtitle ? h("div", { class: "ob-sub" }, subtitle) : null)),
    content,
    h("div", { class: "ob-foot" }, quiet || null, h("span", { class: "spacer" }),
      primary ? h("button", { class: "ob-primary", disabled: !enabled, onclick: onPrimary }, primary) : null));
}

const quietButton = (label, run) => h("button", { class: "ob-quiet", onclick: run }, label);
const statusLine = (text, ok) => h("div", { class: "ob-status " + (ok === true ? "profit" : ok === false ? "loss" : "secondary") },
  icon(ok === true ? "check" : ok === false ? "exclamationmark.triangle" : "questionmark.circle", 14), h("span", {}, text));
const factRow = (name, title, line) => h("div", { class: "ob-fact" }, h("span", { class: "ob-fact-ico" }, icon(name, 17)),
  h("div", {}, h("b", {}, title), h("div", { class: "secondary" }, line)));

/** A sheet over the dotted ground, centered, scrolled when the window is short. */
function sheet(card) { return h("div", { class: "ob-page" }, brandCorner(), h("div", { class: "ob-column" }, card)); }
const brandCorner = () => h("div", { class: "ob-brand" }, mascot(28), h("span", {}, "ihatecrypto"));

function setOverlay(el) { ui.overlay = el; render(); }
function closeOverlay() { ui.overlay = null; render(); }

// MARK: The first screen

/** Which step of the first run is open: the first screen, a new wallet, or bringing one. */
let first = { step: "welcome", scenario: 0, test: false, way: "phrase" };

const scenarios = [
  { chip: ["Buy Tesla on Robinhood", "Купить Tesla на Robinhood"], icon: "bolt", order: ["buy $10 of Tesla on Robinhood", "купи теслу на $10 на robinhood"],
    steps: [["FROM · BASE", "10 USDC", "base"], ["BRIDGE · RELAY", "Base → Robinhood", "relay"], ["BUY · ROBINHOOD", "$10 → TSLA", "robinhood"]] },
  { chip: ["Send from Solana", "Отправить с Solana"], icon: "arrow.right", order: ["send 20 USDC from Solana to Bybit", "отправь 20 USDC с solana на bybit"],
    steps: [["FROM · SOLANA", "20 USDC", "solana"], ["CHECK · ADDRESS BOOK", "Bybit", "check"], ["TO · BYBIT", "20 USDC → Bybit", "solana"]] },
  { chip: ["Short oil by the book", "Шорт нефти по правилам"], icon: "chart.pie", order: ["short oil on a 2% pullback by the book", "шорт нефти на откате 2% по правилам"],
    steps: [["CHECK · RISK RULES", "2% of the account", "check"], ["OPEN SHORT · OIL", "Short CL", "hyperliquid"], ["PROTECT · STOP", "Stop and take", "hyperliquid"]] },
];

/** The first run's two ways in, also for a part that wants them (the preview opens them by name). */
export function startNewWallet() { first.step = "new"; render(); }
export function startBring(way = first.way) { first.step = "bring"; first.way = way; render(); }

export function welcome() {
  if (first.step === "new") return newWalletStep();
  if (first.step === "bring") return bringStep();
  const ru = ui.state.russian, pick = (en, r) => (ru ? r : en);
  const s = scenarios[first.scenario];
  return h("div", { class: "ob-welcome" },
    h("button", { class: "ob-welcome-look", onclick: showWatch }, t("Only watch an address", "Только смотреть адрес")),
    h("div", { class: "ob-welcome-column" },
      h("div", { class: "ob-welcome-logo" }, mascot(84), h("span", {}, "ihatecrypto")),
      h("div", { class: "ob-welcome-slogan" }, t("Even crypto hates crypto.", "Даже крипта ненавидит крипту.")),
      h("div", { class: "ob-welcome-input glass" },
        h("span", { class: "one-line" }, pick(...s.order)), h("span", { class: "ob-caret" }),
        h("span", { class: "spacer" }), icon("mic", 17), h("span", { class: "ob-welcome-send" }, icon("arrow.up", 16))),
      h("div", { class: "ob-welcome-chips" }, scenarios.map((x, i) => h("button", { class: i === first.scenario ? "on" : "", onclick: () => { first.scenario = i; render(); } },
        icon(x.icon, 14), pick(...x.chip)))),
      h("div", { class: "ob-welcome-route" }, s.steps.map(([label, title, fill], i) => [
        i ? h("span", { class: "ob-welcome-arrow" }, icon("arrow.right", 18)) : null,
        h("div", { class: "ob-welcome-step ob-fill-" + fill }, h("div", { class: "ob-welcome-step-pic" }, mascot(30, fill === "check" ? "done" : "idle")),
          h("div", { class: "ob-welcome-step-body" }, h("div", { class: "eyebrow" }, label), h("b", {}, title)))])),
      ui.state.signer.error ? h("div", { class: "ob-status loss" }, ui.state.signer.error) : null,
      h("div", { class: "ob-welcome-buttons" },
        h("button", { class: "ob-primary", onclick: startNewWallet }, t("Get started", "Начать")),
        h("button", { class: "ob-secondary", onclick: () => startBring() }, t("I already have a wallet", "У меня уже есть кошелёк"))),
      h("div", { class: "ob-welcome-lock secondary" }, icon("lock", 14),
        t("Keys stay sealed by Windows Hello and this PC's TPM", "Ключи запечатаны Windows Hello и TPM этого компьютера"))));
}

function newWalletStep() {
  return sheet(stepCard({
    mood: "idle", title: t("A new wallet", "Новый кошелёк"),
    subtitle: t("Made inside the separate signer on this PC, which has no network access.", "Создаётся внутри отдельного подписанта на этом компьютере, у которого нет доступа к сети."),
    content: h("div", { class: "ob-content" },
      factRow("doc", t("12 recovery words", "12 слов восстановления"), t("Shown once, for paper: the only way back to this wallet.", "Показываются один раз, для бумаги: единственный путь обратно к кошельку.")),
      factRow("lock", t("Sealed by Windows Hello and the TPM", "Запечатан Windows Hello и TPM"), t("The key never leaves the signer; nothing can read it back, ihatecrypto included.", "Ключ не покидает подписанта; прочитать его не может никто, даже ihatecrypto.")),
      factRow("person", first.test ? t("Signs without asking", "Подписывает без вопросов") : t("Windows Hello for every move", "Windows Hello на каждое движение"),
        first.test ? t("Keep only a few dollars on it.", "Держи на нём только несколько долларов.") : t("Your face, fingerprint or PIN before money moves.", "Лицо, отпечаток или PIN, прежде чем деньги двинутся.")),
      h("div", { class: "ob-tile" }, h("div", {}, h("b", {}, t("Test wallet: signs without asking", "Тестовый кошелёк: подписывает без вопросов")),
        h("div", { class: "secondary small" }, t("For trying things out with a few dollars.", "Чтобы попробовать с парой долларов."))),
        h("span", { class: "spacer" }), toggle("ob-test", first.test, (on) => { first.test = on; render(); }))),
    primary: first.test ? t("Create a test wallet", "Создать тестовый") : t("Create the wallet", "Создать кошелёк"),
    onPrimary: async () => { if (await act("createWallet", { test: first.test }) !== null) first = { ...first, step: "welcome", test: false }; },
    quiet: quietButton(t("Back", "Назад"), () => { first.step = "welcome"; render(); }),
  }));
}

/** Bringing a wallet: a phrase or a key goes only to the signer; a Ledger keeps its keys; an address only watches. */
const bring = { phrase: "", key: "", solana: "", evm: "" };

function bringStep() {
  const way = first.way;
  // What was typed survives the states the engine sends meanwhile: the field is filled again from `bring`.
  const field = (id, key, placeholder, secret) => {
    const el = h(secret ? "textarea" : "input", {
      id, class: "address ob-field" + (secret ? " secret" : ""), rows: secret ? "3" : null, placeholder, autocomplete: "off", spellcheck: "false",
      oninput: (e) => { bring[key] = e.target.value; document.querySelector(".ob-primary")?.toggleAttribute("disabled", !ready()); },
    });
    el.value = bring[key];
    return el;
  };
  const ready = () => way === "phrase" ? bring.phrase.trim().split(/\s+/).length >= 12 : way === "key" ? bring.key.trim().length > 30
    : way === "ledger" ? true : !!(bring.solana.trim() || bring.evm.trim());
  const ledger = ui.state.ledgerConnect;
  const body = {
    phrase: [field("bring-phrase", "phrase", t("12 or 24 words separated by spaces", "12 или 24 слова через пробел"), true),
      h("div", { class: "secondary small" }, t("The same words open the same addresses here as in Phantom and MetaMask: Solana, every EVM network and NEAR.",
        "Те же слова открывают здесь те же адреса, что в Phantom и MetaMask: Solana, все EVM-сети и NEAR."))],
    key: [field("bring-key", "key", t("Solana key from Phantom, or EVM key from MetaMask", "Ключ Solana из Phantom или EVM-ключ из MetaMask"), true),
      h("div", { class: "secondary small" }, t("Phantom: Settings › Manage accounts › Show private key. MetaMask: Account details › Show private key. A key opens one network only.",
        "Phantom: Настройки › Управление аккаунтами › Показать приватный ключ. MetaMask: Детали аккаунта › Показать приватный ключ. Ключ открывает только одну сеть."))],
    ledger: [factRow("link", t("USB or Bluetooth", "USB или Bluetooth"), t("Nano X, Stax and Flex work over Bluetooth; any Ledger over USB.", "Nano X, Stax и Flex работают по Bluetooth; любой Ledger — по USB.")),
      factRow("lock", t("The keys stay on the device", "Ключи остаются на устройстве"), t("Every move shows on its screen and waits for its buttons.", "Каждое движение показывается на его экране и ждёт его кнопок.")),
      ledger ? h("div", {}, statusLine(ledger.status, ledger.problem ? false : null), ledger.problem ? statusLine(ledger.problem, false) : null,
        ledger.notes.map((n) => h("div", { class: "secondary small" }, n))) : null],
    address: [field("bring-solana", "solana", t("Solana address", "Адрес Solana")),
      field("bring-evm", "evm", t("EVM address (0x…): Base, Arbitrum, Hyperliquid and the rest", "EVM-адрес (0x…): Base, Arbitrum, Hyperliquid и остальные")),
      h("div", { class: "secondary small" }, t("Balances only: nothing can be signed with an address. Never paste a recovery phrase or key here.",
        "Только балансы: адресом ничего не подписать. Никогда не вставляй сюда фразу восстановления или ключ."))],
  }[way];
  const go = async () => {
    if (way === "ledger") { await act("connectLedger"); return; }
    const r = way === "phrase" ? await act("importPhrase", { phrase: bring.phrase.trim().toLowerCase().split(/\s+/).join(" ") })
      : way === "key" ? await act("importKey", { key: bring.key.trim() })
      : await act("watch", { solana: bring.solana.trim(), evm: bring.evm.trim() });
    if (r !== null) { Object.assign(bring, { phrase: "", key: "", solana: "", evm: "" }); first.step = "welcome"; ui.view = "canvas"; render(); }
  };
  const card = stepCard({
    mood: "tuning", title: t("Bring your wallet", "Принеси свой кошелёк"),
    subtitle: t("From Phantom, MetaMask or any wallet. It goes only to the separate signer, which seals it with Windows Hello and this PC's TPM.",
      "Из Phantom, MetaMask или любого кошелька. Он уходит только отдельному подписанту, который запечатывает его Windows Hello и TPM этого компьютера."),
    content: h("div", { class: "ob-content" },
      segmented([["phrase", t("Recovery phrase", "Фраза")], ["key", t("Private key", "Ключ")], ["ledger", "Ledger"], ["address", t("Address only", "Только адрес")]],
        way, (v) => { first.way = v; render(); }, "wide"), body),
    primary: { phrase: t("Find my wallet", "Найти кошелёк"), key: t("Find my wallet", "Найти кошелёк"), ledger: t("Connect a Ledger", "Подключить Ledger"), address: t("Watch these", "Смотреть") }[way],
    onPrimary: go, enabled: ready() && !(way === "ledger" && ledger?.busy),
    quiet: quietButton(t("Back", "Назад"), () => { first.step = "welcome"; render(); }),
  });
  return sheet(card);
}

// MARK: The recovery words

let copied = false;
/** The check after «I wrote them down»: three numbers to pick the words for, from the paper. */
let check = null;

export function phraseScreen() {
  setProtect(true);
  const words = ui.state.phrase;
  if (check && check.key !== words.join(" ")) check = null;
  if (check) return sheet(checkCard(words));
  return sheet(stepCard({
    mood: "waiting", title: t(`Your ${words.length} recovery words`, `Твои ${words.length} слов восстановления`),
    subtitle: t("Write them on paper, in order. They are the only way back to this wallet if this PC is lost or wiped; ihatecrypto won't show them again.",
      "Запиши их на бумагу по порядку. Это единственный путь обратно к кошельку, если компьютер потеряется или будет стёрт; ihatecrypto больше их не покажет."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-words" }, words.map((w, i) => h("div", { class: "ob-word" }, h("span", { class: "ob-n" }, String(i + 1)), h("b", {}, w)))),
      statusLine(t("Never type them into a website, a chat or a photo. Anyone with these words owns the money.",
        "Никогда не вводи их на сайтах, в чатах и не фотографируй. Кто знает эти слова, тот владеет деньгами."), false),
      h("div", { class: "btns" },
        h("button", { class: "btn", disabled: copied, onclick: async (e) => {
          copied = true;
          e.currentTarget.disabled = true;
          try { await invoke("copy_secret", { text: words.join(" ") }); toast(t("Copied once; the clipboard clears in 60 s.", "Скопировано один раз; буфер очистится через 60 с.")); }
          catch { toast(t("Couldn't copy.", "Не скопировалось.")); }
          render();
        } }, icon(copied ? "check" : "copy", 14), copied ? t("Copied", "Скопировано") : t(`Copy all ${words.length} words`, `Скопировать все ${words.length} слов`)),
        h("span", { class: "secondary small" }, t("For a password manager: cleared from the clipboard in 60 s, kept out of clipboard history.",
          "Для менеджера паролей: буфер очистится через 60 с, в историю буфера не попадёт.")))),
    primary: t("I wrote them down", "Я записал"),
    onPrimary: () => {
      const pool = [...words.keys()].sort(() => Math.random() - 0.5);
      check = { key: words.join(" "), asked: pool.slice(0, 3).sort((a, b) => a - b), picked: {} };
      render();
    },
  }));
}

function checkCard(words) {
  const done = check.asked.every((i) => check.picked[i] === words[i]);
  return stepCard({
    mood: done ? "done" : "tuning", title: t("Check three of them", "Проверь три из них"),
    subtitle: t("Pick the words by their numbers, from your paper.", "Выбери слова по их номерам — с бумаги."),
    content: h("div", { class: "ob-content" }, check.asked.map((i) => {
      // The right word among three others from the phrase, in a fixed order per word so a redraw doesn't shuffle them.
      const others = words.filter((w, j) => j !== i && w !== words[i]);
      const choices = [...new Set([words[i], others[(i * 7) % others.length], others[(i * 11 + 3) % others.length], others[(i * 5 + 1) % others.length]])]
        .sort((a, b) => ((a.charCodeAt(0) * 31 + i) % 17) - ((b.charCodeAt(0) * 31 + i) % 17));
      const wrong = check.picked[i] && check.picked[i] !== words[i];
      return h("div", { class: "ob-check-word" }, h("div", { class: "eyebrow" }, t(`Word ${i + 1}`, `Слово ${i + 1}`)),
        h("div", { class: "btns" }, choices.map((w) => h("button", { class: "btn" + (check.picked[i] === w ? " on" : ""), onclick: () => { check.picked[i] = w; render(); } }, w))),
        wrong ? h("div", { class: "loss small" }, t("Not this one: look at the paper again.", "Не это: посмотри на бумагу ещё раз.")) : null);
    })),
    primary: t("Done", "Готово"), enabled: done,
    onPrimary: async () => { check = null; copied = false; await act("phraseSaved"); setProtect(false); },
    quiet: quietButton(t("Show the words again", "Показать слова снова"), () => { check = null; render(); }),
  });
}

// MARK: Dialogs

export function showImport() { importDialog(t("Import a recovery phrase", "Импорт фразы восстановления"), t("12 or 24 words, in order, from MetaMask, Phantom or another wallet.", "12 или 24 слова по порядку — из MetaMask, Phantom или другого кошелька."), "importPhrase", "phrase"); }
export function showImportKey() { importDialog(t("Import a private key", "Импорт приватного ключа"), t("One key from Phantom (Solana) or MetaMask (EVM): it opens that network only.", "Один ключ из Phantom (Solana) или MetaMask (EVM): он открывает только эту сеть."), "importKey", "key"); }

function importDialog(title, hint, op, field) {
  let value = "";
  const done = () => { value = ""; closeOverlay(); };
  const run = async () => { const r = await act(op, { [field]: field === "phrase" ? value.trim().toLowerCase().split(/\s+/).join(" ") : value.trim() }); if (r !== null) done(); };
  setOverlay(sheet(stepCard({
    mood: "tuning", title, subtitle: hint + " " + t("It goes straight to the signer, which keeps it under Windows Hello; nothing else keeps it.",
      "Она уходит прямо подписанту, который хранит её под Windows Hello; больше нигде она не хранится."),
    content: h("div", { class: "ob-content" }, h("textarea", { id: "import-secret", class: "address ob-field secret", rows: "4", autocomplete: "off", spellcheck: "false", oninput: (e) => (value = e.target.value) })),
    primary: t("Import", "Импортировать"), onPrimary: run,
    quiet: quietButton(t("Back", "Назад"), done),
  })));
  setTimeout(() => document.getElementById("import-secret")?.focus(), 0);
}

/** Read-only addresses, as the Mac's «Addresses for balances»: their balances show, and nothing signs for them. Kept
 * across the states the engine sends while the person types; read again from the engine after a save. */
const watchForm = { solana: null, evm: null };

/** The two address fields and their buttons; `back` puts them in a sheet with a way back. */
export function watchFields(back) {
  const form = watchForm;
  if (form.solana === null) form.solana = ui.state.watched.solana || "";
  if (form.evm === null) form.evm = ui.state.watched.evm || "";
  const field = (key, label, hint) => h("div", { class: "field" }, h("label", {}, label),
    h("input", { id: "watch-" + key, class: "address", value: form[key], placeholder: hint, autocomplete: "off", spellcheck: "false", oninput: (e) => (form[key] = e.target.value.trim()) }));
  const save = async () => {
    if (!form.solana && !form.evm) { toast(t("Paste at least one address.", "Вставь хотя бы один адрес.")); return; }
    if (await act("watch", { solana: form.solana, evm: form.evm }) === null) return;
    form.solana = form.evm = null;
    if (back) { ui.overlay = null; ui.view = "canvas"; first.step = "welcome"; }
    render();
  };
  return { save, fields: h("div", { class: "ob-watch" }, field("solana", "Solana", t("Solana address", "Адрес Solana")), field("evm", "EVM · Hyperliquid", "0x…")),
    stop: ui.state.watching ? async () => { form.solana = form.evm = null; await act("watch", { solana: "", evm: "" }); } : null };
}

export function showWatch() {
  const w = watchFields(true);
  setOverlay(sheet(stepCard({
    mood: "idle", title: t("Watch an address", "Смотреть адрес"),
    subtitle: t("Read-only: the balances show, nothing can be sent or signed. Never paste a recovery phrase or key here.",
      "Только просмотр: балансы видны, отправить или подписать ничего нельзя. Никогда не вставляй сюда фразу восстановления или ключ."),
    content: h("div", { class: "ob-content" }, w.fields),
    primary: t("Watch", "Смотреть"), onPrimary: w.save,
    quiet: quietButton(t("Back", "Назад"), closeOverlay),
  })));
}

export function renameDialog(c) {
  let value = c.title;
  const save = async () => { await act("renameCanvas", { wallet: c.id, label: value }); closeOverlay(); };
  setOverlay(sheet(stepCard({
    mood: "tuning", title: t("Rename the canvas", "Переименовать холст"),
    content: h("div", { class: "ob-content" }, h("input", { id: "rename", class: "ob-field", value, oninput: (ev) => (value = ev.target.value), onkeydown: (ev) => { if (ev.key === "Enter") save(); } })),
    primary: t("Save", "Сохранить"), onPrimary: save,
    quiet: quietButton(t("Back", "Назад"), closeOverlay),
  })));
  setTimeout(() => { const f = document.getElementById("rename"); f?.focus(); f?.select(); }, 0);
}

/** ihatecrypto's browser: a site opens in a window of its own with the wallet in it; what it asks for comes here as a card. */
export function browserDialog() {
  let value = "";
  const sites = [["jup.ag", "jup"], ["kamino.com", "kmno"], ["raydium.io", "ray"], ["app.aave.com", "aave"], ["app.uniswap.org", "uni"], ["app.hyperliquid.xyz", "hype"]];
  const open = async (text) => {
    const url = /^https:\/\//.test(text) ? text : "https://" + text.replace(/^\w+:\/\//, "");
    try { await invoke("open_browser", { url }); closeOverlay(); } catch (e) { toast(String(e)); }
  };
  setOverlay(sheet(stepCard({
    mood: "tuning", title: t("Open a site with your wallet", "Открыть сайт со своим кошельком"),
    subtitle: t("The site sees your address only after you connect it, and every signature it asks for comes to the canvas as a card: nothing is signed without Confirm and Windows Hello. Sites on MetaMask's phishing list get no wallet.",
      "Сайт увидит адрес, только когда ты его подключишь, а каждая подпись, которую он просит, приходит на холст карточкой: без «Подтвердить» и Windows Hello ничего не подписывается. Сайты из списка фишинга MetaMask кошелёк не получают."),
    content: h("div", { class: "ob-content" },
      h("input", { id: "site-url", class: "ob-field", placeholder: "jup.ag", oninput: (e) => (value = e.target.value.trim()), onkeydown: (e) => { if (e.key === "Enter" && value) open(value); } }),
      h("div", { class: "ob-site-chips" }, sites.map(([s, l]) => h("button", { class: "ob-site-chip", onclick: () => open(s) }, logo(l, 18), s)))),
    primary: t("Open", "Открыть"), onPrimary: () => value && open(value),
    quiet: quietButton(t("Back", "Назад"), closeOverlay),
  })));
  setTimeout(() => document.getElementById("site-url")?.focus(), 0);
}
