// A trade plan's own parts on its route board (TradePlan.swift, the Mac's plan view): the market picker in its header,
// Cancel and the one change that passes while the rules stop it, «What next», the three ways to take profit, and what
// Confirm does under the route. The route cards themselves are boards.js's.

import { ui, act, engine, h, svg, icon, addIcons, t, render, store, popup } from "./core.js";

// Its own stylesheet, so the page's head needs no change: a link to 'self' is what the CSP allows.
if (!document.querySelector('link[href="trade.css"]')) document.head.append(h("link", { rel: "stylesheet", href: "trade.css" }));

addIcons({
  wand: "M4 20 15 9M14 4v3M12.5 5.5h3M19 9v3M17.5 10.5h3M18 3v2M17 4h2",
  sliders: "M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M10 15v4",
});

/** The head's status for a plan the rules stop (red), one waiting for a detail (orange), or a watched preview; null for
 * the card waiting for Confirm, which says «ready to sign» itself. */
export function planStatus(b) {
  const p = b.plan;
  if (!p || b.state !== "plan") return null;
  if (p.preview) return [t("preview · nothing signs", "просмотр · без подписи"), "secondary"];
  if (p.blocked) return [t(`stopped: ${p.short || "a rule"}`, `остановлен: ${p.short || "правило"}`), "loss"];
  if (p.short) return [p.short, "trade-look"];
  return null;
}

/** A plan's step says its own (the Mac's RouteCards.state): a rule stops it, it wants a look, or its check passed; the
 * rest are ready on the card waiting for Confirm, and plain while it's only a plan (nothing there is to sign yet).
 * null for a board that isn't a plan, or one running or done. */
export function planStepState(b, i, waitingHere) {
  if (!b.plan || !(waitingHere || b.state === "plan")) return null;
  return { blocked: "blocked", attention: "attention", watching: "watching", passed: "done" }[b.steps?.[i]?.status] || (waitingHere ? "ready" : "plain");
}

/** The chips a plan's steps add to the route's (boards.js CHIPS): Stopped, Look, Watching. */
export const PLAN_CHIPS = {
  blocked: () => [t("Stopped", "Нельзя"), "#B42A20", "#FFB3A8", "error"],
  attention: () => [t("Look", "Проверь"), "#8A4B00", "#FFB870", "tuning"],
  watching: () => [t("Watching", "Слежу"), "#8A4B00", "#FFD9A8", "waiting"],
};

/** After the title: the market picker, then — while it's only a plan — Cancel and the fix in Confirm's place. */
export function planHead(b) {
  const p = b.plan, s = ui.state;
  if (!p) return null;
  const busy = s.thinking || !!s.runningBoard;
  const editable = b.state === "plan" || s.card?.id === b.id;
  return [
    marketPicker(b, p, editable && !busy),
    b.state === "plan" ? h("div", { class: "btns route-nowrap" },
      h("button", { class: "btn route-large", disabled: busy, onclick: () => act("planCancel") }, t("Cancel", "Отмена")),
      // The change that passes every rule: a plan edit, never a move — the new card still waits for Confirm.
      p.fix ? h("button", { class: "btn primary route-large", disabled: busy, title: [p.stopped, p.fixDetail].filter(Boolean).join(" "),
        onclick: () => act("send", { text: p.fix.command }) }, icon("wand", 14), p.fix.title)
        : p.preview ? null
        : h("button", { class: "btn route-large trade-locked", disabled: true, title: p.stopped || "" }, icon("lock", 13), t("Confirm", "Подтвердить"))) : null,
  ];
}

/** Under the route: why it stops, «What next», the take-profit ways, what Confirm signs. */
export function planExtras(b) {
  const p = b.plan;
  if (!p) return null;
  return [
    b.state === "plan" && p.stopped ? h("div", { class: "trade-stopped" + (p.blocked ? " loss" : " trade-look"), "data-no-pan": "" },
      icon(p.blocked ? "xmark" : "exclamationmark.triangle", 13), h("span", {}, p.stopped)) : null,
    b.state === "plan" && p.blocked && (p.fix || p.overLimit) ? whatNext(p) : null,
    p.takes?.length ? takeChoices(b, p.takes) : null,
    p.footer ? h("div", { class: "route-note trade-footer", "data-no-pan": "" }, icon("signature", 15), h("div", {}, p.footer)) : null,
  ];
}

/** A plan the rules stop, its ways on (the Mac's NextStepsBoard): what the code checked fits, recommended, and raising the
 * limit when the limit is what stops it. The model's ideas are in the chat, under its reply. */
function whatNext(p) {
  const limit = ui.state.trading?.policy?.maxRiskPct;
  const busy = ui.state.thinking || !!ui.state.runningBoard;
  return h("div", { class: "trade-next", "data-no-pan": "" },
    h("div", { class: "trade-caption" }, t("WHAT NEXT", "ЧТО ДАЛЬШЕ")),
    p.fix ? option(p.fix.title, p.fixDetail, "wand", true, busy, () => act("send", { text: p.fix.command }), p.fix.command) : null,
    p.overLimit ? option(t("Raise the limit", "Поднять лимит"),
      t(`Now at most ${limit ?? "—"}% per trade, in Settings — at your own risk.`, `Сейчас не больше ${limit ?? "—"}% на сделку, в настройках — на свой риск.`),
      "sliders", false, false, () => { store("settingsTab", "agent"); ui.view = "settings"; render(); }, t("Settings › Trading agent", "Настройки › Торговый агент")) : null,
    h("div", { class: "trade-hint" }, t("Or write your own in the chat.", "Или напиши свой вариант в чат.")));
}

function option(title, comment, glyph, recommended, disabled, onclick, hint) {
  return h("button", { class: "trade-option" + (recommended ? " trade-rec" : ""), disabled, title: hint, onclick },
    h("span", { class: "trade-option-icon" }, icon(glyph, 14)),
    h("span", { class: "trade-option-text" },
      h("span", { class: "trade-option-title" }, title, recommended ? h("span", { class: "trade-rec-chip" }, t("Recommended", "Рекомендую")) : null),
      comment ? h("span", { class: "trade-option-comment" }, comment) : null));
}

// MARK: Take profit

/** «Take profit like the pros» (the Mac's TakeChoices): the three ways with what each pays; a click puts it in the plan. */
function takeChoices(b, takes) {
  const busy = ui.state.thinking || !!ui.state.runningBoard;
  return h("div", { class: "trade-takes", "data-no-pan": "" },
    h("div", { class: "trade-caption" }, t("PICK HOW TO TAKE PROFIT", "КАК ВЗЯТЬ ПРИБЫЛЬ")),
    takes.map((c) => h("button", { class: "trade-take" + (c.recommended ? " trade-rec" : ""), disabled: busy, onclick: () => act("planTake", { name: c.id }) },
      h("span", { class: "trade-sketch" }, sketch(c.id)),
      h("span", { class: "trade-take-text" },
        h("span", { class: "trade-take-title" }, c.title, c.recommended ? h("span", { class: "trade-rec-chip" }, t("Recommended", "Рекомендую")) : null),
        h("span", { class: "trade-take-line" }, c.detail),
        h("span", { class: "trade-take-line" }, c.note)),
      h("span", { class: "trade-payoff" }, c.payoff))),
    h("div", { class: "trade-hint" }, t("The stop is the same in all three; only how the profit is taken changes.",
      "Стоп во всех трёх один и тот же; меняется только то, как берётся прибыль.")));
}

/** A tiny price path with the take lines of a way (the Mac's TakeSketch): the stop under it, the takes over it. */
function sketch(style) {
  const w = 84, hgt = 52, y = (v) => (hgt * v).toFixed(1), x = (v) => (w * v).toFixed(1);
  const pts = [[0, 0.72], [0.18, 0.66], [0.3, 0.72], [0.46, 0.52], [0.62, 0.45], [0.8, 0.28], [1, style === "trend" ? 0.08 : 0.2]];
  const level = (v, cls) => svg("path", { d: `M0 ${y(v)}H${w}`, class: "trade-level " + cls });
  const g = svg("svg", { viewBox: `0 0 ${w} ${hgt}`, width: w, height: hgt });
  g.append(level(0.9, "loss"));
  if (style === "classic") g.append(level(0.2, "profit"));
  if (style === "partial") g.append(level(0.52, "profit trade-half"), level(0.2, "profit"));
  if (style === "trend") {
    g.append(level(0.52, "profit trade-half"),
      svg("path", { d: `M${x(0.46)} ${y(0.8)}H${x(0.62)}V${y(0.62)}H${x(0.8)}V${y(0.45)}H${w}`, class: "trade-level trade-trail" }));
  }
  g.append(svg("path", { d: "M" + pts.map(([a, b]) => `${x(a)} ${y(b)}`).join("L"), class: "trade-price" }));
  return g;
}

// MARK: The market

/** Hyperliquid's markets by group, read once («planMarkets» changes nothing, so no new state follows it). */
let markets = null, asking = false;
function loadMarkets() {
  if (markets || asking) return;
  asking = true;
  engine("planMarkets").then((r) => { markets = JSON.parse(r?.json || "{}"); }).catch(() => {}).finally(() => { asking = false; });
}

const GROUPS = { Crypto: ["Crypto", "Крипта"], Commodities: ["Commodities", "Сырьё"], Stocks: ["Stocks", "Акции"], "Indices & FX": ["Indices & FX", "Индексы и валюты"] };
const plain = (name) => name.replace(/^[a-z0-9]+:/, "");

/** «ETH ▾» in the plan's header (the Mac's MarketPicker): Hyperliquid's groups, then their markets; the same plan on the
 * one picked, checked again. A menu over the page, so a new state redrawing the board doesn't close it. */
function marketPicker(b, p, enabled) {
  loadMarkets();
  return h("button", { class: "trade-market", disabled: !enabled, title: t("Market: Hyperliquid — crypto, commodities, stocks, indices", "Рынок: Hyperliquid — крипта, сырьё, акции, индексы"),
    onclick: (e) => { e.stopPropagation(); groupsMenu(e, p.market); } },
    plain(p.market), h("span", { class: "trade-market-place" }, "Hyperliquid"), icon("chevron.down", 10));
}

function groupsMenu(e, current) {
  const list = markets || {};
  const groups = Object.keys(GROUPS).filter((g) => list[g]?.length);
  if (!groups.length) { loadMarkets(); popup(e, [[t("Markets are loading…", "Рынки загружаются…"), () => {}]], "trade-menu"); return; }
  popup(e, groups.map((g) => {
    const names = list[g];
    const dex = names.find((n) => n.includes(":"))?.split(":")[0];
    const [en, ru] = GROUPS[g];
    const label = dex ? t(`${en} — ${dex} (HIP-3)`, `${ru} — ${dex} (HIP-3)`) : t(`${en} — main exchange`, `${ru} — основная биржа`);
    return [[h("span", { class: "trade-menu-name" }, label), names.includes(current) ? icon("check", 12) : null, icon("chevron.right", 10)], () => namesMenu(e, names, current)];
  }), "trade-menu");
}

function namesMenu(e, names, current) {
  popup(e, names.map((n) => [[h("span", { class: "trade-menu-name" }, plain(n)), n === current ? icon("check", 12) : null],
    () => { if (n !== current) act("planMarket", { name: n }); }]), "trade-menu trade-menu-long");
}
