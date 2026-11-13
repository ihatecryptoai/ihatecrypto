// The first run and the sheets over the dotted ground, as the Mac draws them:
// - Welcome.swift's first screen (C01: the logo, the slogan, a demo the coin cursor plays — a request, its answer, a move
//   laid out card by card and confirmed — in English, Spanish or Russian);
// - Onboarding.swift's staged flow after it, one card at a time with its rail (Model › Wallet › Secure › Networks ›
//   Top up › Trading), Windows Hello where the Mac says Touch ID;
// - the small dialogs: importing (with a BIP-39 passphrase and the first addresses compared), watching, renaming,
//   opening a site.
// The step is kept in this window's storage, so a quit comes back to it; the recovery words are the engine's and only in
// its memory, so after a restart they are gone, as on the Mac.

import { ui, act, engine, render, t, h, svg, icon, addIcons, mascot, logo, toast, invoke, store, qrCode, usd, masked, copyButton } from "./core.js";
import * as pages from "./pages.js";
import * as chat from "./chat.js";

addIcons({
  "ob.info": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5.5M12 7.5v.5",
  "ob.sparkles": "M10 3.5l1.6 4.4L16 9.5l-4.4 1.6L10 15.5l-1.6-4.4L4 9.5l4.4-1.6zM18 14l.8 2.2L21 17l-2.2.8L18 20l-.8-2.2L15 17l2.2-.8z",
  "ob.bell": "M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2H4.5zM10 20.5a2 2 0 0 0 4 0",
  "ob.chart": "M3.5 18.5l5-6 4 3.5 7.5-9M15 7h5v5",
  "ob.desktop": "M3.5 4.5h17v11h-17zM9 20h6M12 15.5V20",
  "ob.grid": "M6 6h.01M12 6h.01M18 6h.01M6 12h.01M12 12h.01M18 12h.01M6 18h.01M12 18h.01M18 18h.01",
  "ob.slider": "M4 7h9M17 7h3M4 17h3M11 17h9M15 4.5v5M9 14.5v5",
});

let protectedOn = false;
/** While the recovery words are on screen the window is left out of screenshots and screen sharing. */
export function setProtect(on) {
  if (on === protectedOn) return;
  protectedOn = on;
  invoke("protect", { on }).catch(() => {});
}

// The lock screen's part makes the 6-digit code (lock.js `codeMaker(onMade)`); without it the Passcode step only skips.
let lock = null;
import("./lock.js").then((m) => { lock = m; render(); }).catch(() => {});

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

/** One step's card (OnboardingCard): the coin with the step's face, a title and a line, what the step asks, then the quiet
 * ways back or past it on the left and the step's one orange button on the right. */
export function stepCard({ mood = "idle", title, subtitle, content, primary, onPrimary, enabled = true, busy = false, quiet }) {
  return h("div", { class: "ob-card" },
    h("div", { class: "ob-head" }, mascot(56, mood),
      h("div", {}, h("div", { class: "ob-title" }, title), subtitle ? h("div", { class: "ob-sub" }, subtitle) : null)),
    content,
    h("div", { class: "ob-foot" }, quiet || null, h("span", { class: "spacer" }), busy ? h("span", { class: "ob-spinner" }) : null,
      primary ? h("button", { class: "ob-primary", disabled: !enabled || busy, onclick: onPrimary }, primary) : null));
}

const quietButton = (label, run) => h("button", { class: "ob-quiet", onclick: run }, label);
const statusLine = (text, ok) => h("div", { class: "ob-status " + (ok === true ? "profit" : ok === false ? "loss" : "secondary") },
  icon(ok === true ? "check" : ok === false ? "exclamationmark.triangle" : "ob.info", 14), h("span", {}, text));
const factRow = (name, title, line) => h("div", { class: "ob-fact" }, h("span", { class: "ob-fact-ico" }, icon(name, 17)),
  h("div", {}, h("b", {}, title), h("div", { class: "secondary" }, line)));
/** A tinted row a step lists things in (Tile): full width, ringed when chosen. */
const tile = (on, ...children) => h("div", { class: "ob-tile" + (on ? " on" : "") }, ...children);
const tileText = (title, line, more) => h("div", { class: "ob-tile-text" }, h("b", {}, title), line ? h("div", { class: "secondary small" }, line) : null, more || null);
/** «Hfe3…Kpqk»: the ends of an address, as a confirm card shows them. */
const ends = (a) => (a && a.length > 14 ? a.slice(0, 6) + "…" + a.slice(-6) : a || "");

/** A sheet over the dotted ground, centered, scrolled when the window is short. */
function sheet(card) { return h("div", { class: "ob-page" }, brandCorner(), h("div", { class: "ob-column" }, card)); }
const brandCorner = () => h("div", { class: "ob-brand" }, mascot(28), h("span", {}, "ihatecrypto"));

// Return presses the step's orange button (the Mac's `.keyboardShortcut(.defaultAction)`), as long as the button is on
// and the key isn't for the field it was pressed in: a list, the API key's own Save, the code's digits. The flow's text
// areas hold one phrase or key, wrapped: Return never adds a line there, as in the Mac's one-line fields.
document.addEventListener("keydown", (e) => {
  if (e.key !== "Enter" || e.shiftKey || e.isComposing || !onboardingRunning() || ui.overlay) return;
  const at = e.target;
  if (at?.tagName === "SELECT" || at?.tagName === "BUTTON" || at?.closest?.(".ob-key-row, .lk-maker, .ob-local-panel")) return;
  if (at?.tagName === "TEXTAREA") e.preventDefault();
  const primary = document.querySelector(".ob-card .ob-primary");
  if (primary && !primary.disabled) { e.preventDefault(); primary.click(); }
});

function setOverlay(el) { ui.overlay = el; render(); }
function closeOverlay() { ui.overlay = null; render(); }

// MARK: The staged flow (Onboarding.swift)

const stageOf = { model: "model", newWallet: "wallet", words: "wallet", check: "wallet", bring: "wallet", found: "wallet", ledger: "wallet",
  secure: "secure", passcode: "secure", networks: "networks", money: "money", stayInTouch: "money", tradingKey: "trading", rules: "trading" };
const stages = () => [["model", t("Model", "Модель")], ["wallet", t("Wallet", "Кошелёк")], ["secure", t("Secure", "Защита")],
  ["networks", t("Networks", "Сети")], ["money", t("Top up", "Пополнение")], ["trading", t("Trading", "Торговля")]];

/** What the flow keeps in memory: the cards back, what an import opened, Top up's balances, Hyperliquid's deposit. */
const flow = { history: [], accounts: null, others: "only", money: null, funded: {}, tried: false, busy: false };
const step = () => store("onboardingStep") || "";

/** The flow is on: app.js draws it in place of the canvas (the Mac's ContentView: Settings › Onboarding › Welcome › Studio). */
export const onboardingRunning = () => !!stageOf[step()];
/** «Look around first», or the flow finished: the first screen is passed. */
export const welcomeDone = () => store("welcomeDone") === true;

/** The engine holds its chat offers (new modules, the trading key, gas) while the flow is on, as the Mac's offers check
 *  «onboardingStep»; told only when that changes. */
let toldRunning = false;
function tellEngine() {
  const on = onboardingRunning();
  if (on !== toldRunning) { toldRunning = on; act("onboarding", { test: on }); }
}
// Each step starts afresh when it comes again, as the Mac's step views are made anew (`switch step`): what the one being
// left had typed, picked or shown goes with it — Bring's phrase, passphrase, key and addresses out of the page, the model's
// unsaved key, the Test switch, a half-made code, a failed approve's line, Top up's balances.
function leave() {
  switch (step()) {
    case "model": flow.key = ""; flow.keyProvider = null; break;
    case "newWallet": flow.test = false; break;
    case "bring": Object.assign(bring, { way: "phrase", phrase: "", passphrase: "", key: "", keyFound: null, keyError: null, solana: "", evm: "" }); keyAsked++; break;
    case "passcode": lock?.resetMaker(); break;
    case "money": flow.money = null; break;
    case "tradingKey": flow.tried = false; flow.keyReply = null; break;
  }
}
function go(next) { leave(); flow.history.push(step()); store("onboardingStep", next); tellEngine(); render(); }
function back() { leave(); store("onboardingStep", flow.history.pop() || ""); tellEngine(); render(); }
function finish() {
  Object.assign(flow, { history: [], accounts: null, money: null, tried: false });
  store("onboardingStep", "");
  store("welcomeDone", true);
  tellEngine();
  ui.view = "canvas";
  render();
}
function start(path) { store("onboardingPath", path); flow.history = []; go("model"); }

const activeWallet = () => ui.state.wallets.find((w) => w.id === ui.state.active) || null;
const networks = () => ui.state.modules?.networks || [];
/** Whether the wallet (or a watched address) has an address there. */
function hasAddress(id) {
  const a = ui.state.addresses, key = id.replace(/^network\./, "");
  return key === "solana" ? !!a.solana : key === "near" ? !!a.near : !!a.evm;
}
/** Hyperliquid on, and a wallet that can sign there. */
const trades = () => !!networks().find((n) => n.id === "network.hyperliquid")?.on && !!activeWallet()?.evm;
/** Only an account that has had a deposit takes a trading key: the trading steps come last, and only then. */
const showsTrading = () => trades() && flow.funded[activeWallet()?.evm] === true;

/** The wallet's Hyperliquid deposit, asked once per address when Top up comes (nothing signs for it). */
function readFunded() {
  const evm = activeWallet()?.evm;
  if (!trades() || !evm || evm in flow.funded) return;
  flow.funded[evm] = null;
  act("hlFunded").then((r) => { flow.funded[evm] = r?.funded === "true"; render(); });
}

/** The flow's page: the logo, the rail of stages, the step's card. */
export function onboarding(card = stepView()) {
  tellEngine();
  return h("div", { class: "ob-page" },
    h("div", { class: "ob-top" }, h("div", { class: "ob-top-brand" }, mascot(28), h("span", {}, "ihatecrypto")), railView(), h("span", { class: "ob-top-pad" })),
    h("div", { class: "ob-column" }, card));
}

function railView() {
  const list = stages().filter(([id]) => id !== "trading" || showsTrading() || stageOf[step()] === "trading");
  const current = list.findIndex(([id]) => id === stageOf[step()]);
  return h("div", { class: "ob-rail", "aria-label": t("Step ", "Шаг ") + (list[current]?.[1] || "") },
    list.map(([id, title], i) => [
      i ? icon("chevron.right", 9) : null,
      h("span", { class: "ob-stop" + (i === current ? " now" : i < current ? " done" : "") },
        h("span", { class: "ob-stop-n" }, i < current ? icon("check", 10) : String(i + 1)), title)]));
}

function stepView() {
  const st = step();
  if (stageOf[st] === "money" || stageOf[st] === "trading") readFunded();
  switch (st) {
    case "model": return modelStep();
    case "newWallet": return newWalletStep();
    case "words": case "check": return wordsGoneStep();
    case "bring": return bringStep();
    case "found": return foundStep();
    case "ledger": return ledgerStep();
    case "secure": return secureStep();
    case "passcode": return passcodeStep();
    case "networks": return networksStep();
    case "money": return moneyStep();
    case "stayInTouch": return stayInTouchStep();
    case "tradingKey": return tradingKeyStep();
    case "rules": return rulesStep();
    default: return h("div");
  }
}

// Model: the chat needs one, so there's no Skip (the owner: no chat without a model).

function modelStep() {
  const s = ui.state, m = s.model;
  const prov = (id) => s.providers.find((p) => p.id === id);
  const connected = (id) => m.provider === id && m.connected;
  // Connected from this step, the account becomes the chat's (the Mac's Onboarding: `ws.providerID = id`): the person
  // picked it here to chat with. Claude without Claude Code's login on this PC says so, as the Mac's row does.
  const connect = async (p, login) => {
    if (p.id === "claude-code" && !p.existingLogin && !p.signIn) { toast(t("No sign-in found on this PC.", "Вход на этом компьютере не найден.")); return; }
    flow.busy = true;
    render();
    if (p.signIn && !login) toast(t("Finish the sign-in in the browser.", "Закончи вход в браузере."));
    const r = await act(p.signIn && !login ? "signIn" : "connectModel", { provider: p.id });
    if (r !== null) await act("useAccount", { provider: p.id });
    flow.busy = false;
    render();
  };
  const way = (id, title, line, file) => {
    const p = prov(id);
    if (!p) return null;
    const on = connected(id);
    // A login already on this PC is a link under the words, not a second button beside them.
    const login = !on && p.existingLogin && p.signIn
      ? h("button", { class: "ob-link", onclick: () => connect(p, true) }, t(p.existingLogin, "Взять вход, который уже есть на этом компьютере")) : null;
    return tile(on, logo(file, 30), tileText(title, p.unavailable || line, login), h("span", { class: "spacer" }),
      on ? h("span", { class: "ob-ok profit" }, icon("check", 14), t("Connected", "Подключено"))
        : p.unavailable ? null
        : h("button", { class: "btn small", disabled: flow.busy, onclick: () => connect(p) },
          p.signIn ? t(p.signIn, "Войти через браузер") : p.existingLogin ? t(p.existingLogin, "Взять вход с этого компьютера") : t("Connect", "Подключить")));
  };
  // Ways in by an API key; the local one has its own row, OpenRouter its sign-in.
  const keyed = s.providers.filter((p) => p.takesKey && !p.unavailable && p.id !== "openrouter");
  if (!keyed.some((p) => p.id === flow.keyProvider)) flow.keyProvider = keyed.find((p) => p.id === "openai")?.id || keyed[0]?.id;
  const keyRow = keyed.length ? tile(keyed.some((p) => connected(p.id)), h("span", { class: "ob-tile-ico" }, icon("lock", 16)),
    tileText(t("An API key", "API-ключ"), null, h("div", { class: "ob-key-row" },
      h("select", { id: "ob-key-provider", onchange: (e) => (flow.keyProvider = e.target.value) },
        keyed.map((p) => h("option", { value: p.id, selected: p.id === flow.keyProvider }, p.name))),
      h("input", { id: "ob-key", type: "password", autocomplete: "off", placeholder: t("Key", "Ключ"), value: flow.key || "", oninput: (e) => (flow.key = e.target.value) }),
      h("button", { class: "btn small", onclick: async () => {
        const key = (flow.key || "").trim();
        if (!key) { toast(t("Paste the key first.", "Сначала вставь ключ.")); return; }
        flow.key = "";
        if (await act("connectModel", { provider: flow.keyProvider, key }) !== null) { await act("useAccount", { provider: flow.keyProvider }); render(); }
      } }, t("Save", "Сохранить"))))) : null;
  const local = s.providers.find((p) => p.kind === "local");
  // The Mac's local row: the whole local-models panel — set up, start, switch to or download a model for this PC.
  const localRow = local ? tile(connected(local.id), h("span", { class: "ob-tile-ico" }, icon("ob.desktop", 16)),
    tileText(t("Free, on this PC", "Бесплатно, на этом компьютере"), null, h("div", { class: "ob-local-panel" }, pages.localModelsPanel(s)))) : null;
  // The person picks the model; the chat's menu changes it later. The catalog lists the provider's default first.
  const current = prov(m.provider);
  // The account's own list, read once when its picker first shows (the Mac refreshes it for the account picked).
  if (m.connected && current && current.kind !== "local" && flow.modelsFor !== current.id) { flow.modelsFor = current.id; act("refreshModels"); }
  const list = m.models.includes(m.model) || !m.model ? m.models : [...m.models, m.model];
  const picker = m.connected && current?.kind !== "local" && list.length ? h("div", { class: "ob-model-row" },
    h("b", {}, t("Model", "Модель")),
    h("select", { id: "ob-model", onchange: (e) => act("setModel", { model: e.target.value }) },
      list.map((id) => h("option", { value: id, selected: id === m.model }, (chat.modelName?.(id) || id) + (id === current?.defaultModel ? t(" · recommended", " · рекомендую") : "")))),
    h("span", { class: "spacer" }), h("span", { class: "secondary small" }, t("Change it any time in the chat's menu.", "Её можно сменить в меню чата."))) : null;
  return stepCard({
    mood: "tuning", title: t("What does the chat think with?", "Чем думает чат?"),
    subtitle: t("The chat needs a model to understand you: one you already pay for, an API key, or a free one on this PC. It reads your words, the plan and your balances, never your keys. A model on this PC keeps them all here.",
      "Чату нужна модель, чтобы понимать тебя: подписка, за которую ты уже платишь, API-ключ или бесплатная модель на этом компьютере. Она читает твои слова, план и балансы, но никогда не ключи. Модель на этом компьютере оставляет всё здесь."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-tiles" },
        way("chatgpt", t("ChatGPT subscription", "Подписка ChatGPT"), t("Plus or Pro: sign in once in the browser", "Plus или Pro: войти один раз в браузере"), "llm-openai"),
        way("claude-code", t("Claude subscription", "Подписка Claude"), t("Uses the Claude Code already on this PC", "Через Claude Code, который уже есть на этом компьютере"), "llm-claude"),
        way("openrouter", "OpenRouter", t("One key for many models: sign in in the browser", "Один ключ для многих моделей: вход в браузере"), "llm-openrouter"),
        keyRow, localRow),
      picker),
    primary: t("Continue", "Дальше"), enabled: m.connected, busy: flow.busy,
    onPrimary: () => go(store("onboardingPath") === "bring" ? "bring" : "newWallet"),
    quiet: quietButton(t("Back", "Назад"), back),
  });
}

// A new wallet, its words and their check.

/** The signer's own note when Windows Hello isn't set up (the signer's presence note, signer/src/host.rs): then only a test
 * wallet can be made, as nothing could ask before a move. The Mac adapts its texts to a Mac without Touch ID. */
export const noHello = () => (ui.state.signer?.notes || []).some((n) => n.startsWith("Windows Hello isn't set up"));
const helloMissing = () => factRow("person", t("Windows Hello isn't set up on this PC", "Windows Hello на этом компьютере не настроен"),
  t("So only a test wallet can be made, and a phrase or key can't be brought: nothing could ask before a move. Turn on a face, fingerprint or PIN in Windows Settings › Accounts › Sign-in options, then come back.",
    "Поэтому можно создать только тестовый кошелёк, а фразу или ключ не принести: перед движением некому было бы спросить. Включи лицо, отпечаток или PIN в «Параметры Windows › Учетные записи › Варианты входа» и вернись."));

function newWalletStep() {
  const hello = !noHello(), test = !!flow.test || !hello;
  return stepCard({
    mood: "idle", title: t("A new wallet", "Новый кошелёк"),
    subtitle: t("Made inside the separate signer on this PC, which has no network access.", "Создаётся внутри отдельного подписанта на этом компьютере, у которого нет доступа к сети."),
    content: h("div", { class: "ob-content" },
      factRow("doc", t("12 recovery words", "12 слов восстановления"), t("Made from this PC's random number generator. They are the only way back if the PC is lost. You see them once, next.",
        "Из генератора случайных чисел этого компьютера. Это единственный путь обратно, если компьютер потеряется. Ты увидишь их один раз, дальше.")),
      test ? factRow("lock", t("Encrypted for your Windows user", "Зашифрован для твоего пользователя Windows"), t("A test wallet's key never leaves the signer, but Windows Hello doesn't seal it, so it can sign with no one there.",
        "Ключ тестового кошелька не покидает подписанта, но Windows Hello его не запечатывает — поэтому он подписывает, даже когда никого нет."))
        : factRow("lock", t("Sealed by Windows Hello and the TPM", "Запечатан Windows Hello и TPM"), t("The key never leaves the signer. Nothing can read it back, ihatecrypto included.", "Ключ не покидает подписанта. Прочитать его не может никто, даже ihatecrypto.")),
      factRow("person", test ? t("Signs without asking", "Подписывает без вопросов") : t("Windows Hello for every move", "Windows Hello на каждое движение"),
        test ? t("A test wallet: keep only a few dollars on it.", "Тестовый кошелёк: держи на нём только несколько долларов.")
          : t("Every signature asks first: one face, fingerprint or PIN covers one move.", "Каждая подпись сначала спрашивает: одно лицо, отпечаток или PIN — на одно движение.")),
      hello ? tile(false, tileText(t("Test wallet: signs without asking", "Тестовый кошелёк: подписывает без вопросов"), t("For trying things out with a few dollars.", "Чтобы попробовать с парой долларов.")),
        h("span", { class: "spacer" }), toggle("ob-test", test, (on) => { flow.test = on; render(); }))
        : helloMissing(),
      ui.state.signer.error ? statusLine(ui.state.signer.error, false) : null),
    primary: test ? t("Create a test wallet", "Создать тестовый кошелёк") : t("Create a new wallet", "Создать кошелёк"), busy: flow.busy,
    onPrimary: async () => {
      flow.busy = true;
      render();
      const r = await act("createWallet", { test });
      flow.busy = false;
      // The engine holds the words now: app.js draws them (phraseScreen), inside this flow.
      if (r !== null) { flow.test = false; go("words"); } else render();
    },
    quiet: quietButton(t("Back", "Назад"), back),
  });
}

let copied = false;
/** The copied words off the clipboard if it still holds them (main.rs clear_secret); a build without it waits its 60 s. */
const wipeCopied = () => invoke("clear_secret").catch(() => {});
/** The check after «I wrote them down» (CheckStep): three of the words by number, picked in order from a few shuffled ones. */
let check = null;

/** The recovery words, while the engine holds them: as a step of the flow when it runs, a sheet of its own otherwise. */
export function phraseScreen() {
  setProtect(true);
  const words = ui.state.phrase;
  if (check && check.key !== words.join(" ")) check = null;
  const card = check ? checkCard(words) : wordsCard(words);
  return onboardingRunning() ? onboarding(card) : sheet(card);
}

function wordsCard(words) {
  return stepCard({
    mood: "waiting", title: t(`Your ${words.length} recovery words`, `Твои ${words.length} слов восстановления`),
    subtitle: t("Write them on paper, in order. They are the only way back to this wallet if this PC is lost or wiped. ihatecrypto won't show them again.",
      "Запиши их на бумагу по порядку. Это единственный путь обратно к кошельку, если компьютер потеряется или будет стёрт. ihatecrypto больше их не покажет."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-words" }, words.map((w, i) => h("div", { class: "ob-word" }, h("span", { class: "ob-n" }, String(i + 1)), h("b", {}, w)))),
      statusLine(t("Never type them into a website, a chat or a photo. Anyone with these words owns the money.",
        "Никогда не вводи их на сайтах, в чатах и не фотографируй. Кто знает эти слова, тот владеет деньгами."), false),
      h("div", { class: "btns" },
        h("button", { class: "btn", disabled: copied, onclick: async (e) => {
          copied = true;
          e.currentTarget.disabled = true;
          try { await invoke("copy_secret", { text: words.join(" ") }); toast(t("Copied once. The clipboard clears in 60 s.", "Скопировано один раз. Буфер очистится через 60 с.")); }
          catch { toast(t("Couldn't copy.", "Не скопировалось.")); }
          render();
        } }, icon(copied ? "check" : "copy", 14), copied ? t("Copied", "Скопировано") : t(`Copy all ${words.length} words`, `Скопировать все ${words.length} слов`)),
        h("span", { class: "secondary small" }, t("For a password manager: cleared from the clipboard in 60 s, kept out of clipboard history.",
          "Для менеджера паролей: буфер очистится через 60 с, в историю буфера не попадёт.")))),
    primary: t("I wrote them down", "Я записал"),
    onPrimary: () => {
      // The words leave the clipboard now, while it still holds them (the Mac's wipe by change count), not in 60 s.
      if (copied) wipeCopied();
      const order = [...words.keys()].sort(() => Math.random() - 0.5);
      const asked = order.slice(0, 3).sort((a, b) => a - b);
      const others = order.slice(3, 8);
      check = { key: words.join(" "), asked, filled: 0, miss: null,
        choices: [...new Set([...asked, ...others].map((i) => words[i]))].sort(() => Math.random() - 0.5) };
      if (onboardingRunning() && step() !== "check") go("check"); else render();
    },
    // No way back: the wallet is made, and a step back would make another.
  });
}

function checkCard(words) {
  const { asked, filled } = check, done = filled === asked.length;
  const pick = (w) => {
    if (done) return;
    if (words[asked[filled]] === w) { check.filled += 1; check.miss = null; }
    else check.miss = t(`That isn't word ${asked[filled] + 1}. Look at your paper again.`, `Это не слово ${asked[filled] + 1}. Посмотри на бумагу ещё раз.`);
    render();
  };
  return stepCard({
    mood: done ? "done" : "tuning", title: t("Check three of them", "Проверь три из них"),
    subtitle: t("Pick the words by their numbers, from your paper.", "Выбери слова по их номерам — с бумаги."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-check-slots" }, asked.map((i, k) => h("div", { class: "ob-check-slot" + (k < filled ? " filled" : k === filled ? " next" : "") },
        h("div", { class: "eyebrow" }, t(`Word ${i + 1}`, `Слово ${i + 1}`)), h("b", {}, k < filled ? words[i] : " ")))),
      h("div", { class: "ob-check-choices" }, check.choices.map((w) => {
        // A word the phrase repeats stays until it has been picked as many times as it's asked.
        const asks = asked.filter((i) => words[i] === w).length;
        const used = asks > 0 && asked.slice(0, filled).filter((i) => words[i] === w).length === asks;
        return h("button", { class: "ob-choice", disabled: used || done, onclick: () => pick(w) }, w);
      })),
      check.miss ? statusLine(check.miss, false) : null),
    primary: t("Continue", "Дальше"), enabled: done,
    onPrimary: async () => {
      check = null;
      if (copied) wipeCopied();
      copied = false;
      await act("phraseSaved");
      setProtect(false);
      if (onboardingRunning()) go("secure");
    },
    quiet: quietButton(t("Show the words again", "Показать слова снова"), () => { check = null; if (step() === "check") back(); else render(); }),
  });
}

/** After a restart the words are gone from the engine's memory, as they should be (WordsGoneStep). */
function wordsGoneStep() {
  return stepCard({
    mood: "idle", title: t("The words were shown once", "Слова показываются один раз"),
    subtitle: t("ihatecrypto doesn't keep them, so they aren't here after a restart. If they aren't on paper, make a new wallet from the wallet menu before putting money on this one.",
      "ihatecrypto их не хранит, поэтому после перезапуска их здесь нет. Если они не записаны, создай новый кошелёк из меню кошельков, прежде чем класть деньги на этот."),
    primary: t("Continue", "Дальше"), onPrimary: () => go("secure"),
  });
}

// Secure: how the signer asks before a move. Windows Hello stands where the Mac has Touch ID and its password.

function secureStep() {
  const test = !!activeWallet()?.test, made = !!ui.state.lock?.codeSet, hello = !noHello();
  const way = (name, title, line, on) => h("div", { class: "ob-way" + (on ? " on" : "") }, icon(name, 16), h("b", {}, title), h("div", { class: "secondary small" }, line));
  return stepCard({
    mood: "tuning", title: test ? t("A test wallet signs by itself", "Тестовый кошелёк подписывает сам") : t("Each move asks once", "Каждое движение спрашивает один раз"),
    subtitle: test ? t("It doesn't ask before a move: keep only a few dollars on it.", "Он не спрашивает перед движением: держи на нём только несколько долларов.")
      : t("So the first real move is no surprise: each move asks once, and so does switching wallets.", "Чтобы первое настоящее движение не стало сюрпризом: каждое движение спрашивает один раз, и смена кошелька тоже."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-ways" },
        way("person", "Windows Hello", hello ? t("Face or fingerprint", "Лицо или отпечаток") : t("Not set up on this PC", "Не настроен на этом компьютере"), hello),
        way("lock", t("Windows Hello PIN", "PIN Windows Hello"), hello ? t("Where there's no camera or reader", "Где нет камеры или сканера") : t("Not set up on this PC", "Не настроен на этом компьютере"), hello),
        way("ob.grid", t("Passcode", "Код"), t("Opens the app · next step", "Открывает приложение · следующий шаг"), false)),
      test ? null : h("div", { class: "ob-secure-row" }, h("span", { class: "ob-ring" }, icon(made ? "check" : "person", 30)),
        h("div", {}, h("b", {}, made ? t("Each move asks for Windows Hello", "Каждое движение спрашивает Windows Hello") : t("Windows Hello asks on every move", "Windows Hello спрашивает на каждом движении")),
          h("div", { class: "secondary small" }, made ? t("Your code is already set.", "Твой код уже задан.")
            : t("Your face, fingerprint or PIN, once a move. The code on the next step only opens the app.", "Лицо, отпечаток или PIN — один раз на движение. Код на следующем шаге только открывает приложение."))))),
    primary: t("Continue", "Дальше"), onPrimary: () => go("passcode"),
    quiet: quietButton(t("Back", "Назад"), back),
  });
}

/** The app's own 6-digit code (Passcode P1): it opens the app after you step away. Made by the lock screen's part. */
function passcodeStep() {
  const l = ui.state.lock, made = !!l?.codeSet;
  const after = l?.lockAfter === 60 ? t("an hour", "час") : t(`${l?.lockAfter ?? 5} min`, `${l?.lockAfter ?? 5} мин`);
  // The signer asks Windows Hello before it keeps a code (Vault.swift): without it, said here, not after the code is typed
  // twice — as the Mac adapts this step to a Mac without Touch ID.
  const hello = !noHello();
  return stepCard({
    mood: made ? "done" : "tuning", title: made ? t("Your code is set", "Код задан") : t("Lock ihatecrypto when you step away", "Блокируй ihatecrypto, когда отходишь"),
    subtitle: made ? t(`ihatecrypto locks after ${after} away, and when the PC sleeps. Change it in Settings › Wallet.`, `ihatecrypto блокируется после ${after} без тебя и когда компьютер засыпает. Изменить — в «Настройки › Кошелёк».`)
      : hello ? t("Six digits open it again. Windows Hello opens it too.", "Шесть цифр открывают его снова. Windows Hello тоже открывает.")
      : t("Six digits open it again. A code is confirmed with Windows Hello, so it can be made once Windows Hello is set up.",
        "Шесть цифр открывают его снова. Код подтверждается через Windows Hello, поэтому задать его можно, когда Windows Hello будет настроен."),
    content: made ? null : !hello ? h("div", { class: "ob-content" }, factRow("person", t("Windows Hello isn't set up on this PC", "Windows Hello на этом компьютере не настроен"),
        t("Turn on a face, fingerprint or PIN in Windows Settings › Accounts › Sign-in options, then make the code in Settings › Wallet. Skip it for now.",
          "Включи лицо, отпечаток или PIN в «Параметры Windows › Учетные записи › Варианты входа», потом задай код в «Настройки › Кошелёк». Пока пропусти.")))
      : h("div", { class: "ob-content" }, lock?.codeMaker && l ? lock.codeMaker(l, () => render())
      : statusLine(t("The code is made in Settings › Wallet once the lock screen is there. Skip it for now.", "Код задаётся в «Настройки › Кошелёк», когда появится экран блокировки, а пока пропусти."))),
    primary: t("Continue", "Дальше"), enabled: made, onPrimary: () => go("networks"),
    quiet: [quietButton(t("Back", "Назад"), back), made ? null : quietButton(t("Skip for now", "Пропустить"), () => go("networks"))],
  });
}

// Bring a wallet: a phrase (with its passphrase) or a key goes only to the signer; a Ledger keeps its keys; an address
// only watches.

const bring = { way: "phrase", phrase: "", passphrase: "", key: "", solana: "", evm: "" };

function bringStep() {
  const way = bring.way;
  // What was typed survives the states the engine sends meanwhile: the field is filled again from `bring`.
  const field = (id, key, placeholder, secret, rows) => {
    const el = h(rows ? "textarea" : "input", {
      id, class: "address ob-field" + (secret ? " secret" : ""), rows: rows || null, type: !rows && secret ? "password" : null, placeholder, autocomplete: "off", spellcheck: "false",
      oninput: (e) => { bring[key] = e.target.value; document.querySelector(".ob-card .ob-primary")?.toggleAttribute("disabled", !ready()); if (key === "key") readKey(); },
    });
    el.value = bring[key];
    return el;
  };
  // Without Windows Hello the signer can't seal a phrase or key (Host.swift): said here, not on the press.
  const blocked = (way === "phrase" || way === "key") && noHello();
  const ready = () => blocked ? false : way === "phrase" ? bring.phrase.trim().split(/\s+/).length >= 12 : way === "key" ? !!bring.keyFound
    : way === "ledger" ? true : !!(bring.solana.trim() || bring.evm.trim());
  const body = {
    phrase: [field("bring-phrase", "phrase", t("12 or 24 words separated by spaces", "12 или 24 слова через пробел"), true, "3"),
      field("bring-passphrase", "passphrase", t("Extra passphrase, if your wallet has one", "Дополнительная парольная фраза, если она есть у кошелька"), true),
      h("div", { class: "secondary small" }, t("The same words open the same addresses here as in Phantom and MetaMask: Solana, every EVM network and NEAR.",
        "Те же слова открывают здесь те же адреса, что в Phantom и MetaMask: Solana, все EVM-сети и NEAR."))],
    // Read as it's typed, before anything goes to the signer (the Mac's ImportedKey.parse): its chain and its address,
    // or why it isn't a key; «Find my wallet» only for a key that parses.
    key: [field("bring-key", "key", t("Solana key from Phantom, or EVM key from MetaMask", "Ключ Solana из Phantom или EVM-ключ из MetaMask"), true, "2"),
      bring.keyFound ? [statusLine(`${bring.keyFound.chain}: ${bring.keyFound.address}`, true),
        h("div", { class: "secondary small" }, t(`A key opens one network only: ${bring.keyFound.chain}. For every network, use the recovery phrase instead.`,
          `Ключ открывает только одну сеть: ${bring.keyFound.chain}. Чтобы открыть все сети, принеси фразу восстановления.`))]
        : bring.keyError ? statusLine(bring.keyError, false)
        : h("div", { class: "secondary small" }, t("Phantom: Settings › Manage accounts › Show private key. MetaMask: Account details › Show private key. A key opens one network only.",
          "Phantom: Настройки › Управление аккаунтами › Показать приватный ключ. MetaMask: Детали аккаунта › Показать приватный ключ. Ключ открывает только одну сеть."))],
    ledger: [factRow("link", t("USB or Bluetooth", "USB или Bluetooth"), t("Nano X, Stax and Flex work over Bluetooth, any Ledger over USB.", "Nano X, Stax и Flex работают по Bluetooth, любой Ledger — по USB.")),
      factRow("lock", t("The keys stay on the device", "Ключи остаются на устройстве"), t("Every move shows on its screen and waits for its buttons.", "Каждое движение показывается на его экране и ждёт его кнопок."))],
    address: [field("bring-solana", "solana", t("Solana address", "Адрес Solana")),
      field("bring-evm", "evm", t("EVM address (0x…): Base, Arbitrum, Hyperliquid and the rest", "EVM-адрес (0x…): Base, Arbitrum, Hyperliquid и остальные")),
      h("div", { class: "secondary small" }, t("Balances only: nothing can be signed with an address. Add a key or a phrase later from the wallet menu.",
        "Только балансы: адресом ничего не подписать. Ключ или фразу можно добавить позже из меню кошельков."))],
  }[way];
  const goOn = async () => {
    if (way === "ledger") { go("ledger"); return; }
    flow.busy = true;
    render();
    const r = way === "phrase" ? await act("importPhrase", { phrase: bring.phrase.trim().toLowerCase().split(/\s+/).join(" "), passphrase: bring.passphrase })
      : way === "key" ? await act("importKey", { key: bring.key.trim() })
      : await act("watch", { solana: bring.solana.trim(), evm: bring.evm.trim() });
    flow.busy = false;
    if (r === null) { render(); return; }
    flow.accounts = parseAccounts(r);
    flow.others = "only";
    Object.assign(bring, { phrase: "", passphrase: "", key: "", keyFound: null, keyError: null, solana: "", evm: "" });
    go(way === "address" ? "networks" : "found");
  };
  return stepCard({
    mood: "tuning", title: t("Bring your wallet", "Принеси свой кошелёк"),
    subtitle: t("From Phantom, MetaMask or any wallet. It goes only to the separate signer, which seals it with Windows Hello and this PC's TPM.",
      "Из Phantom, MetaMask или любого кошелька. Он уходит только отдельному подписанту, который запечатывает его Windows Hello и TPM этого компьютера."),
    content: h("div", { class: "ob-content" },
      segmented([["phrase", t("Recovery phrase", "Фраза")], ["key", t("Private key", "Ключ")], ["ledger", "Ledger"], ["address", t("Address only", "Только адрес")]],
        way, (v) => { bring.way = v; render(); }, "wide"), blocked ? helloMissing() : null, body),
    primary: { phrase: t("Find my wallet", "Найти кошелёк"), key: t("Find my wallet", "Найти кошелёк"), ledger: t("Connect a Ledger", "Подключить Ledger"), address: t("Watch these", "Смотреть") }[way],
    onPrimary: goOn, enabled: ready(), busy: flow.busy,
    quiet: quietButton(t("Back", "Назад"), back),
  });
}

/** The key in the field, read by the engine without saving anything (keyAddress): the newest typing wins. */
let keyAsked = 0;
async function readKey() {
  const n = ++keyAsked, key = bring.key.trim();
  if (!key) { Object.assign(bring, { keyFound: null, keyError: null }); render(); return; }
  try { const r = await engine("keyAddress", { key }); if (n === keyAsked) Object.assign(bring, { keyFound: r, keyError: null }); }
  catch (e) { if (n === keyAsked) Object.assign(bring, { keyFound: null, keyError: e.message }); }
  if (n === keyAsked) render();
}

/** The first addresses an imported phrase makes (WalletOps.firstAccounts), as the engine answered them. */
function parseAccounts(r) {
  try { return r?.accounts ? JSON.parse(r.accounts) : null; } catch { return null; }
}

/** What the phrase or key opened, to compare with the wallet it came from (FoundStep). A key opens one chain: the rest is
 * offered. */
function foundStep() {
  const w = activeWallet(), a = ui.state.addresses;
  const keyChain = w?.kind === "solana" ? "Solana" : w?.kind === "evm" ? "EVM" : null;
  const row = (file, name, line, address) => tile(false, logo(file, 28), tileText(name, line), h("span", { class: "spacer" }),
    address ? [h("span", { class: "ob-ends address" }, ends(address)), h("span", { class: "profit" }, icon("check", 15))]
      : h("span", { class: "secondary small" }, t("No address yet", "Адреса пока нет")));
  const choice = (value, title, line) => h("button", { class: "ob-choice-row" + (flow.others === value ? " on" : ""), onclick: () => { flow.others = value; render(); } },
    h("span", { class: "ob-radio" }), tileText(title, line));
  const done = async () => {
    if (keyChain && flow.others === "phrase") { bring.way = "phrase"; back(); return; }
    // A single key: the networks it can't sign on go off, so the assistant never plans a move through them.
    if (keyChain) for (const n of networks()) if (n.on && !hasAddress(n.id)) await act("module", { name: n.id, test: false });
    go("secure");
  };
  return stepCard({
    mood: "done", title: keyChain ? t(`This key opens ${keyChain}`, `Этот ключ открывает ${keyChain}`) : t("Found your wallet", "Кошелёк найден"),
    subtitle: t("The addresses it opens. Check them against the wallet you came from.", "Адреса, которые он открывает. Сверь их с кошельком, из которого ты пришёл."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-tiles" },
        row("sol", "Solana", "Jupiter, Kamino, JitoSOL", a.solana),
        row("base", "EVM", "Base, Arbitrum, Hyperliquid, Robinhood Chain, Polygon, Ethereum, Tempo", a.evm),
        row("near", "NEAR", "NEAR Intents, 1Click", a.near)),
      keyChain ? [
        h("b", { class: "ob-sub-title" }, t("What about the other networks?", "А остальные сети?")),
        choice("only", t(`Only ${keyChain} for now`, `Пока только ${keyChain}`), t("The other networks stay off. Add a wallet for them any time from the wallet menu.", "Остальные сети выключены, а кошелёк для них можно добавить в любой момент из меню кошельков.")),
        choice("phrase", t("Use a recovery phrase instead", "Лучше фраза восстановления"), t("A phrase opens every network at once. This key stays as a wallet you can remove.", "Фраза открывает сразу все сети. Этот ключ останется кошельком, который можно удалить.")),
        h("div", { class: "ob-soon" }, icon("ob.sparkles", 15), tileText(t("Make them for me · soon", "Сделать их за меня · скоро"), t("New addresses for the missing networks, in the same wallet.", "Новые адреса для недостающих сетей, в том же кошельке.")))]
        : flow.accounts?.length ? compareTable(flow.accounts)
        : statusLine(t("Compare with MetaMask and Phantom: the same phrase shows the same addresses there.", "Сверь с MetaMask и Phantom: та же фраза показывает там те же адреса."))),
    primary: keyChain && flow.others === "phrase" ? t("Enter a phrase", "Ввести фразу") : t("That's my wallet", "Это мой кошелёк"), onPrimary: done,
    quiet: quietButton(keyChain ? t("Try another key", "Другой ключ") : t("Try another phrase", "Другая фраза"), back),
  });
}

/** «Compare with MetaMask and Phantom»: the first addresses the phrase makes, by their number in those wallets. */
function compareTable(accounts) {
  return h("div", { class: "ob-compare" },
    h("div", { class: "ob-compare-head" }, h("b", {}, t("Compare with MetaMask and Phantom", "Сверь с MetaMask и Phantom")),
      h("span", { class: "secondary small" }, t("The first addresses this phrase makes.", "Первые адреса, которые делает эта фраза."))),
    h("div", { class: "ob-compare-grid" },
      h("span", { class: "secondary small" }, "#"), h("span", { class: "secondary small" }, "EVM · Hyperliquid  m/44'/60'/0'/0/i"), h("span", { class: "secondary small" }, "Solana  m/44'/501'/i'/0'"),
      accounts.map((x, i) => [h("span", { class: "secondary" }, String(i)), h("span", { class: "address selectable" }, x.evm), h("span", { class: "address selectable" }, x.solana)])));
}

/** Leaving the Ledger step: the block its search pinned closes, so the search stops and nothing stays on the canvas (the
 *  Mac sets ledgerConnect nil on Back and after «Use this Ledger»; Engine's closeBoard does the same here). */
function leaveLedger() {
  const looked = flow.ledgerLooked;
  flow.ledgerLooked = false;
  const b = looked ? (ui.state.boards || []).find((x) => x.kind === "ledger") : null;
  if (b) act("closeBoard", { board: b.id });
}

/** A Ledger: the keys stay on the device (LedgerStep). */
function ledgerStep() {
  const lc = flow.ledgerLooked ? ui.state.ledgerConnect : null;
  const connected = !!lc && !lc.busy && !lc.problem && ui.state.wallets.some((w) => w.id === "ledger" && w.evm);
  return stepCard({
    mood: connected ? "done" : lc?.busy ? "thinking" : "waiting", title: t("Connect a Ledger", "Подключи Ledger"),
    subtitle: t("The keys stay on the device: every move shows on its screen and waits for its buttons.", "Ключи остаются на устройстве: каждое движение показывается на его экране и ждёт его кнопок."),
    content: h("div", { class: "ob-content" },
      factRow("link", t("Plug it in, or turn its Bluetooth on", "Подключи его или включи на нём Bluetooth"), t("USB for any Ledger. Bluetooth for Nano X, Stax and Flex.", "USB — для любого Ledger. Bluetooth — для Nano X, Stax и Flex.")),
      factRow("lock", t("Unlock it with its PIN", "Разблокируй его PIN-кодом"), t("On the device itself: the PIN never touches this PC.", "На самом устройстве: PIN никогда не попадает на этот компьютер.")),
      factRow("check", t("Allow the Ethereum and Solana apps", "Разреши приложения Ethereum и Solana"), t("The Ledger asks for each, then ihatecrypto reads the addresses from them.", "Ledger спросит про каждое, потом ihatecrypto прочитает из них адреса.")),
      lc ? [lc.model ? statusLine(lc.model, true) : null, statusLine(lc.problem || lc.status, lc.problem ? false : connected ? true : null),
        lc.notes.map((n) => h("div", { class: "ob-note" }, n))] : null),
    primary: connected ? t("Use this Ledger", "Работать с этим Ledger") : !lc ? t("Look for it", "Найти его") : t("Try again", "Ещё раз"), busy: !!lc?.busy,
    onPrimary: async () => {
      if (!connected) { flow.ledgerLooked = true; await act("connectLedger"); return; }
      if (await act("switchWallet", { wallet: "ledger" }) !== null) { leaveLedger(); go("networks"); }
    },
    quiet: quietButton(t("Back", "Назад"), () => { leaveLedger(); back(); }),
  });
}

// Networks: the assistant plans moves only on the ones that are on.

function networksStep() {
  const hl = trades();
  return stepCard({
    mood: "tuning", title: t("Which networks do you use?", "Какими сетями ты пользуешься?"),
    subtitle: t("The assistant plans moves only on the ones that are on. Money already on the others still shows.", "Ассистент планирует движения только в включённых сетях. Деньги в остальных всё равно видны."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-tiles tight" }, networks().map((n) => {
        const has = hasAddress(n.id), on = has && n.on;
        const line = !has ? t("No address for it in this wallet", "В этом кошельке нет для неё адреса")
          : n.id === "network.hyperliquid" && n.on ? n.uses + t(" · its trading key comes after the first deposit", " · торговый ключ — после первого депозита") : n.uses;
        return h("div", { class: "ob-tile" + (n.id === "network.hyperliquid" && hl ? " on" : "") + (has ? "" : " faded") },
          logo(n.logo, 26), tileText(n.name, line), h("span", { class: "spacer" }),
          toggle("ob-net-" + n.id, on, (v) => act("module", { name: n.id, test: v }), !has));
      })),
      h("div", { class: "secondary small" }, t("Change any time in Settings › Modules, or from the networks above the chat.", "Поменять можно в любой момент в «Настройки › Модули» или в сетях над чатом."))),
    primary: t("Continue", "Дальше"), onPrimary: () => go("money"),
    quiet: quietButton(t("Back", "Назад"), back),
  });
}

// Top up (variant A): the Top up panel inside the card; once money lands, «Money landed». A wallet that already holds
// money (one brought along) first shows what's on it.

function moneyStep() {
  const s = ui.state, p = s.portfolio;
  // From the step's first draw: a landing the board already showed isn't new. As the Mac: «Money landed» comes from the
  // Top up board's own watched coin whenever it lands, never any row rising (P/L, HLP, yields), balance whole or not.
  if (!flow.money) flow.money = { landed: null, more: false, stale: s.topUp?.arrived ?? null };
  const m = flow.money;
  const landed = (text) => { if (!m.landed && text !== m.stale) { m.landed = text; render(); } };
  if (m.landed) return landedCard(m.landed);
  // «Already has money» only from the whole balance (the Mac's balanceReady).
  if (!m.more && p.ready && Number(p.total) >= 1) return hasMoneyCard();
  return stepCard({
    mood: "waiting", title: t("Top up ihatecrypto", "Пополни ihatecrypto"),
    subtitle: t("From a wallet in this browser, your phone or an exchange. It shows here the moment it lands.", "Из кошелька в браузере, с телефона или с биржи. Здесь будет видно, как только деньги придут."),
    content: h("div", { class: "ob-content" }, pages.topUpPanel ? pages.topUpPanel({ embedded: true, landed }) : receiveTiles(), gasWarning()),
    primary: t("Continue", "Дальше"), enabled: false,
    quiet: [quietButton(t("Back", "Назад"), back), quietButton(t("I'll do it later", "Сделаю позже"), finish)],
  });
}

/** Where to send money, until the Top up part offers its panel for here: each address with its QR, whole, and Copy. */
function receiveTiles() {
  const a = ui.state.addresses;
  const card = (file, name, line, address) => address ? h("div", { class: "ob-receive" },
    h("div", { class: "ob-receive-qr" }, qrCode(address, "ob-qr"), h("span", { class: "ob-receive-logo" }, logo(file, 24))),
    h("div", { class: "ob-receive-text" }, h("b", {}, name), h("div", { class: "secondary small" }, line), h("div", { class: "address selectable" }, address), copyButton(address))) : null;
  return h("div", { class: "ob-tiles" },
    card("sol", "Solana", t("USDC and SOL, and any Solana token. Every move starts here.", "USDC и SOL и любые токены Solana. С них начинается каждый перевод."), a.solana),
    card("base", t("EVM networks", "EVM-сети"), t("Base · Arbitrum · Ethereum · Polygon · Optimism · BNB · Hyperliquid: the same address on each. Send on the network you mean.",
      "Base · Arbitrum · Ethereum · Polygon · Optimism · BNB · Hyperliquid: один адрес во всех. Отправляй в той сети, которую имеешь в виду."), a.evm));
}

function landedCard(line) {
  const keyWaits = trades() && flow.funded[activeWallet()?.evm] !== true && !ui.state.trading?.key?.active;
  return stepCard({
    mood: "done", title: t("Money landed", "Деньги пришли"), subtitle: t("It's in your portfolio. On the canvas the chat can put it to work.", "Они в портфеле. На холсте чат может пустить их в дело."),
    content: h("div", { class: "ob-content" }, statusLine(line, true), gasWarning(),
      keyWaits ? factRow("lock", t("The Hyperliquid trading key is waiting", "Торговый ключ Hyperliquid ждёт"),
        t("It turns on after Hyperliquid's first deposit. On the canvas, say «move 10 USDC to Hyperliquid», then approve the key in Settings › Wallet › Hyperliquid.",
          "Он включается после первого депозита на Hyperliquid. На холсте скажи «переведи 10 USDC на Hyperliquid», потом одобри ключ в «Настройки › Кошелёк › Hyperliquid».")) : null),
    primary: t("Continue", "Дальше"), onPrimary: () => go("stayInTouch"),
    quiet: [quietButton(t("Back", "Назад"), back), quietButton(t("Top up more", "Пополнить ещё"), () => { flow.money.stale = flow.money.landed; flow.money.landed = null; flow.money.more = true; render(); })],
  });
}

/** The portfolio's networks as its cards order them (the Mac's PortfolioSplit.networkCards): Solana, Hyperliquid and
 * Robinhood first, the others by money, $100 or less folded. */
function hasMoneyCard() {
  const p = ui.state.portfolio, pinned = ["solana", "hyperliquid", "robinhood"], byKey = new Map(p.networks.map((n) => [n.key, n]));
  const rest = p.networks.filter((n) => !pinned.includes(n.key) && Number(n.usd) >= 0.01)
    .sort((a, b) => Number(b.usd) - Number(a.usd) || a.key.localeCompare(b.key));
  // Solana and Hyperliquid whenever the wallet has them (the engine sends them only then), Robinhood once it holds money.
  const shown = [...pinned.map((k) => byKey.get(k)).filter((n) => n && (n.key !== "robinhood" || Number(n.usd) >= 0.01)),
    ...rest.filter((n) => Number(n.usd) > 100)], folded = rest.filter((n) => Number(n.usd) <= 100);
  const row = (mark, name, value, detail) => h("div", { class: "ob-money-row" }, mark, tileText(name, detail), h("span", { class: "spacer" }), h("b", { class: "num" }, masked(usd(value))));
  return stepCard({
    mood: "done", title: t("Your wallet already has money", "На кошельке уже есть деньги"),
    subtitle: t(`${masked(usd(p.total))} across your networks. Top up more now, or any time from Top up in the sidebar.`, `${masked(usd(p.total))} по твоим сетям. Пополни ещё сейчас или в любой момент через «Пополнить» на боковой панели.`),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-tiles tight" }, shown.map((n) => row(logo(n.key === "solana" ? "sol" : n.key, 26), n.name, n.usd)),
        folded.length ? row(h("span", { class: "ob-tile-ico" }, icon("rectangle.stack", 14)), t("Other networks", "Другие сети"), folded.reduce((x, n) => x + Number(n.usd), 0), folded.map((n) => n.name).join(", ")) : null),
      gasWarning(),
      h("button", { class: "ob-tile ob-tile-button", onclick: () => { flow.money.more = true; render(); } },
        h("span", { class: "ob-tile-ico accent-ink" }, icon("arrow.down.circle", 18)),
        tileText(t("Top up more", "Пополнить ещё"), t("The same Top up as for a new wallet: amount, network, browser wallet, phone or exchange", "То же пополнение, что для нового кошелька: сумма, сеть, кошелёк в браузере, телефон или биржа")),
        h("span", { class: "spacer" }), icon("chevron.right", 11))),
    primary: t("Continue", "Дальше"), onPrimary: () => go("stayInTouch"),
    quiet: quietButton(t("Back", "Назад"), back),
  });
}

/** Dollars where the network's fee coin is missing: the first move there would stop, so it's said here with its way out. */
function gasWarning() {
  const stuck = ui.state.stuck?.[0];
  if (!stuck) return null;
  const line = !stuck.ways ? t(`The dollars there can't move until ≈ ${usd(stuck.usd)} of ${stuck.coin} pays the network's fees. Top up ${stuck.coin} the same way.`,
    `Доллары там не сдвинуть, пока ≈ ${usd(stuck.usd)} ${stuck.coin} не оплатят комиссии сети. Пополни ${stuck.coin} так же.`)
    : stuck.key === "solana" ? t(`The chat offers to swap 5 USDC for SOL, Jupiter paying that fee. Or top up ≈ ${usd(stuck.usd)} of SOL.`, `Чат предложит обменять 5 USDC на SOL — комиссию заплатит Jupiter. Или пополни SOL на ≈ ${usd(stuck.usd)}.`)
    : t(`The chat offers to move them by signature, Relay paying the gas. Or top up ≈ ${usd(stuck.usd)} of ${stuck.coin}.`, `Чат предложит перевести их по подписи — газ заплатит Relay. Или пополни ${stuck.coin} на ≈ ${usd(stuck.usd)}.`);
  return h("button", { class: "ob-tile ob-tile-button", onclick: () => { if (flow.money) { flow.money.stale = flow.money.landed ?? flow.money.stale; flow.money.landed = null; flow.money.more = true; } render(); } },
    h("span", { class: "ob-tile-ico accent-ink" }, icon("bolt", 16)),
    tileText(!stuck.ways ? t(`Add ${stuck.coin} for fees on ${stuck.network}`, `Добавь ${stuck.coin} на комиссии в ${stuck.network}`) : t(`No ${stuck.coin} for fees on ${stuck.network}`, `Нет ${stuck.coin} на комиссии в ${stuck.network}`), line),
    h("span", { class: "spacer" }), icon("chevron.right", 11));
}

/** Stay in touch: Windows' notifications and the tray stand where the Mac has its notifications and the menu bar. */
function stayInTouchStep() {
  const last = !showsTrading(), s = ui.state;
  const item = (name, title, line, end) => tile(false, h("span", { class: "ob-tile-ico accent-ink" }, icon(name, 15)), tileText(title, line), h("span", { class: "spacer" }), end);
  return stepCard({
    mood: "idle", title: t("Stay in touch", "Оставайся на связи"),
    subtitle: t("So a price alert or a finished move reaches you, and schedules run with the window closed.", "Чтобы оповещение о цене или законченное движение дошли до тебя, а расписание работало с закрытым окном."),
    content: h("div", { class: "ob-content" },
      h("div", { class: "ob-tiles tight" },
        item("ob.bell", t("Notifications", "Уведомления"), t("Money arrived, an alert fired, a move finished or needs your Confirm. Windows turns them off in Settings › System › Notifications.",
          "Пришли деньги, сработало оповещение, движение закончилось или ждёт «Подтвердить». Выключаются в Windows: «Параметры › Система › Уведомления»."),
          h("span", { class: "ob-ok profit" }, icon("check", 14), t("On", "Включены"))),
        // The Mac's «Keep running in the menu bar» switch: Settings › General › Keep always running (back after a quit or
        // a crash, and at sign-in, in the tray).
        item("rectangle.stack", t("Keep running in the tray", "Работать в трее"), t("Stops, alerts and scheduled buys keep working after the window is closed, and it comes back after a quit or a crash.",
          "Стопы, оповещения и покупки по расписанию работают и при закрытом окне, а после выхода или сбоя приложение возвращается."),
          toggle("ob-keep", s.localSetup?.keepRunning === true, (v) => act("keepRunning", { test: v }), s.localSetup?.keepRunning === undefined))),
      h("div", { class: "secondary small" }, t("Both are in Settings › General too.", "Это есть и в «Настройки › Основные»."))),
    primary: last ? t("Finish", "Готово") : t("Continue", "Дальше"), onPrimary: () => (last ? finish() : go("tradingKey")),
    quiet: quietButton(t("Back", "Назад"), back),
  });
}

// Trading, only with Hyperliquid on and an account that has had a deposit.

function tradingKeyStep() {
  const key = ui.state.trading?.key || {}, noDeposit = flow.funded[activeWallet()?.evm] === false;
  const ready = key.active || noDeposit, ask = key.ledger ? t("the Ledger's buttons", "кнопок Ledger") : "Windows Hello";
  // The approval runs in the engine's background: the step waits as the Mac's does, until the key is on or the engine has
  // said how it went with Windows Hello no longer asking — never a fixed few seconds.
  const chatNow = ui.state.chat.length;
  if (flow.keyFrom != null && (key.active || (chatNow > flow.keyFrom && !ui.state.signer?.waiting))) {
    flow.keyReply = key.active ? null : ui.state.chat.slice(flow.keyFrom).filter((m) => !m.fromUser).pop()?.text || null;
    flow.keyFrom = null; flow.busy = false; flow.tried = true;
  }
  const said = flow.tried && !key.active ? flow.keyReply : null;
  return stepCard({
    mood: key.active ? "done" : flow.busy ? "thinking" : "idle", title: t("A trading key for Hyperliquid", "Торговый ключ для Hyperliquid"),
    subtitle: noDeposit && !key.active
      ? t(`Hyperliquid takes it once the account has had a deposit, so it waits until then. Until then every order asks for ${ask}.`, `Hyperliquid примет его, когда на аккаунт придёт первый депозит, поэтому он ждёт. До тех пор каждый ордер спрашивает ${key.ledger ? "кнопки Ledger" : "Windows Hello"}.`)
      : t(`Because Hyperliquid is on. Without it every order asks for ${ask}. With it, orders go straight through the rules.`, `Потому что Hyperliquid включён. Без него каждый ордер спрашивает ${key.ledger ? "кнопки Ledger" : "Windows Hello"}. С ним ордера идут сразу по правилам.`),
    content: h("div", { class: "ob-content" },
      factRow("arrow.right", t("It opens and closes positions", "Он открывает и закрывает позиции"), t("Orders, stops and takes, within the trade rules — new positions for 7 days, then one confirmation renews them.", "Ордера, стопы и тейки — по правилам сделок. Новые позиции 7 дней, потом одно подтверждение продлевает.")),
      factRow("eye.slash", t("It can't withdraw", "Он не может выводить"), t("Hyperliquid itself refuses a withdrawal signed by it.", "Hyperliquid сам отклоняет вывод, подписанный им.")),
      factRow("lock", t("Pause it any time", "Можно поставить на паузу"), t("Say «lock» in the chat. Settings › Wallet › Hyperliquid turns it back on.", "Скажи в чате «заблокируй». «Настройки › Кошелёк › Hyperliquid» включает его снова.")),
      key.active ? statusLine(t("The trading key is on.", "Торговый ключ включён."), true)
        : noDeposit ? statusLine(t("After the first deposit to Hyperliquid: Settings › Wallet › Hyperliquid has the button.", "После первого депозита на Hyperliquid: кнопка есть в «Настройки › Кошелёк › Hyperliquid»."))
        : said ? statusLine(said, false) : null),
    primary: ready ? t("Continue", "Дальше") : key.ledger ? t("Approve on the Ledger", "Одобрить на Ledger") : t("Approve with Windows Hello", "Одобрить через Windows Hello"),
    busy: flow.busy,
    onPrimary: async () => {
      if (ready) { go("rules"); return; }
      flow.busy = true;
      flow.tried = false;
      flow.keyFrom = ui.state.chat.length;
      render();
      if (await act("tradingKey") === null) { flow.keyFrom = null; flow.busy = false; render(); }
    },
    quiet: [quietButton(t("Back", "Назад"), back), ready ? null : quietButton(t("Later", "Позже"), () => go("rules"))],
  });
}

function rulesStep() {
  const s = ui.state, p = s.trading?.policy;
  if (!p) return wordsGoneStep();
  const n = (v) => String(Number(v));
  const rule = (title, line, end) => tile(false, tileText(title, line), h("span", { class: "spacer" }), end || null);
  // A draft saved on Continue: the signer asks Windows Hello once for it, not at every step.
  const shown = flow.risk ?? Number(p.maxRiskPct);
  // The same steps as Settings: half a percent to 10, then 15, 25, 50 and no limit.
  const steps = [...Array(20)].map((_, i) => (i + 1) / 2).concat([15, 25, 50, 1000]);
  const risk = (d) => { flow.risk = (d > 0 ? steps.find((x) => x > shown) : [...steps].reverse().find((x) => x < shown)) ?? shown; render(); };
  const changed = flow.risk != null && flow.risk !== Number(p.maxRiskPct);
  const go = async () => {
    if (changed) {
      flow.busy = true;
      render();
      const ok = await act("policy", { policy: { ...p, maxRiskPct: flow.risk } });
      flow.busy = false;
      if (ok === null) { render(); return }
    }
    flow.risk = null;
    finish();
  };
  return stepCard({
    mood: "idle", title: t("Rules every trade passes", "Правила, которые проходит каждая сделка"),
    subtitle: t("Because Hyperliquid is on. Checked in code before a trade is signed, not by the model.", "Потому что Hyperliquid включён. Проверяются кодом до подписи сделки, а не моделью."),
    content: h("div", { class: "ob-content" }, h("div", { class: "ob-tiles tight" },
      rule(Number(shown) >= 1000 ? t("No limit on the risk of one trade", "Без лимита на риск одной сделки") : t(`At most ${n(shown)}% of the account at risk`, `Под риском не больше ${n(shown)}% счёта`),
        t("What one trade may lose at its stop", "Сколько одна сделка может потерять на стопе"),
        h("span", { class: "ob-stepper" },
          h("button", { class: "btn small", disabled: flow.busy || shown <= 0.5, onclick: () => risk(-0.5), "aria-label": t("Less", "Меньше") }, "−"),
          h("button", { class: "btn small", disabled: flow.busy || shown >= 1000, onclick: () => risk(0.5), "aria-label": t("More", "Больше") }, "+"))),
      rule(t(`By the book: ${n(p.defaultRiskPct)}% risk, take at ${n(p.defaultRewardRisk)}R`, `По правилам: риск ${n(p.defaultRiskPct)}%, тейк на ${n(p.defaultRewardRisk)}R`), t("When you say «by the book» and name no amount", "Когда говоришь «по правилам» и не называешь сумму")),
      rule(t(`Reward at least ${n(p.minRewardRisk)}× the risk`, `Прибыль не меньше ${n(p.minRewardRisk)}× риска`), t("A plan that pays less is stopped", "План, который платит меньше, остановится")),
      rule(t(`Stop by volatility: ${n(p.atrStopMultiple)}× the 4-hour ATR`, `Стоп по волатильности: ${n(p.atrStopMultiple)}× 4-часового ATR`),
        t(`Leverage up to ${n(p.maxLeverage)}×, all open positions together at most ${n(p.maxOpenRiskPct)}%`, `Плечо до ${n(p.maxLeverage)}×, все открытые позиции вместе — не больше ${n(p.maxOpenRiskPct)}%`)),
      rule(t("Second check", "Вторая проверка"), t("Another model reads every plan before you confirm it", "Другая модель перечитывает каждый план, прежде чем ты его подтвердишь"),
        toggle("ob-second", !!s.secondCheck, (v) => act("secondCheck", { test: v }))))),
    primary: changed ? t("Continue · Windows Hello", "Дальше · Windows Hello") : t("Continue", "Дальше"), onPrimary: go, busy: flow.busy,
    quiet: quietButton(t("Back", "Назад"), () => { flow.risk = null; back(); }),
  });
}

// MARK: The first screen (Welcome.swift, C01)

/** The screen's language: a flag's pick, else the app's (en/ru) or Windows' (es). */
const LANGS = ["en", "es", "ru"];
const welcomeLang = () => {
  const picked = store("welcomeLanguage");
  if (LANGS.includes(picked)) return picked;
  if (ui.state?.russian) return "ru";
  return (navigator.languages || [navigator.language]).map((l) => (l || "").slice(0, 2)).find((l) => LANGS.includes(l)) || "en";
};
const L = (en, es, ru) => ({ en, es, ru })[welcomeLang()];

/** The three requests, played in turn (English, Spanish, Russian). The route cards and Confirm stay English, as in the app. */
const scenarios = [
  { chip: ["Buy Tesla on Robinhood", "Comprar Tesla en Robinhood", "Купить Tesla на Robinhood"], icon: "ob.chart",
    ask: ["show me the Tesla chart", "muéstrame el gráfico de Tesla", "покажи график теслы"],
    order: ["buy $10 of Tesla on Robinhood", "compra $10 de Tesla en Robinhood", "купи теслу на $10 на robinhood"],
    info: { label: "CHART · ROBINHOOD", title: "Tesla · TSLA", asset: "tsla", up: true },
    steps: [["FROM · BASE", "10 USDC", "Your wallet on Base", "base"], ["BRIDGE · RELAY", "Base → Robinhood", "10 USDC across", "relay"],
      ["BUY · ROBINHOOD", "$10 → TSLA", "Stock token, in your wallet", "robinhood"]], done: "Done · TSLA in your wallet" },
  { chip: ["Send from Solana", "Enviar desde Solana", "Отправить с Solana"], icon: "arrow.right",
    ask: ["show my Solana wallet", "muéstrame mi billetera de Solana", "покажи мой кошелёк на solana"],
    order: ["send 20 USDC from Solana to Bybit", "envía 20 USDC de Solana a Bybit", "отправь 20 USDC с solana на bybit"],
    info: { label: "WALLET · SOLANA", title: "Solana", asset: "sol", amount: "20 USDC", note: "Ready to send" },
    steps: [["FROM · SOLANA", "20 USDC", "Your wallet on Solana", "solana"], ["CHECK · ADDRESS BOOK", "Bybit", "Saved and approved", "check"],
      ["TO · BYBIT", "20 USDC → Bybit", "Your exchange account", "solana"]], done: "Done · 20 USDC sent" },
  { chip: ["Short oil by the book", "Corto en petróleo con reglas", "Шорт нефти по правилам"], icon: "ob.slider",
    ask: ["show me the oil chart", "muéstrame el gráfico del petróleo", "покажи график нефти"],
    order: ["short oil on a 2% pullback by the book", "corto en petróleo en un retroceso del 2% con reglas", "шорт нефти на откате 2% по правилам"],
    info: { label: "CHART · HYPERLIQUID", title: "Oil · CL", asset: "cl", up: false },
    steps: [["CHECK · RISK RULES", "2% of the account", "Stop, take at 1.5R", "check"], ["OPEN SHORT · OIL", "Short CL", "On a 2% pullback", "hyperliquid"],
      ["PROTECT · STOP", "Stop and take", "Placed with the order", "hyperliquid"]], done: "Done · short open, stop set" },
];

/** Drawn as one 1280×800 composition, centered, and shrunk to fit a smaller window (WelcomeView). The composition stays
 * the same element across the window's redraws, so the demo it plays isn't restarted by a new state. */
const SIZE = { w: 1280, h: 800 };
const CARD = { w: 189, h: Math.round((189 * 292) / 236) }, CARDS_Y = 370, CARDS_BOTTOM = CARDS_Y + CARD.h;
let stage = null;

export function welcome() {
  // The flow's first steps (Model, a new wallet, bringing one) come before there's a wallet; the demo stops meanwhile.
  if (onboardingRunning()) { stage = null; return onboarding(); }
  const lang = welcomeLang();
  // A new language goes on with the request that was playing, as the Mac's (WelcomeView keeps its scenario).
  if (!stage || stage.lang !== lang) stage = buildStage(lang, stage?.scenario ?? 0);
  stage.error.replaceChildren(ui.state.signer.error ? h("div", { class: "ob-status loss" }, ui.state.signer.error) : "");
  fit();
  return stage.root;
}

function fit() {
  if (!stage) return;
  const k = Math.min(1, window.innerWidth / SIZE.w, window.innerHeight / SIZE.h);
  stage.box.style.transform = `translate(-50%, -50%) scale(${k})`;
}
window.addEventListener("resize", fit);

function buildStage(lang, scenario = 0) {
  const i = lang === "es" ? 1 : lang === "ru" ? 2 : 0;
  const pos = (x, y, el) => { el.style.left = x + "px"; el.style.top = y + "px"; return el; };
  const at = (x, y, el) => { el.classList.add("ob-c-center"); return pos(x, y, el); };
  const text = h("span", { class: "ob-c-typed", "data-empty": ["Say what you want", "Di lo que quieres", "Скажи, что нужно"][i] });
  const input = at(640, 240, h("div", { class: "ob-c-input glass" }, h("span", { class: "ob-c-field" }, text, h("span", { class: "ob-caret" })),
    icon("mic", 16), h("span", { class: "ob-welcome-send" }, icon("arrow.up", 15))));
  const chipRow = at(640, 296, h("div", { class: "ob-welcome-chips" }));
  const bubbles = [pos(170, 330, h("div", { class: "ob-c-bubble" })), pos(479, 330, h("div", { class: "ob-c-bubble" }))];
  const info = pos(170, 370, h("div", { class: "ob-c-info card" }));
  const route = pos(479, 370, h("div", { class: "ob-c-route" }));
  const confirm = pos(683, CARDS_BOTTOM + 8, h("div", { class: "ob-c-confirm" }));
  const cursor = h("div", { class: "ob-c-cursor" }, mascot(30, "thinking"));
  const flags = at(1236, 400, h("div", { class: "ob-c-flags glass" }, LANGS.map((l, k) => h("button", {
    class: "ob-flag ob-flag-" + l + (l === lang ? " on" : ""), title: ["English", "Español", "Русский"][k], "aria-label": ["English", "Español", "Русский"][k],
    onclick: () => { store("welcomeLanguage", l); render(); } }))));
  const error = h("div", { class: "ob-c-error" });
  const box = h("div", { class: "ob-c" },
    at(640, 94, h("div", { class: "ob-welcome-logo" }, mascot(84), h("span", {}, "ihatecrypto"))),
    at(640, 170, h("div", { class: "ob-welcome-slogan" }, ["Even crypto hates crypto.", "Hasta la cripto odia la cripto.", "Даже крипта ненавидит крипту."][i])),
    input, chipRow, h("div", { class: "ob-c-stage" }, ...bubbles, info, route, confirm), flags,
    at(640, 676, h("div", { class: "ob-welcome-buttons" },
      h("button", { class: "ob-primary big", onclick: () => start("fresh") }, ["Get started", "Empezar", "Начать"][i]),
      h("button", { class: "ob-secondary big", onclick: () => start("bring") }, ["I already have a wallet", "Ya tengo una billetera", "У меня уже есть кошелёк"][i]))),
    at(640, 714, error),
    at(640, 746, h("div", { class: "ob-welcome-lock secondary" }, icon("lock", 14),
      ["Keys stay sealed by Windows Hello and this PC's TPM", "Las claves quedan selladas por Windows Hello y el TPM de este PC", "Ключи запечатаны Windows Hello и TPM этого компьютера"][i])),
    cursor);
  const root = h("div", { class: "ob-welcome" },
    h("button", { class: "ob-welcome-look", onclick: () => { store("welcomeDone", true); render(); } }, ["Look around first", "Explorar primero", "Сначала осмотреться"][i]),
    box);
  const s = { lang, i, root, box, text, input, chipRow, bubbles, info, route, confirm, cursor, error, scenario, run: 0 };
  play(s);
  return s;
}

/** The chips switch the request; the flags the language. */
function drawChips(s) {
  s.chipRow.replaceChildren(...scenarios.map((x, k) => h("button", { class: k === s.scenario ? "on" : "", title: "«" + x.order[s.i] + "»",
    onclick: () => { s.scenario = k; play(s); } }, icon(x.icon, 14), x.chip[s.i])));
}

/** A made-up price line, the same every time: up or down with some wobble (PriceLine). */
function priceLine(up) {
  let y = up ? 0.85 : 0.2, seed = up ? 11 : 5;
  const pts = [];
  for (let k = 0; k < 25; k++) {
    seed = (Math.imul(seed, 1103515245) + 12345) >>> 0;
    const wobble = ((seed >>> 16) & 0x7fff) / 32767 - 0.5;
    y = Math.min(0.92, Math.max(0.08, y + (up ? -0.028 : 0.028) + wobble * 0.14));
    pts.push([(238 * k) / 24, 96 * y]);
  }
  const d = "M" + pts.map(([x, y2]) => x.toFixed(1) + " " + y2.toFixed(1)).join("L");
  const tint = up ? "var(--profit)" : "var(--loss)";
  return svg("svg", { viewBox: "0 0 238 96", class: "ob-c-line", preserveAspectRatio: "none" },
    svg("path", { d: d + "L238 96L0 96Z", fill: tint, "fill-opacity": "0.12", class: "ob-c-area" }),
    svg("path", { d, fill: "none", stroke: tint, "stroke-width": "2.2", "stroke-linecap": "round", "stroke-linejoin": "round", class: "ob-c-stroke", pathLength: "1" }));
}

const CHIP = { ready: ["Ready", "waiting"], touch: ["Windows Hello", "tuning"], now: ["Now", "thinking"], done: ["Done", "done"], next: ["Next", "idle"] };
function routeCard(step, state) {
  const [label, title, line, fill] = step, [chip, mood] = CHIP[state];
  return h("div", { class: "ob-c-card ob-fill-" + fill + " st-" + state },
    h("div", { class: "ob-c-card-pic" }, h("span", { class: "ob-c-chip" }, mascot(13, mood), chip), mascot(34, fill === "check" ? "done" : state === "touch" ? "tuning" : "idle")),
    h("div", { class: "ob-c-card-body" }, h("div", { class: "eyebrow" }, label), h("b", {}, title), h("div", { class: "secondary small" }, line)));
}

/** One run of a request, about 20 s; then the next one. A chip's click or a new language starts its own. */
async function play(s) {
  const run = ++s.run, sc = scenarios[s.scenario];
  drawChips(s);
  const live = () => s.run === run && stage === s;
  const pause = (ms) => new Promise((ok, no) => setTimeout(() => (live() ? ok() : no()), ms));
  const show = (el, on) => el.classList.toggle("shown", on);
  const move = ([x, y], seconds = 0.75) => {
    s.cursor.style.transitionDuration = seconds + "s";
    s.cursor.style.transform = `translate(${x - 1}px, ${y - 1}px)`;
  };
  const face = (mood) => s.cursor.replaceChildren(mascot(30, mood));
  const type = async (txt) => { for (let k = 1; k <= txt.length; k++) { s.text.textContent = txt.slice(0, k); await pause(34 + ((k * 7) % 30)); } };
  const states = (list) => list.forEach((st, k) => {
    const old = s.route.children[k * 2], card = routeCard(sc.steps[k], st);
    if (old?.classList.contains("shown")) card.classList.add("shown");
    old?.replaceWith(card);
  });
  const confirmFace = (kind) => {
    s.confirm.className = "ob-c-confirm shown " + kind;
    s.confirm.replaceChildren(icon(kind === "done" ? "check" : "person", 14), kind === "done" ? sc.done : kind === "touch" ? "Windows Hello" : "Confirm");
  };
  // Where the cursor's tip goes: the field, its send button, the answer card, each route card, Confirm.
  const spot = { input: [390, 244], send: [932, 240], info: [404, 533], confirm: [794, CARDS_BOTTOM + 33], card: (k) => [638 + 221 * k, CARDS_BOTTOM - 26] };
  // The stage before the run.
  s.text.textContent = "";
  s.bubbles.forEach((b, k) => { b.textContent = k ? sc.order[s.i] : sc.ask[s.i]; show(b, false); });
  s.info.replaceChildren(
    h("div", { class: "ob-c-info-head" }, h("span", { class: "eyebrow" }, sc.info.label), h("span", { class: "spacer" }),
      sc.info.up !== undefined ? ["1D", "1W", "1M", "1Y"].map((x) => h("span", { class: "ob-c-period" + (x === "1M" ? " on" : "") }, x)) : null),
    h("div", { class: "ob-c-info-title" }, logo(sc.info.asset, 24), h("b", {}, sc.info.title)),
    ...(sc.info.up !== undefined ? [priceLine(sc.info.up)] : [h("div", { class: "ob-c-amount num" }, sc.info.amount), h("div", { class: "secondary small" }, sc.info.note)]));
  s.info.classList.remove("shown", "drawn");
  s.route.replaceChildren(...sc.steps.flatMap((st, k) => [routeCard(st, "ready"), k < 2 ? h("span", { class: "ob-c-arrow" }, icon("arrow.right", 18)) : null]).filter(Boolean));
  [...s.route.children].forEach((c) => show(c, false));
  s.confirm.className = "ob-c-confirm";
  show(s.input, false);
  show(s.cursor, false);
  face("thinking");
  move([1180, 760], 0);
  // Without motion: the end of a run, all at once.
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    show(s.input, true); s.bubbles.forEach((b) => show(b, true)); s.info.classList.add("shown", "drawn");
    [...s.route.children].forEach((c) => show(c, true)); states(["done", "done", "done"]); confirmFace("done");
    return;
  }
  try {
    await pause(900); show(s.cursor, true);
    await pause(300); move(spot.input);
    await pause(800); show(s.input, true);
    await pause(250); await type(sc.ask[s.i]);
    await pause(250); move(spot.send, 0.45);
    await pause(500); s.text.textContent = ""; show(s.bubbles[0], true);
    await pause(100); move(spot.info);
    await pause(750); s.info.classList.add("shown");
    await pause(150); s.info.classList.add("drawn");
    await pause(1700); move(spot.input);
    await pause(800); await type(sc.order[s.i]);
    await pause(250); move(spot.send, 0.45);
    await pause(500); s.text.textContent = ""; show(s.bubbles[1], true);
    // Laid out as the canvas does it: the cursor puts a card down every 420 ms.
    for (let k = 0; k < 3; k++) {
      await pause(k ? 0 : 100); move(spot.card(k), k ? 0.38 : 0.75);
      await pause(k ? 420 : 800); show(s.route.children[k * 2], true); if (k) show(s.route.children[k * 2 - 1], true);
    }
    await pause(550); show(s.cursor, false);
    await pause(250); confirmFace("confirm");
    await pause(500); show(s.cursor, true); move(spot.confirm);
    await pause(850); confirmFace("touch"); face("tuning"); states(["touch", "next", "next"]);
    await pause(1000); states(["done", "now", "next"]); face("thinking");
    await pause(1100); states(["done", "done", "now"]);
    await pause(1000); states(["done", "done", "done"]); confirmFace("done"); face("done");
    await pause(2600); show(s.cursor, false);
    await pause(900);
    s.scenario = (s.scenario + 1) % scenarios.length;
    play(s);
  } catch { /* another run took over */ }
}

// MARK: Dialogs

/** Settings' «Import a recovery phrase» (ImportWalletView): by default only the addresses it makes are shown, to see its
 * balances — nothing saved; with «Save the key in secure storage to sign» on, the phrase and its passphrase go to the
 * signer. Either way the first addresses follow, to compare with MetaMask and Phantom. */
export function showImport() {
  const form = { phrase: "", passphrase: "", store: false };
  const hintText = () => form.store
    ? t(`The phrase goes only to the separate signer, which has no network access. It keeps the key sealed under this computer's Windows Hello and signs only what passes the guard, after Windows Hello. It is added next to your other wallets.`,
      `Фраза уходит только отдельному подписанту, у которого нет доступа к сети. Он хранит ключ запечатанным под Windows Hello этого компьютера и подписывает только то, что прошло проверку, после Windows Hello. Кошелёк добавится к остальным.`)
    : t("The phrase isn't saved or sent anywhere. The app takes only the public addresses from it to show balances. Try the test phrase first.",
      "Фраза нигде не сохраняется и никуда не отправляется. Приложение берёт из неё только публичные адреса, чтобы показать балансы. Сначала попробуй тестовую фразу.");
  const label = () => (form.store ? t("Save and show addresses", "Сохранить и показать адреса") : t("Show addresses", "Показать адреса"));
  const hint = h("div", { class: "secondary small" }, hintText());
  const shown = (accounts, saved) => setOverlay(sheet(stepCard({
    mood: "done", title: saved ? t("Key saved", "Ключ сохранён") : t("Its addresses", "Её адреса"),
    subtitle: saved ? t("The wallet is next to your other wallets. Check its addresses against the wallet you came from.", "Кошелёк добавлен к остальным. Сверь его адреса с кошельком, из которого ты пришёл.")
      : t("Nothing was saved. Watch address #0 to see its balances here. Nothing goes to the signer.", "Ничего не сохранено. Следи за адресом #0, чтобы видеть его балансы здесь. Подписанту ничего не уходит."),
    content: h("div", { class: "ob-content" }, compareTable(accounts),
      saved ? null : h("button", { class: "btn small", title: t("Puts address #0 into «Addresses for balances» — nothing goes to the signer", "Добавит адрес #0 в «Адреса для балансов» — подписанту ничего не уходит"),
        onclick: async () => { if (await act("watch", { solana: accounts[0].solana, evm: accounts[0].evm }) !== null) { toast(t("Watching the balances of address #0.", "Слежу за балансами адреса #0.")); closeOverlay(); } } },
        icon("eye", 13), t("Watch #0", "Следить за #0"))),
    primary: t("Done", "Готово"), onPrimary: closeOverlay,
  })));
  const run = async () => {
    const phrase = form.phrase.trim().toLowerCase().split(/\s+/).join(" "), passphrase = form.passphrase;
    form.phrase = form.passphrase = "";
    const r = await act(form.store ? "importPhrase" : "phraseAccounts", { phrase, passphrase });
    if (r === null) return;
    const accounts = parseAccounts(r);
    if (!accounts?.length) { closeOverlay(); return; }
    shown(accounts, form.store);
  };
  const card = stepCard({
    mood: "tuning", title: t("Import a recovery phrase", "Импорт фразы восстановления"),
    subtitle: t("12 or 24 words, in order, from MetaMask, Phantom or another wallet.", "12 или 24 слова по порядку — из MetaMask, Phantom или другого кошелька."),
    content: h("div", { class: "ob-content" },
      h("textarea", { id: "import-secret", class: "address ob-field secret", rows: "4", autocomplete: "off", spellcheck: "false", placeholder: t("12 or 24 words separated by spaces", "12 или 24 слова через пробел"), oninput: (e) => (form.phrase = e.target.value) }),
      h("input", { id: "import-passphrase", type: "password", class: "ob-field", autocomplete: "off", spellcheck: "false", placeholder: t("Extra passphrase, if any", "Дополнительная парольная фраза, если есть"), oninput: (e) => (form.passphrase = e.target.value) }),
      h("label", { class: "ob-option" }, toggle("import-store", false, (on) => { form.store = on; hint.textContent = hintText(); primaryEl().textContent = label(); }),
        h("span", {}, t("Save the key in secure storage to sign", "Сохранить ключ в защищённом хранилище, чтобы подписывать"))),
      hint),
    primary: label(), onPrimary: run,
    quiet: quietButton(t("Back", "Назад"), closeOverlay),
  });
  const primaryEl = () => card.querySelector(".ob-primary");
  setOverlay(sheet(card));
  setTimeout(() => document.getElementById("import-secret")?.focus(), 0);
}

/** A single private key: Phantom's Solana key or MetaMask's EVM key, recognized as it's pasted and shown as its address
 * to compare before anything is saved (ImportWalletView.keyImport); only «Save as a wallet» sends it to the signer. */
export function showImportKey() {
  let value = "", asked = 0, read = null;
  const done = () => { value = ""; closeOverlay(); };
  // Saved: said in words, with the address it signs for (the Mac's «Key saved as a wallet: … It signs now.»).
  const run = async () => {
    if (await act("importKey", { key: value.trim() }) === null) return;
    done();
    toast(read ? t(`Key saved as a wallet: ${read}. It signs now.`, `Ключ сохранён как кошелёк: ${read}. Теперь подписывает он.`) : t("Key saved as a wallet. It signs now.", "Ключ сохранён как кошелёк. Теперь подписывает он."));
  };
  const idle = () => h("div", { class: "secondary small" }, t("The key goes only to the separate signer, which seals it under Windows Hello like a phrase and never shows it again.",
    "Ключ уходит только отдельному подписанту, который запечатывает его под Windows Hello, как фразу, и больше никогда не показывает."));
  const found = h("div", { class: "ob-key-found" }, idle());
  const check = async () => {
    const n = ++asked, key = value.trim();
    const save = card.querySelector(".ob-primary");
    if (!key) { found.replaceChildren(idle()); save.disabled = true; return; }
    try {
      const r = await engine("keyAddress", { key });
      read = r.address;
      if (n !== asked) return;
      const other = r.chain === "Solana" ? "Phantom" : "MetaMask";
      found.replaceChildren(
        h("div", { class: "ob-key-address" }, h("span", { class: "secondary small" }, r.chain), h("span", { class: "address selectable" }, r.address)),
        h("div", { class: "secondary small" }, t(`Compare it with ${other} before saving. This wallet signs only on ${r.chain}. For the other chains, switch wallets.`,
          `Сверь его с ${other}, прежде чем сохранять. Этот кошелёк подписывает только в ${r.chain}. Для других сетей переключай кошельки.`)));
      save.disabled = false;
    } catch (e) {
      if (n !== asked) return;
      found.replaceChildren(h("div", { class: "loss small" }, e.message));
      save.disabled = true;
    }
  };
  const card = stepCard({
    mood: "tuning", title: t("Import a private key", "Импорт приватного ключа"),
    subtitle: t("One key from Phantom (Solana) or MetaMask (EVM): it opens that network only.", "Один ключ из Phantom (Solana) или MetaMask (EVM): он открывает только эту сеть."),
    content: h("div", { class: "ob-content" },
      h("textarea", { id: "import-secret", class: "address ob-field secret", rows: "3", autocomplete: "off", spellcheck: "false", placeholder: t("Solana key from Phantom, or EVM key from MetaMask", "Ключ Solana из Phantom или EVM-ключ из MetaMask"),
        oninput: (e) => { value = e.target.value; check(); } }),
      found),
    primary: t("Save as a wallet", "Сохранить как кошелёк"), onPrimary: run, enabled: false,
    quiet: quietButton(t("Back", "Назад"), done),
  });
  setOverlay(sheet(card));
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
    if (back) { ui.overlay = null; ui.view = "canvas"; }
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

/** ihatecrypto's browser: a site opens beside the chat with the wallet in it (its panel can move to its own window); what it
 * asks for comes here as a card. */
export function browserDialog() {
  let value = "";
  const sites = [["jup.ag", "jup"], ["kamino.com", "kmno"], ["raydium.io", "ray"], ["app.aave.com", "aave"], ["app.uniswap.org", "uni"], ["app.hyperliquid.xyz", "hype"]];
  const open = async (text) => {
    const url = /^https:\/\//.test(text) ? text : "https://" + text.replace(/^\w+:\/\//, "");
    try { await invoke("open_browser", { url }); closeOverlay(); } catch (e) { toast(String(e)); }
  };
  setOverlay(sheet(stepCard({
    mood: "tuning", title: t("Open a site with your wallet", "Открыть сайт со своим кошельком"),
    subtitle: t("The site opens beside the chat. It sees your address only after you connect it, and every signature it asks for comes to the canvas as a card: nothing is signed without Confirm and Windows Hello. Sites on MetaMask's phishing list get no wallet.",
      "Сайт откроется рядом с чатом. Он увидит адрес, только когда ты его подключишь, а каждая подпись, которую он просит, приходит на холст карточкой: без «Подтвердить» и Windows Hello ничего не подписывается. Сайты из списка фишинга MetaMask кошелёк не получают."),
    content: h("div", { class: "ob-content" },
      h("input", { id: "site-url", class: "ob-field", placeholder: "jup.ag", oninput: (e) => (value = e.target.value.trim()), onkeydown: (e) => { if (e.key === "Enter" && value) open(value); } }),
      h("div", { class: "ob-site-chips" }, sites.map(([s, l]) => h("button", { class: "ob-site-chip", onclick: () => open(s) }, logo(l, 18), s)))),
    primary: t("Open", "Открыть"), onPrimary: () => value && open(value),
    quiet: quietButton(t("Back", "Назад"), closeOverlay),
  })));
  setTimeout(() => document.getElementById("site-url")?.focus(), 0);
}
