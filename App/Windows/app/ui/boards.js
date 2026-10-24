// The blocks on the canvas, as the Mac's plan column draws them (Studio.swift plan, RouteCards.swift, Canvases.swift):
// the newest on top; a money move as its route's cards — a picture of what the step does with the coin in it wearing the
// step's face, the state in words on it, the balance it changes — with Cancel and Confirm while it waits; an earlier move
// folded to a one-line pill. Live blocks (receive, schedule, favorites…) are live.js's, in this file's frame.

import { ui, act, engine, h, svg, icon, addIcons, mascot, logo, t, suggest, popup, copy, render, presence, usd, signedUsd, masked, qrCode, link, store } from "./core.js";
import { liveBlock, liveKinds, liveTitle } from "./live.js";
import { isAttached } from "./chat.js";
import { planStatus, planHead, planExtras, planStepState, PLAN_CHIPS } from "./trade.js";
// A namespace, so a step's «Open <place>» shows only once portfolio.js exports toggleDetail (its place details board).
import * as portfolio from "./portfolio.js";

addIcons({
  "exclamationmark.octagon": "M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3zM12 7.5v5.5M12 16v.5",
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
  "bubble.left": "M5 5h14a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H10l-4 3.5V16H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z",
  "questionmark.bubble": "M5 5h14a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H10l-4 3.5V16H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM10 9a2 2 0 1 1 2.6 1.9c-.4.2-.6.5-.6.9v.4M12 13.8v.2",
  "xmark.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 9l6 6M15 9l-6 6",
  cart: "M3 4h2.5l2 11h10.5l2-7.5H6.5M9.5 20a1 1 0 1 0 0-2 1 1 0 0 0 0 2zM17 20a1 1 0 1 0 0-2 1 1 0 0 0 0 2z",
  "arrow.down.right.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 9l6 6M15 10v5h-5",
  "arrow.triangle.2.circlepath": "M4 12a8 8 0 0 1 13.7-5.7L20 8.5M20 4v4.5h-4.5M20 12a8 8 0 0 1-13.7 5.7L4 15.5M4 20v-4.5h4.5",
  "checkmark.shield": "M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6zM9 12l2 2 4-4",
  "minus.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 12h8",
  "arrow.up.right": "M7 17L17 7M9 7h8v8",
  flame: "M12 21c-3.6 0-6-2.5-6-5.8 0-3.6 3.2-5.5 4-9.2 2.6 1.6 3.9 4 3.6 6.2 1-.6 1.7-1.6 1.9-2.8 1.6 1.6 2.5 3.6 2.5 5.8 0 3.3-2.4 5.8-6 5.8z",
  "arrow.down.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7.5v8.5M8.5 12.5 12 16l3.5-3.5",
});

// MARK: The column

/** Blocks already drawn: only a new one fades in, so a redraw never blinks the canvas. */
const seen = new Set();
let seenReady = false;

export function planColumn() {
  const boards = ui.state.boards;
  // The newest move is shown whole, as the Mac's board of the canvas; the ones before it fold to pills.
  const newest = boards.find((b) => b.kind === "move")?.id;
  const { chart, plan, at } = chartPlace(boards);
  const fade = (b, el) => { if (!seen.has(b.id)) { seen.add(b.id); if (seenReady) el.classList.add("fresh"); } return el; };
  // A watched token's card sits to the right of Favorites, not above it (the owner, 2026-10-16: «открывай справа сбоку»).
  const favorites = boards.find((b) => b.kind === "favorites");
  const beside = favorites && boards.find((b) => b.kind === "token" && b.data?.token?.favorite);
  const items = boards.length ? boards.filter((b) => (b !== chart || at === "column") && b !== beside).map((b) =>
    b === favorites && beside ? h("div", { class: "route-fav-row" }, fade(b, drawBoard(b)), fade(beside, tokenBlock(beside)))
      : fade(b, b === plan ? moveBoard(b, b.id === newest, chart) : drawBoard(b, b.id === newest))) : [starter()];
  // What was on the canvas when it opened is already there; only what comes after fades in.
  setTimeout(() => { seenReady = true; }, 0);
  const column = h("div", { class: "route-plan" }, items);
  return at === "beside" ? h("div", { class: "route-plan-row" }, column, h("div", { class: "route-beside" }, fade(chart, chartBlock(chart)))) : column;
}

/** Where the newest chart goes (the Mac's chartPlace, the owner's picks, 2026-10-23): a trade plan of its coin waiting or
 * running is drawn on it («plan»); beside anything else on the canvas, so no Confirm sits above the chart; on its own,
 * in the column. */
function chartPlace(boards) {
  const chart = boards.find((b) => b.kind === "chart" && b.chart);
  if (!chart) return {};
  const s = ui.state, live = (b) => s.card?.id === b.id || s.runningBoard === b.id || b.state === "plan";
  const plan = boards.find((b) => b.kind === "move" && b.trade && live(b) && sameCoin(chart.chart, b.trade.coin));
  if (plan) return { chart, plan, at: "plan" };
  return { chart, at: boards.some((b) => b !== chart && b.kind !== "chart") ? "beside" : "column" };
}

/** A chart's coin is the plan's: by Hyperliquid's name, or for a chart drawn before it had one, by its title. */
function sameCoin(c, coin) {
  if (c.coin) return c.coin.toLowerCase() === coin.toLowerCase();
  return c.title.split("·")[0].trim().split(" ")[0].toUpperCase() === coin.replace(/^.*:/, "").toUpperCase();
}

export function blockTitle(b) {
  return liveKinds.includes(b.kind) ? liveTitle(b.kind) : b.title;
}

function drawBoard(b, newest) {
  if (b.kind === "move") return moveBoard(b, newest);
  if (b.kind === "token" && b.data?.token) return tokenBlock(b);
  if (b.kind === "note") return noteBlock(b);
  if (liveKinds.includes(b.kind)) return liveBlock(b);
  if (b.kind === "chart" && b.chart) return chartBlock(b);
  return resultBlock(b);
}

/** A block's frame: the Mac's board card with its glyph and title, its own buttons and the close; right-click brings it into the chat. */
export function blockFrame(b, title, body, { extra, width, tint, icon: glyph } = {}) {
  const el = h("div", { class: "route-block" + (isAttached(b.id) ? " route-attached" : ""), oncontextmenu: (e) => blockMenu(e, b, title) },
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
  const on = isAttached(b.id), s = ui.state;
  menu(e, [
    on ? [[icon("xmark.circle", 14), t("Take out of the chat", "Убрать из чата")], () => { ui.attached = null; render(); }]
      : [[icon("bubble.left", 14), t("Ask or act in chat", "Спросить или поручить в чате")], () => attach(b)],
    [[icon("questionmark.bubble", 14), t("What's going on here?", "Что здесь происходит?")],
      () => act("send", { text: t(`What's going on in «${title}»?`, `Что происходит в «${title}»?`), board: b.id }), !s.thinking && !!s.model?.connected],
    ...(more.length ? [null, ...more] : []),
  ]);
}

/** popup() with a third, optional element per item: false greys it out, as the Mac's .disabled. */
function menu(e, items) {
  const el = popup(e, items.map((it) => it && [it[0], it[1]]), "route-menu");
  const buttons = el.querySelectorAll(".item");
  items.filter(Boolean).forEach((it, n) => { if (it[2] === false) buttons[n].disabled = true; });
}

function attach(b, step = null) {
  ui.attached = b.id;
  // A route step's «Send to chat»: chat.js sends it as `step` with this block (the engine's stepBoard).
  ui.attachedStep = step === null ? null : { board: b.id, index: step };
  render();
  document.getElementById("composer")?.focus();
}

/** «Oct 22 at 10:38 PM», as the Mac's Fmt.date. */
function fmtDate(iso) {
  const d = new Date(iso), ru = ui.state.russian, locale = ru ? "ru-RU" : "en-US";
  return d.toLocaleDateString(locale, { month: "short", day: "numeric" }) + (ru ? " в " : " at ") + d.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
}

// MARK: A move

function moveBoard(b, newest, onChart) {
  const s = ui.state;
  // Running wins over the board's own state and the card: the engine can send both for a moment.
  const runningHere = s.runningBoard === b.id;
  const waitingHere = !runningHere && !!s.card && s.card.id === b.id;
  // A site's request waits in a slot of its own, answered by its id, never by the card's Confirm (Consent.swift).
  const siteHere = !runningHere && s.site?.id === b.id;
  const finished = !waitingHere && !runningHere && !siteHere;
  // A click on a finished move flips it from how it shows by default: the newest open, the others folded.
  const open = !finished || newest !== ui.opened.has("move:" + b.id);
  const toggle = () => { ui.opened.has("move:" + b.id) ? ui.opened.delete("move:" + b.id) : ui.opened.add("move:" + b.id); render(); };
  // A move stays on its canvas once it's done (EarlierBoards, the owner's «nothing disappears»): no Remove here.
  const menu = (e) => blockMenu(e, b, b.title, [[t("Copy details", "Скопировать подробности"), () => copy(moveText(b))]]);
  const steps = b.steps || [];
  const draft = waitingHere && s.multisigDraft;
  const states = steps.map((_, i) => stepState(b, i, waitingHere, runningHere));
  const route = !steps.length || draft ? null : routeCards(b, steps, states);

  if (finished && !newest && b.state !== "plan") {
    // An earlier move: its pill, and its route under it once opened.
    return h("div", { class: "route-move" + (isAttached(b.id) ? " route-attached" : ""), oncontextmenu: menu },
      pill(b, open, toggle), open ? route : null, open ? notes(b) : null);
  }
  const status = planStatus(b) || (siteHere ? [t("waiting for your answer", "ждёт твоего ответа"), "route-accent"] : waitingHere ? [t("ready to sign", "готово к подписи"), "profit"] : runningHere ? [t("in progress", "выполняется"), "route-accent"]
    : { done: [t("executed", "выполнено"), "profit"], failed: [t("failed", "не вышло"), "loss"], unknown: [t("sent, status unknown", "отправлено, статус неизвестен"), "route-amber"],
      nothing: [t("nothing to do", "нечего делать"), "secondary"], cancelled: [t("not signed", "не подписано"), "secondary"] }[b.state] || [b.state || "", "secondary"]);
  const head = h("div", { class: "route-move-head" },
      h("span", { class: "route-move-title" }, b.title),
      b.plan ? planHead(b) : null,
      h("span", { class: "route-status-chip " + status[1] }, status[0]),
      runningHere ? h("span", { class: "route-rc-spinner" }) : null,
      // A move left as it was (saved, never signed): only Refresh, which asks its request again for numbers of now.
      finished && !["done", "failed", "unknown", "nothing", "plan"].includes(b.state) && b.request ? h("button", { class: "btn route-bordered route-refresh", disabled: s.thinking,
        title: t(`Asks «${b.request}» again for fresh numbers`, `Спросит «${b.request}» снова, со свежими цифрами`), onclick: () => act("send", { text: b.request }) },
        icon("arrow.clockwise", 13), t("Refresh", "Обновить")) : null,
      // Right after the chip, not at the route's far end (the Mac's Spacer): a wide route would put Confirm under
      // «Add block» and Windows' caption buttons.
      waitingHere ? h("div", { class: "btns route-nowrap" },
        h("button", { class: "btn route-large", onclick: () => act("cancel", { board: s.card.id }) }, t("Cancel", "Отмена")),
        confirmButton(s.card)) : null);
  return h("div", { class: "route-move" + (isAttached(b.id) ? " route-attached" : ""), oncontextmenu: menu },
    onChart ? planChart(b, onChart, head) : head,
    finished && b.state !== "plan" ? outcome(b) : null,
    siteHere ? siteRequest(s.site) : draft ? multisigSetup(draft) : route,
    b.plan ? planExtras(b) : null,
    // What will be signed and what's checked, read before Confirm (the Mac's route footer under the waiting move).
    waitingHere && b.note ? h("div", { class: "route-footer", "data-no-pan": "" }, icon("signature", 15), h("div", {}, b.note)) : null,
    waitingHere && s.card.warnings.length ? h("div", { class: "route-warn-box", "data-no-pan": "" }, icon("exclamationmark.triangle", 14),
      h("div", {}, s.card.warnings.map((w) => h("div", {}, w)))) : null,
    runningHere ? signing(b) : null,
    notes(b));
}

/** The waiting move's Confirm (PendingButtons): a priced swap's holds 15 s after its quote, counting down, then turns into
 * Refresh; while it's quoted, «Quoting…». The chat's field shows the same (chat.js waitingBar). */
let confirmTimer = null;
function confirmButton(card) {
  const s = ui.state, q = s.quote;
  const leftNow = () => Math.min(15, Math.max(0, Math.ceil(15 - (Date.now() - new Date(q.at).getTime()) / 1000)));
  const left = q?.state === "fresh" && q.at ? leftNow() : 0;
  const state = q?.state === "fresh" && left === 0 ? "stale" : q?.state;
  const busy = s.thinking || !!s.runningBoard;
  clearInterval(confirmTimer);
  if (state === "stale") {
    return h("button", { class: "btn primary route-large", disabled: busy, title: t("The price is over 15 seconds old: ask for it again", "Цене больше 15 секунд: спроси её снова"),
      onclick: () => act("requote") }, icon("arrow.clockwise"), t("Refresh", "Обновить"));
  }
  if (state === "quoting") return h("span", { class: "route-quoting secondary" }, icon("hourglass", 13), t("Quoting…", "Котирую…"));
  const label = h("span", { class: "num" }, state === "fresh" ? `${card.confirm} · ${left}` : card.confirm);
  if (state === "fresh") {
    confirmTimer = setInterval(() => {
      const n = leftNow();
      if (n === 0 || !document.body.contains(label)) { clearInterval(confirmTimer); if (n === 0) render(); return; }
      label.textContent = `${card.confirm} · ${n}`;
    }, 1000);
  }
  return h("button", { class: "btn primary route-large", disabled: busy, "aria-label": card.confirm, onclick: () => act("confirm", { board: card.id }) }, icon("hello"), label);
}

/** The ticks a site's request has had, by its id: Approve counts only once each risk it carries is ticked. */
const siteTicks = new Map();

/** A site's request as the Mac's ApprovalSheet: the site and its whole origin with how ihatecrypto knows it, what it asks
 * for, the warnings, then a tick for each risk (a lookalike, a payee or a program not on the list, a contract it can't
 * read, the second check's doubt) before Approve; Decline always. */
function siteRequest(site) {
  if (!siteTicks.has(site.id)) siteTicks.set(site.id, new Set());
  const ticked = siteTicks.get(site.id);
  const all = site.ticks.every((x) => ticked.has(x.id));
  const chip = {
    known: [t("Known site", "Знакомый сайт"), "profit", "checkmark.seal"],
    unknown: [t("Unfamiliar site", "Незнакомый сайт"), "route-amber", "questionmark.circle"],
    lookalike: [site.real ? t(`Lookalike of ${site.real}`, `Двойник ${site.real}`) : t("Lookalike", "Двойник"), "loss", "exclamationmark.octagon"],
  }[site.check];
  return h("div", { class: "route-site", "data-no-pan": "" },
    h("div", { class: "route-site-head" },
      h("span", { class: "route-site-letter" }, (site.host || "?").slice(0, 1).toUpperCase()),
      h("div", { class: "route-site-name" },
        h("div", { class: "route-site-host" }, h("b", { class: "one-line" }, site.host),
          chip ? h("span", { class: "route-status-chip " + chip[1] }, icon(chip[2], 11), chip[0]) : null),
        // The whole origin too: a phishing copy differs in what the big line leaves out.
        h("div", { class: "route-site-origin selectable" }, site.origin))),
    site.lines.length ? h("div", { class: "route-site-lines selectable" }, site.lines.map((l) => h("div", {}, l))) : null,
    site.warnings.length ? h("div", { class: "route-warn-box" }, icon("exclamationmark.triangle", 14), h("div", {}, site.warnings.map((w) => h("div", {}, w)))) : null,
    site.ticks.map((x) => h("div", { class: "route-site-tick" },
      h("b", {}, x.title), h("span", { class: "secondary small" }, x.detail),
      h("label", { class: "route-site-toggle" },
        h("input", { type: "checkbox", checked: ticked.has(x.id), onchange: (e) => { e.target.checked ? ticked.add(x.id) : ticked.delete(x.id); render(); } }),
        h("span", {}, x.toggle)))),
    h("div", { class: "route-site-buttons" }, h("span", { class: "spacer" }),
      h("button", { class: "btn route-large", onclick: () => { siteTicks.delete(site.id); act("siteAnswer", { board: site.id, test: false }); } }, t("Decline", "Отклонить")),
      h("button", { class: "btn primary route-large", disabled: !all, title: all ? site.approve : t("Tick each box above first", "Сначала отметь каждый пункт выше"),
        onclick: () => { const ids = [...ticked]; siteTicks.delete(site.id); act("siteAnswer", { board: site.id, test: true, ticks: ids }); } }, site.approve)));
}

/** «✓ Executed · Oct 22 at 10:38 PM» over a finished move's cards. */
function outcome(b) {
  // Sent with no answer back (the Mac's outcome.unknown): said so, and never offered again — it may have gone through.
  const [text, cls, mark] = { done: [t("Executed", "Выполнено"), "profit", true], failed: [t("Failed", "Не вышло"), "loss", false],
    unknown: [t("Sent, status unknown — check the links in the chat before doing it again", "Отправлено, статус неизвестен — проверь ссылки в чате, прежде чем повторять"), "route-amber", null],
    nothing: [t("Nothing to do", "Нечего делать"), "secondary", null] }[b.state]
    || [t("Not signed", "Не подписано"), "secondary", null];
  const retry = b.state === "failed" && ui.state.retryBoard === b.id
    ? h("button", { class: "btn small route-retry", disabled: ui.state.thinking || !!ui.state.runningBoard, onclick: () => act("retry") }, icon("arrow.clockwise", 12), t("Try again", "Ещё раз"))
    : null;
  return h("div", { class: "route-outcome " + cls }, statusMark(mark), text + " · " + fmtDate(b.at), retry);
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
  if (b.steps?.[i]?.status === "then") return "next";
  if (runningHere) {
    const current = currentStep(b.steps, ui.state.running?.lines || []);
    return i < current ? "done" : i > current ? "next" : ui.state.signer.waiting ? "touch" : "now";
  }
  const planned = planStepState(b, i, waitingHere);
  if (planned) return planned;
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
  const l = englishVerb(label.toUpperCase());
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
/** A Russian label's verb in English («ОТКУДА · SOLANA» → «FROM · SOLANA»), so it picks the same drawing: the Mac keeps
 *  its labels English, the engine here says them in the reply's language. */
const RU_VERBS = [["ОБМЕН МЕЖДУ СЕТЯМИ", "SWAP ACROSS"], ["ОБМЕН RELAY", "RELAY SWAP"], ["СЖЕЧЬ И ВЫПУСТИТЬ", "BURN & MINT"], ["ОТКУДА", "FROM"],
  ["КУДА", "TO"], ["РАЗРЕШЕНИЕ", "APPROVE"], ["ПРИДЁТ", "ARRIVES"], ["ЗАЁМ", "BORROW"], ["МОСТ", "BRIDGE"], ["ПОКУПКА", "BUY"], ["ПРОДАЖА", "SELL"],
  ["ПРОВЕРКА", "CHECK"], ["ЗАБРАТЬ", "CLAIM"], ["ЗАКРЫТИЕ", "CLOSE"], ["ЗАЛОГ", "COLLATERAL"], ["КАЖДЫЕ", "EVERY"], ["ГАЗ", "GAS"],
  ["ЗАБЛОКИРОВАНО", "LOCKED"], ["МУЛЬТИСИГ", "MULTISIG"], ["ОПЛАТА", "PAY"], ["ВОЗВРАТ", "REPAY"], ["ОТЗЫВ", "REVOKE"], ["ПОДПИСЬ", "SIGN"],
  ["СИМУЛЯЦИЯ", "SIMULATION"], ["ОБМЕН", "SWAP"], ["СДЕЛКА", "TRADE"], ["ОТКРЫТЬ ЛОНГ", "OPEN LONG"], ["ОТКРЫТЬ ШОРТ", "OPEN SHORT"]];
function englishVerb(l) {
  const hit = RU_VERBS.find(([ru]) => l.startsWith(ru) && !/\p{L}/u.test(l[ru.length] || ""));
  if (hit) return hit[1] + l.slice(hit[0].length);
  // «В · KAMINO LEND» goes into a deposit; «В BASE» (on Base) stays a place.
  return l.startsWith("В ·") ? "INTO" + l.slice(1) : l;
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
Object.assign(CHIPS, PLAN_CHIPS);

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
  // An approval wears its spender's colour: the next step's place (the Mac's route colours APPROVE · EXACT so).
  const next = kind === "approve" ? b.steps?.[i + 1]?.label.split("·").map((x) => x.trim()) : null;
  const fill = next ? fillFor(next.length > 1 ? pretty(next.slice(1).join(" · ")) : "", next[0]) : fillFor(place || "", parts[0]);
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
    oncontextmenu: (e) => { e.preventDefault(); e.stopPropagation(); menu(e, [
      [[icon("questionmark.circle", 14), t("Why this way?", "Почему так?")], () => act("whyStep", { board: b.id, index: i })],
      [[icon("pencil", 14), t("Edit step…", "Изменить шаг…")], () => openStep(b, i, true), !!editOf(b, state)],
      // That step goes with the next message, with its money and rows (the Mac's sendToChat), not the whole block.
      [[mascot(14, "idle"), t("Send to chat", "Отправить в чат")], () => { closeStep(b, i); attach(b, i); }],
      null,
      [[icon("copy", 14), t("Copy details", "Скопировать подробности")], () => copy(stepText(step, i))],
    ]); },
  },
    h("div", { class: "route-step-pic" }, scene(kind, state, step, w), state !== "plain" && kind !== "result" ? stateChip(state, compact) : null),
    stepInfo(step, state, kind, place, compact, isOpen),
    state === "now" || state === "touch" ? h("span", { class: "route-ring" + (state === "touch" ? " route-touch" : "") }) : null);
}

/** Opens a step's details, as its click does; by default the first step of the move waiting for Confirm. */
export function openStep(b = ui.state.boards.find((x) => x.id === ui.state.card?.id), i = 0, focus = false) {
  if (!b) return;
  for (const k of [...ui.opened]) if (k.startsWith("step:" + b.id + ":")) ui.opened.delete(k);
  ui.opened.add("step:" + b.id + ":" + i);
  render();
  if (focus) setTimeout(() => document.getElementById("route-amount")?.focus(), 0);
}

// A step's details: ← and → go to the step before and after, Esc closes them (the Mac's StepDetails keys); not while
// an amount is being typed, where the arrows move the cursor.
document.addEventListener("keydown", (e) => {
  if (!["ArrowLeft", "ArrowRight", "Escape"].includes(e.key) || /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "")) return;
  const key = [...ui.opened].find((k) => k.startsWith("step:"));
  if (!key || document.querySelector(".menu")) return;
  const [, id, n] = key.split(":");
  const count = ui.state?.boards?.find((x) => x.id === id)?.steps?.length || 0;
  e.preventDefault();
  e.stopPropagation();
  ui.opened.delete(key);
  if (e.key !== "Escape" && count > 1) ui.opened.add(`step:${id}:${(Number(n) + (e.key === "ArrowLeft" ? -1 : 1) + count) % count}`);
  render();
}, true);

function closeStep(b, i) {
  if (ui.opened.delete("step:" + b.id + ":" + i)) render();
}

/** What the waiting move lets change in this step (the engine's stepEdit): only a ready step of the move on its card. */
function editOf(b, state) {
  const e = ui.state.stepEdit;
  return state === "ready" && e && e.board === b.id ? e : null;
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

/** What's typed in an opened step's details, kept across the redraws a new state makes: its key, the amount, the way. */
let edit = null;

/** The opened step, under its route: what it is, its money and lines, «Ask in chat», Close; the notch points at its card.
 * A step of the move waiting for Confirm that can change (StepDetails) gets the amount and, for a trade on an EVM
 * network, the way; Save makes it a new plan through the same checks and the same Confirm. */
function stepDetails(b, steps, i, state, notch) {
  const step = steps[i];
  const stepKey = "step:" + b.id + ":" + i;
  const can = editOf(b, state);
  if (can && edit?.key !== stepKey) edit = { key: stepKey, amount: can.amount ?? "", way: can.way ?? null, problem: null };
  const go = (d) => { const next = (i + d + steps.length) % steps.length; ui.opened.delete(stepKey); ui.opened.add("step:" + b.id + ":" + next); render(); };
  const close = () => { ui.opened.delete(stepKey); if (edit?.key === stepKey) edit = null; render(); };
  const save = async () => {
    try {
      await engine("editStep", { board: b.id, ...(can.amount != null ? { text: edit.amount } : {}), ...(can.ways ? { name: edit.way || "" } : {}) });
      ui.opened.delete(stepKey);
      edit = null;
      render();
    } catch (e) { edit.problem = e.message; render(); }
  };
  const hint = steps.flatMap((x) => x.lines).find((l) => l.startsWith("free"));
  const place = (step.label.split("·")[1] || "").trim();
  const key = logoKey(place);
  const el = h("div", { class: "route-step-details", "data-no-pan": "" },
    h("span", { class: "route-notch" }),
    h("div", { class: "route-details-head" },
      key ? placeLogo(key, 40) : h("span", { class: "route-details-glyph" }, mascot(30, "idle")),
      h("div", { class: "route-details-name" }, h("b", { class: "one-line" }, detailsName(step.label)),
        h("span", { class: "one-line" }, t(`Step ${i + 1} of ${steps.length} · ${step.title}`, `Шаг ${i + 1} из ${steps.length} · ${step.title}`))),
      h("span", { class: "spacer" }),
      openPlace(step),
      steps.length > 1 ? [h("button", { class: "icon-btn", title: t("Previous step · ←", "Предыдущий шаг · ←"), onclick: () => go(-1) }, icon("chevron.left", 13)),
        h("button", { class: "icon-btn", title: t("Next step · →", "Следующий шаг · →"), onclick: () => go(1) }, icon("chevron.right", 13))] : null,
      h("button", { class: "icon-btn", title: t("Close · Esc", "Закрыть · Esc"), onclick: close }, icon("xmark", 13))),
    can && can.amount != null ? amountField(can, hint, save) : null,
    can && can.ways ? ways(can.quotes) : null,
    can && can.stock ? stockWays(can.quotes) : null,
    step.money ? h("div", { class: "route-money-box route-big" }, h("div", { class: "route-money-head" }, state === "done" ? t("MONEY · WAS → NOW", "ДЕНЬГИ · БЫЛО → СТАЛО") : t("MONEY · NOW → AFTER", "ДЕНЬГИ · СЕЙЧАС → ПОСЛЕ")),
      moneyLine(step.money)) : null,
    !can && (step.lines.length || b.note) ? h("div", { class: "route-details-lines selectable" }, step.lines.filter(Boolean).map((l) => h("div", {}, l.replace("\t", " "))),
      b.note ? h("div", { class: "route-details-note" }, b.note) : null) : null,
    can && edit.problem ? h("div", { class: "route-edit-problem loss" }, icon("exclamationmark.triangle", 13), edit.problem) : null,
    h("div", { class: "route-details-buttons" },
      h("button", { class: "btn route-bordered route-large route-ask", title: t("Attaches this block to your next message", "Прикрепит этот блок к следующему сообщению"), onclick: () => { close(); attach(b); } },
        mascot(20, "waiting"), t("Ask in chat", "Спросить в чате")),
      h("span", { class: "spacer" }),
      can ? [h("button", { class: "btn route-bordered route-large", onclick: close }, t("Cancel", "Отмена")),
        h("button", { class: "btn primary route-large", disabled: !!ui.state.thinking, onclick: save }, t("Save", "Сохранить"))]
        : h("button", { class: "btn route-bordered route-large", onclick: close }, t("Close", "Закрыть"))));
  el.querySelector(".route-notch").style.left = Math.min(Math.max(notch - 7, 24), 520 - 38) + "px";
  return el;
}

/** The place a step's logo names opens its own details (the Mac's Openable.place(logo:)): «Open Kamino». */
const OPENABLE = { sol: "solana", hype: "hyperliquid", jup: "jupiter", kamino: "kamino", near: "near", robinhood: "robinhood", morpho: "morpho" };
function openPlace(step) {
  const where = (step.label.split("·")[1] || "").trim();
  const place = OPENABLE[logoKey(where) || logoKey(where.split(" ")[0])];
  if (!place || typeof portfolio.toggleDetail !== "function") return null;
  const name = place[0].toUpperCase() + place.slice(1);
  return h("button", { class: "btn route-bordered route-small-btn", title: t("The place's own details", "Подробности этого места"), onclick: () => portfolio.toggleDetail(place) },
    t(`Open ${name}`, `Открыть ${name}`));
}

/** «ROUTES · BEST IS TAKEN»: a Robinhood Chain trade's ways as quoted now, the best marked, what the others leave less;
 * Confirm takes the best. */
function stockWays(quotes) {
  const name = (w) => (w === "v4" ? "Uniswap v4" : w === "v3" ? "Uniswap v3" : t("Relay swap", "Обмен через Relay"));
  return h("div", { class: "route-edit" },
    h("div", { class: "route-edit-head" }, t("ROUTES · BEST IS TAKEN", "ПУТИ · БЕРЁМ ЛУЧШИЙ")),
    quotes && quotes.length ? [
      quotes.map((q, i) => h("div", { class: "route-way route-stock" + (i === 0 ? " route-on" : "") },
        h("span", { class: "route-way-logos" }, logoRound(q.way === "relay" ? "relay" : "uni", 24)),
        h("span", { class: "route-way-text" }, h("b", {}, name(q.way)), h("span", { class: "one-line" }, `≈ ${q.gets}` + (q.less ? t(` · ${q.less} less`, ` · на ${q.less} меньше`) : ""))),
        h("span", { class: "spacer" }),
        i === 0 ? h("span", { class: "route-way-badge" }, t("Best", "Лучший")) : null)),
      h("div", { class: "route-edit-foot" }, t("Quoted again right before signing; the price must sit within 5% of the market.", "Котировка повторится перед подписью; цена должна быть не дальше 5% от рынка.")),
    ] : h("div", { class: "route-edit-foot" }, quotes ? t("No way quotes it right now.", "Сейчас ни один путь не даёт котировку.") : t("Comparing Uniswap v4, v3 and Relay’s swap…", "Сравниваю Uniswap v4, v3 и обмен Relay…")));
}

/** «BRIDGE · RELAY» → «Bridge · Relay» (StepDetails.pretty). */
function detailsName(label) {
  return label.replace(/\s*·\s*/g, " · ").toLowerCase().replace(/(^|[\s·(-])(\p{L})/gu, (m, a, b) => a + b.toUpperCase())
    .replace(/ Or /g, " or ").replace(/Usdc/g, "USDC").replace(/Jitosol/g, "JitoSOL").replace(/Hlp/g, "HLP").replace(/V3/g, "v3")
    .replace(/Layerzero/g, "LayerZero").replace(/Cctp/g, "CCTP");
}

/** «YOU SEND»: the amount as one number, in what the move spends; Enter saves. */
function amountField(can, hint, save) {
  const key = logoKey(can.unit);
  return h("div", { class: "route-edit" },
    h("div", { class: "route-edit-head" }, t("YOU SEND", "ОТПРАВЛЯЕШЬ")),
    h("label", { class: "route-amount" },
      key ? logoRound(key, 24) : null,
      h("input", { id: "route-amount", class: "num", value: edit.amount, inputmode: "decimal", autocomplete: "off", spellcheck: "false", "aria-label": t("Amount", "Сумма"),
        oninput: (e) => { edit.amount = e.target.value; }, onkeydown: (e) => { if (e.key === "Enter") { e.preventDefault(); save(); } } }),
      h("span", { class: "route-amount-unit" }, can.unit),
      h("span", { class: "spacer" }),
      hint ? h("span", { class: "route-amount-hint one-line" }, hint) : null));
}

/** «ROUTE»: the best of both (quoted again before signing), Relay's swap, or Relay's bridge + Uniswap; each with what it
 * gets now once the engine has quoted them (the better first). */
function ways(quotes) {
  const name = (w) => (w === "relay" ? t("Relay swap", "обмен через Relay") : "Relay + Uniswap");
  const quoted = (w) => { const q = quotes?.find((x) => x.way === w); return q && t(`≈ ${q.gets} · ${q.worth} after gas`, `≈ ${q.gets} · ${q.worth} после газа`); };
  const best = quotes?.[0];
  const row = (value, title, detail, logos, recommended) => h("button", {
    class: "route-way" + (edit.way === value ? " route-on" : ""), "aria-pressed": String(edit.way === value),
    onclick: () => { edit.way = value; render(); },
  },
    h("span", { class: "route-radio" }),
    h("span", { class: "route-way-logos" }, logos.map((k) => logoRound(k, 24))),
    h("span", { class: "route-way-text" }, h("b", {}, title), h("span", { class: "one-line" }, detail)),
    h("span", { class: "spacer" }),
    recommended ? h("span", { class: "route-way-badge" }, t("Recommended", "Рекомендую")) : null);
  return h("div", { class: "route-edit" },
    h("div", { class: "route-edit-head" }, t("ROUTE", "ПУТЬ")),
    row(null, t("The best one", "Лучший"), best ? t(`now ${name(best.way)}, ≈ ${best.gets} · checked again before signing`, `сейчас ${name(best.way)}, ≈ ${best.gets} · проверим ещё раз перед подписью`)
      : t("both quoted, gas counted · checked again before signing", "оба пути посчитаны с газом · проверим ещё раз перед подписью"), ["relay", "uni"], true),
    row("relay", t("Relay swap", "Обмен через Relay"), quoted("relay") || t("one Solana signature, Relay swaps on the way", "одна подпись в Solana, Relay обменяет по пути"), ["relay"]),
    row("uniswap", t("Relay bridge + Uniswap", "Мост Relay + Uniswap"), quoted("uniswap") || t("USDC over the bridge, then Uniswap with an exact approve", "USDC по мосту, потом Uniswap с точным разрешением"), ["relay", "uni"]));
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
/** What making it costs on each network picked (the engine's multisigFees, the Mac's), asked again when the networks or
 * the keys change. */
const msigFee = { key: null, fees: {} };
function msigFees(d) {
  const key = d.networks.join(",") + "/" + d.keys.length;
  if (msigFee.key !== key) {
    msigFee.key = key;
    msigFee.fees = {};
    engine("multisigFees").then((r) => { if (msigFee.key === key) { msigFee.fees = r || {}; render(); } }).catch(() => {});
  }
  return msigFee.fees;
}

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
          const on = d.networks.includes(key), fee = on ? msigFees(d)[key] : null;
          return h("button", { class: "route-net-chip" + (on ? " route-on" : ""), onclick: () => act("multisigNetwork", { network: key }) }, logo(logoName, 18), name,
            fee ? h("span", { class: "secondary small" }, fee) : null);
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
  const d = b.data || {};
  if (d.pnl) return pnlBlock(b, d.pnl);
  if (d.rates) return ratesBlock(b, d.rates);
  if (d.movers) return moversBlock(b, d.movers);
  if (d.approvals) return approvalsBlock(b, d.approvals);
  if (d.dca) return dcaBlock(b, d.dca);
  if (d.receive) return receiveBlock(b, d.receive);
  if (d.service) return serviceBlock(b, d.service);
  const rows = (b.lines || []).map((l) => l === "—" ? h("div", { class: "route-result-divider" }) : h("div", { class: "route-result-row selectable" }, l));
  return blockFrame(b, b.title, h("div", { class: "route-result" },
    rows.length ? h("div", { class: "route-result-rows" }, rows) : null,
    b.actions && b.actions.length ? h("div", { class: "btns" }, b.actions.map((a) => h("button", { class: "btn route-bordered", title: a.command, onclick: () => suggest(a.command) }, a.title))) : null),
  { width: 520, extra: refresh(b) });
}

/** ↻ on a board drawn by a request: asks it again, for numbers of now (the Mac's Refresh on a board left as it was). */
function refresh(b) {
  if (!b.request) return null;
  return h("button", { class: "icon-btn", title: t(`Ask again: «${b.request}»`, `Спросить снова: «${b.request}»`), disabled: ui.state.thinking,
    onclick: () => act("send", { text: b.request }) }, icon("arrow.clockwise"));
}

const RANGES = [["day", "24h"], ["week", "7d"], ["month", "30d"]];

/** A chart's moving parts (AssetPrice + AssetChart): the price now or under the pointer, the move, the range picker and
 * the line with its fill; with a plan's levels across it and its loss and gain as bands (ChartZone), the domain
 * stretched to show them, as the Mac's. Drawn from numbers only. */
function chartParts(b, { levels = [], zones = [], H = 190 } = {}) {
  const c = b.chart, pts = c.closes;
  const coin = c.title.split("·")[0].trim();
  // The engine's titles: «ETH · 24 h», «ETH · 7 days», «ETH · 30 days» («7 days» has «day» in it, so day is what's left).
  const range = /30|month|мес/i.test(c.title) ? "month" : /\b7\b|week|недел/i.test(c.title) ? "week" : "day";
  const up = !String(c.change).trim().startsWith("-") && !String(c.change).trim().startsWith("−");
  const W = 438;
  const lo0 = Math.min(...pts), hi0 = Math.max(...pts);
  const shown = levels.filter((l) => l.price > lo0 * 0.85 && l.price < hi0 * 1.15);
  const bottom = Math.min(lo0, ...shown.map((l) => l.price)), top = Math.max(hi0, ...shown.map((l) => l.price));
  const pad = Math.max((top - bottom) * 0.1, top * 0.002) || 1;
  const lo = bottom - pad, span = top + pad - lo;
  const X = (i) => (i / Math.max(pts.length - 1, 1)) * W, Y = (p) => H - ((Math.min(Math.max(p, lo), lo + span) - lo) / span) * H;
  const line = pts.map((p, i) => `${i ? "L" : "M"}${X(i).toFixed(1)} ${Y(p).toFixed(1)}`).join("");
  const gid = "fill-" + b.id;
  const dot = svg("circle", { r: 4, class: "route-chart-dot", cx: -100, cy: -100 });
  const rule = svg("path", { d: "", class: "route-chart-rule" });
  const plot = svg("svg", { class: "route-chart-plot " + (up ? "route-up" : "route-down"), "data-no-pan": "", viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: "none" },
    svg("defs", {}, svg("linearGradient", { id: gid, x1: 0, y1: 0, x2: 0, y2: 1 }, svg("stop", { offset: "0", class: "route-grad-top" }), svg("stop", { offset: "1", class: "route-grad-bottom" }))),
    // Under the price: the bands, then the line, then the levels over it.
    zones.map((z) => { const y1 = Y(Math.max(z.from, z.to)), y2 = Y(Math.min(z.from, z.to)); return svg("rect", { x: 0, y: y1, width: W, height: Math.max(y2 - y1, 0), class: "route-zone route-" + z.tint }); }),
    svg("path", { d: line + `L${W} ${H}L0 ${H}Z`, fill: `url(#${gid})` }), svg("path", { d: line, class: "route-chart-line" }),
    shown.map((l) => svg("path", { d: `M0 ${Y(l.price).toFixed(1)}H${W}`, class: "route-level route-" + l.tint + (l.dashed ? " route-dashed" : "") })),
    rule, dot);
  plot.style.height = H + "px";
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
  // A level's name and price over its line, in its color (the Mac's annotation): HTML, so the stretched drawing doesn't stretch it.
  const area = h("div", { class: "route-chart-area" }, plot,
    shown.map((l) => { const el = h("span", { class: "route-level-label route-" + l.tint }, `${l.label} ${price(l.price)}`); el.style.top = `calc(${(Y(l.price) / H) * 100}% - 15px)`; return el; }));
  const pick = h("div", { class: "route-chart-range" }, RANGES.map(([key, label]) => h("button", { class: key === range ? "route-on" : "", onclick: () => key !== range && reload(c, coin, key) }, label)));
  return { coin, range, up, area, priceEl, changeEl, pick };
}

/** The chart again in a range, in place (the Mac's picker); an older board without its market's name asks the chat. */
function reload(c, coin, range) {
  return c.coin ? act("openChart", { name: c.coin, text: range }) : act("send", { text: `chart ${coin} ${range}` });
}

/** A coin's closes over the range, as the Mac's chart board: the price now (or under the pointer), the move, the range
 * picker, the line with its fill, the low and high. */
function chartBlock(b) {
  const c = b.chart, x = c.extras || {};
  // The position's entry, its stop and take orders and the alerts across the line (the Mac's ChartBoard levels).
  const { coin, range, up, area, priceEl, changeEl, pick } = chartParts(b, { levels: x.levels || [] });
  const badge = logo(coin, 38);
  badge.classList.add("route-asset-badge");
  const network = x.network && x.network !== "hyperliquid" ? (ui.state.portfolio?.networks || []).find((n) => n.key === x.network)?.name || x.network : null;
  const day = x.change24h === null || x.change24h === undefined ? null : Number(x.change24h);
  const el = h("div", { class: "route-block route-chart-block" + (isAttached(b.id) ? " route-attached" : ""), oncontextmenu: (e) => blockMenu(e, b, b.title) },
    h("div", { class: "route-asset-head" }, badge,
      h("div", { class: "route-asset-name" }, h("b", {}, coin),
        h("span", {}, [network, t("Hyperliquid chart", "График Hyperliquid"), rangeTitle(range), t("as of ", "на ") + fmtDate(b.at)].filter(Boolean).join(" · "))),
      h("span", { class: "spacer" }),
      h("button", { class: "icon-btn", title: t("Load fresh candles from Hyperliquid", "Загрузить свежие свечи с Hyperliquid"), onclick: () => reload(c, coin, range) }, icon("arrow.clockwise")),
      h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => act("closeBoard", { board: b.id }) }, icon("xmark"))),
    h("div", { class: "route-asset-price" }, h("div", {}, priceEl, changeEl), h("span", { class: "spacer" }), pick),
    area,
    // The market's numbers, as the Mac's AssetStats; an older board with no market read keeps its low and high.
    x.tiles?.length ? [
      day !== null ? h("div", { class: "route-token-changes" }, h("span", { class: "route-change-chip" }, h("span", { class: "secondary" }, "24h "),
        h("b", { class: day < 0 ? "loss" : "profit" }, (day < 0 ? "−" : "+") + Math.abs(day).toFixed(1) + "%"))) : null,
      h("div", { class: "route-tiles" }, x.tiles.map((f) => tile(f.title, f.value)))]
      : h("div", { class: "route-tiles" },
        tile(t("Low", "Минимум"), c.low), tile(t("High", "Максимум"), c.high), tile(t("Change", "Изменение"), c.change, up ? "profit" : "loss")),
    x.held ? heldCard(x.held, coin) : null,
    assetActions(b, coin, coinTrades(c.coin || coin, x.network)));
  return el;
}

/** A trade plan drawn on its coin's chart (the Mac's PlanChartBoard, the owner's pick C): the plan's title with Cancel
 * and Confirm on top, the entry, stop and take across the line with the loss and the gain between them, and the trade's
 * numbers under it; the route cards follow under the board. */
function planChart(b, chart, head) {
  const p = b.trade;
  const levels = [
    ...p.alerts.map((price) => ({ price, label: t("Alert", "Оповещение"), tint: "alert", dashed: true })),
    { price: p.entry, label: p.limit ? t("Limit", "Лимит") : t("Entry ≈", "Вход ≈"), tint: "entry", dashed: true },
    { price: p.stop, label: t("Stop", "Стоп"), tint: "loss" },
    ...(p.take != null ? [{ price: p.take, label: t("Take", "Тейк"), tint: "profit" }] : []),
  ];
  const zones = [{ from: p.entry, to: p.stop, tint: "loss" }, ...(p.take != null ? [{ from: p.entry, to: p.take, tint: "profit" }] : [])];
  const { area, priceEl, changeEl, pick, coin, range } = chartParts(chart, { levels, zones, H: 250 });
  const side = p.side === "long" ? t("Long", "Лонг") : t("Short", "Шорт");
  return h("div", { class: "route-block route-chart-block route-plan-chart", "data-no-pan": "" },
    head,
    h("div", { class: "route-asset-price" }, h("div", {}, priceEl, changeEl), h("span", { class: "spacer" }), pick,
      h("button", { class: "icon-btn", title: t("Load fresh candles from Hyperliquid", "Загрузить свежие свечи с Hyperliquid"), onclick: () => reload(chart.chart, coin, range) }, icon("arrow.clockwise")),
      h("button", { class: "icon-btn", title: t("Hide the chart; the plan stays", "Скрыть график; план останется"), onclick: () => act("closeBoard", { board: chart.id }) }, icon("xmark"))),
    area,
    h("div", { class: "route-tiles route-five" },
      tile(`${side} · ${p.leverage}×`, `${p.notional} · ${p.size}`),
      tile(t("Margin", "Маржа"), p.margin),
      tile(t("Liquidation", "Ликвидация"), p.liquidation),
      tile(t("At the stop", "На стопе"), p.lossAtStop + (p.lossPct ? ` · ${p.lossPct}%` : ""), "loss"),
      tile(t("At the take", "На тейке"), p.profitAtTake, "profit")));
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

// MARK: Rich result boards (the Mac's TokenBoard, PnLBoard, YieldBubbles, MoversBoard, ApprovalsBoard, JupiterDCABoard,
// ReceiveAcrossBoard, ServiceAnswerBoard), drawn from the engine's data; their lines are what the model reads.

/** A network's coin: what its Buy and Sell put in the chat (the Mac's coinTrades). */
const GAS = { base: "ETH", arbitrum: "ETH", ethereum: "ETH", optimism: "ETH", robinhood: "ETH", polygon: "POL", bnb: "BNB", hyperevm: "HYPE" };
export function coinTrades(coin, chain) {
  const c = coin.toUpperCase(), pair = (buy, sell) => [[t(`Buy ${c}`, `Купить ${c}`), "cart", buy], [t("Sell", "Продать"), "arrow.down.right.circle", sell]];
  if (c === "SOL" && (!chain || chain === "solana")) return pair("buy SOL for 2 USDC", "sell 0.01 SOL");
  if (c === "NEAR") return pair("buy NEAR for 2 USDC", "sell 1 NEAR");
  if (chain === "hyperliquid") return pair(`buy ${c} on hyperliquid spot for 10 USDC`, `sell all ${c} on hyperliquid spot`);
  const net = GAS[chain] === c ? chain : Object.keys(GAS).find((k) => GAS[k] === c && k !== "robinhood");
  if (net) {
    const some = { POL: "5", BNB: "0.002", HYPE: "0.02" }[c] || "0.001", dollar = net === "robinhood" ? "USDG" : "USDC";
    return pair(`swap 2 ${dollar} for ${c} on ${net}`, `swap ${some} ${c} for ${dollar} on ${net}`);
  }
  return pair(`buy ${c} for 2 USDC`, `sell all ${c}`);
}

/** A token's Buy and Sell by its network, as the chat takes them (the Mac's tokenPhrases): stock tokens by ticker, a
 * verified Solana token by ticker, everything else by contract; bridged USDC.e first swaps to Circle's own; the network's own
 * dollar (`dollar`) is bought by selling the network's coin. */
export function tradePhrases(chain, address, symbol, { stock = false, verified = false, dollar = false } = {}) {
  if (dollar) {
    const coin = coinTrades(chain === "solana" ? "SOL" : GAS[chain] || "ETH", chain);
    return [[t(`Buy ${symbol}`, `Купить ${symbol}`), "cart", coin[1][2]], [t("Sell", "Продать"), "arrow.down.right.circle", coin[0][2]]];
  }
  const [buy, sell] = chain === "robinhood" ? (stock ? [`buy ${symbol} stock for 2 USDC`, `sell ${symbol} stock`] : [`buy token ${address} for 2 USDC`, `sell token ${address}`])
    : chain === "solana" ? (verified ? [`buy ${symbol} on solana for 2 USDC`, `sell ${symbol} on solana`] : [`buy token ${address} for 2 USDC`, `sell tokens ${address}`])
    : chain === "tempo" ? [`swap 2 USDC for ${symbol} on tempo`, `swap all ${symbol} for USDC on tempo`]
    : nets.has(chain) ? [`buy token ${address} on ${chain} for 2 USDC`, `sell tokens ${address} on ${chain}`] : [`buy ${symbol} for 2 USDC`, `sell all ${symbol}`];
  const bridged = symbol === "USDC.e" ? [[t("Swap to Circle's USDC", "Обменять на USDC от Circle"), "arrow.triangle.2.circlepath", `swap all USDC.e for USDC on ${chain}`]] : [];
  return [...bridged, [t(`Buy ${symbol}`, `Купить ${symbol}`), "cart", buy], [t("Sell", "Продать"), "arrow.down.right.circle", sell]];
}
const nets = new Set(["base", "arbitrum", "ethereum", "polygon", "optimism", "bnb", "hyperevm", "robinhood", "tempo"]);

/** The pills under a card: ask in the chat with it attached, then each phrase put in the chat's field. */
function assetActions(b, symbol, phrases, source) {
  return h("div", { class: "route-asset-actions" },
    h("button", { class: "btn small route-bordered", onclick: () => attach(b) }, icon("bubble.left", 13), t(`Ask about ${symbol}`, `Спросить про ${symbol}`)),
    phrases.map(([title, ic, phrase]) => h("button", { class: "btn small" + (ic === "cart" ? " route-buy" : ""), title: t(`Puts «${phrase}» in the chat`, `Вставит «${phrase}» в чат`), onclick: () => suggest(phrase) }, icon(ic, 13), title)),
    source ? h("span", { class: "route-asset-source" }, source) : null);
}

const TRANGES = [["day", "24h"], ["week", "7d"], ["month", "30d"]];

/** One token on the canvas (TokenBoard): its price and chart in a range, the pool's numbers, what you hold, links, then
 * ask, buy, sell or watch. */
function tokenBlock(b) {
  const c = b.data.token;
  // A token nobody vouches for never wears the famous logo of its ticker: its own picture, else its letter.
  const letter = () => { const el = h("span", { class: "logo letter route-token-pic" }, (c.symbol || "?").slice(0, 1).toUpperCase()); el.style.cssText = "width:38px;height:38px;font-size:17px"; return el; };
  const fallback = () => (c.verified === false ? letter() : logo(c.symbol, 38));
  const head = h("div", { class: "route-asset-head" },
    c.image ? h("img", { class: "route-token-pic", src: c.image, alt: "", referrerpolicy: "no-referrer", onerror: (e) => e.target.replaceWith(fallback()) }) : fallback(),
    h("div", { class: "route-asset-name" },
      h("b", { title: c.fullName }, c.name, c.verified === true ? h("span", { class: "route-verified", title: t(`Verified by ${c.verifiedBy || "a token list"}`, `Проверен: ${c.verifiedBy || "список токенов"}`) }, icon("checkmark.seal", 13))
        : c.verified === false ? h("span", { class: "route-unverified", title: t(`${c.verifiedBy || "The list"} hasn't verified it: it may be a copy under a famous name`, `${c.verifiedBy || "Список"} его не проверил: это может быть копия под известным именем`) }, t("not verified", "не проверен")) : null),
      h("span", {}, c.subtitle)),
    h("span", { class: "spacer" }),
    !c.loading ? h("button", { class: "icon-btn", title: t("Read the market again", "Прочитать рынок снова"), onclick: () => act("tokenRange", { board: b.id, text: c.range }) }, icon("arrow.clockwise")) : null,
    h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => act("closeBoard", { board: b.id }) }, icon("xmark")));
  const change = c.change === null || c.change === undefined ? null : Number(c.change);
  const chart = c.closes.length > 1 ? chartParts({ id: b.id, chart: { title: `${c.symbol} · ${c.range}`, closes: c.closes, change: (change < 0 ? "−" : "+") + Math.abs(change ?? 0).toFixed(2) + "%" } }) : null;
  const pick = h("div", { class: "route-chart-range" }, TRANGES.map(([key, label]) => h("button", { class: key === c.range ? "route-on" : "", onclick: () => key !== c.range && act("tokenRange", { board: b.id, text: key }) }, label)));
  const priceEl = chart ? chart.priceEl : h("span", { class: "route-chart-price" }, c.price !== null && c.price !== undefined ? price(Number(c.price)) : "—");
  const changeEl = change === null ? null : h("span", { class: "route-chart-change " + (change < 0 ? "loss" : "profit") }, (change < 0 ? "−" : "+") + Math.abs(change).toFixed(2) + "% · " + rangeTitle(c.range));
  const held = c.held ? heldCard(c.held, c.symbol) : null;
  const phrases = tradePhrases(c.chain, c.address, c.symbol, { stock: c.stock, verified: c.verified === true, dollar: c.dollar });
  const watch = !c.favorite && !c.dollar ? [[t(`Watch ${c.symbol}`, `Следить за ${c.symbol}`), "star", `watch token ${c.address} on ${c.chain}`]] : [];
  return h("div", { class: "route-block route-chart-block route-token" + (isAttached(b.id) ? " route-attached" : ""), oncontextmenu: (e) => blockMenu(e, b, b.title) },
    head,
    h("div", { class: "route-asset-price" }, h("div", {}, priceEl, changeEl), h("span", { class: "spacer" }), pick),
    c.loading ? h("div", { class: "route-token-loading secondary" }, t("Reading its market…", "Читаю его рынок…"))
      : chart ? chart.area : h("div", { class: "route-token-loading secondary" }, c.error || t("No chart for this period.", "Нет графика за этот период.")),
    c.changes.length ? h("div", { class: "route-token-changes" }, c.changes.map((x) => h("span", { class: "route-change-chip" }, h("span", { class: "secondary" }, x.title + " "),
      h("b", { class: Number(x.value) < 0 ? "loss" : "profit" }, (Number(x.value) < 0 ? "−" : "+") + Math.abs(Number(x.value)).toFixed(1) + "%")))) : null,
    c.tiles.length ? h("div", { class: "route-tiles" }, c.tiles.map((x) => tile(x.title, x.value))) : null,
    held,
    h("div", { class: "route-token-links" },
      h("button", { class: "link-btn", title: c.address, onclick: () => copy(c.address) }, icon("copy", 11), c.address.slice(0, 6) + "…" + c.address.slice(-4)),
      c.links.map((l) => link(l.url, l.label))),
    assetActions(b, c.symbol, [...phrases, ...watch], "DexScreener · GeckoTerminal"));
}

/** «You hold» on a token's or a chart's card (HeldRow): since when the P/L counts, and under it what was bought, what
 * came in and what it's worth. */
function heldCard(held, symbol) {
  return h("div", { class: "route-held plan-held" },
    h("div", {}, h("span", { class: "eyebrow" }, t("You hold", "У тебя") + (held.since ? " · " + held.since : "")), h("b", { class: "num" }, masked(`${held.amount} ${symbol}`)),
      h("span", { class: "secondary small" }, held.how + (held.today !== null && held.today !== undefined ? " · " + t("today ", "сегодня ") + masked(signedUsd(held.today)) : ""))),
    h("span", { class: "spacer" }),
    h("div", { class: "route-held-value" }, h("b", { class: "num" }, masked(held.usd !== null && held.usd !== undefined ? usd(held.usd) : "—")),
      held.pnl ? h("span", { class: "num small " + (Number(held.gain) < 0 ? "loss" : "profit") }, masked(held.pnl)) : null),
    held.paid ? h("div", { class: "plan-held-paid secondary small" }, masked(held.paid)) : null);
}

/** P/L (PnLBoard): the period picker, trading in the period, open positions, what's held, the portfolio's change, by
 * market, the best and the worst trade. */
const pnlPeriod = new Map();
function pnlBlock(b, p) {
  const traded = p.reports.some((r) => Number(r.realized) || Number(r.funding) || Number(r.unrealized) || r.byAsset.length);
  const onlyHeld = p.held.length > 0 && !traded;
  const key = pnlPeriod.get(b.id) || p.period, r = p.reports.find((x) => x.period === key) || p.reports[0];
  const tone = (v) => (Number(v) < 0 ? "loss" : "profit");
  const line = (title, value, detail) => h("div", { class: "route-pnl-line" },
    h("div", {}, h("b", {}, title), h("span", { class: "secondary small" }, masked(detail))), h("span", { class: "spacer" }), h("b", { class: "num " + tone(value) }, masked(signedUsd(value))));
  const picker = onlyHeld ? null : h("div", { class: "route-chart-range" }, p.reports.map((x) =>
    h("button", { class: x.period === r.period ? "route-on" : "", onclick: () => { pnlPeriod.set(b.id, x.period); render(); } }, x.title)));
  const priced = p.held.filter((x) => x.gain !== null && x.gain !== undefined);
  const worth = p.held.reduce((a, x) => a + Number(x.usd || 0), 0), gain = priced.reduce((a, x) => a + Number(x.gain), 0);
  const body = h("div", { class: "route-pnl" },
    onlyHeld
      ? h("div", { class: "route-pnl-head" }, h("span", { class: "secondary small" }, t("Held now · every network", "Сейчас в кошельке · все сети")),
          h("b", { class: "route-pnl-big num " + (priced.length ? tone(gain) : "") }, masked(priced.length ? signedUsd(gain) : usd(worth))),
          h("span", { class: "secondary small" }, masked(t(`worth ${usd(worth)}`, `стоит ${usd(worth)}`) + (priced.length ? t(` · bought for ${usd(priced.reduce((a, x) => a + Number(x.paid || 0), 0))}`, ` · куплено за ${usd(priced.reduce((a, x) => a + Number(x.paid || 0), 0))}`) : t(" · cost unknown", " · цена покупки неизвестна")))))
      : h("div", { class: "route-pnl-head" }, h("span", { class: "secondary small" }, t("Trading · ", "Торговля · ") + r.title),
          h("b", { class: "route-pnl-big num " + tone(r.trading) }, masked(signedUsd(r.trading))),
          h("span", { class: "secondary small" }, masked(t(`realized after fees ${signedUsd(r.realized)} · funding ${signedUsd(r.funding)}`, `реализовано после комиссий ${signedUsd(r.realized)} · фандинг ${signedUsd(r.funding)}`))
            + t(` · ${r.closed} closed, ${r.wins} in profit`, ` · закрыто ${r.closed}, в плюсе ${r.wins}`))),
    onlyHeld ? null : line(t("Open positions now", "Открытые позиции сейчас"), r.unrealized, t("not realized yet", "ещё не реализовано")),
    p.held.length ? h("div", { class: "route-pnl-section" }, h("span", { class: "eyebrow" }, t("Held now", "Сейчас в кошельке")),
      p.held.map((x) => h("div", { class: "route-pnl-row" }, logo(logoKey(x.network) || x.network, 22),
        h("div", { class: "route-pnl-name" }, h("b", { class: "one-line" }, `${x.symbol} · ${x.place}`),
          h("span", { class: "secondary small one-line" }, masked(x.amount + (x.usd !== null && x.usd !== undefined ? ` ≈ ${usd(x.usd)}` : "")
            + (x.paid !== null && x.paid !== undefined ? t(` · bought for ${usd(x.paid)}`, ` · куплено за ${usd(x.paid)}`) : t(" · cost unknown", " · цена покупки неизвестна"))))),
        h("span", { class: "spacer" }),
        h("b", { class: "num " + (x.gain !== null && x.gain !== undefined ? tone(x.gain) : "") }, masked(x.gain !== null && x.gain !== undefined ? signedUsd(x.gain) : x.usd !== null && x.usd !== undefined ? usd(x.usd) : t("no price", "нет цены")))))) : null,
    !p.coin ? (r.change !== null && r.change !== undefined && r.changeSince
      ? line(t(`Portfolio since ${fmtDate(r.changeSince)}`, `Портфель с ${fmtDate(r.changeSince)}`), r.change,
          Number(r.flows) === 0 ? t("prices, yield and trading together", "цены, доходность и торговля вместе") : t(`without ${signedUsd(r.flows)} you moved in or out`, `без ${signedUsd(r.flows)}, которые ты завёл или вывел`))
      : h("div", { class: "secondary small" }, t("No portfolio history for this period yet: it builds up every ten minutes while the app is open.", "Истории портфеля за этот период пока нет: она копится каждые десять минут, пока приложение открыто."))) : null,
    r.byAsset.length ? h("div", { class: "route-pnl-section" }, h("span", { class: "eyebrow" }, t("By market", "По рынкам")),
      r.byAsset.map((a) => h("div", { class: "route-pnl-row" }, logo(a.coin.replace(/^.*:/, ""), 22), h("b", { class: "one-line" }, a.name),
        h("span", { class: "secondary small" }, t(`${a.closed} closed`, `закрыто ${a.closed}`)), h("span", { class: "spacer" }), h("b", { class: "num " + tone(a.realized) }, masked(signedUsd(a.realized)))))) : null,
    [r.best && Number(r.best.realized) > 0 ? [t("Best", "Лучшая"), r.best] : null, r.worst && Number(r.worst.realized) < 0 ? [t("Worst", "Худшая"), r.worst] : null].filter(Boolean).map(([label, x]) =>
      h("div", { class: "route-pnl-row" }, h("span", { class: "eyebrow route-pnl-tag" }, label),
        h("span", { class: "small one-line" }, `${x.long ? t("Long", "Лонг") : t("Short", "Шорт")} ${x.name} · ${fmtDate(x.closed)}`), h("span", { class: "spacer" }),
        h("b", { class: "num " + tone(x.realized) }, masked(signedUsd(x.realized))))),
    h("div", { class: "secondary small" }, onlyHeld
      ? t("The cost is what the wallet paid in stablecoins in the same transaction, read from the network. A token that came some other way has no cost.", "Цена покупки — сколько кошелёк заплатил стейблкоинами в той же транзакции, по данным сети. У токена, пришедшего иначе, цены покупки нет.")
      : t("Trading comes from Hyperliquid itself. Lending yield isn't counted separately: it's inside the portfolio change. Money that arrived while the app was closed can look like gain.", "Торговля — по данным самого Hyperliquid. Доход от депозитов отдельно не считается: он внутри изменения портфеля. Деньги, пришедшие, пока приложение было закрыто, могут выглядеть как прибыль.")));
  return blockFrame(b, "P/L" + (p.coin ? " · " + (onlyHeld ? p.held[0].symbol : p.coin.replace(/^.*:/, "")) : ""), body, { width: 470, extra: h("span", { class: "route-pnl-tools" }, picker, refresh(b)) });
}

/** Where money earns now (YieldBubbles): a bubble a rate, its size by the rate; a tap puts its phrase in the chat. */
const BUBBLE = [["Jupiter", "#131D27", "#C7F284"], ["Kamino", "#082A56", "#ffffff"], ["Jito", "#1F9D6A", "#ffffff"], ["HLP", "var(--hl)", "var(--hl-mint)"]];
function ratesBlock(b, rates) {
  const top = Math.max(...rates.map((o) => Number(o.apy)), 0.0001);
  const rateLogo = (p) => { const n = p.toLowerCase(); return n.includes("jupiter") ? "jup" : n.includes("kamino") ? "kamino" : n.includes("jito") ? "jitosol" : n.includes("hlp") ? "hype" : n.includes("aave") ? "aave" : n.includes("morpho") ? "morpho" : "usdc"; };
  return blockFrame(b, b.title, h("div", { class: "route-result" },
    h("span", { class: "eyebrow" }, t("Rates now · tap to put the phrase in the chat", "Ставки сейчас · нажми, чтобы вставить фразу в чат")),
    h("div", { class: "route-bubbles" }, rates.map((o) => {
      const size = 96 + 70 * (Number(o.apy) / top);
      const [, fill, ink] = BUBBLE.find(([k]) => o.place.includes(k)) || [null, "var(--solana-tint)", "var(--text)"];
      const el = h("button", { class: "route-bubble" + (o.how ? "" : " route-off"), title: o.note || o.how || "", disabled: !o.how, onclick: () => o.how && suggest(o.how) },
        logo(rateLogo(o.place), 26), h("b", { class: "num" + (size > 140 ? " route-big" : "") }, Number(o.apy).toFixed(2) + "%"), h("span", { class: "one-line" }, o.place), h("span", { class: "route-bubble-asset" }, o.asset));
      el.style.cssText = `width:${size}px;height:${size}px;background:${fill};color:${ink}`;
      return el;
    })),
    h("div", { class: "secondary small" }, t("Tip: don’t keep everything in one protocol — say “spread USDC across the best rates”.", "Совет: не держи всё в одном протоколе — скажи «разложи USDC по лучшим ставкам»."))),
  { width: 520, extra: refresh(b) });
}

/** Movers · 24 h (MoversBoard): up and down, each opens its chart. */
function moversBlock(b, m) {
  const col = (title, rows, up) => h("div", { class: "route-movers-col" }, h("span", { class: "eyebrow " + (up ? "profit" : "loss") }, title),
    rows.map((r) => h("button", { class: "route-mover " + (Number(r.change) < 0 ? "route-down" : "route-up"), title: t(`Open the ${r.name} chart`, `Открыть график ${r.name}`), onclick: () => act("openChart", { name: r.coin }) },
      logo(r.coin.replace(/^.*:/, ""), 24), h("span", { class: "route-mover-name" }, h("b", { class: "one-line" }, r.name), h("span", { class: "secondary num" }, r.price)),
      h("span", { class: "spacer" }), h("b", { class: "num " + (Number(r.change) < 0 ? "loss" : "profit") }, (Number(r.change) < 0 ? "−" : "+") + Math.abs(Number(r.change)).toFixed(1) + "%"))));
  return blockFrame(b, b.title, h("div", { class: "route-result" },
    h("div", { class: "route-movers" }, col(t("Up", "Рост"), m.up, true), col(t("Down", "Падение"), m.down, false)),
    h("div", { class: "tertiary small" }, t("Hyperliquid markets with over $1M traded today: crypto, stocks, commodities. Tap one for its chart.", "Рынки Hyperliquid с оборотом больше $1M за сегодня: крипта, акции, сырьё. Нажми на рынок — откроется график."))),
  { width: 470, icon: "flame", extra: refresh(b) });
}

/** Token approvals (ApprovalsBoard): who may spend what, unlimited ones in red; Revoke draws its card for Confirm, as the Mac's. */
function approvalsBlock(b, a) {
  const open = a.list.filter((x) => x.unlimited).length;
  const sub = `${a.list.length} ` + t("active", "активных") + (open ? ` · ${open} ` + t("unlimited", "без лимита") : "") + t(" · ihatecrypto's own are exact and spent at once", " · свои ihatecrypto даёт точно и тратит сразу");
  return blockFrame(b, t("Token approvals", "Разрешения на токены"), h("div", { class: "route-result" },
    h("span", { class: "secondary small" }, sub),
    a.list.length ? h("div", { class: "route-result-rows" }, a.list.map((x) => h("div", { class: "route-approval" + (x.unlimited ? " route-danger" : ""),
      title: t(`${x.spender} may spend ${x.unlimited ? "all of" : "up to this much"} ${x.symbol} (${x.token}) of yours on ${x.place}`, `${x.spender} может тратить ${x.unlimited ? "все" : "до этой суммы"} ${x.symbol} (${x.token}) в ${x.place}`) },
      logo(logoKey(x.chain) || x.chain, 22),
      h("div", { class: "route-approval-text" }, h("b", { class: "one-line" }, `${x.symbol} → ${x.spenderName || x.spender.slice(0, 6) + "…" + x.spender.slice(-4)}`),
        h("span", { class: "small one-line " + (x.unlimited ? "loss" : "secondary") }, `${x.place}${x.permit2 ? t(" · through Permit2", " · через Permit2") : ""} · ${x.amount}`)),
      h("span", { class: "spacer" }),
      h("button", { class: "btn small", disabled: !!ui.state.thinking, title: t(`Sets it to zero: one ${x.place} transaction, nothing spent`, `Обнуляет: одна транзакция в ${x.place}, ничего не тратится`), onclick: () => act("send", { text: x.revoke }) }, t("Revoke", "Отозвать")))))
      : h("div", { class: "secondary" }, t("No token approvals left: nothing can spend your tokens but you.", "Разрешений не осталось: никто, кроме тебя, не может тратить твои токены.")),
    a.unread.length ? h("div", { class: "secondary small" }, t(`Couldn't read ${a.unread.join(", ")} now — try again in a minute.`, `Не удалось прочитать ${a.unread.join(", ")} — попробуй через минуту.`)) : null),
  { width: 520, icon: "checkmark.shield", extra: h("button", { class: "icon-btn", title: t("Read them again", "Прочитать заново"), onclick: () => act("refreshApprovals", { board: b.id }) }, icon("arrow.clockwise")) });
}

/** Jupiter DCA (JupiterDCABoard): each order's progress; Cancel puts the cancel in the chat. */
function dcaBlock(b, rows) {
  return blockFrame(b, "Jupiter DCA", h("div", { class: "route-result" },
    h("span", { class: "route-status-chip profit route-dca-chip" }, rows.length === 1 ? t("1 running", "1 идёт") : t(`${rows.length} running`, `идёт: ${rows.length}`)),
    rows.map((o) => {
      const done = Number(o.deposited) > 0 ? Math.min(Math.max(Number(o.used) / Number(o.deposited), 0), 1) : 0;
      return h("div", { class: "route-dca" },
        h("div", { class: "route-dca-head" }, logo("usdc", 22), icon("arrow.right", 12), logo(o.coin, 22),
          h("b", { class: "one-line" }, t(`${o.perCycle} USDC → ${o.coin} every ${o.every}`, `${o.perCycle} USDC → ${o.coin} раз в ${o.every}`)), h("span", { class: "spacer" }),
          h("button", { class: "link-btn loss", disabled: !!ui.state.thinking, title: t("Its card comes for Confirm: what wasn't spent and the rent come back", "Придёт карточка на подтверждение: вернутся непотраченное и рента"), onclick: () => act("send", { text: o.cancel }) }, t("Cancel", "Отменить"))),
        h("div", { class: "route-progress" }, h("span", { style: `width:${(done * 100).toFixed(1)}%` })),
        h("div", { class: "route-dca-foot secondary small num" },
          h("span", {}, t(`spent ${o.used} of ${o.deposited} USDC · ${o.fills} ${o.fills === 1 ? "swap" : "swaps"}`, `потрачено ${o.used} из ${o.deposited} USDC · обменов: ${o.fills}`)),
          h("span", {}, t(`bought ${o.received} ${o.coin}`, `куплено ${o.received} ${o.coin}`))));
    }),
    h("div", { class: "secondary small" }, t("On-chain: Jupiter's keepers buy even with ihatecrypto closed. Cancel returns what wasn't spent and the rent.", "В сети: Jupiter покупает, даже когда ihatecrypto закрыт. Отмена вернёт непотраченное и ренту."))),
  { width: 520, extra: refresh(b) });
}

/** Coins from a network without a wallet here (ReceiveAcrossBoard): the address, its QR, the least it takes, until when,
 * what lands on Solana, and 1Click's word on it. */
function receiveBlock(b, d) {
  const until = d.until ? new Date(d.until).toLocaleTimeString(ui.state.russian ? "ru-RU" : "en-US", { hour: "numeric", minute: "2-digit" }) : null;
  return blockFrame(b, t(`Receive ${d.amount} ${d.token} from ${d.title}`, `Получить ${d.amount} ${d.token} из ${d.title}`), h("div", { class: "route-result" },
    h("span", { class: "secondary small" }, t(`through NEAR Intents · lands as ≈ ${d.arrives} USDC on Solana`, `через NEAR Intents · придёт ≈ ${d.arrives} USDC на Solana`)),
    h("div", { class: "route-receive" },
      h("div", { class: "route-receive-qr" }, qrCode(d.address, "route-qr")),
      h("div", { class: "route-receive-text" },
        h("span", { class: "secondary small" }, t(`Send ${d.token} on ${d.title} to`, `Отправь ${d.token} в сети ${d.title} на`)),
        h("div", { class: "route-receive-address selectable" }, d.address),
        h("button", { class: "btn small", onclick: () => copy(d.address) }, icon("copy", 13), t("Copy address", "Скопировать адрес")),
        h("div", { class: "secondary small" }, t(`At least ${d.minimum} ${d.token}`, `Не меньше ${d.minimum} ${d.token}`) + (until ? t(`, until ${until}`, `, до ${until}`) : "")
          + t(`. Only ${d.token} on ${d.title} — anything else sent here is lost.`, `. Только ${d.token} в сети ${d.title} — всё остальное, отправленное сюда, пропадёт.`)))),
    h("div", { class: "route-receive-state" }, icon("arrow.down.circle", 13), "1Click · " + d.state),
    h("div", { class: "secondary small" }, t("If it can't be swapped, 1Click returns it to your NEAR Intents account, under this wallet's NEAR key.", "Если обменять не выйдет, 1Click вернёт это на твой счёт NEAR Intents, под NEAR-ключ этого кошелька."))),
  { width: 520 });
}

/** A page in the panel beside the chat, as an explorer link opens. */
const openUrl = (url) => link(url, "").click();

/** A paid (or free) service's answer (ServiceAnswerBoard): who answered and what it cost, with the payment on Tempo, then
 * its writing, results as rows (pictures as pictures) or its values; Copy takes the whole answer. */
function serviceBlock(b, sv) {
  const a = sv.answer || {};
  const row = (item) => {
    const url = item.url, isImage = url && /\.(png|jpe?g|webp|gif)(\?|$)/i.test(url);
    let host = ""; try { host = new URL(url).host; } catch {}
    return h("div", { class: "route-service-item" + (url ? " route-link" : ""), onclick: url && !isImage ? () => openUrl(url) : null },
      isImage ? h("img", { class: "route-service-img", src: url, alt: "", referrerpolicy: "no-referrer", loading: "lazy", onerror: (e) => e.target.remove() }) : null,
      h("b", {}, item.title), host ? h("span", { class: "route-service-host" }, host) : null, item.detail ? h("span", { class: "secondary small" }, item.detail) : null);
  };
  const body = a.writing
    ? [h("div", { class: "route-service-writing selectable" }, a.writing), (a.links || []).slice(0, 6).map((l) => row({ title: (() => { try { const u = new URL(l); return u.host + u.pathname; } catch { return l; } })(), url: l }))]
    : (a.items || []).length ? (a.items || []).map(row)
    : (a.facts || []).length ? (a.facts || []).map((f) => h("div", { class: "route-service-fact" }, h("span", { class: "secondary" }, f.key), h("b", { class: "selectable" }, f.value)))
    : [h("div", { class: "selectable" }, a.text || t("The service sent nothing back.", "Сервис ничего не вернул."))];
  return blockFrame(b, sv.service, h("div", { class: "route-result" },
    h("span", { class: "secondary small" }, sv.usdc !== null && sv.usdc !== undefined ? t(`paid ${sv.usdc} USDC on Tempo`, `оплачено ${sv.usdc} USDC в Tempo`) : t("free answer", "бесплатный ответ")),
    h("div", { class: "route-service" }, body)),
  { width: 520, extra: h("span", { class: "route-pnl-tools" },
    sv.proof ? h("button", { class: "link-btn", title: t("The payment on Tempo's explorer", "Оплата в обозревателе Tempo"), onclick: () => openUrl(sv.proof) }, "Tempo", icon("arrow.up.right", 11)) : null,
    h("button", { class: "icon-btn", title: t("Copy the whole answer", "Скопировать весь ответ"), onclick: () => copy(sv.raw) }, icon("copy", 14))) });
}

/** A note from the chat (NoteBoard): a card per note, newest at the bottom, «Open» on the first when it's another canvas's. */
function noteBlock(b) {
  const head = (i) => h("div", { class: "route-block-head" }, h("span", { class: "route-block-icon" }, icon("doc", 15)),
    h("b", { class: "route-block-title one-line secondary" }, b.title ? t(`Note · ${b.title}`, `Заметка · ${b.title}`) : t("Note", "Заметка")),
    h("span", { class: "spacer" }),
    i === 0 && b.opens ? h("button", { class: "btn small", title: t("The note's own canvas, in its folder", "Холст заметки, в её папке"),
      disabled: !!ui.state.runningBoard, onclick: () => act("openCanvas", { wallet: b.opens }) }, t("Open", "Открыть")) : null,
    i === 0 ? h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => act("closeBoard", { board: b.id }) }, icon("xmark")) : null);
  return h("div", { class: "route-notes", oncontextmenu: (e) => blockMenu(e, b, b.title) },
    (b.lines || []).map((text, i) => h("div", { class: "route-block route-note-card" + (isAttached(b.id) ? " route-attached" : "") }, head(i), h("div", { class: "route-note-text selectable" }, text))));
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
