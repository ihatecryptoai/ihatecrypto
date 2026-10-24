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

// MARK: The templates menu's preview

addIcons({ "arrow.down": "M12 5v14M6 13l6 6 6-6" });

/** What each template's scheme looks like (the Mac's TemplatesPanel.steps, S15): label and line, English and Russian. */
const TEMPLATE_STEPS = {
  "Trade by the book": [["From", "Откуда", "USDC on Solana", "USDC в Solana"], ["Bridge", "Мост", "Relay or 1Click — the cheaper", "Relay или 1Click — что дешевле"],
    ["Position", "Позиция", "long or short, up to 5×", "лонг или шорт, до 5×"], ["Stop and take", "Стоп и тейк", "stop 1.5 ATR · take in parts", "стоп 1,5 ATR · тейк частями"],
    ["Rules", "Правила", "risk 1% of capital", "риск 1% капитала"], ["Second check", "Вторая проверка", "checks the plan", "проверяет план"]],
  "Short on a pullback": [["Entry", "Вход", "limit 2% above the market", "лимит на 2% выше рынка"], ["Position", "Позиция", "short oil · xyz exchange", "шорт нефти · биржа xyz"],
    ["Stop and take", "Стоп и тейк", "by the book", "по правилам"], ["Guardian", "Страж", "breakeven after ½", "безубыток после ½"]],
  "Best rates": [["Free", "Свободные", "USDC on Solana", "USDC в Solana"], ["Split", "Деление", "best rates · ≤ 50% per protocol", "лучшие ставки · ≤ 50% в один протокол"],
    ["Deposit", "Депозит", "Jupiter Lend, Kamino", "Jupiter Lend, Kamino"], ["Result", "Итог", "a year’s yield in USDC", "доход за год в USDC"]],
  "Where to keep it": [["Rates", "Ставки", "Jupiter Lend, Kamino, HLP, JitoSOL", "Jupiter Lend, Kamino, HLP, JitoSOL"],
    ["Conditions", "Условия", "minimums, rent, lockups", "минимумы, рента, блокировки"], ["Advice", "Совет", "at most half in one place", "не больше половины в одном месте"]],
  "Every week": [["Buy", "Покупка", "SOL for 2 USDC via Jupiter", "SOL на 2 USDC через Jupiter"], ["Every", "Как часто", "week, while ihatecrypto runs", "раз в неделю, пока ihatecrypto работает"],
    ["Checks", "Проверки", "address, simulation, signature", "адрес, симуляция, подпись"], ["Warning", "Предупреждение", "when Solana runs short", "когда в Solana не хватает денег"]],
  "Back to Solana": [["From", "Откуда", "Hyperliquid", "Hyperliquid"], ["Bridge", "Мост", "Relay", "Relay"], ["To", "Куда", "your Solana address", "твой адрес в Solana"],
    ["Signature", "Подпись", "main key — the trading key can’t withdraw", "основной ключ — торговый не может выводить"]],
  "Protect a position": [["Position", "Позиция", "open on Hyperliquid", "открыта на Hyperliquid"], ["Stop", "Стоп", "to the entry — breakeven", "на точку входа — безубыток"],
    ["Result", "Итог", "worst case ≈ $0 and fees", "в худшем случае ≈ $0 и комиссии"]],
  "Stake SOL": [["From", "Откуда", "SOL on Solana", "SOL в Solana"], ["Swap", "Обмен", "SOL → JitoSOL", "SOL → JitoSOL"], ["Earns", "Доход", "the staking yield", "доход от стейкинга"],
    ["Exit", "Выход", "swap back any time", "обменять обратно в любой момент"]],
  "Aave on Base": [["From", "Откуда", "USDC on Solana", "USDC в Solana"], ["Bridge", "Мост", "Relay to Base + a little gas", "Relay в Base + немного газа"],
    ["Approve", "Разрешение", "exactly the amount", "ровно на сумму"], ["Supply", "Вклад", "Aave v3 · ≈ 4% a year", "Aave v3 · ≈ 4% годовых"]],
  "Price alert": [["Watch", "Слежу", "the mark price every 10–30 s", "цена каждые 10–30 с"], ["Fire", "Сигнал", "one notification at the level", "одно уведомление на уровне"],
    ["Then", "Потом", "the alert is done", "оповещение снимается"]],
  "Buy a stock token": [["From", "Откуда", "USDC on Solana", "USDC в Solana"], ["Bridge", "Мост", "Relay → USDG on Robinhood Chain", "Relay → USDG в Robinhood Chain"],
    ["Swap", "Обмен", "Uniswap · Robinhood's registered token", "Uniswap · токен из реестра Robinhood"], ["Hold", "Хранение", "SPCX, TSLA… in your wallet", "SPCX, TSLA… в твоём кошельке"]],
  "Profit and loss": [["Trading", "Торговля", "Hyperliquid fills and funding", "сделки и фандинг Hyperliquid"], ["Holdings", "Монеты", "every coin, cost from its buys", "каждая монета, цена по её покупкам"],
    ["Result", "Итог", "dollars made or lost this week", "сколько заработал или потерял за неделю"]],
  "What's moving": [["Markets", "Рынки", "Hyperliquid's coins and stocks", "монеты и акции Hyperliquid"], ["Sorted", "Порядок", "by today's change", "по изменению за сегодня"],
    ["Then", "Потом", "pick one to chart or trade", "выбери, чтобы открыть график или торговать"]],
  "Chart a coin": [["Price", "Цена", "a week of SOL", "неделя SOL"], ["Facts", "Факты", "high, low, change", "максимум, минимум, изменение"],
    ["Then", "Потом", "trade it or set an alert", "торговать или поставить оповещение"]],
  "Buy a memecoin": [["From", "Откуда", "USDC on Solana", "USDC в Solana"], ["Swap", "Обмен", "Jupiter · a verified token", "Jupiter · проверенный токен"],
    ["Hold", "Хранение", "BONK in your wallet", "BONK в твоём кошельке"]],
  "Swap on Base": [["From", "Откуда", "USDC on Base", "USDC в Base"], ["Swap", "Обмен", "Uniswap · the best of v2, v3, v4", "Uniswap · лучший из v2, v3, v4"],
    ["Get", "Получишь", "ETH itself, not WETH", "сам ETH, не WETH"]],
  "Move to Base": [["From", "Откуда", "USDC on Solana", "USDC в Solana"], ["Bridge", "Мост", "Relay · a few seconds", "Relay · несколько секунд"],
    ["To", "Куда", "your own address on Base", "твой адрес в Base"]],
  "Sweep dust": [["Find", "Поиск", "tokens under $5 on Base, Arbitrum, Polygon", "токены дешевле $5 в Base, Arbitrum, Polygon"],
    ["Sell", "Продажа", "each for USDC on its own network", "каждый за USDC в его сети"], ["Result", "Итог", "USDC instead of crumbs", "USDC вместо крошек"]],
  "Claim rewards": [["Check", "Проверка", "Merkl on Base, Arbitrum, Ethereum, Polygon", "Merkl в Base, Arbitrum, Ethereum, Polygon"],
    ["Claim", "Получение", "Morpho's, Aave's and others' rewards", "награды Morpho, Aave и других"], ["Keep", "Куда", "they land in your wallet", "приходят в твой кошелёк"]],
  "Hyperliquid spot": [["Pay", "Оплата", "10 USDC — the book's minimum", "10 USDC — минимум рынка"], ["Buy", "Покупка", "HYPE on Hyperliquid's spot book", "HYPE на спотовом рынке Hyperliquid"],
    ["Hold", "Хранение", "the coin itself, no leverage", "сама монета, без плеча"]],
  "Trade gold": [["Market", "Рынок", "gold · Hyperliquid's xyz exchange", "золото · биржа xyz на Hyperliquid"], ["Position", "Позиция", "long, up to 5×", "лонг, до 5×"],
    ["Stop and take", "Стоп и тейк", "by the book", "по правилам"]],
  "Make a multisig": [["Keys", "Ключи", "this PC + a Ledger, a phone or a partner", "этот ПК + Ledger, телефон или партнёр"],
    ["Rule", "Правило", "a majority must sign each move", "каждое движение подписывает большинство"], ["Network", "Сеть", "Base unless you name others", "Base, если не назовёшь другие"]],
};

/** The scheme a template draws, beside the templates menu: each step a coloured bar, its label and line. */
export function templatePreview(en, ru, colors) {
  const steps = TEMPLATE_STEPS[en] || [];
  return h("div", { class: "plan-tpl-preview" },
    h("span", { class: "plan-tpl-head" }, t(`Preview · ${en}`, `Схема · ${ru}`)),
    h("span", { class: "plan-tpl-sub" }, t("this is how the scheme will look", "так она будет выглядеть")),
    steps.map(([le, lr, ve, vr], i) => [
      i ? h("span", { class: "plan-tpl-arrow" }, icon("arrow.down", 10)) : null,
      h("div", { class: "plan-tpl-step" }, h("span", { class: "plan-tpl-bar", style: `background:${colors[i % colors.length]}` }),
        h("span", { class: "plan-tpl-text" }, h("span", { class: "plan-tpl-label" }, t(le, lr)), h("b", {}, t(ve, vr))))]),
    h("span", { class: "plan-tpl-foot" }, t("Say just the asset and direction — ihatecrypto fills the rest from the rules.",
      "Назови только актив и направление — остальное ihatecrypto заполнит по правилам.")));
}
