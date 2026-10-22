// The window's page: it draws what the engine says (the «state» event after every change) and sends it what the person
// did. It holds no key and no money logic: a move is a card the engine made, and Confirm is the only way it runs.

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);

let state = null;
let view = "chat";
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

/** An icon drawn from a fixed path: nothing the engine sends becomes markup. */
function icon(name) {
  const paths = {
    chat: "M4 5h16v11H8l-4 4z",
    portfolio: "M4 19V9m6 10V5m6 14v-7m4 7H2",
    receive: "M12 4v12m0 0-5-5m5 5 5-5M5 20h14",
    book: "M6 4h11a1 1 0 0 1 1 1v15H7a2 2 0 0 1-2-2V5a1 1 0 0 1 1-1zm-1 14a2 2 0 0 1 2-2h11",
    history: "M12 7v5l3 2M4 12a8 8 0 1 0 2.3-5.7M4 4v4h4",
    settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm8-3 2 1-2 3.5-2.2-.6a7 7 0 0 1-1.6 1L16 19h-4l-.6-2.1a7 7 0 0 1-1.6-1l-2.2.6L5.6 13l2-1a7 7 0 0 1 0-2l-2-1L7.6 5.5l2.2.6a7 7 0 0 1 1.6-1L12 3h4l.6 2.1a7 7 0 0 1 1.6 1l2.2-.6L22.4 9l-2 1a7 7 0 0 1 0 2z",
  };
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
  p.setAttribute("d", paths[name] || "");
  svg.append(p);
  return svg;
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

function link(url, text) {
  return h("a", { href: "#", onclick: (e) => { e.preventDefault(); invoke("open_link", { url }).catch((x) => toast(String(x))); } }, text);
}

// MARK: Drawing

const app = document.getElementById("app");
let composerText = "";

function render() {
  if (!state) return;
  const scroller = document.querySelector(".chat");
  const atBottom = !scroller || scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;
  const focused = document.activeElement?.id;
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
  invoke("protect", { on: false }).catch(() => {});
  if (!state.wallets.length) {
    app.style.gridTemplateColumns = "1fr";
    app.append(welcome());
    return;
  }
  app.style.gridTemplateColumns = "";
  app.append(rail(), h("div", { class: "main" }, page()));
  if (view === "chat") {
    const chat = document.querySelector(".chat");
    if (chat && atBottom) chat.scrollTop = chat.scrollHeight;
    if (focused === "composer") document.getElementById("composer")?.focus();
  }
}

function rail() {
  const items = [
    ["chat", t("Chat", "Чат")], ["portfolio", t("Portfolio", "Портфель")], ["receive", t("Receive", "Получить")],
    ["book", t("Addresses", "Адреса")], ["history", t("History", "История")], ["settings", t("Settings", "Настройки")],
  ];
  const active = state.wallets.find((w) => w.id === state.active) || state.wallets[0];
  return h("nav", { class: "rail" },
    h("div", { class: "brand" }, h("img", { src: "coin.png", alt: "" }), h("span", { class: "brand-name" }, "ihatecrypto")),
    items.map(([id, label]) => h("button", { class: "nav" + (view === id ? " on" : ""), onclick: () => { view = id; render(); } }, icon(id), h("span", { class: "label" }, label))),
    h("div", { class: "spacer" }),
    h("button", { class: "wallet-chip", onclick: walletMenu },
      h("span", { class: "dot" + (active?.test || active?.automation ? " test" : "") }),
      h("span", { class: "name" }, active ? active.label : t("No wallet", "Нет кошелька"))),
  );
}

function walletMenu(e) {
  document.querySelector(".menu")?.remove();
  const r = e.currentTarget.getBoundingClientRect();
  const close = () => menu.remove();
  const menu = h("div", { class: "menu", style: `left:${r.left}px; bottom:${window.innerHeight - r.top + 6}px` },
    state.wallets.map((w) => h("div", { class: "item" },
      h("span", {}, w.label, w.test ? h("span", { class: "muted small" }, " · test") : null),
      w.id === state.active ? h("span", { class: "check" }, "✓")
        : h("button", { class: "btn", onclick: async () => { close(); await act("switchWallet", { wallet: w.id }); } }, t("Switch", "Переключить")))),
    h("div", { class: "item" }, h("button", { class: "btn", onclick: () => { close(); view = "settings"; render(); } }, t("Wallets…", "Кошельки…"))),
  );
  document.body.append(menu);
  setTimeout(() => document.addEventListener("click", function off(ev) { if (!menu.contains(ev.target)) { menu.remove(); document.removeEventListener("click", off); } }), 0);
}

function page() {
  switch (view) {
    case "portfolio": return portfolio();
    case "receive": return receive();
    case "book": return addresses();
    case "history": return history();
    case "settings": return settings();
    default: return chat();
  }
}

// MARK: Chat

function chat() {
  const s = state;
  const items = s.chat.map((m) => h("div", { class: "msg " + (m.fromUser ? "user" : "app") }, m.text, m.note ? h("span", { class: "note" }, m.note) : null));
  if (s.thinking) items.push(h("div", { class: "thinking" }, s.draft || h("span", { class: "dots" }, t("Reading", "Читаю"))));
  if (s.running) {
    items.push(h("div", { class: "move running" }, h("h3", {}, s.running.title), h("ul", {}, s.running.lines.map((l) => h("li", {}, l))),
      s.signer.waiting ? h("div", { class: "muted" }, t("Windows Hello is asking for you…", "Windows Hello спрашивает тебя…")) : null));
  }
  if (s.card) {
    items.push(h("div", { class: "move" },
      h("h3", {}, s.card.title),
      h("ul", {}, s.card.lines.map((l) => h("li", {}, l)), s.card.warnings.map((w) => h("li", { class: "warn" }, w))),
      h("div", { class: "btns" },
        h("button", { class: "btn primary", disabled: s.thinking || s.running, onclick: () => act("confirm") }, s.card.confirm),
        h("button", { class: "btn", onclick: () => act("cancel") }, t("Cancel", "Отмена")))));
  }
  if (s.options.length && !s.thinking) {
    items.push(h("div", { class: "options" }, s.options.map((o, i) =>
      h("button", { class: "option" + (o.recommended ? " rec" : ""), onclick: () => act("pick", { index: i }) }, h("b", {}, o.title), o.comment ? h("span", {}, o.comment) : null))));
  }
  if (!s.chat.length) {
    items.push(h("div", { class: "empty" }, t("Tell me what to do: “buy SOL for 10 USDC”, “send 5 USDC to mom”, “long ETH risk $5”. Nothing moves until you press Confirm and Windows Hello asks for you.",
      "Скажи, что сделать: «купи SOL на 10 USDC», «отправь 5 USDC маме», «лонг ETH риск $5». Ничего не двигается, пока ты не нажмёшь «Подтвердить» и Windows Hello не спросит тебя.")));
  }
  const field = h("textarea", {
    id: "composer", rows: "1", placeholder: t("Message", "Сообщение"),
    oninput: (e) => { composerText = e.target.value; autosize(e.target); },
    onkeydown: (e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendComposer(); } },
  });
  field.value = composerText;
  setTimeout(() => autosize(field), 0);
  const m = s.model;
  return [
    h("div", { class: "chat" }, h("div", { class: "chat-inner" }, items)),
    h("div", { class: "composer" },
      h("div", { class: "composer-inner" }, field, h("button", { class: "btn primary", disabled: s.thinking, onclick: sendComposer }, t("Send", "Отправить"))),
      h("div", { class: "model-line" }, m.connected ? `${m.name} · ${m.model || ""}` : t("No model connected: plain commands only. Settings › Model.", "Модель не подключена: только простые команды. Настройки › Модель."))),
  ];
}

function autosize(el) { el.style.height = "auto"; el.style.height = Math.min(el.scrollHeight, 160) + "px"; }

function sendComposer() {
  const text = composerText.trim();
  if (!text || state.thinking) return;
  composerText = "";
  act("send", { text });
  render();
}

// MARK: Portfolio, Receive, Addresses, History

function portfolio() {
  const p = state.portfolio;
  return h("div", { class: "page" }, h("div", { class: "narrow" },
    h("h1", {}, t("Portfolio", "Портфель")),
    h("div", { class: "total" }, usd(p.total)),
    h("div", { class: "muted small" }, p.updatedAt ? t("Updated ", "Обновлено ") + new Date(p.updatedAt).toLocaleTimeString() : t("Reading the networks…", "Читаю сети…"),
      " · ", h("a", { href: "#", onclick: (e) => { e.preventDefault(); act("refresh"); } }, t("Refresh", "Обновить"))),
    state.positions.length ? [h("h2", {}, t("Open positions", "Открытые позиции")), h("div", { class: "card" }, state.positions.map((x) =>
      h("div", { class: "row" }, h("span", { class: "sym" }, `${x.side} ${x.coin}`), h("span", { class: "num" }, `${x.size} @ ${x.entry} · ${x.pnl}`))))] : null,
    h("h2", {}, t("By network", "По сетям")),
    p.networks.length ? h("div", { class: "cards" }, p.networks.map((n) => h("div", { class: "card" },
      h("div", { class: "row" }, h("b", {}, n.name), h("b", { class: "num" }, usd(n.usd))),
      n.rows.map((r) => h("div", { class: "row" },
        h("span", { class: "sym" }, r.icon ? h("img", { src: r.icon, alt: "" }) : null, r.symbol),
        h("span", { class: "num" }, r.amount, r.usd !== undefined && r.usd !== null ? h("span", { class: "muted small" }, "  " + usd(r.usd)) : null))))))
      : h("div", { class: "empty" }, t("Nothing here yet. Receive shows where to send money.", "Пока пусто. Во вкладке «Получить» — куда отправить деньги.")),
    p.errors.length ? h("p", { class: "muted small" }, t("Didn't answer: ", "Не ответили: ") + p.errors.join("; ")) : null,
  ));
}

function receive() {
  const a = state.addresses;
  const rows = [
    ["Solana", a.solana, t("USDC and SOL (recommended), and any Solana token", "USDC и SOL (рекомендую) и любые токены Solana")],
    ["Base · Arbitrum · Ethereum · Polygon · Optimism · BNB · Hyperliquid", a.evm, t("The same address on every EVM network: send on the network you mean", "Один адрес во всех EVM-сетях: отправляй в той сети, которую имеешь в виду")],
    ["NEAR", a.near, t("NEAR", "NEAR")],
  ].filter((r) => r[1]);
  return h("div", { class: "page" }, h("div", { class: "narrow" },
    h("h1", {}, t("Receive", "Получить")),
    h("p", { class: "muted" }, t("ihatecrypto can't pull money in: send it here from an exchange or another wallet, on that network.",
      "ihatecrypto не может сам забрать деньги: отправь их сюда с биржи или другого кошелька, именно в этой сети.")),
    rows.map(([name, address, note]) => h("div", { class: "card", style: "margin-bottom:12px" },
      h("b", {}, name), h("p", { class: "muted small" }, note),
      h("div", { class: "address" }, address),
      h("div", { class: "btns", style: "margin-top:10px" }, h("button", { class: "btn", onclick: () => copy(address) }, t("Copy", "Копировать"))))),
  ));
}

const networks = ["solana", "base", "arbitrum", "ethereum", "polygon", "optimism", "bnb", "hyperliquid", "hyperevm", "robinhood", "tempo", "near",
  "bitcoin", "tron", "xrp", "dogecoin", "zcash", "sui"];

function addresses() {
  const form = { name: "", network: "solana", address: "", memo: "" };
  return h("div", { class: "page" }, h("div", { class: "narrow" },
    h("h1", {}, t("Addresses", "Адреса")),
    h("p", { class: "muted" }, t("Money goes only to saved addresses. Saving one asks Windows Hello with the whole address on its window: check every character.",
      "Деньги уходят только на сохранённые адреса. Сохранение спрашивает Windows Hello, и в его окне весь адрес — сверь каждый символ.")),
    state.book.length ? h("div", { class: "card" }, state.book.map((e) => h("div", { class: "row" },
      h("div", {}, h("b", {}, e.name), h("span", { class: "muted small" }, "  " + e.network), h("div", { class: "address" }, e.address), e.memo ? h("div", { class: "muted small" }, "memo " + e.memo) : null),
      h("button", { class: "btn danger", onclick: () => act("removeAddress", { name: e.name, network: e.network }) }, t("Remove", "Удалить")))))
      : h("div", { class: "empty" }, t("No saved addresses yet.", "Сохранённых адресов пока нет.")),
    h("h2", {}, t("Add an address", "Добавить адрес")),
    h("div", { class: "card" },
      h("div", { class: "field" }, h("label", {}, t("Name", "Имя")), h("input", { oninput: (e) => (form.name = e.target.value), placeholder: "Bybit" })),
      h("div", { class: "field" }, h("label", {}, t("Network", "Сеть")),
        h("select", { onchange: (e) => (form.network = e.target.value) }, networks.map((n) => h("option", { value: n }, n)))),
      h("div", { class: "field" }, h("label", {}, t("Address", "Адрес")), h("input", { class: "address", oninput: (e) => (form.address = e.target.value.trim()) })),
      h("div", { class: "field" }, h("label", {}, t("Memo / tag (exchanges)", "Memo / тег (для бирж)")), h("input", { oninput: (e) => (form.memo = e.target.value.trim()) })),
      h("button", { class: "btn primary", onclick: async () => {
        if (await act("saveAddress", form) !== null) toast(t("Saved.", "Сохранено."));
      } }, t("Save with Windows Hello", "Сохранить через Windows Hello"))),
  ));
}

function history() {
  return h("div", { class: "page" }, h("div", { class: "narrow" },
    h("h1", {}, t("History", "История")),
    h("p", { class: "muted small" }, t("Everything signed, written before it was sent.", "Всё подписанное, записанное до отправки.")),
    state.history.length ? h("div", { class: "card" }, state.history.map((e) => h("div", { class: "row" },
      h("div", {}, h("b", {}, e.note || e.chain), h("div", { class: "muted small" }, `${new Date(e.at).toLocaleString()} · ${e.chain} · ${e.kind}`)),
      e.link ? link(e.link, t("Explorer", "Обозреватель")) : null)))
      : h("div", { class: "empty" }, t("Nothing signed yet.", "Пока ничего не подписано.")),
  ));
}

// MARK: Settings

function settings() {
  const s = state;
  const choice = { provider: s.model.provider || "anthropic", key: "", model: s.model.model || "" };
  const providers = s.providers.filter((p) => !p.unavailable);
  const chosen = () => s.providers.find((p) => p.id === choice.provider);
  const sessions = [[60, t("One move (1 min)", "Один перевод (1 мин)")], [300, "5 min"], [900, "15 min"], [1800, "30 min"]];
  return h("div", { class: "page" }, h("div", { class: "narrow" },
    h("h1", {}, t("Settings", "Настройки")),

    h("h2", {}, t("Model", "Модель")),
    h("div", { class: "card" },
      h("p", { class: "muted small" }, t("The chat's model reads what you write. It never signs: a move is a card, and only Confirm runs it.",
        "Модель чата читает, что ты пишешь. Она никогда не подписывает: действие — это карточка, и запускает её только «Подтвердить».")),
      h("div", { class: "field" }, h("label", {}, t("Provider", "Провайдер")),
        h("select", { onchange: (e) => { choice.provider = e.target.value; } }, providers.map((p) =>
          h("option", { value: p.id, selected: p.id === choice.provider }, p.name + (p.connected ? " ✓" : ""))))),
      h("div", { class: "field" }, h("label", {}, t("API key (empty: a login another app already has, or a local model)", "API-ключ (пусто — вход, который уже есть у другой программы, или локальная модель)")),
        h("input", { type: "password", autocomplete: "off", oninput: (e) => (choice.key = e.target.value) })),
      h("div", { class: "field" }, h("label", {}, t("Model id (empty: the provider's default)", "Модель (пусто — по умолчанию у провайдера)")),
        h("input", { value: choice.model, oninput: (e) => (choice.model = e.target.value.trim()) })),
      h("div", { class: "btns" },
        h("button", { class: "btn primary", onclick: async () => {
          const r = await act("connectModel", { provider: choice.provider, key: choice.key || undefined, model: choice.model || undefined });
          if (r !== null) toast(t("Connected.", "Подключено."));
        } }, t("Connect", "Подключить")),
        s.model.connected ? h("button", { class: "btn", onclick: () => act("disconnectModel", { provider: s.model.provider }) }, t("Disconnect", "Отключить")) : null)),

    h("h2", {}, t("Language", "Язык")),
    h("div", { class: "card" }, h("select", { onchange: (e) => act("language", { language: e.target.value }) },
      [["auto", t("As I write", "Как я пишу")], ["en", "English"], ["ru", "Русский"]].map(([v, l]) => h("option", { value: v, selected: s.language === v }, l)))),

    h("h2", {}, t("Signing", "Подпись")),
    h("div", { class: "card" },
      s.signer.notes.map((n) => h("p", { class: "small" }, n)),
      s.signer.error ? h("p", { class: "small", style: "color:var(--danger)" }, s.signer.error) : null,
      h("div", { class: "field" }, h("label", {}, t("One Windows Hello covers", "Одно подтверждение Windows Hello действует")),
        h("select", { onchange: (e) => act("sessionLength", { seconds: Number(e.target.value) }) },
          sessions.map(([v, l]) => h("option", { value: v, selected: s.signer.sessionLength === v }, l)))),
      h("button", { class: "btn", onclick: () => act("lock") }, t("Lock now", "Заблокировать сейчас"))),

    h("h2", {}, t("Wallets", "Кошельки")),
    h("div", { class: "card" },
      s.wallets.map((w) => h("div", { class: "row" },
        h("div", {}, h("b", {}, w.label), w.test ? h("span", { class: "muted small" }, " · test, signs without Windows Hello") : null,
          w.solana ? h("div", { class: "address small" }, "Solana " + w.solana) : null, w.evm ? h("div", { class: "address small" }, "EVM " + w.evm) : null),
        h("div", { class: "btns" },
          w.id === s.active ? h("span", { class: "check" }, "✓") : h("button", { class: "btn", onclick: () => act("switchWallet", { wallet: w.id }) }, t("Switch", "Переключить")),
          h("button", { class: "btn danger", onclick: () => act("removeWallet", { wallet: w.id }) }, t("Remove", "Удалить"))))),
      h("div", { class: "btns", style: "margin-top:12px" },
        h("button", { class: "btn", onclick: () => act("createWallet", { test: false }) }, t("New wallet", "Новый кошелёк")),
        h("button", { class: "btn", onclick: showImport }, t("Import a phrase", "Импортировать фразу")),
        h("button", { class: "btn", onclick: () => act("createWallet", { test: true }) }, t("New test wallet", "Новый тестовый")))),

    h("h2", {}, t("Chat", "Чат")),
    h("div", { class: "card" }, h("button", { class: "btn", onclick: () => act("clearChat") }, t("Clear the chat", "Очистить чат"))),
  ));
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
  invoke("protect", { on: true }).catch(() => {});
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
      h("button", { class: "btn primary", onclick: async () => { phraseCopied = false; await act("phraseSaved"); invoke("protect", { on: false }).catch(() => {}); } },
        t("I wrote them down", "Я записал")))));
}

// MARK: Start

engine("hello").catch((e) => toast(e.message));
