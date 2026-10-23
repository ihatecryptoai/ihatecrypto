// The blocks on the canvas, as the Mac's plan column draws them (Studio.swift plan, RouteCards.swift, Canvases.swift):
// the newest on top; a money move as its route's cards — a picture of what the step does with the coin in it wearing the
// step's face, the state in words on it, the balance it changes — with Cancel and Confirm while it waits; an earlier move
// folded to a one-line pill. Live blocks (receive, schedule, favorites…) are live.js's, in this file's frame.

import { ui, act, h, svg, icon, addIcons, mascot, logo, t, suggest, popup, copy, render, presence } from "./core.js";
import { liveBlock, liveKinds, liveTitle } from "./live.js";

addIcons({
  hello: "M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8M16 4h2.5A1.5 1.5 0 0 1 20 5.5V8M20 16v2.5a1.5 1.5 0 0 1-1.5 1.5H16M8 20H5.5A1.5 1.5 0 0 1 4 18.5V16M9 9.5v1M15 9.5v1M9 15c1.6 1.4 4.4 1.4 6 0",
  signature: "M3 16c3-6 5-9 7-9 2.5 0-1 9 1.5 9 1.5 0 2.5-3 4-3 1 0 1 2 2.5 2H21M3 20.5h18",
  "chart.line.uptrend": "M3 17l6-6 4 4 8-8M15 7h6v6",
  "arrow.down.right": "M7 7l10 10M17 9v8H9",
  percent: "M19 5L5 19M7.5 9a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM16.5 18a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z",
  "building.columns": "M3 9.5L12 4l9 5.5M5 10v8M9.5 10v8M14.5 10v8M19 10v8M3 20h18",
  "arrow.uturn.left": "M9 14L4 9l5-5M4 9h10.5a5.5 5.5 0 0 1 0 11H11",
  laptop: "M5 6h14v9H5zM3 18h18",
  iphone: "M8 3h8a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM11 18h2",
  ledger: "M4 6h7v12H4zM11 6h9v5h-9M15 14h5v4h-5",
  "info.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5.5M12 7.5v.5",
});

// MARK: The column

/** Blocks already drawn: only a new one fades in, so a redraw never blinks the canvas. */
const seen = new Set();
let seenReady = false;

export function planColumn() {
  const boards = ui.state.boards;
  // The newest move is shown whole, as the Mac's board of the canvas; the ones before it fold to pills.
  const newest = boards.find((b) => b.kind === "move")?.id;
  const items = boards.length ? boards.map((b) => {
    const el = drawBoard(b, b.id === newest);
    if (!seen.has(b.id)) { seen.add(b.id); if (seenReady) el.classList.add("fresh"); }
    return el;
  }) : [starter()];
  // What was on the canvas when it opened is already there; only what comes after fades in.
  setTimeout(() => { seenReady = true; }, 0);
  return h("div", { class: "route-plan" }, items);
}

export function blockTitle(b) {
  return liveKinds.includes(b.kind) ? liveTitle(b.kind) : b.title;
}

function drawBoard(b, newest) {
  if (b.kind === "move") return moveBoard(b, newest);
  if (liveKinds.includes(b.kind)) return liveBlock(b);
  if (b.kind === "chart" && b.chart) return chartBlock(b);
  return resultBlock(b);
}

/** A block's frame: the Mac's board card with its glyph and title, its own buttons and the close; right-click brings it into the chat. */
export function blockFrame(b, title, body, { extra, width, tint, icon: glyph } = {}) {
  const el = h("div", { class: "route-block" + (ui.attached === b.id ? " route-attached" : ""), oncontextmenu: (e) => blockMenu(e, b, title) },
    h("div", { class: "route-block-head" }, glyph ? h("span", { class: "route-block-icon" }, icon(glyph, 16)) : null,
      h("b", { class: "route-block-title one-line" }, title), h("span", { class: "spacer" }), extra || null,
      h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => act("closeBoard", { board: b.id }) }, icon("xmark"))),
    body);
  if (width) el.style.width = width + "px";
  if (tint) el.style.background = tint;
  return el;
}

/** Right-click on any block (BlockMenu): the chat with it attached, or «What's going on here?» straight away. */
function blockMenu(e, b, title, more = []) {
  e.preventDefault();
  const on = ui.attached === b.id;
  popup(e, [
    on ? [t("Take out of the chat", "Убрать из чата"), () => { ui.attached = null; render(); }]
      : [t("Ask or act in chat", "Спросить или поручить в чате"), () => attach(b)],
    [t("What's going on here?", "Что здесь происходит?"), () => act("send", { text: t(`What's going on in «${title}»?`, `Что происходит в «${title}»?`), board: b.id })],
    ...more,
  ]);
}

function attach(b) {
  ui.attached = b.id;
  render();
  document.getElementById("composer")?.focus();
}

/** «Oct 22 at 10:38 PM», as the Mac's Fmt.date. */
function fmtDate(iso) {
  const d = new Date(iso), ru = ui.state.russian, locale = ru ? "ru-RU" : "en-US";
  return d.toLocaleDateString(locale, { month: "short", day: "numeric" }) + (ru ? " в " : " at ") + d.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
}

// MARK: A move

function moveBoard(b, newest) {
  const s = ui.state;
  // Running wins over the board's own state and the card: the engine can send both for a moment.
  const runningHere = s.runningBoard === b.id;
  const waitingHere = !runningHere && !!s.card && s.card.id === b.id;
  const finished = !waitingHere && !runningHere;
  // A click on a finished move flips it from how it shows by default: the newest open, the others folded.
  const open = !finished || newest !== ui.opened.has("move:" + b.id);
  const toggle = () => { ui.opened.has("move:" + b.id) ? ui.opened.delete("move:" + b.id) : ui.opened.add("move:" + b.id); render(); };
  const menu = (e) => blockMenu(e, b, b.title, [
    [t("Copy details", "Скопировать подробности"), () => copy(moveText(b))],
    finished ? [t("Remove from the canvas", "Убрать с холста"), () => act("closeBoard", { board: b.id })] : null,
  ].filter(Boolean));
  const steps = b.steps || [];
  const draft = waitingHere && s.multisigDraft;
  const states = steps.map((_, i) => stepState(b, i, waitingHere, runningHere));
  const route = !steps.length || draft ? null : routeCards(b, steps, states);

  if (finished && !newest) {
    // An earlier move: its pill, and its route under it once opened.
    return h("div", { class: "route-move" + (ui.attached === b.id ? " route-attached" : ""), oncontextmenu: menu },
      pill(b, open, toggle), open ? route : null, open ? notes(b) : null);
  }
  const status = waitingHere ? [t("ready to sign", "готово к подписи"), "profit"] : runningHere ? [t("in progress", "выполняется"), "route-accent"]
    : { done: [t("executed", "выполнено"), "profit"], failed: [t("failed", "не вышло"), "loss"], cancelled: [t("not signed", "не подписано"), "secondary"] }[b.state] || [b.state || "", "secondary"];
  return h("div", { class: "route-move" + (ui.attached === b.id ? " route-attached" : ""), oncontextmenu: menu },
    h("div", { class: "route-move-head" },
      h("span", { class: "route-move-title" }, b.title),
      h("span", { class: "route-status-chip " + status[1] }, status[0]),
      runningHere ? h("span", { class: "route-rc-spinner" }) : null,
      h("span", { class: "spacer" }),
      waitingHere ? h("div", { class: "btns route-nowrap" },
        h("button", { class: "btn route-large", onclick: () => act("cancel") }, t("Cancel", "Отмена")),
        h("button", { class: "btn primary route-large", disabled: s.thinking || !!s.runningBoard, onclick: () => act("confirm") }, icon("hello"), s.card.confirm)) : null),
    finished ? outcome(b) : null,
    draft ? multisigSetup(draft) : route,
    waitingHere && s.card.warnings.length ? h("div", { class: "route-warn-box", "data-no-pan": "" }, icon("exclamationmark.triangle", 14),
      h("div", {}, s.card.warnings.map((w) => h("div", {}, w)))) : null,
    runningHere ? signing(b) : null,
    notes(b));
}

/** «✓ Executed · Oct 22 at 10:38 PM» over a finished move's cards. */
function outcome(b) {
  const [text, cls, mark] = { done: [t("Executed", "Выполнено"), "profit", true], failed: [t("Failed", "Не вышло"), "loss", false] }[b.state]
    || [t("Not signed", "Не подписано"), "secondary", null];
  return h("div", { class: "route-outcome " + cls }, statusMark(mark), text + " · " + fmtDate(b.at));
}

/** The move's own lines under its route (what's signed, how it went), as the Mac's route footer. */
function notes(b) {
  const lines = (b.lines || []).filter((l) => !l.startsWith("⚠ "));
  return lines.length ? h("div", { class: "route-note", "data-no-pan": "" }, icon("signature", 15), h("div", {}, lines.map((l) => h("div", {}, l)))) : null;
}

function pill(b, open, toggle) {
  const mark = b.state === "done" ? true : b.state === "failed" ? false : null;
  return h("button", { class: "route-move-pill", title: open ? t("Hide its route", "Скрыть маршрут") : t("Show its route", "Показать маршрут"), onclick: toggle },
    statusMark(mark), h("span", { class: "route-pill-title one-line" }, b.title), h("span", { class: "route-when" }, fmtDate(b.at)),
    h("span", { class: "route-chev" + (open ? " route-open" : "") }, icon("chevron.right", 11)));
}

/** A filled circle with a check (done), an octagon with a cross (failed), a dash (nothing signed). */
function statusMark(ok) {
  const g = svg("svg", { viewBox: "0 0 20 20", class: "route-mark " + (ok === true ? "profit" : ok === false ? "loss" : "secondary") });
  if (ok === false) g.append(svg("path", { d: "M6.7 1.5h6.6l5.2 5.2v6.6l-5.2 5.2H6.7l-5.2-5.2V6.7z", class: "route-mark-bg" }), svg("path", { d: "M7 7l6 6M13 7l-6 6", class: "route-mark-ink" }));
  else g.append(svg("circle", { cx: 10, cy: 10, r: 9, class: "route-mark-bg" }), svg("path", { d: ok ? "M6 10.3l2.7 2.7L14 7.5" : "M6.5 10h7", class: "route-mark-ink" }));
  return g;
}

function moveText(b) {
  return [b.title, ...(b.steps || []).map((s, i) => `${i + 1}. ${s.label} · ${s.title}` + (s.money ? ` (${s.money.place} ${s.money.was} → ${s.money.after} ${s.money.symbol})` : "")
    + (s.lines.length ? "\n   " + s.lines.join("\n   ") : "")), ...(b.lines || [])].join("\n");
}

/** Where a step is: waiting for Confirm, done, the one a running move is on (or waiting for Windows Hello), still to come,
 * failed, or plain when nothing was signed. */
function stepState(b, i, waitingHere, runningHere) {
  if (runningHere) {
    const current = currentStep(b.steps, ui.state.running?.lines || []);
    return i < current ? "done" : i > current ? "next" : ui.state.signer.waiting ? "touch" : "now";
  }
  if (waitingHere) return "ready";
  return { done: "done", failed: "failed" }[b.state] || "plain";
}

/** The step a running move is on: the furthest one whose place a report line has named (RunningBoard.currentStep). */
// ponytail: matches place names in the engine's lines; a line naming the destination early lights it up early.
function currentStep(steps, lines) {
  const said = lines.join(" ").toLowerCase();
  let current = 0;
  steps.forEach((s, i) => {
    const place = (s.label.split("·")[1] || "").trim().split(" ")[0].toLowerCase();
    if (/^\p{L}/u.test(place) && said.includes(place)) current = i;
  });
  return current;
}

/** Signing now: Windows Hello's turn in orange, the Ledger's in black, and the engine's last line. */
function signing(b) {
  const s = ui.state;
  const last = s.running?.lines?.at(-1) || t("Signing", "Подписываю");
  return [
    s.signer.ledger ? h("div", { class: "route-sign-card route-ledger" }, icon("ledger", 26),
      h("div", { class: "route-sign-text" }, h("b", { class: "one-line" }, t(`Confirm on your Ledger: ${b.title}`, `Подтверди на Ledger: ${b.title}`)), h("span", {}, s.signer.ledger)))
      : s.signer.waiting ? h("div", { class: "route-sign-card" }, mascot(30, "waiting"),
        h("div", { class: "route-sign-text" }, h("b", { class: "one-line" }, t(`Sign with ${presence()}: ${b.title}`, `Подпиши через ${presence()}: ${b.title}`)),
          h("span", {}, t("Look at the camera, touch the fingerprint reader or type your PIN when Windows asks. It covers this move only.",
            "Посмотри в камеру, приложи палец или введи PIN, когда Windows спросит. Подпись — только на это действие."))),
        icon("hello", 24)) : null,
    h("div", { class: "route-progress-line" }, icon("bolt", 12), last),
  ];
}

// MARK: The route

const CARD = { w: 236, h: 292, gap: 40, perRow: 3 };
const SMALL = { w: 220, h: 272, gap: 26, perRow: 4 };

/** One card per step, arrows between them, rows of three (four smaller ones over three steps) turning down to the next row;
 * the opened step's details under the route. */
function routeCards(b, steps, states) {
  const size = steps.length > CARD.perRow ? SMALL : CARD;
  const rowGap = size === SMALL ? 36 : 46;
  const rows = [];
  for (let i = 0; i < steps.length; i += size.perRow) rows.push(steps.slice(i, i + size.perRow).map((_, j) => i + j));
  const openKey = [...ui.opened].find((k) => k.startsWith("step:" + b.id + ":"));
  const openIndex = openKey ? Number(openKey.split(":").pop()) : -1;
  const parts = [];
  rows.forEach((row, r) => {
    if (r > 0) parts.push(rowTurn(rows[r - 1].length, size, rowGap, states[row[0]] !== "next"));
    parts.push(h("div", { class: "route-row" }, row.flatMap((i) => [
      i !== row[0] ? arrow(states[i] !== "next", size.gap) : null,
      stepCard(b, steps[i], i, states[i], size === SMALL, i === openIndex),
    ])));
  });
  const width = Math.min(steps.length, size.perRow) * size.w + (Math.min(steps.length, size.perRow) - 1) * size.gap;
  const route = h("div", { class: "route-cards", style: `width:${width}px` }, parts);
  if (openIndex >= 0 && steps[openIndex]) {
    const col = openIndex % size.perRow;
    return [route, stepDetails(b, steps, openIndex, states[openIndex], col * (size.w + size.gap) + size.w / 2)];
  }
  return route;
}

/** The arrow from one card to the next: solid where the money goes or went, dotted to a step still to come. */
function arrow(solid, width) {
  const y = 9;
  return svg("svg", { class: "route-step-arrow" + (solid ? "" : " route-dotted"), width, height: 18, viewBox: `0 0 ${width} 18` },
    svg("path", { d: `M7 ${y}H${width - 8}`, class: "route-shaft" }),
    svg("path", { d: `M${width - 14} ${y - 6}L${width - 8} ${y}L${width - 14} ${y + 6}`, class: "route-head" }));
}

/** A long route's turn to the next row: down from the row's last card, back under it, down into the next row's first. */
function rowTurn(count, size, gap, solid) {
  const from = (count - 1) * (size.w + size.gap) + size.w / 2, to = size.w / 2, mid = gap / 2;
  const width = count * size.w + (count - 1) * size.gap;
  return svg("svg", { class: "route-row-turn" + (solid ? "" : " route-dotted"), width, height: gap, viewBox: `0 0 ${width} ${gap}` },
    svg("path", { d: `M${from} 6V${mid}H${to}V${gap - 7}`, class: "route-shaft" }),
    svg("path", { d: `M${to - 6} ${gap - 13}L${to} ${gap - 7}L${to + 6} ${gap - 13}`, class: "route-head" }));
}

/** What a step does, read from its label («BUY · Solana», «SEND · Solana», «FROM · Hyperliquid»…): it picks the drawing. */
function stepKind(label, title) {
  const l = label.toUpperCase();
  const place = (l.split("·").pop() || "").trim();
  const has = (...w) => w.some((x) => l.includes(x));
  const starts = (...w) => w.some((x) => l.startsWith(x));
  if (starts("WHEN")) return l.includes("RISES") ? "alert-up" : "alert-down";
  if (starts("CHECK")) return "check";
  if (starts("MULTISIG", "МУЛЬТИСИГ")) return "lock";
  if (starts("JEV", "SECOND")) return "jev";
  if (starts("OPEN LONG")) return "long";
  if (starts("OPEN SHORT")) return "short";
  if (starts("HYPERLIQUID", "TRADE", "СДЕЛКА")) return /short|шорт/i.test(title) ? "short" : "long";
  if (starts("PROTECT")) return "protect";
  if (starts("RESULT")) return "result";
  if (starts("LATER")) return "schedule";
  if (has("BEST RATES")) return "split";
  if (starts("EVERY")) return "recurring";
  if (starts("ARRIVES")) return "wait";
  if (starts("APPROVE")) return "approve";
  if (starts("GAS")) return "gas";
  if (starts("SIMULATION")) return "simulate";
  if (starts("SIGN ", "SIGN·")) return "sign";
  if (has("LOCKED")) return "lock";
  if (starts("CLOSE")) return "close";
  if (starts("CLAIM")) return "claim";
  if (starts("TOP UP")) return "receive";
  if (starts("REBALANCE")) return "rebalance";
  if (starts("PREDICTION")) return "predict";
  if (has("BRIDGE") || starts("RELAY", "MOVE")) return "bridge";
  if (starts("SWAP")) return "swap";
  if (starts("BUY", "SELL")) return "buy";
  if (has("JITO", "STAK")) return "stake";
  if (starts("INTO", "DEPOSIT", "LEND", "SUPPLY", "HLP") || has("AAVE", "MORPHO", "LEND", "VAULT", "KAMINO")) return "deposit";
  if (starts("SEND", "ПЕРЕВОД", "PAY", "ОПЛАТА", "SAVE", "СОХРАНИТЬ")) return "send";
  if (starts("TO") && !OWN_PLACES.some((p) => place.startsWith(p))) return "send";
  return "wallet";
}
/** «TO · SOLANA» lands in one of the user's own places; «TO · Mom» is someone else. */
const OWN_PLACES = ["SOLANA", "BASE", "ARBITRUM", "HYPERLIQUID", "ROBINHOOD", "NEAR", "USDC", "USDG", "POLYGON", "ETHEREUM", "TEMPO", "HYPEREVM", "OPTIMISM", "BNB"];

/** A place's color by its name (RouteFill.place): the card's picture is that place's color. */
function fillFor(place, label) {
  const n = (place + " " + label).toLowerCase();
  const byName = [["kamino", "kamino"], ["jupiter", "jupiter"], ["jito", "jito"], ["hlp", "hlp"], ["hyperevm", "hyperevm"], ["hyperliquid", "hyperliquid"],
    ["morpho", "morpho"], ["aave", "aave"], ["relay", "relay"], ["base", "base"], ["arbitrum", "arbitrum"], ["robinhood", "robinhood"], ["near", "near"], ["tempo", "tempo"],
    ["browser", "check"], ["браузер", "check"], ["safe", "check"], ["check", "check"], ["result", "check"]];
  return byName.find(([k]) => n.includes(k))?.[1] || "solana";
}
const DARK_FILLS = new Set(["relay", "hyperliquid", "hlp", "jupiter", "kamino", "jito", "aave", "morpho", "base", "arbitrum", "robinhood", "near", "tempo", "hyperevm"]);

/** A place's or token's bundled logo name. */
function logoKey(word) {
  const w = (word || "").toLowerCase().trim();
  return { solana: "sol", hyperliquid: "hype", arbitrum: "arb", ethereum: "eth", polygon: "pol", optimism: "op", "bnb chain": "bnb", jupiter: "jup",
    uniswap: "uni", jito: "jitosol", hlp: "hype", kamino: "kamino", aave: "aave", morpho: "morpho", relay: "relay", base: "base", near: "near",
    robinhood: "robinhood", tempo: "tempo", hyperevm: "hyperevm", usdc: "usdc", usdt: "usdt", usdg: "usdg", sol: "sol", eth: "eth", hype: "hype" }[w] || null;
}

/** «KAMINO LEND» → «Kamino Lend»: short words and tickers keep their capitals. */
function pretty(place) {
  const small = new Set(["ON", "OF", "TO", "OR", "AND", "IN", "AT", "FOR", "WITH", "FROM"]);
  const kept = { USDC: "USDC", USDG: "USDG", USDT: "USDT", NEAR: "NEAR", JITOSOL: "JitoSOL", "1CLICK": "1Click", HYPEREVM: "HyperEVM", LAYERZERO: "LayerZero", CCTP: "CCTP" };
  return place.split(" ").map((w, i) => {
    if (kept[w]) return kept[w];
    if (w !== w.toUpperCase()) return w;
    if (small.has(w)) return i > 0 ? w.toLowerCase() : w[0] + w.slice(1).toLowerCase();
    if (w.length <= 3 || /\d/.test(w) || !/\p{L}/u.test(w)) return w;
    return w[0] + w.slice(1).toLowerCase();
  }).join(" ");
}

const CHIPS = {
  ready: () => [t("Ready", "К подписи"), "#7A5B00", "#FFD23F", "waiting"],
  done: () => [t("Done", "Готово"), "#177A52", "#7EEBC0", "done"],
  now: () => [t("Now", "Сейчас"), "#6A4FD0", "#B7A6FF", "thinking"],
  touch: () => [ui.state.signer.ledger ? "Ledger" : presence(), "#FFFFFF", "#FFD23F", "tuning"],
  next: () => [t("Next", "Дальше"), "#6A6A72", "#D9D9DE", "idle"],
  failed: () => [t("Failed", "Не вышло"), "#B42A20", "#FFB3A8", "error"],
};

function stateChip(state, compact) {
  // A state the chip has no word for (a newer engine's) draws no chip, as a plain card.
  if (!CHIPS[state]) return null;
  const [text, ink, halo, mood] = CHIPS[state]();
  const el = h("span", { class: "route-state-chip" + (state === "touch" ? " route-touch" : "") + (compact ? " route-compact" : "") },
    h("span", { class: "route-halo" }, mascot(compact ? 13 : 15, mood)), text);
  el.style.color = ink;
  el.firstChild.style.background = halo;
  return el;
}

function stepCard(b, step, i, state, compact, isOpen) {
  const kind = stepKind(step.label, step.title);
  const parts = step.label.split("·").map((x) => x.trim());
  const place = parts.length > 1 ? pretty(parts.slice(1).join(" · ")) : null;
  const fill = fillFor(place || "", parts[0]);
  const pale = state === "next";
  const w = compact ? SMALL.w : CARD.w;
  const cls = ["route-step", "route-fill-" + fill, "route-kind-" + kind, "route-state-" + state, compact && "route-compact", pale && "route-pale", !pale && DARK_FILLS.has(fill) && "route-dark", isOpen && "route-open"]
    .filter(Boolean).join(" ");
  const key = "step:" + b.id + ":" + i;
  const toggle = () => {
    for (const k of [...ui.opened]) if (k.startsWith("step:" + b.id + ":")) ui.opened.delete(k);
    if (!isOpen) ui.opened.add(key);
    render();
  };
  return h("div", {
    class: cls, "data-no-pan": "", title: t("Details · right-click for more", "Подробности · правый клик — ещё"), onclick: toggle,
    oncontextmenu: (e) => { e.preventDefault(); e.stopPropagation(); popup(e, [
      [t("Ask in chat", "Спросить в чате"), () => attach(b)],
      [t("Copy details", "Скопировать подробности"), () => copy(stepText(step, i))],
    ]); },
  },
    h("div", { class: "route-step-pic" }, scene(kind, state, step, w), state !== "plain" && kind !== "result" ? stateChip(state, compact) : null),
    stepInfo(step, state, kind, place, compact, isOpen),
    state === "now" || state === "touch" ? h("span", { class: "route-ring" + (state === "touch" ? " route-touch" : "") }) : null);
}

function stepText(step, i) {
  return [`${i + 1}. ${step.label} · ${step.title}`, step.money ? `${step.money.place} ${step.money.was} → ${step.money.after} ${step.money.symbol}` : null,
    ...step.lines.map((l) => l.replace("\t", " "))].filter(Boolean).join("\n");
}

/** Under the picture: what the step does and where, the move itself, its numbers, one line of the rest, «Details ›». */
function stepInfo(step, state, kind, place, compact, isOpen) {
  const verb = step.label.split("·")[0].trim();
  const rows = step.lines.filter((l) => l.includes("\t"));
  const notes = step.lines.filter((l) => l && !l.includes("\t"));
  const said = !step.money && !rows.length ? notes : [];
  const foot = said.length ? null : notes[0];
  const room = compact ? 3 : 4;
  const boxRows = [...rows, ...said].slice(0, Math.max(0, room - (step.money ? 1 : 0)));
  return h("div", { class: "route-step-body" },
    h("div", { class: "route-step-label" },
      h("span", { class: "route-step-verb" + (kind === "long" ? " profit" : kind === "short" ? " loss" : "") }, verb),
      place ? [h("span", { class: "route-rc-dot" }), logoKey(place) ? placeLogo(logoKey(place), compact ? 16 : 18) : null, h("span", { class: "route-step-place one-line" }, place)] : null),
    stepTitle(step.title, compact),
    step.money || boxRows.length ? h("div", { class: "route-money-box" },
      step.money ? [h("div", { class: "route-money-head" }, state === "done" ? t("MONEY · WAS → NOW", "ДЕНЬГИ · БЫЛО → СТАЛО") : t("MONEY · NOW → AFTER", "ДЕНЬГИ · СЕЙЧАС → ПОСЛЕ")),
        moneyLine(step.money)] : null,
      boxRows.map((l, n) => stepLine(l, state === "failed" && n === 0 && !step.money))) : null,
    h("div", { class: "route-step-foot" },
      foot ? h("span", { class: "route-step-note" }, foot) : h("span", { class: "spacer" }),
      h("span", { class: "route-step-more" + (isOpen ? " route-open" : "") }, isOpen ? t("Close", "Закрыть") : t("Details", "Подробнее"), icon(isOpen ? "xmark" : "chevron.right", 9))));
}

/** «150 USDC», «2 USDC → Base»: each side with its token's or place's logo when there's one; shrinks to fit, as the Mac's. */
function stepTitle(title, compact) {
  const base = compact ? 17 : 19;
  const sides = title.split("→").map((x) => x.trim());
  const el = h("div", { class: "route-step-title" }, sides.flatMap((side, i) => {
    const key = logoKey(side.split(" ").pop());
    return [i > 0 ? h("span", {}, "→") : null, side && key ? logoRound(key, base + 1) : null, side ? h("span", { class: "one-line" }, side) : null];
  }));
  const len = title.length + sides.length * 2;
  el.style.fontSize = Math.max(base * 0.55, Math.min(base, (base * (compact ? 14 : 15)) / Math.max(len, 1))) + "px";
  return el;
}

function logoRound(key, size) {
  const l = placeLogo(key, size);
  if (key !== "hype") l.classList.add("round");
  return l;
}

/** A bundled logo; Hyperliquid's mark sits on its own dark green, as the Mac's TokenLogo draws «hype». */
function placeLogo(key, size) {
  const l = logo(key, size);
  if (key === "hype") { l.classList.add("route-hl-logo"); l.style.padding = Math.round(size * 0.14) + "px"; }
  return l;
}

/** A balance a step changes: where, now, an arrow, what it comes to (bold), the ticker. */
function moneyLine(m) {
  return h("div", { class: "route-money-line" }, h("span", { class: "route-place one-line" }, m.place), h("span", { class: "route-was" }, m.was), h("span", { class: "route-arr" }, "→"),
    h("b", {}, m.after), h("span", { class: "route-sym" }, m.symbol));
}

/** «entry\t≈ 119.31» is a row: the label quiet on the left, the value bold on the right; anything else is a sentence. */
function stepLine(line, alert) {
  const [label, value] = line.split("\t");
  if (value === undefined) return h("div", { class: "route-step-line" + (alert ? " loss route-strong" : "") }, line);
  const tint = /stop/i.test(label) || value.startsWith("−") ? "loss" : /take/i.test(label) || value.startsWith("+") ? "profit" : "";
  return h("div", { class: "route-step-row" }, h("span", { class: "one-line" }, label), h("b", { class: tint }, value));
}

/** The opened step, under its route: what it is, its money and lines, «Ask in chat», Close; the notch points at its card. */
function stepDetails(b, steps, i, state, notch) {
  const step = steps[i];
  const go = (d) => { const next = (i + d + steps.length) % steps.length; ui.opened.delete("step:" + b.id + ":" + i); ui.opened.add("step:" + b.id + ":" + next); render(); };
  const close = () => { ui.opened.delete("step:" + b.id + ":" + i); render(); };
  const place = (step.label.split("·")[1] || "").trim();
  const key = logoKey(place);
  const el = h("div", { class: "route-step-details", "data-no-pan": "" },
    h("span", { class: "route-notch" }),
    h("div", { class: "route-details-head" },
      key ? placeLogo(key, 40) : h("span", { class: "route-details-glyph" }, mascot(30, "idle")),
      h("div", { class: "route-details-name" }, h("b", { class: "one-line" }, pretty(step.label.replace(/\s*·\s*/g, " · ").toUpperCase())),
        h("span", { class: "one-line" }, t(`Step ${i + 1} of ${steps.length} · ${step.title}`, `Шаг ${i + 1} из ${steps.length} · ${step.title}`))),
      h("span", { class: "spacer" }),
      steps.length > 1 ? [h("button", { class: "icon-btn", title: t("Previous step", "Предыдущий шаг"), onclick: () => go(-1) }, icon("chevron.left", 13)),
        h("button", { class: "icon-btn", title: t("Next step", "Следующий шаг"), onclick: () => go(1) }, icon("chevron.right", 13))] : null,
      h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: close }, icon("xmark", 13))),
    step.money ? h("div", { class: "route-money-box route-big" }, h("div", { class: "route-money-head" }, state === "done" ? t("MONEY · WAS → NOW", "ДЕНЬГИ · БЫЛО → СТАЛО") : t("MONEY · NOW → AFTER", "ДЕНЬГИ · СЕЙЧАС → ПОСЛЕ")),
      moneyLine(step.money)) : null,
    step.lines.length ? h("div", { class: "route-details-lines selectable" }, step.lines.filter(Boolean).map((l) => h("div", {}, l.replace("\t", " ")))) : null,
    h("div", { class: "route-details-buttons" },
      h("button", { class: "btn route-bordered route-large route-ask", title: t("Attaches this block to your next message", "Прикрепит этот блок к следующему сообщению"), onclick: () => attach(b) },
        mascot(20, "waiting"), t("Ask in chat", "Спросить в чате")),
      h("span", { class: "spacer" }),
      h("button", { class: "btn route-bordered route-large", onclick: close }, t("Close", "Закрыть"))));
  el.querySelector(".route-notch").style.left = Math.min(Math.max(notch - 7, 24), 520 - 38) + "px";
  return el;
}

// MARK: The drawing

/** Where the coin sits in each drawing and how big (StepScene.coin): clear of the state chip on the picture's top left. */
function coinAt(kind, state) {
  switch (kind) {
    case "wallet": return [236, 50, 1.5];
    case "bridge": return [state === "done" ? 292 : state === "ready" || state === "next" ? 150 : 200, 35, 1.4];
    case "buy": return [150, 62, 1.4];
    case "swap": return [96, 68, 1.4];
    case "deposit": return [170, 52, 1.4];
    case "stake": return [170, 44, 1.4];
    case "send": return [80, 72, 1.4];
    case "check": return [130, 66, 1.4];
    case "jev": return [120, 66, 1.4];
    case "long": return [250, 35, 1.4];
    case "short": return [250, 57, 1.4];
    case "protect": return [150, 64, 1.4];
    case "result": return [90, 68, 1.4];
    case "alert-up": return [236, 58, 1.4];
    case "alert-down": return [236, 74, 1.4];
    case "schedule": return [130, 66, 1.4];
    case "split": return [160, 36, 1.4];
    case "recurring": return [170, 62, 1.2];
    case "approve": return [140, 60, 1.4];
    case "simulate": case "predict": return [118, 66, 1.4];
    case "close": return [212, 36, 1.3];
    case "rebalance": return [130, 70, 1.4];
    default: return [130, 66, 1.4];
  }
}

const MOODS = { ready: "waiting", done: "done", now: "thinking", touch: "tuning", next: "idle", failed: "error", plain: "idle" };

function P(d, cls = "route-ink", style) {
  const el = svg("path", { d, class: cls });
  if (style) Object.assign(el.style, style);
  return el;
}
function C(cx, cy, r, cls = "route-ink", style) {
  const el = svg("circle", { cx, cy, r, class: cls });
  if (style) Object.assign(el.style, style);
  return el;
}
function R(x, y, w, hgt, rx, cls = "route-body") {
  return svg("rect", { x, y, width: w, height: hgt, rx, class: cls });
}

/** The top of a card: what the step does, drawn on the Mac's 340 × 120 grid (its picture is 140 tall, the drawing 10 down),
 * with the coin in it wearing the step's face. */
function scene(kind, state, step, width) {
  const g = svg("svg", { class: "route-scene", viewBox: "0 -10 340 140", preserveAspectRatio: "xMinYMin slice", "aria-hidden": "true" });
  const [x, y, scale] = coinAt(kind, state);
  const d = 34 * scale * 1.1;
  const coin = mascot(d, MOODS[state] || "idle");
  for (const [k, v] of Object.entries({ x: x - d / 2, y: y - d / 2, width: d, height: d })) coin.setAttribute(k, v);
  const front = kind === "wallet" || kind === "deposit" || kind === "protect";
  const drawing = draw(kind, state, step);
  if (front) g.append(coin, ...drawing); else g.append(...drawing, coin);
  return g;
}

function draw(kind, state, step) {
  const thin = { strokeWidth: 1.6 };
  switch (kind) {
    case "wallet":
      return [P("M150 140L150 76Q150 62 164 62L360 62L360 140Z", "route-body"), P("M360 82L300 82C278.7 82 278.7 114 300 114L360 114", "route-paper"), C(302, 98, 5, "route-solid")];
    case "bridge": {
      const water = ["M70 114"];
      for (let i = 0; i < 10; i++) { const x = 70 + i * 20; water.push(`Q${x + 10} ${i % 2 === 0 ? 109 : 119} ${x + 20} 114`); }
      const hangers = [[88, 99], [126, 93], [170, 91], [214, 93], [252, 99]].map(([x, y]) => `M${x} 60L${x} ${y}`).join("");
      return [P("M-4 60L344 60"), P("M50 60L50 124M290 60L290 124"), P("M50 124C50 80 290 80 290 124"), P(hangers, "route-ink", thin),
        P(water.join(""), "route-ink", { ...thin, opacity: 0.45 }),
        state === "now" || state === "touch" ? P("M148 28L164 28M140 40L160 40", "route-ink", { strokeWidth: 2.5, opacity: 0.55 }) : null].filter(Boolean);
    }
    case "buy": {
      const ticker = step.title.split("→").pop().trim().split(" ").pop();
      const text = svg("text", { x: 254, y: 80, class: "route-tag-text" });
      text.textContent = ticker && ticker.length <= 6 && /\p{L}/u.test(ticker) ? ticker : "";
      return [P("M172 58Q206 40 231 78", "route-ink", thin), P("M240 30L340 30Q350 30 350 40L350 130L240 130L214 80Z", "route-body"), C(236, 80, 5, "route-ink", { strokeWidth: 2.5 }), text];
    }
    case "swap":
      return [P("M132 48L196 48M188 40L196 48L188 56M196 74L132 74M140 66L132 74L140 82"), C(236, 60, 23, "route-token"), C(236, 60, 16, "route-token-rim")];
    case "deposit":
      return [R(110, 70, 120, 44, 8), P("M150 70L190 70", "route-ink", { strokeWidth: 5 })];
    case "stake":
      return [100, 86, 72].map((y) => svg("ellipse", { cx: 170, cy: y, rx: 60, ry: 11, class: "route-body" }));
    case "send":
      return [P("M112 56Q176 4 236 44", "route-ink", { strokeWidth: 2.2, strokeDasharray: "4 7" }), C(266, 42, 11), P("M242 100Q266 50 290 100")];
    case "check":
      return [R(190, 14, 100, 120, 10), P("M206 48L211 53L220 44M206 76L211 81L220 72", "route-good"), P("M232 49L276 49M232 77L276 77", "route-ink", { strokeWidth: 3.5 })];
    case "jev":
      return [R(176, 16, 92, 112, 8), C(250, 80, 26, "route-body"), P("M269 99L294 124", "route-ink", { strokeWidth: 6 }), P("M238 80L246 88L260 74", "route-good")];
    case "long": case "short": {
      const ys = kind === "long" ? [112, 84, 98, 30] : [30, 58, 44, 112];
      const line = `M-4 ${ys[0]}L90 ${ys[1]}L140 ${ys[2]}L344 ${ys[3]}`;
      const side = kind === "long" ? "route-good" : "route-bad";
      return [P(line + "L344 130L-4 130Z", side + "-area"), P(line, side)];
    }
    case "schedule":
      return [R(196, 20, 104, 104, 12), P("M196 46L300 46M224 10L224 30M272 10L272 30"),
        ...[[222, 68], [248, 68], [274, 68], [222, 96], [248, 96]].map(([x, y]) => C(x, y, 3.2, "route-solid", { opacity: 0.55 })), C(274, 96, 10), C(274, 96, 3.6, "route-solid")];
    case "split":
      return [P("M-4 62L232 62M130 62C176 62 186 26 232 26M130 62C176 62 186 98 232 98"),
        ...[13, 49, 85].flatMap((y) => [R(236, y, 72, 26, 6), P(`M258 ${y}L286 ${y}`, "route-ink", { strokeWidth: 4 })])];
    case "recurring": {
      const c = 46 * 0.5523;
      return [P(`M170 16C${170 + c} 16 216 ${62 - c} 216 62C216 ${62 + c} ${170 + c} 108 170 108C${170 - c} 108 124 ${62 + c} 124 62M116 70L124 60L132 70`),
        ...[[250, 1], [276, 0.6], [302, 0.3]].map(([x, o]) => C(x, 62, 6, "route-solid", { opacity: o }))];
    }
    case "wait":
      return [P("M210 16C210 56 236 60 236 70C236 80 210 84 210 124L270 124C270 84 244 80 244 70C244 60 270 56 270 16Z", "route-body"),
        P("M202 16L278 16M202 124L278 124"), P("M226 36L254 36", "route-ink", { strokeWidth: 2, opacity: 0.5 }), P("M222 124Q240 100 258 124Z", "route-solid", { opacity: 0.5 })];
    case "approve":
      return [C(226, 60, 20, "route-body"), C(226, 60, 6), P("M246 60L332 60M300 60L300 76M316 60L316 72")];
    case "gas":
      return [R(200, 22, 64, 104, 8), R(212, 36, 40, 30, 4, "route-ink"), P("M232 40L241 51L232 62L223 51Z", "route-solid"),
        P("M264 46L280 46Q292 46 292 58L292 92Q292 100 300 100Q308 100 308 92L308 56L296 44")];
    case "simulate":
      return [R(186, 16, 128, 88, 10), P("M186 36L314 36", "route-ink", { strokeWidth: 2 }), ...[200, 210, 220].map((x) => C(x, 26, 2.6, "route-solid")),
        P("M226 68L240 82L272 50", "route-good", { strokeWidth: 5 }), P("M250 104L250 118M226 120L274 120")];
    case "sign":
      return [P("M200 70C204 46 222 22 246 22C272 22 290 42 292 66", "route-ink", { opacity: 0.55 }),
        P("M212 104C204 70 220 36 244 36C268 36 282 58 278 88M224 108C218 82 224 52 244 52C262 52 268 70 264 96M238 112C234 92 236 68 246 68C254 68 256 82 252 102")];
    case "lock":
      return [P("M212 60L212 42C212 14 268 14 268 42L268 60"), R(194, 58, 92, 66, 12), C(240, 84, 7, "route-solid"), P("M240 88L240 104", "route-ink", { strokeWidth: 5 })];
    case "close":
      return [P("M-4 104L70 78L120 90L196 46"), P("M252 124L252 22"), P("M252 24L308 24L296 40L308 56L252 56Z", "route-body"), P("M266 40L273 47L286 34", "route-good")];
    case "claim":
      return [R(202, 58, 92, 66, 8), R(194, 42, 108, 20, 6), P("M248 42L248 124M248 42C232 18 212 30 248 42C284 30 264 18 248 42"),
        P("M170 14L170 28M163 21L177 21M318 22L318 32M313 27L323 27", "route-ink", { strokeWidth: 2.2, opacity: 0.65 })];
    case "receive":
      return [P("M244 12L244 66M228 50L244 66L260 50"), P("M192 80L220 80L230 96L258 96L268 80L296 80L296 124L192 124Z", "route-body")];
    case "rebalance":
      return [P("M250 22A40 40 0 1 1 227.06 94.77", "route-ink", { strokeWidth: 14, strokeLinecap: "butt" }),
        P("M225.37 93.52A40 40 0 0 1 247.91 22.05", "route-ink", { strokeWidth: 14, strokeLinecap: "butt", opacity: 0.4 }),
        P("M190 30Q208 6 236 6M228 0L237 6L228 13", "route-ink", { strokeWidth: 2.5 })];
    case "predict": {
      const yes = svg("text", { x: 221, y: 63, class: "route-ticket-text route-good-fill" }), no = svg("text", { x: 291, y: 63, class: "route-ticket-text route-bad-fill" });
      yes.textContent = "YES"; no.textContent = "NO";
      return [R(186, 24, 140, 78, 10), P("M256 32L256 94", "route-ink", { strokeWidth: 2, strokeDasharray: "3 5" }), yes, no];
    }
    case "protect":
      return [P("M230 10L290 30L290 64C290 98 264 120 230 130C196 120 170 98 170 64L170 30Z", "route-body"),
        P("M186 52L274 52", "route-good", { strokeWidth: 3, strokeDasharray: "6 6" }), P("M190 86L270 86", "route-bad", { strokeWidth: 3 })];
    case "result":
      return [P("M126 50C170 20 206 18 244 22", "route-good", { strokeWidth: 4, strokeDasharray: "7 7" }), P("M126 72C170 100 206 104 244 100", "route-bad", { strokeWidth: 4 })];
    case "alert-up": case "alert-down": {
      const up = kind === "alert-up", yy = (v) => (up ? 132 - v : v), cy = up ? 96 : 36;
      const price = [[46, 52], [90, 40], [138, 66], [180, 56], [212, 80]].map(([px, py]) => `L${px} ${yy(py)}`).join("");
      return [P(`M-4 ${yy(98)}L344 ${yy(98)}`, "route-ink", { strokeWidth: 2.5, strokeDasharray: "8 7", opacity: 0.7 }), P(`M-4 ${yy(34)}${price}`),
        P(`M284 ${cy + 12}C284 ${cy - 22} 316 ${cy - 22} 316 ${cy + 12}M279 ${cy + 12}L321 ${cy + 12}`, "route-body"), C(300, cy + 18, 4.5, "route-solid"), C(300, cy - 15, 3, "route-ink", thin)];
    }
    default:
      return [];
  }
}

// MARK: The multisig being set up

const MULTISIG_NETWORKS = [["base", "Base", "base"], ["arbitrum", "Arbitrum", "arb"], ["optimism", "Optimism", "op"], ["ethereum", "Ethereum", "eth"],
  ["polygon", "Polygon", "pol"], ["bnb", "BNB Chain", "bnb"], ["hyperevm", "HyperEVM", "hyperevm"], ["robinhood", "Robinhood", "robinhood"], ["tempo", "Tempo", "tempo"]];

/** The set-up card waiting for Confirm (MultisigSetupCard): the keys, how many must sign, where — each changes here. */
function multisigSetup(d) {
  const n = d.keys.length, m = d.threshold, majority = Math.floor(n / 2) + 1;
  const names = (keys) => keys.map((k) => k.kind === "pc" ? t(`${presence()} on this PC`, `${presence()} на этом ПК`) : k.kind === "ledger" ? "Ledger" : k.name).join(t(" and ", " и "));
  const spare = d.keys.slice(m);
  const risk = m === n ? (n === 1 ? t("One key, one device: a multisig only matters with two keys or more.", "Один ключ на одном устройстве: мультисиг имеет смысл от двух ключей.")
      : t("Every key, every time. Lose any one device and the money stays locked for good.", "Каждый ключ каждый раз. Потеряешь любое устройство — деньги заперты навсегда."))
    : m === 1 ? t("One key moves everything: whoever gets any one device takes the money.", "Один ключ двигает всё: кто получит любое устройство, заберёт деньги.")
    : t(`Lose up to ${n - m} ${n - m === 1 ? "device" : "devices"} and you still get in. A thief needs ${m} of them at once.`,
      `Потеряешь до ${n - m} устройств — всё равно войдёшь. Вору нужны ${m} сразу.`);
  return h("div", { class: "route-msig-card", "data-no-pan": "" },
    keyholes(n, m),
    h("div", { class: "route-msig-body" },
      h("div", {},
        h("div", { class: "route-msig-heading" }, h("span", { class: "route-step-verb" }, t("NEW MULTISIG", "НОВЫЙ МУЛЬТИСИГ")), h("span", { class: "route-rc-dot" }), h("span", { class: "route-safe-s" }, "S"),
          h("span", { class: "route-step-place" }, "Safe · " + (d.networks.length === 1 ? (MULTISIG_NETWORKS.find((x) => x[0] === d.networks[0])?.[1] || d.networks[0]) : t(`${d.networks.length} networks`, `сетей: ${d.networks.length}`)))),
        h("div", { class: "route-msig-title" }, m === n ? t(`All ${n} keys move the money`, `Все ${n} ключа двигают деньги`) : t(`Any ${m} of ${n} keys move the money`, `Любые ${m} из ${n} ключей двигают деньги`))),
      h("div", { class: "route-msig-section" },
        h("div", { class: "route-section-label" }, h("span", {}, t("Keys · one on each device", "Ключи · по одному на устройство")), h("span", {}, t(`${n} keys`, `ключей: ${n}`))),
        d.keys.map((k, i) => h("div", { class: "route-msig-key-row" },
          h("span", { class: "route-key-glyph" }, icon(k.kind === "pc" ? "laptop" : k.kind === "ledger" ? "ledger" : "iphone", 15)),
          h("div", { class: "route-key-name" }, h("b", { class: "one-line" }, k.name),
            h("span", { class: "one-line" }, `${k.address.slice(0, 6)}…${k.address.slice(-4)} · ` + (k.kind === "pc" ? presence() : k.kind === "ledger" ? t("the Ledger's buttons", "кнопки Ledger") : t("signs in Safe's app", "подписывает в приложении Safe")))),
          h("span", { class: "route-role" + (i < m ? " route-signs" : "") }, i < m ? t("signs", "подписывает") : t("spare", "запасной")),
          k.kind === "pc" || n <= 1 ? null : h("button", { class: "icon-btn", title: t("Remove this key", "Убрать ключ"), onclick: () => act("multisigKey", { wallet: k.address }) }, icon("xmark", 13)))),
        h("div", { class: "route-msig-add" },
          h("span", { class: "route-add-label" }, icon("plus", 12), t("Add a key:", "Добавить ключ:")),
          ui.state.candidates.map((c) => h("button", { class: "route-candidate", onclick: () => act("multisigKey", { address: c.address }) }, icon(c.kind === "ledger" ? "ledger" : "person", 13), c.name)),
          h("button", { class: "link-btn route-another", onclick: () => suggest("save 0x… as Phone on base") }, icon("square.and.pencil", 13),
            ui.state.candidates.length ? t("Another address…", "Другой адрес…") : t("Save a phone's or a person's address first", "Сначала сохрани адрес телефона или человека"))),
        h("div", { class: "route-msig-foot" }, t(`Checked: no two keys from one phrase, one key at most on this PC, every address a plain wallet saved with ${presence()}.`,
          `Проверено: нет двух ключей из одной фразы, не больше одного ключа на этом ПК, каждый адрес — обычный кошелёк, сохранённый через ${presence()}.`))),
      h("div", { class: "route-msig-section" },
        h("div", { class: "route-section-label" }, t("How many keys move money", "Сколько ключей двигают деньги")),
        h("div", { class: "route-stepper" },
          h("button", { class: "btn route-bordered route-square", disabled: m <= 1, title: t("One signature fewer", "На одну подпись меньше"), onclick: () => act("multisigThreshold", { threshold: m - 1 }) }, icon("minus")),
          h("span", { class: "route-big-num" }, String(m), h("span", {}, t(` of ${n}`, ` из ${n}`))),
          h("button", { class: "btn route-bordered route-square", disabled: m >= n, title: t("One signature more", "На одну подпись больше"), onclick: () => act("multisigThreshold", { threshold: m + 1 }) }, icon("plus"))),
        h("div", { class: "route-msig-risk" }, h("span", { class: m === n || m === 1 ? "loss" : "" }, risk),
          m === majority ? h("span", { class: "route-recommended" }, t("Recommended", "Рекомендую"))
            : h("button", { class: "btn route-bordered small", onclick: () => act("multisigThreshold", { threshold: majority }) }, t(`Use ${majority} of ${n}`, `Взять ${majority} из ${n}`)))),
      h("div", { class: "route-msig-section" },
        h("div", { class: "route-section-label" }, h("span", {}, t("Networks · pick one or several", "Сети · одна или несколько")), h("span", {}, t("same address on each", "один адрес во всех"))),
        h("div", { class: "route-msig-networks" }, MULTISIG_NETWORKS.map(([key, name, logoName]) => {
          const on = d.networks.includes(key);
          return h("button", { class: "route-net-chip" + (on ? " route-on" : ""), onclick: () => act("multisigNetwork", { network: key }) }, logo(logoName, 18), name);
        })),
        h("div", { class: "route-msig-foot" }, t("Only networks where Safe runs. Add another later, or say it in the chat.", "Только сети, где работает Safe. Другую можно добавить позже или попросить в чате."))),
      d.address ? h("div", { class: "route-msig-address" }, h("span", { class: "secondary" }, t("Address", "Адрес")), h("span", { class: "address one-line" }, d.address),
        h("button", { class: "btn route-bordered small", onclick: () => copy(d.address) }, icon("copy", 12), t("Copy", "Копировать"))) : null,
      h("div", { class: "route-msig-foot route-info" }, icon("info.circle", 13),
        t(`Every send: ${names(d.keys.slice(0, m))}.` + (spare.length ? ` Spare: ${names(spare)}.` : "") + " Making it costs only the network fee; Safe takes nothing.",
          `Каждая отправка: ${names(d.keys.slice(0, m))}.` + (spare.length ? ` Запасные: ${names(spare)}.` : "") + " Создание стоит только комиссию сети; Safe ничего не берёт."))));
}

/** The multisig's picture: its body with one keyhole per key, the ones a move needs lit, and its state. */
function keyholes(total, lit) {
  const n = Math.max(1, Math.min(total, 8)), sp = n <= 4 ? 36 : 28;
  const w = sp * n + 36, hgt = 66, x = 492 - w - 26, y = (100 - hgt) / 2 + 6;
  const g = svg("svg", { class: "route-keyholes", viewBox: "0 0 492 100", preserveAspectRatio: "xMaxYMid meet" },
    svg("rect", { x, y, width: w, height: hgt, rx: 10, class: "route-kh-box" }));
  for (let i = 0; i < n; i++) {
    const cx = x + 18 + sp / 2 + sp * i;
    g.append(svg("path", { d: `M${cx + 6} ${y + 22}A6 6 0 1 0 ${cx - 6} ${y + 22}A6 6 0 1 0 ${cx + 6} ${y + 22}M${cx - 4} ${y + 25}L${cx + 4} ${y + 25}L${cx + 6} ${y + 44}L${cx - 6} ${y + 44}Z`,
      class: i < lit ? "route-kh-lit" : "route-kh-hole" }));
  }
  return h("div", { class: "route-keyholes-pic" }, g, h("span", { class: "route-kh-chip" }, t("Ready · waits for Confirm", "Готово · ждёт «Подтвердить»")));
}

// MARK: Results and charts

/** A block of lines from the engine (rates, P/L, a token, a site's refusal): rows, and its buttons that fill the chat field. */
function resultBlock(b) {
  const rows = (b.lines || []).map((l) => l === "—" ? h("div", { class: "route-result-divider" }) : h("div", { class: "route-result-row selectable" }, l));
  return blockFrame(b, b.title, h("div", { class: "route-result" },
    rows.length ? h("div", { class: "route-result-rows" }, rows) : null,
    b.actions && b.actions.length ? h("div", { class: "btns" }, b.actions.map((a) => h("button", { class: "btn route-bordered", title: a.command, onclick: () => suggest(a.command) }, a.title))) : null),
  { width: 520 });
}

const RANGES = [["day", "24h"], ["week", "7d"], ["month", "30d"]];

/** A coin's closes over the range, as the Mac's chart board: the price now (or under the pointer), the move, the range
 * picker, the line with its fill, the low and high. Drawn from numbers only. */
function chartBlock(b) {
  const c = b.chart, pts = c.closes;
  const coin = c.title.split("·")[0].trim();
  const range = /24|day|день|сут/i.test(c.title) ? "day" : /30|month|мес/i.test(c.title) ? "month" : "week";
  const up = !String(c.change).trim().startsWith("-") && !String(c.change).trim().startsWith("−");
  const W = 438, H = 190;
  const min = Math.min(...pts), max = Math.max(...pts), pad = Math.max((max - min) * 0.1, max * 0.002) || 1;
  const lo = min - pad, span = max + pad - lo;
  const X = (i) => (i / Math.max(pts.length - 1, 1)) * W, Y = (p) => H - ((p - lo) / span) * H;
  const line = pts.map((p, i) => `${i ? "L" : "M"}${X(i).toFixed(1)} ${Y(p).toFixed(1)}`).join("");
  const gid = "fill-" + b.id;
  const dot = svg("circle", { r: 4, class: "route-chart-dot", cx: -100, cy: -100 });
  const rule = svg("path", { d: "", class: "route-chart-rule" });
  const plot = svg("svg", { class: "route-chart-plot " + (up ? "route-up" : "route-down"), "data-no-pan": "", viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: "none" },
    svg("defs", {}, svg("linearGradient", { id: gid, x1: 0, y1: 0, x2: 0, y2: 1 }, svg("stop", { offset: "0", class: "route-grad-top" }), svg("stop", { offset: "1", class: "route-grad-bottom" }))),
    svg("path", { d: line + `L${W} ${H}L0 ${H}Z`, fill: `url(#${gid})` }), svg("path", { d: line, class: "route-chart-line" }), rule, dot);
  const priceEl = h("span", { class: "route-chart-price" }, price(pts.at(-1)));
  const changeEl = h("span", { class: "route-chart-change " + (up ? "profit" : "loss") }, c.change + " · " + rangeTitle(range));
  plot.addEventListener("mousemove", (e) => {
    const r = plot.getBoundingClientRect();
    const i = Math.round(((e.clientX - r.left) / r.width) * (pts.length - 1));
    if (!(i >= 0 && i < pts.length)) return;
    dot.setAttribute("cx", X(i)); dot.setAttribute("cy", Y(pts[i]));
    rule.setAttribute("d", `M${X(i)} 0V${H}`);
    priceEl.textContent = price(pts[i]);
  });
  plot.addEventListener("mouseleave", () => { dot.setAttribute("cx", -100); rule.setAttribute("d", ""); priceEl.textContent = price(pts.at(-1)); });
  const badge = logo(coin, 38);
  badge.classList.add("route-asset-badge");
  const el = h("div", { class: "route-block route-chart-block" + (ui.attached === b.id ? " route-attached" : ""), oncontextmenu: (e) => blockMenu(e, b, b.title) },
    h("div", { class: "route-asset-head" }, badge,
      h("div", { class: "route-asset-name" }, h("b", {}, coin), h("span", {}, t("Hyperliquid chart", "График Hyperliquid") + " · " + fmtDate(b.at))),
      h("span", { class: "spacer" }),
      h("button", { class: "icon-btn", title: t("Load fresh candles from Hyperliquid", "Загрузить свежие свечи с Hyperliquid"), onclick: () => act("send", { text: `chart ${coin} ${range}` }) }, icon("arrow.clockwise")),
      h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => act("closeBoard", { board: b.id }) }, icon("xmark"))),
    h("div", { class: "route-asset-price" }, h("div", {}, priceEl, changeEl), h("span", { class: "spacer" }),
      h("div", { class: "route-chart-range" }, RANGES.map(([key, label]) => h("button", { class: key === range ? "route-on" : "", onclick: () => key !== range && act("send", { text: `chart ${coin} ${key}` }) }, label)))),
    plot,
    h("div", { class: "route-tiles" },
      tile(t("Low", "Минимум"), c.low), tile(t("High", "Максимум"), c.high), tile(t("Change", "Изменение"), c.change, up ? "profit" : "loss")));
  return el;
}

function rangeTitle(range) {
  return { day: t("24 hours", "24 часа"), week: t("7 days", "7 дней"), month: t("30 days", "30 дней") }[range];
}

function tile(title, value, cls = "") {
  return h("div", { class: "route-tile" }, h("span", {}, title.toUpperCase()), h("b", { class: cls }, value || "—"));
}

/** A price as the Mac's Fmt.price: cents over a dollar, more digits under it. */
function price(p) {
  if (!Number.isFinite(p)) return "—";
  const digits = p >= 1000 ? 2 : p >= 1 ? 2 : p >= 0.01 ? 4 : 8;
  return "$" + p.toLocaleString("en-US", { minimumFractionDigits: Math.min(digits, 2), maximumFractionDigits: digits });
}

// MARK: An empty canvas

const STARTERS = [
  ["Trade by the book", "Сделка по правилам", "long ETH by the book", "chart.line.uptrend", "profit", "long ETH with a stop", "лонг ETH со стопом"],
  ["Short on a pullback", "Шорт на откате", "short oil on a 2% pullback by the book", "arrow.down.right", "loss", "short oil on a bounce", "шорт нефти на отскоке"],
  ["Best rates", "Лучшие ставки", "spread USDC across the best rates", "percent", "route-blue", "free USDC where it earns most", "свободные USDC туда, где больше"],
  ["Where to keep it", "Где держать", "where to keep USDC", "building.columns", "route-lime", "compare where USDC earns", "сравнить, где USDC зарабатывают"],
  ["Every week", "Каждую неделю", "buy SOL for 2 USDC every week", "calendar", "route-violet", "a small SOL buy every week", "небольшая покупка SOL каждую неделю"],
  ["Back to Solana", "Обратно в Solana", "withdraw 1 USDC to Solana", "arrow.uturn.left", "route-amber", "1 USDC home from Hyperliquid", "1 USDC домой с Hyperliquid"],
];

/** An empty canvas: the coin, «Pick a starting block» and six block cards; a click puts the block's phrase into the chat. */
function starter() {
  return h("div", { class: "route-starter" },
    h("div", { class: "route-starter-head" }, mascot(34), h("div", {},
      h("b", {}, t("Pick a starting block", "Выбери блок для начала")),
      h("span", {}, ui.state.model.connected ? t("or type anything in the chat", "или напиши что угодно в чат") : t("Connect a model below to start", "Подключи модель ниже, чтобы начать")))),
    h("div", { class: "route-starter-grid" }, STARTERS.map(([en, ru, phrase, ic, tint, lineEn, lineRu]) =>
      h("button", { class: "route-starter-card", title: t(`Puts “${phrase}” into the chat`, `Вставит «${phrase}» в чат`), onclick: () => suggest(phrase) },
        h("span", { class: "route-starter-icon " + tint }, icon(ic, 15)), h("b", {}, t(en, ru)), h("span", {}, t(lineEn, lineRu))))));
}
