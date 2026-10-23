// The portfolio frame, as the Mac's (StudioPortfolio, PortfolioNetworks.swift): the total with the week's change and
// its line, the ring of where the money is, the next scheduled runs, then one card per network — Solana, Hyperliquid
// and Robinhood first, the others by money, those with $100 or less folded under «Other networks».
// A row opens the coin (its board or chart) through the chat field; nothing here moves money by itself.

import { ui, h, svg, act, icon, addIcons, t, logo, usd, signedUsd, pct, masked, suggest, popup, render } from "./core.js";

addIcons({
  bell: "M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0",
  bolt: "M13 3L5 13.5h6L10 21l8-10.5h-6z",
  hand: "M8 13V6.5a1.5 1.5 0 0 1 3 0V12M11 11V4.5a1.5 1.5 0 0 1 3 0V11M14 11V6a1.5 1.5 0 0 1 3 0v7c0 4-2.5 7.5-6.5 7.5-2.5 0-4-1.2-5.5-3.5L3.5 13a1.5 1.5 0 0 1 2.5-1.6L8 14",
  "clock.arrow.circlepath": "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4M12 8v4l3 2",
  "lock.square.stack": "M5 9h14v11H5zM7.5 6h9M9.5 3.5h5M9.5 13.5v-1a2.5 2.5 0 0 1 5 0v1M9 13.5h6v4H9z",
});

/** Each network's card fill, the line under its name, and its color on the ring (NetworkCard.style). Light, dark. */
const styles = {
  solana: ["#F4ECFF", "#2A2140", "#5B3AA8", "#C9B3FF", "#9945FF"],
  hyperliquid: ["#072723", "#072723", "#97FCE4", "#97FCE4", "#2FB39A"],
  robinhood: ["#F3FFD6", "#1D2608", "#5A7A00", "#A8CC3C", "#7FA600"],
  base: ["#EEF3FF", "#151C2E", "#0052FF", "#7EA2FF", "#0052FF"],
  arbitrum: ["#EAF5FF", "#12243A", "#1A6FB0", "#7CC8FF", "#28A0F0"],
  ethereum: ["#EEF0FB", "#1A1D33", "#4C5FD5", "#A5B4FC", "#627EEA"],
  polygon: ["#F3EDFF", "#221A36", "#6B32C9", "#C4A7FF", "#8247E5"],
  near: ["#E6FBF3", "#0E2A20", "#05785A", "#00EC97", "#00C08B"],
  tempo: ["#F1F2F5", "#22252C", "#5F6470", "#C5CAD3", "#8A8F98"],
  hyperevm: ["#E9F6F2", "#10261F", "#0E4D3F", "#8FE3CC", "#0E4D3F"],
  optimism: ["#FFEDEE", "#2E1416", "#C2020F", "#FF8A92", "#FF0420"],
  bnb: ["#FFF8E1", "#2B2410", "#8A6500", "#F3CD5A", "#F0B90B"],
};
const networkLogo = { solana: "sol", hyperliquid: "hype", base: "base", near: "near", arbitrum: "arb", robinhood: "robinhood", polygon: "pol",
  ethereum: "eth", tempo: "tempo", hyperevm: "hyperevm", optimism: "op", bnb: "bnb" };
const pinned = ["solana", "hyperliquid", "robinhood"];
const stables = new Set(["USDC", "USDT", "USDS", "USDE", "DAI", "USDG", "USDT0", "PATHUSD"]);

/** The card's colors as CSS variables, so the dark appearance picks its own. */
function tone(key) {
  const s = styles[key] || styles.tempo;
  return `--tint:${s[0]};--tint-dark:${s[1]};--ink:${s[2]};--ink-dark:${s[3]}`;
}
export const ringColor = (key) => (styles[key] || styles.tempo)[4];

/** Digits as dots while the balance is hidden (BalanceAmount without a mask). */
const amount = (text) => masked(text, String(text).replace(/\d/g, "•"));

// MARK: The frame

export function portfolioFrame() {
  const p = ui.state.portfolio;
  return h("div", { class: "portfolio" }, p.updatedAt ? loaded(p) : loading());
}

/** Until the first read is in: the card's shape, shimmering. */
function loading() {
  return h("div", { class: "pf-stack" },
    h("div", { class: "eyebrow pf-eyebrow" }, t("Portfolio", "Портфель")),
    h("div", { class: "pf-skeleton", style: "width:110px;height:22px" }),
    h("div", { class: "pf-skeleton", style: "height:30px;border-radius:8px" }),
    h("div", { class: "pf-ring" }, h("div", { class: "pf-skeleton", style: "width:58px;height:58px;border-radius:50%" }),
      h("div", { class: "pf-legend" }, [0, 1, 2, 3].map(() => h("div", { class: "pf-skeleton", style: "height:8px" })))),
    [0, 1, 2].map(() => h("div", { class: "pf-skeleton", style: "height:56px;border-radius:16px" })));
}

function loaded(p) {
  const series = (p.equity || []).map((e) => ({ at: Date.parse(e.at), usd: Number(e.usd) })).filter((e) => Number.isFinite(e.usd));
  const s = ui.state;
  const pnl = s.positions.length && s.positionsPnl !== null && s.positionsPnl !== undefined ? Number(s.positionsPnl) : null;
  let change = null;
  if (series.length > 2) {
    const first = series[0], last = series[series.length - 1];
    change = { usd: last.usd - first.usd, days: Math.max(1, Math.ceil((last.at - first.at) / 86_400_000)) };
  } else if (p.change7d !== null && p.change7d !== undefined) {
    change = { usd: Number(p.change7d), days: 7 };
  }
  return h("div", { class: "pf-stack" },
    h("div", { class: "pf-head" },
      h("div", {},
        h("div", { class: "eyebrow pf-eyebrow" }, t("Portfolio", "Портфель")),
        // Both wallets, as the rail and the Mac's BalanceAmount: what went to Automation is still the person's.
        h("div", { class: "pf-total num" }, amount(usd(Number(p.total) + Number(s.automationUSD || 0)))),
        change ? h("div", { class: "pf-change num " + (change.usd < 0 ? "loss" : "profit") },
          masked(signedUsd(change.usd)), t(` · ${change.days} d`, ` · ${change.days} дн.`)) : null),
      pnl !== null ? h("span", { class: "pf-chip " + (pnl < 0 ? "loss" : "profit") }, masked(signedUsd(pnl))) : null),
    series.length > 2 ? sparkline(series) : null,
    ring(p.networks),
    scheduleTop3(),
    networkCards(p),
    alertsCard(),
    p.errors.length ? h("div", { class: "pf-errors" }, t("Didn't answer: ", "Не ответили: ") + p.errors.join("; ")) : null);
}

/** The week's value as a line, green when it grew, red when it fell, over a fading area (EquitySparkline). */
function sparkline(points) {
  const w = 242, hgt = 30;
  const values = points.map((p) => p.usd);
  const lo = Math.min(...values), hi = Math.max(...values);
  const span = Math.max(hi - lo, Math.max(hi * 0.002, 0.01));
  const start = points[0].at, total = Math.max(points[points.length - 1].at - start, 1);
  const xy = points.map((p) => [(w * (p.at - start)) / total, hgt * (1 - (p.usd - lo) / span)]);
  const line = xy.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`).join("");
  const up = values[values.length - 1] >= values[0];
  const id = "spark-" + (up ? "up" : "down");
  return svg("svg", { class: "pf-spark " + (up ? "profit" : "loss"), viewBox: `0 0 ${w} ${hgt}`, preserveAspectRatio: "none", "aria-label": t("Portfolio value over the week", "Стоимость портфеля за неделю") },
    svg("defs", {}, svg("linearGradient", { id, x1: "0", y1: "0", x2: "0", y2: "1" },
      svg("stop", { offset: "0", "stop-color": "currentColor", "stop-opacity": "0.25" }),
      svg("stop", { offset: "1", "stop-color": "currentColor", "stop-opacity": "0" }))),
    svg("path", { d: `${line}L${w} ${hgt}L0 ${hgt}Z`, fill: `url(#${id})` }),
    svg("path", { d: line, fill: "none", stroke: "currentColor", "stroke-width": "1.6", "stroke-linecap": "round", "stroke-linejoin": "round", "vector-effect": "non-scaling-stroke" }));
}

/** Shares of the positive parts, largest first, to one decimal; the last takes the rounding (PortfolioSplit.slices). */
function slices(networks) {
  const parts = networks.filter((n) => Number(n.usd) > 0).sort((a, b) => Number(b.usd) - Number(a.usd));
  const total = parts.reduce((s, n) => s + Number(n.usd), 0);
  let used = 0;
  return parts.map((n, i) => {
    const share = i === parts.length - 1 ? 100 - used : Math.round((Number(n.usd) / total) * 1000) / 10;
    used += share;
    return { key: n.key, name: n.name, share };
  });
}

/** Where the money sits, as a ring in each network's card color, with the shares beside it (PortfolioRing). */
function ring(networks) {
  const parts = slices(networks);
  if (parts.length < 2) return null;
  const r = 23.5, len = 2 * Math.PI * r, gap = (len * 1.5) / 360;
  let at = 0;
  const g = svg("svg", { class: "pf-donut", viewBox: "0 0 58 58" });
  for (const s of parts) {
    const part = (s.share / 100) * len;
    g.append(svg("circle", { cx: "29", cy: "29", r: String(r), fill: "none", stroke: ringColor(s.key), "stroke-width": "11",
      "stroke-dasharray": `${Math.max(part - gap, 0.3)} ${len}`, "stroke-dashoffset": String(-at - gap / 2), transform: "rotate(-90 29 29)" }));
    at += part;
  }
  return h("div", { class: "pf-ring", title: t("Where the money is now", "Где сейчас деньги") }, g,
    h("div", { class: "pf-legend" }, parts.map((s) => h("div", { class: "pf-share" },
      h("span", { class: "pf-dot", style: `background:${ringColor(s.key)}` }), h("span", { class: "one-line" }, s.name),
      h("span", { class: "spacer" }), h("b", { class: "num" }, Math.round(s.share) + "%")))));
}

// MARK: The schedule's next three

/** «OCT» over «24»: the month on a strip, orange when it's today (DateTile). */
function dateTile(iso) {
  if (!iso) return h("div", { class: "pf-date-tile" }, h("div", { class: "pf-date-icon" }, icon("calendar", 14)));
  const d = new Date(iso), now = new Date();
  const today = d <= now || d.toDateString() === now.toDateString();
  return h("div", { class: "pf-date-tile" },
    h("div", { class: "pf-date-month" + (today ? " today" : "") }, d.toLocaleDateString(undefined, { month: "short" }).replace(".", "").toUpperCase()),
    h("div", { class: "pf-date-day num" }, String(d.getDate())));
}

/** «today 9:00 · in 19 h»: when it runs next, then how long until it within a day (NextLine). */
function nextLine(task) {
  if (task.paused) return t("paused", "на паузе");
  if (!task.next) return "";
  const d = new Date(task.next), now = new Date();
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  const day = d.toDateString() === now.toDateString() ? t("today", "сегодня")
    : d.toDateString() === new Date(now.getTime() + 86_400_000).toDateString() ? t("tomorrow", "завтра")
    : d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  const left = d - now;
  if (left <= 0) return `${day} ${time} · ` + t("any moment", "вот-вот");
  if (left < 86_400_000) {
    const hours = Math.floor(left / 3_600_000), mins = Math.round((left % 3_600_000) / 60_000);
    return `${day} ${time} · ` + (hours ? t(`in ${hours} h`, `через ${hours} ч`) : t(`in ${mins} min`, `через ${mins} мин`));
  }
  return `${day} ${time}`;
}

/** The next three runs on their own; a paused one isn't among them (ScheduleTop3). A row or «All» opens the Schedule. */
function scheduleTop3() {
  const tasks = ui.state.tasks;
  const running = tasks.filter((x) => !x.paused);
  const next = running.slice(0, 3);
  const paused = tasks.length - running.length;
  const openSchedule = () => act("addBoard", { name: "schedule" });
  const marks = [...new Set(next.map((x) => (x.automatic ? "auto" : "hello")))];
  return h("div", { class: "pf-schedule" },
    h("div", { class: "pf-row" },
      h("span", { class: "eyebrow pf-eyebrow" }, t("Schedule", "Расписание")),
      h("span", { class: "spacer" }),
      h("button", { class: "icon-btn pf-mini-btn", title: t("New schedule: puts a start in the chat", "Новое расписание: начало появится в чате"),
        onclick: () => suggest(t("every day at 9:00: buy SOL for 5 USDC", "каждый день в 9:00: купи SOL на 5 USDC")) }, icon("plus", 11)),
      h("button", { class: "icon-btn pf-all", onclick: openSchedule }, t("All", "Все"), icon("chevron.right", 10))),
    next.length ? null : h("div", { class: "pf-empty" }, paused ? t(`Nothing running · ${paused} paused`, `Ничего не идёт · на паузе: ${paused}`) : t("Nothing scheduled yet", "Пока ничего не запланировано")),
    next.map((x) => h("div", { class: "pf-task pf-lit", onclick: openSchedule },
      dateTile(x.next),
      h("div", { class: "pf-task-text" }, h("div", { class: "pf-task-title" }, x.text), h("div", { class: "pf-task-next one-line" }, nextLine(x))),
      h("span", { class: "pf-mark " + (x.automatic ? "auto" : "hello"), title: x.automatic ? t("Runs by itself", "Идёт сама") : t("Asks Windows Hello", "Спросит Windows Hello") },
        icon(x.automatic ? "bolt" : "hand", 14)))),
    marks.length ? h("div", { class: "pf-legend-marks" }, marks.map((m) => h("span", { class: "pf-mark " + m },
      icon(m === "auto" ? "bolt" : "hand", 11), m === "auto" ? t("runs by itself", "идёт сама") : t("asks Windows Hello", "спросит Windows Hello")))) : null);
}

// MARK: Network cards

let unfolded = false;
const tuckedOpen = new Set();

/** The cards in their order (PortfolioSplit.networkCards): the three pinned when they hold money, then the others by
 * money; one of the others with $100 or less folds under «Other networks». */
function networkCards(p) {
  const cent = 0.01;
  const byKey = new Map(p.networks.map((n) => [n.key, n]));
  const rest = p.networks.filter((n) => !pinned.includes(n.key) && Number(n.usd) >= cent)
    .sort((a, b) => Number(b.usd) - Number(a.usd) || a.key.localeCompare(b.key));
  const cards = [...pinned.map((k) => byKey.get(k)).filter((n) => n && (n.rows.length || Number(n.usd) >= cent)), ...rest.filter((n) => Number(n.usd) > 100)];
  const folded = rest.filter((n) => Number(n.usd) <= 100);
  const s = ui.state;
  if (!cards.length && !folded.length) {
    return h("div", { class: "pf-nothing" }, t("Nothing here yet. Add the Receive block to see where to send money.", "Пока пусто. Добавь блок «Получить», чтобы увидеть, куда отправить деньги."));
  }
  return h("div", { class: "pf-cards" },
    cards.map(networkCard),
    s.multisigs.map(multisigCard),
    folded.length ? [
      h("button", { class: "pf-other pf-lit", title: unfolded ? t("Fold them again", "Свернуть") : folded.map((n) => n.name).join(" · ") + t(": $100 or less each, open to see what's on them", ": до $100 в каждой, открой, чтобы увидеть"),
        onclick: () => { unfolded = !unfolded; render(); } },
        h("div", { class: "pf-other-name" }, h("div", {}, t("Other networks", "Другие сети")),
          h("div", { class: "pf-other-logos" }, folded.slice(0, 5).map((n) => logoRound(networkLogo[n.key] || n.key, 14)),
            folded.length > 5 ? h("span", { class: "secondary" }, "+" + (folded.length - 5)) : null)),
        h("span", { class: "spacer" }),
        h("b", { class: "num" }, amount(usd(folded.reduce((x, n) => x + Number(n.usd), 0)))),
        icon(unfolded ? "chevron.down" : "chevron.right", 11)),
      unfolded ? folded.map(networkCard) : null] : null,
    automationCard());
}

function logoRound(name, size, src) {
  const el = logo(name, size, src);
  el.classList.add("round");
  return el;
}

/** One network: its logo, name, what its money does, its total; then everything on it, coins first, deposits under
 * their own name. Hyperliquid's card is dark green in both appearances. */
function networkCard(n) {
  const defi = n.rows.filter((r) => r.section === "defi");
  // Hyperliquid's «Perps» row is its margin, which the note under the name already says (the Mac shows no row); it stays
  // in the network's total.
  const coins = n.rows.filter((r) => r.section !== "defi" && !(n.key === "hyperliquid" && r.symbol === "Perps")).sort((a, b) => Number(b.usd ?? 0) - Number(a.usd ?? 0));
  // Dust and coins without a price fold under «N hidden», as the Mac tucks what isn't worth a row.
  const shown = coins.filter((r) => r.usd !== null && r.usd !== undefined && Number(r.usd) >= 0.01);
  const tucked = coins.filter((r) => !shown.includes(r));
  const note = n.note ?? (n.key === "tempo" ? t("USDC for paid services", "USDC для платных сервисов") : n.rows.length ? null : t("nothing here yet", "пока пусто"));
  const open = tuckedOpen.has(n.key);
  return h("div", { class: "pf-net" + (n.key === "hyperliquid" ? " dark" : ""), style: tone(n.key) },
    h("div", { class: "pf-net-head" },
      logo(networkLogo[n.key] || n.key, 34),
      h("div", { class: "pf-net-name" }, h("div", { class: "pf-net-title" }, n.name), note ? h("div", { class: "pf-net-note" }, masked(note, note.replace(/\$[\d,.]+/g, "$•••"))) : null),
      h("span", { class: "spacer" }),
      h("b", { class: "pf-net-usd num" }, amount(usd(n.usd)))),
    n.key === "hyperliquid" ? positions() : null,
    shown.length && defi.length ? h("div", { class: "pf-section" }, t("Tokens", "Токены")) : null,
    shown.map((r) => coinRow(r, n)),
    tucked.length ? h("button", { class: "pf-tucked", title: t("Dust and coins without a price", "Пыль и монеты без цены"),
      onclick: () => { open ? tuckedOpen.delete(n.key) : tuckedOpen.add(n.key); render(); } },
      open ? t("Collapse", "Свернуть") : t(`${tucked.length} hidden · show`, `скрыто: ${tucked.length} · показать`)) : null,
    open ? tucked.map((r) => coinRow(r, n, true)) : null,
    defi.length ? [shown.length || tucked.length ? h("div", { class: "pf-divider" }) : null, h("div", { class: "pf-section" }, "DeFi"), defi.map((r) => depositRow(r, n))] : null);
}

/** The blue seal with a white check (VerifiedBadge). */
function seal() {
  return svg("svg", { viewBox: "0 0 24 24", width: "12", height: "12", "aria-label": t("verified", "проверен") },
    svg("path", { d: "M12 1.5l2.6 1.9 3.2-.1 1 3 2.6 1.9-1 3 1 3-2.6 1.9-1 3-3.2-.1L12 22.5l-2.6-1.9-3.2.1-1-3L2.6 15.8l1-3-1-3 2.6-1.9 1-3 3.2.1z", fill: "#1D9BF0" }),
    svg("path", { d: "M7.8 12.2l2.8 2.8 5.6-5.8", fill: "none", stroke: "#fff", "stroke-width": "2.4", "stroke-linecap": "round", "stroke-linejoin": "round" }));
}

/** «−8.7%» out of the engine's «−$1.74 (−8.7%)». */
function pnlPct(r) {
  const m = r.pnl && r.pnl.match(/\(([^)]*%)\)/);
  return m ? m[1].replace("-", "−") : null;
}

/** What a click on a coin opens: its board by contract, or the chart of a coin everyone knows by name. */
function opener(r, n) {
  return r.contract ? `show token ${r.contract}` : `chart ${r.symbol} day`;
}

function coinRow(r, n, dim) {
  const dollar = stables.has(r.symbol.toUpperCase());
  const p = dollar ? null : pnlPct(r);
  const change = dollar ? null : r.change24h;
  const verified = r.verified === true ? h("span", { class: "pf-verified", title: t("Verified by the token list: the real token, not a copy under its name", "Проверен по списку токенов: настоящий, а не копия с тем же именем") }, seal())
    : r.verified === false ? h("span", { class: "pf-unverified" }, t("unverified", "не проверен")) : null;
  return h("div", { class: "pf-coin pf-lit" + (dim ? " dim" : ""), title: `${r.symbol}: ` + t("details and chart · right-click to ask the chat, buy or sell", "подробности и график · правый клик — спросить чат, купить или продать"),
    onclick: () => suggest(opener(r, n)), oncontextmenu: (e) => coinMenu(e, r, n) },
    logoRound(r.symbol, 20, r.verified === false ? undefined : r.icon),
    h("div", { class: "pf-coin-name" },
      h("div", { class: "pf-coin-symbol" }, h("span", { class: "one-line" }, r.symbol), verified),
      h("div", { class: "pf-coin-amount num" }, h("span", { class: "secondary" }, amount(r.amount)),
        p ? h("span", { class: (r.gain ?? 0) < 0 || p.startsWith("−") ? "loss" : "profit" }, " " + p) : null)),
    h("span", { class: "spacer" }),
    h("div", { class: "pf-coin-value" },
      h("div", { class: "num" }, r.usd !== null && r.usd !== undefined ? amount(usd(r.usd)) : t("no price", "нет цены")),
      change !== null && change !== undefined ? h("div", { class: "pf-24h num" }, h("span", { class: Number(change) < 0 ? "loss" : "profit" }, pct(change)), h("span", { class: "secondary" }, " 24h")) : null));
}

/** A deposit: the protocol, what it holds and earns, the money in it. */
function depositRow(r, n) {
  const usdLike = /^[\d.,]+$/.test(r.amount) && Math.abs(Number(r.amount.replace(/,/g, "")) - Number(r.usd ?? 0)) < 0.02;
  return h("div", { class: "pf-coin pf-lit", onclick: () => suggest(t(`What's my ${r.symbol} deposit doing?`, `Что с моим депозитом в ${r.symbol}?`)), oncontextmenu: (e) => coinMenu(e, r, n) },
    logo(r.symbol, 20, r.icon),
    h("div", { class: "pf-coin-name" }, h("div", { class: "pf-coin-symbol" }, h("span", { class: "one-line" }, r.symbol)),
      r.note ? h("div", { class: "pf-coin-amount secondary one-line" }, r.note) : null),
    h("span", { class: "spacer" }),
    h("div", { class: "pf-coin-value" },
      usdLike ? h("div", { class: "pf-usdc num" }, logo("usdc", 12), amount(r.amount)) : h("div", { class: "num" }, amount(r.amount)),
      !usdLike && r.usd ? h("div", { class: "pf-24h secondary num" }, amount("≈ " + usd(r.usd))) : null));
}

/** Right-click on a coin: ask the chat about it, or put a buy or a sale into the field (TokenMenu). */
function coinMenu(e, r, n) {
  e.preventDefault();
  e.stopPropagation();
  const on = n.key === "solana" ? "" : n.key === "hyperliquid" ? " on hyperliquid" : ` on ${n.key}`;
  popup(e, [
    [t("Details and chart", "Подробности и график"), () => suggest(opener(r, n))],
    [t(`Ask the chat about ${r.symbol}`, `Спросить чат про ${r.symbol}`), () => suggest(t(`What about my ${r.symbol} on ${n.name}?`, `Что с моим ${r.symbol} в ${n.name}?`))],
    null,
    [t(`Buy ${r.symbol}…`, `Купить ${r.symbol}…`), () => suggest(`buy ${r.symbol}${on} for 10 USDC`)],
    [t(`Sell ${r.symbol}…`, `Продать ${r.symbol}…`), () => suggest(`sell all ${r.symbol}${on}`)],
  ]);
}

/** Hyperliquid's open positions on its card; a click puts the coin's chart into the chat. */
function positions() {
  return ui.state.positions.map((x) => {
    const loss = String(x.pnl).trim().startsWith("-") || String(x.pnl).includes("−");
    return h("div", { class: "pf-position pf-lit", title: t(`Open the ${x.coin} chart`, `Открыть график ${x.coin}`), onclick: () => suggest(`chart ${x.coin} day`) },
      h("div", { class: "pf-row" }, h("b", { class: "one-line" }, `${x.side === "long" ? t("Long", "Лонг") : t("Short", "Шорт")} ${x.coin}`), h("span", { class: "spacer" }),
        h("b", { class: "num " + (loss ? "pos-loss" : "pos-gain") }, masked(x.pnl))),
      h("div", { class: "pf-position-line" }, `${x.size} @ ${x.entry}` + (x.liquidation ? t(` · liq. ${x.liquidation}`, ` · ликв. ${x.liquidation}`) : "")));
  });
}

/** The automation wallet: what the Schedule's tasks spend from without Windows Hello. A click opens the Schedule. */
function automationCard() {
  const s = ui.state;
  if (!s.automation || s.automationUSD === null || s.automationUSD === undefined) return null;
  return h("button", { class: "pf-plain pf-lit", title: t("Your automation wallet: the Schedule's tasks spend from it without Windows Hello. Click to open the Schedule.",
    "Кошелёк автоматизации: задачи расписания тратят с него без Windows Hello. Нажми, чтобы открыть расписание."), onclick: () => act("addBoard", { name: "schedule" }) },
    h("span", { class: "pf-badge auto" }, icon("clock.arrow.circlepath", 16)),
    h("div", { class: "pf-net-name" }, h("div", { class: "pf-net-title" }, s.automation.label), h("div", { class: "pf-net-note secondary" }, t("your Schedule wallet", "кошелёк расписания"))),
    h("span", { class: "spacer" }),
    h("b", { class: "pf-net-usd num" }, amount(usd(s.automationUSD))));
}

function multisigCard(m) {
  return h("button", { class: "pf-plain pf-lit", onclick: () => act("addBoard", { name: "multisig" }) },
    h("span", { class: "pf-badge safe" }, icon("lock.square.stack", 16)),
    h("div", { class: "pf-net-name" }, h("div", { class: "pf-net-title" }, t("Multisig", "Мультисиг")),
      h("div", { class: "pf-net-note secondary" }, t(`Safe · any ${m.threshold} of ${m.keys.length} keys · `, `Safe · любые ${m.threshold} из ${m.keys.length} ключей · `) + m.networks.join(", "))));
}

/** Price alerts waiting, each with a way to drop it (AlertsCard). */
function alertsCard() {
  const alerts = ui.state.alerts;
  if (!alerts.length) return null;
  return h("div", { class: "pf-alerts" },
    h("div", { class: "pf-alerts-title" }, icon("bell", 12), t("Price alerts", "Ценовые оповещения")),
    alerts.map((a) => h("div", { class: "pf-row" }, h("span", { class: "pf-alert-text one-line" }, a.text), h("span", { class: "spacer" }),
      h("button", { class: "icon-btn pf-mini-btn", title: t("Remove the alert", "Убрать оповещение"), onclick: () => act("removeAlert", { wallet: a.id }) }, icon("xmark", 10)))));
}

