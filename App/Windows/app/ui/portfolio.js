// The portfolio frame, as the Mac's (StudioPortfolio, PortfolioNetworks.swift): the total with the week's change and
// its line, the ring of where the money is, the next scheduled runs, then one card per network — Solana, Hyperliquid
// and Robinhood first, the others by money, those with $100 or less folded under «Other networks».
// A row opens the coin (its board or chart) through the chat field; nothing here moves money by itself.

import { ui, h, svg, act, engine, icon, addIcons, t, logo, usd, signedUsd, pct, masked, suggest, popup, render, when, copyButton, link } from "./core.js";
import { coinTrades, tradePhrases } from "./boards.js";

addIcons({
  bell: "M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0",
  bolt: "M13 3L5 13.5h6L10 21l8-10.5h-6z",
  hand: "M8 13V6.5a1.5 1.5 0 0 1 3 0V12M11 11V4.5a1.5 1.5 0 0 1 3 0V11M14 11V6a1.5 1.5 0 0 1 3 0v7c0 4-2.5 7.5-6.5 7.5-2.5 0-4-1.2-5.5-3.5L3.5 13a1.5 1.5 0 0 1 2.5-1.6L8 14",
  "clock.arrow.circlepath": "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4M12 8v4l3 2",
  "lock.square.stack": "M5 9h14v11H5zM7.5 6h9M9.5 3.5h5M9.5 13.5v-1a2.5 2.5 0 0 1 5 0v1M9 13.5h6v4H9z",
  "arrow.triangle.swap": "M4 8h13l-3.5-3.5M20 16H7l3.5 3.5",
  "arrow.up.right.square": "M5 5h6M5 5v14h14v-6M13 4h7v7M20 4l-9 9",
  shield: "M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6zM12 3v18",
  key: "M14.5 9.5a4 4 0 1 1-3.5 4M11 13.5l-7 7M6 18.5l2 2M8.5 16l2 2",
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
  if (!cards.length && !folded.length && !(s.inTransit || []).length) {
    return h("div", { class: "pf-nothing" }, t("Nothing here yet. Add the Receive block to see where to send money.", "Пока пусто. Добавь блок «Получить», чтобы увидеть, куда отправить деньги."));
  }
  return h("div", { class: "pf-cards" },
    transitCard(),
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
  const defi = n.rows.filter((r) => r.section === "defi" && !(n.key === "hyperliquid" && r.symbol === "HLP"));
  // Hyperliquid's «Perps» row is its margin, which the note under the name already says (the Mac shows no row); it stays
  // in the network's total.
  const byUsd = (a, b) => Number(b.usd ?? 0) - Number(a.usd ?? 0);
  const shown = n.rows.filter((r) => r.section !== "defi" && !(n.key === "hyperliquid" && r.symbol === "Perps")).sort(byUsd);
  // What the person hid, and airdrops that look like scams (the engine tucks them, HiddenTokens): faded, each with its eye.
  const tucked = (n.hidden || []).filter((r) => r.section !== "defi").sort(byUsd);
  const note = n.note ?? (n.key === "tempo" ? t("USDC for paid services", "USDC для платных сервисов") : n.rows.length || tucked.length ? null : t("nothing here yet", "пока пусто"));
  const open = tuckedOpen.has(n.key);
  const opens = OPENS.has(n.key);
  return h("div", { class: "pf-net" + (n.key === "hyperliquid" ? " dark" : "") + (opens ? " pf-opens" : "") + (detail === n.key ? " open" : ""), style: tone(n.key),
    title: opens ? t("Details", "Подробности") : null,
    // A row opens its coin; the card around it opens the network's details (the Mac's Openable).
    onclick: opens ? (e) => { if (!e.target.closest(".pf-coin, .pf-position, .pf-tucked, button, a")) toggleDetail(n.key); } : null },
    h("div", { class: "pf-net-head" },
      logo(networkLogo[n.key] || n.key, 34),
      h("div", { class: "pf-net-name" }, h("div", { class: "pf-net-title" }, n.name), note ? h("div", { class: "pf-net-note" }, masked(note, note.replace(/\$[\d,.]+/g, "$•••"))) : null),
      h("span", { class: "spacer" }),
      h("b", { class: "pf-net-usd num" }, amount(usd(n.usd)))),
    n.key === "hyperliquid" ? positions() : null,
    shown.length && defi.length ? h("div", { class: "pf-section" }, t("Tokens", "Токены")) : null,
    shown.map((r) => coinRow(r, n)),
    tucked.length ? h("button", { class: "pf-tucked", title: t("Tokens you hid, and airdrops that look like scams: unverified, with no real pool",
      "Токены, которые ты скрыл, и раздачи, похожие на мошенничество: не проверены, без настоящего пула"),
      onclick: () => { open ? tuckedOpen.delete(n.key) : tuckedOpen.add(n.key); render(); } },
      open ? t("Collapse", "Свернуть") : t(`${tucked.length + (n.hiddenMore || 0)} hidden · show`, `скрыто: ${tucked.length + (n.hiddenMore || 0)} · показать`)) : null,
    open ? tucked.map((r) => coinRow(r, n, true)) : null,
    open && n.hiddenMore ? h("div", { class: "pf-d-note" }, t(`and ${n.hiddenMore} more airdrops with no price`, `и ещё ${n.hiddenMore} раздач без цены`)) : null,
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

/** How a held coin opens and trades (the Mac's opener and TokenMenu): a token by its contract, and the network's own
 * dollar, open their card; a network's own coin or a Hyperliquid spot token, its market's chart; other dollars, nothing. */
function coinKind(r, n) {
  if (r.section === "defi") return null;
  if (stables.has(r.symbol.toUpperCase())) return !r.contract && n.key !== "hyperliquid" && (r.symbol === "USDC" || r.symbol === "USDG") ? "dollar" : r.contract ? "token" : null;
  return n.key === "hyperliquid" || !r.contract || r.contract === "native" || (n.key === "solana" && r.symbol === "SOL") ? "coin" : "token";
}

/** Opens the coin's card or chart on the canvas; resolves to the engine's answer (`board`: the block's id). */
function openCoin(r, n) {
  const kind = coinKind(r, n);
  return kind === "coin" ? act("openChart", { name: r.symbol })
    : kind ? act("openToken", { address: kind === "token" ? r.contract : "", network: n.key }) : Promise.resolve(null);
}

/** «Ask in chat»: the coin's card (or chart) opens and goes into the chat with the next message. */
async function askCoin(r, n) {
  const a = await openCoin(r, n);
  if (!a?.board) return suggest(t(`What about my ${r.symbol} on ${n.name}?`, `Что с моим ${r.symbol} в ${n.name}?`));
  ui.attached = a.board;
  render();
  document.getElementById("composer")?.focus();
}

function coinRow(r, n, dim) {
  const dollar = stables.has(r.symbol.toUpperCase());
  const p = dollar ? null : pnlPct(r);
  const change = dollar ? null : r.change24h;
  const verified = r.verified === true ? h("span", { class: "pf-verified", title: t("Verified by the token list: the real token, not a copy under its name", "Проверен по списку токенов: настоящий, а не копия с тем же именем") }, seal())
    : r.verified === false ? h("span", { class: "pf-unverified" }, t("unverified", "не проверен")) : null;
  return h("div", { class: "pf-coin pf-lit" + (dim ? " dim" : ""), title: `${r.symbol}: ` + t("details and chart · right-click to ask the chat, buy or sell", "подробности и график · правый клик — спросить чат, купить или продать"),
    onclick: () => openCoin(r, n), oncontextmenu: (e) => coinMenu(e, r, n) },
    logoRound(r.symbol, 20, r.verified === false ? undefined : r.icon),
    h("div", { class: "pf-coin-name" },
      h("div", { class: "pf-coin-symbol" }, h("span", { class: "one-line" }, r.symbol), verified),
      h("div", { class: "pf-coin-amount num" }, h("span", { class: "secondary" }, amount(r.amount)),
        p ? h("span", { class: (r.gain ?? 0) < 0 || p.startsWith("−") ? "loss" : "profit" }, " " + p) : null)),
    h("span", { class: "spacer" }),
    r.id ? h("button", { class: "icon-btn pf-eye" + (dim ? " on" : ""),
      title: dim ? t(`Show ${r.symbol} in the portfolio`, `Показывать ${r.symbol} в портфеле`) : t(`Hide ${r.symbol}: it leaves the portfolio and its total`, `Скрыть ${r.symbol}: он уйдёт из портфеля и его суммы`),
      onclick: (e) => { e.stopPropagation(); act("showToken", { address: r.id, test: !!dim }); } }, icon(dim ? "eye" : "eye.slash", 12)) : null,
    h("div", { class: "pf-coin-value" },
      h("div", { class: "num" }, r.usd !== null && r.usd !== undefined ? amount(usd(r.usd)) : t("no price", "нет цены")),
      change !== null && change !== undefined ? h("div", { class: "pf-24h num" }, h("span", { class: Number(change) < 0 ? "loss" : "profit" }, pct(change)), h("span", { class: "secondary" }, " 24h")) : null));
}

/** A deposit: the protocol, what it holds and earns, the money in it. */
function depositRow(r, n) {
  const usdLike = /^[\d.,]+$/.test(r.amount) && Math.abs(Number(r.amount.replace(/,/g, "")) - Number(r.usd ?? 0)) < 0.02;
  const place = depositPlace(r.symbol);
  return h("div", { class: "pf-coin pf-lit", onclick: () => place ? toggleDetail(place) : suggest(t(`What's my ${r.symbol} deposit doing?`, `Что с моим депозитом в ${r.symbol}?`)), oncontextmenu: (e) => coinMenu(e, r, n) },
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
  const kind = coinKind(r, n);
  const phrases = kind === "coin" ? coinTrades(r.symbol, n.key)
    : kind ? tradePhrases(n.key, r.contract || "", r.symbol, { verified: r.verified === true, dollar: kind === "dollar" }) : [];
  popup(e, [
    ...(kind ? [[[icon(kind === "coin" ? "chart.xyaxis.line" : "info.circle", 14), kind === "coin" ? t("Chart", "График") : t("Details", "Подробности")], () => openCoin(r, n)]] : []),
    [[icon("bubble.left", 14), t("Ask in chat", "Спросить в чате")], () => askCoin(r, n)],
    ...(phrases.length ? [null] : []),
    ...phrases.map(([title, glyph, phrase]) => [[icon(glyph, 14), title], () => suggest(phrase)]),
    ...(r.id ? [null, (n.hidden || []).includes(r) ? [t(`Show ${r.symbol}`, `Показывать ${r.symbol}`), () => act("showToken", { address: r.id, test: true })]
      : [t(`Hide ${r.symbol}`, `Скрыть ${r.symbol}`), () => act("showToken", { address: r.id, test: false })]] : []),
  ]);
}

/** Hyperliquid's open positions on its card; a click opens the coin's chart. */
function positions() {
  return ui.state.positions.map((x) => {
    const loss = String(x.pnl).trim().startsWith("-") || String(x.pnl).includes("−");
    return h("div", { class: "pf-position pf-lit", title: t(`Open the ${x.coin} chart`, `Открыть график ${x.coin}`), onclick: () => act("openChart", { name: x.coin }) },
      h("div", { class: "pf-row" }, h("b", { class: "one-line" }, `${x.side === "long" ? t("Long", "Лонг") : t("Short", "Шорт")} ${x.coin}`), h("span", { class: "spacer" }),
        h("b", { class: "num " + (loss ? "pos-loss" : "pos-gain") }, masked(x.pnl))),
      h("div", { class: "pf-position-line" }, masked(x.summary || `${x.size} @ ${x.entry}`)),
      x.guardian ? h("div", { class: "pf-guardian" }, icon("shield", 11), h("span", {}, x.guardian)) : null);
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
  const held = ui.state.multisigHeld?.[m.address];
  const total = held && held.every((x) => x.usd !== null && x.usd !== undefined) ? held.reduce((a, x) => a + Number(x.usd), 0) : null;
  return h("button", { class: "pf-plain pf-lit", onclick: () => act("addBoard", { name: "multisig" }) },
    h("span", { class: "pf-badge safe" }, icon("lock.square.stack", 16)),
    h("div", { class: "pf-net-name" }, h("div", { class: "pf-net-title" }, t("Multisig", "Мультисиг")),
      h("div", { class: "pf-net-note secondary" }, t(`Safe · any ${m.threshold} of ${m.keys.length} keys · `, `Safe · любые ${m.threshold} из ${m.keys.length} ключей · `) + m.networks.join(", "))),
    h("span", { class: "spacer" }),
    h("b", { class: "pf-net-usd num" }, total === null ? "…" : amount(usd(total))));
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


// MARK: In transit

/** Rows opened by a click, with the bridge's word on each once it answers. */
const transitOpen = new Map();

function since(iso) {
  const minutes = Math.floor((Date.now() - Date.parse(iso)) / 60_000);
  return minutes < 1 ? t("for under a minute", "меньше минуты") : minutes < 60 ? t(`for ${minutes} min`, `${minutes} мин`)
    : t(`for ${Math.floor(minutes / 60)} h ${minutes % 60} min`, `${Math.floor(minutes / 60)} ч ${minutes % 60} мин`);
}

/** Money between networks: each bridge order signed and not landed yet, with how long it's been on its way (InTransit). */
function transitCard() {
  const list = ui.state.inTransit || [];
  if (!list.length) return null;
  return h("div", { class: "pf-transit" },
    h("div", { class: "pf-transit-title" }, icon("arrow.triangle.swap", 13), t("In transit", "В пути")),
    list.map((e) => {
      const open = transitOpen.has(e.id);
      const toggle = () => {
        if (open) { transitOpen.delete(e.id); render(); return; }
        transitOpen.set(e.id, null);
        render();
        engine("transitState", { name: e.chain, address: e.id }).then((r) => { transitOpen.set(e.id, r?.state ?? "—"); render(); })
          .catch(() => { transitOpen.set(e.id, t("didn't answer", "не ответил")); render(); });
      };
      return h("div", { class: "pf-transit-item" },
        h("button", { class: "pf-transit-row", onclick: toggle,
          title: t(`Signed ${when(e.at)}; it shows here until the bridge says it landed or went back. Click for details.`,
            `Подписано ${when(e.at)}; видно здесь, пока мост не скажет, что дошло или вернулось. Нажми, чтобы увидеть подробности.`) },
          logoDisc(e.logo, 18),
          h("div", { class: "pf-transit-text" },
            h("div", { class: open ? "" : "one-line" }, e.what),
            h("div", { class: "secondary" + (open ? "" : " one-line") },
              `${e.bridge} · ${e.receiving ? t("waiting for your deposit", "ждёт твоего депозита") : t("on its way", "в пути")} ${since(e.at)}`)),
          icon(open ? "chevron.up" : "chevron.down", 9)),
        open ? transitDetails(e, transitOpen.get(e.id)) : null);
    }));
}

function transitDetails(e, state) {
  const line = (label, value) => h("div", { class: "pf-transit-line" }, h("span", { class: "secondary" }, label), h("span", {}, value));
  return h("div", { class: "pf-transit-details" },
    line(t("State", "Состояние"), state ?? t("asking…", "спрашиваю…")),
    line(t("Signed", "Подписано"), when(e.at)),
    e.tx ? line(t("Transaction", "Транзакция"), `${e.tx.slice(0, 10)}…${e.tx.slice(-6)}`) : null,
    e.receiving ? [h("div", { class: "address" }, e.id), h("div", {}, copyButton(e.id, t("Copy address", "Скопировать адрес")))]
      : line(t("Order", "Заказ"), `${e.id.slice(0, 12)}…`),
    e.links.length ? h("div", { class: "pf-transit-links" }, e.links.map((l) => link(l.url, l.title))) : null);
}

function logoDisc(name, size) {
  const el = logo(name, size);
  el.classList.add("round");
  return el;
}

// MARK: A place's details

/** The networks whose card opens their details (NetworkCard.opens), and the deposits that do. */
const OPENS = new Set(["solana", "hyperliquid", "robinhood", "base", "arbitrum", "near"]);
const depositPlace = (symbol) => ({ "Jupiter Lend": "jupiter", "Kamino Lend": "kamino", Kamino: "kamino", Aave: "aave", Morpho: "morpho" })[symbol] || null;

/** The place open in its details board, above the plan (Workspace.detail): one at a time; a second click closes it. */
let detail = null;
export function toggleDetail(place) { detail = detail === place ? null : place; render(); }
/** Opens a place's details (the preview's `&call=portfolio.openDetail`). */
export function openDetail(place = "hyperliquid") { detail = place; render(); }

const places = {
  solana: { logo: "sol", title: () => t("Solana wallet", "Кошелёк Solana"), about: () => t("everything you hold on Solana; every move starts here", "всё, что лежит в Solana; каждый перевод начинается отсюда"),
    actions: () => [[t("Top up", "Пополнить"), "topup", "arrow.down.circle"], [t("Stake SOL", "Застейкать SOL"), "stake 0.01 SOL", "lock"], [t("Send to a saved address", "Отправить на сохранённый адрес"), "send 1 USDC to ", "arrow.up"]] },
  hyperliquid: { logo: "hype", title: () => t("Hyperliquid account", "Счёт Hyperliquid"), about: () => t("margin for perps, stocks and commodities", "маржа для фьючерсов, акций и сырья"),
    actions: () => [[t("Withdraw to Solana", "Вывести в Solana"), "withdraw 1 USDC to Solana", "arrow.up"], [t("Trade by the book", "Сделка по правилам"), "long ETH by the book", "chart.pie"], [t("What’s moving", "Что движется"), "what's moving?", "bolt"]] },
  robinhood: { logo: "robinhood", title: () => "Robinhood Chain", about: () => t("Robinhood Stock Tokens · economic exposure, not shares", "токены акций Robinhood · доля в цене, а не сами акции"),
    actions: () => [[t("Buy SpaceX", "Купить SpaceX"), "buy SpaceX stock for 2 USDC", "plus"], [t("Sell", "Продать"), "sell my SpaceX shares", "arrow.right"], [t("USDG to Solana", "USDG в Solana"), "move USDG from Robinhood to Solana", "arrow.up"]],
    note: () => t("Stock Tokens aren’t offered to US persons, or in Canada, the UK and Switzerland; they give economic exposure, not the shares.",
      "Токены акций не продаются жителям США, Канады, Великобритании и Швейцарии; они повторяют цену акции, но акцией не являются.") },
  near: { logo: "near", title: () => t("NEAR account", "Аккаунт NEAR"), about: () => t("native NEAR on the NEAR chain · bought through NEAR Intents", "NEAR в сети NEAR · куплен через NEAR Intents"),
    actions: () => [[t("Buy more", "Купить ещё"), "buy NEAR for 3 USDC", "plus"]],
    note: () => t("The account is yours from the same phrase (m/44'/397'/0').", "Аккаунт твой, из той же фразы (m/44'/397'/0').") },
  base: { logo: "base", title: () => "Base", about: () => t("USDC at your EVM address · free, not earning", "USDC на твоём EVM-адресе · свободны, не приносят дохода"),
    actions: () => [[t("Lend on Aave", "Дать в долг на Aave"), "lend 1 USDC on Aave on Base", "plus"], [t("Move to Solana", "Перевести в Solana"), "move USDC from Base to Solana", "arrow.up"], [t("Best rates", "Лучшие ставки"), "where to keep USDC", "chart.pie"]] },
  arbitrum: { logo: "arb", title: () => "Arbitrum", about: () => t("USDC at your EVM address · free, not earning", "USDC на твоём EVM-адресе · свободны, не приносят дохода"),
    actions: () => [[t("Lend on Aave", "Дать в долг на Aave"), "lend 1 USDC on Aave on Arbitrum", "plus"], [t("Move to Solana", "Перевести в Solana"), "move USDC from Arbitrum to Solana", "arrow.up"], [t("Best rates", "Лучшие ставки"), "where to keep USDC", "chart.pie"]] },
  jupiter: { logo: "jup", deposit: ["Jupiter Lend"], title: () => "Jupiter Lend", about: () => t("lending vaults on Solana · withdraw any time", "займы в Solana · вывести можно в любой момент"),
    actions: () => [[t("Deposit", "Положить"), "deposit 1 USDC into Jupiter", "plus"], [t("Withdraw", "Вывести"), "withdraw 1 USDC from Jupiter", "arrow.up"], [t("Best rates", "Лучшие ставки"), "where to keep USDC", "chart.pie"]] },
  kamino: { logo: "kamino", deposit: ["Kamino Lend", "Kamino"], title: () => "Kamino Lend", about: () => t("lending on Solana · withdraw any time", "займы в Solana · вывести можно в любой момент"),
    actions: () => [[t("Withdraw all", "Вывести всё"), "withdraw USDC from Kamino", "arrow.up"], [t("Best rates", "Лучшие ставки"), "where to keep USDC", "chart.pie"]] },
  aave: { logo: "aave", deposit: ["Aave"], title: () => "Aave v3", about: () => t("lending on Base · withdraw any time", "займы в Base · вывести можно в любой момент"),
    actions: () => [[t("Lend more", "Дать ещё"), "lend 1 USDC on Aave", "plus"], [t("Withdraw to Solana", "Вывести в Solana"), "withdraw USDC from Aave", "arrow.up"], [t("Best rates", "Лучшие ставки"), "where to keep USDC", "chart.pie"]] },
  morpho: { logo: "morpho", deposit: ["Morpho"], title: () => "Morpho", about: () => t("a curated vault · lending on Base · withdraw any time", "курируемое хранилище · займы в Base · вывести можно в любой момент"),
    actions: () => [[t("Deposit more", "Добавить"), "deposit 1 USDC into Morpho", "plus"], [t("Withdraw to Solana", "Вывести в Solana"), "withdraw USDC from Morpho", "arrow.up"], [t("Best rates", "Лучшие ставки"), "where to keep USDC", "chart.pie"]] },
};

/** Where the place's address opens in its explorer. */
function explorer(place) {
  const a = ui.state.addresses;
  const evm = a.evm;
  return { solana: a.solana && `https://solscan.io/account/${a.solana}`, hyperliquid: evm && `https://app.hyperliquid.xyz/explorer/address/${evm}`,
    robinhood: evm && `https://robinhoodchain.blockscout.com/address/${evm}`, base: evm && `https://basescan.org/address/${evm}`, arbitrum: evm && `https://arbiscan.io/address/${evm}`,
    near: a.near && `https://nearblocks.io/address/${a.near}`, jupiter: "https://jup.ag/lend/earn", kamino: "https://app.kamino.finance", aave: evm && `https://basescan.org/address/${evm}`,
    morpho: "https://app.morpho.org" }[place] || null;
}

/** The board a portfolio card opens, above the plan (PlaceDetailBoard): the address, everything there with what each
 * row opens, the explorer, and the usual next steps, each put into the chat field. */
export function placeDetail() {
  const place = detail && places[detail];
  if (!place) return null;
  const s = ui.state, nets = s.portfolio.networks;
  const net = nets.find((n) => n.key === detail);
  const address = { solana: s.addresses.solana, near: s.addresses.near, jupiter: null, kamino: null }[detail] ?? (["solana", "near", "jupiter", "kamino"].includes(detail) ? null : s.addresses.evm);
  const rows = [];
  if (net && detail !== "hyperliquid") {
    const byUsd = (a, b) => Number(b.usd ?? 0) - Number(a.usd ?? 0);
    for (const r of net.rows.filter((r) => r.section !== "defi").sort(byUsd)) rows.push(detailCoin(r, net));
    for (const r of net.rows.filter((r) => r.section === "defi")) rows.push(detailDeposit(r));
  }
  if (place.deposit) for (const n of nets) for (const r of n.rows.filter((r) => r.section === "defi" && place.deposit.includes(r.symbol))) rows.push(detailDeposit(r, n));
  if (detail === "hyperliquid") rows.push(...hyperliquidRows());
  const rate = { kamino: s.places?.rates?.kamino, jupiter: s.places?.rates?.jupiter, aave: s.places?.rates?.aave, morpho: s.places?.rates?.morpho }[detail];
  if (rate !== undefined && rate !== null) rows.push(h("div", { class: "pf-d-note" },
    detail === "morpho" ? t(`≈ ${pct(rate)} a year after the vault’s fee.`, `≈ ${pct(rate)} годовых после комиссии хранилища.`)
      : detail === "aave" ? t(`Supplied USDC earns ≈ ${pct(rate)} a year.`, `Выданные USDC приносят ≈ ${pct(rate)} годовых.`)
      : t(`USDC rate now ≈ ${pct(rate)} a year.`, `Ставка USDC сейчас ≈ ${pct(rate)} годовых.`)));
  if (!rows.length) rows.push(h("div", { class: "pf-d-note" }, t("Nothing here yet.", "Пока пусто.")));
  const url = explorer(detail);
  return h("div", { class: "pf-detail", "data-no-pan": "" },
    h("div", { class: "pf-d-head" }, logo(place.logo, 34),
      h("div", { class: "pf-d-title" }, h("b", {}, place.title()), h("span", { class: "secondary" }, place.about())),
      h("span", { class: "spacer" }),
      h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => toggleDetail(detail) }, icon("xmark", 14))),
    address ? h("div", { class: "pf-d-address", title: address }, h("span", { class: "address" }, `${address.slice(0, 6)} … ${address.slice(-4)}`), h("span", { class: "spacer" }), copyButton(address)) : null,
    rows,
    place.note ? h("div", { class: "pf-d-note" }, place.note()) : null,
    url ? h("div", { class: "pf-d-link" }, icon("arrow.up.right.square", 12), link(url, t("Open in the explorer", "Открыть в обозревателе"))) : null,
    h("div", { class: "pf-d-actions" }, place.actions().map(([label, phrase, glyph]) =>
      h("button", { class: "pf-d-action", title: phrase === "topup" ? t("Your addresses and ways to top up", "Твои адреса и способы пополнить") : t(`Puts “${phrase}” into the chat`, `Вставит в чат «${phrase}»`),
        onclick: () => { if (phrase === "topup") { ui.view = "topup"; render(); } else suggest(phrase); } }, icon(glyph, 13), label))));
}

/** Hyperliquid's account in full (PlaceDetailBoard): its value and what's free, each HIP-3 exchange, spot USDC, the HLP
 * vault and its lock, the positions with their stops, and whether orders go through the trading key. */
function hyperliquidRows() {
  const s = ui.state, hl = s.places?.hyperliquid, key = s.trading?.key;
  const fact = (title, value, sub) => h("div", { class: "pf-d-row" }, logoDisc("HYPE", 24),
    h("div", { class: "pf-d-name" }, h("b", { class: "one-line" }, title), sub ? h("span", { class: "secondary one-line" }, sub) : null),
    h("span", { class: "spacer" }), h("b", { class: "num" }, value));
  const out = [];
  if (hl) {
    out.push(fact(t("Account value", "Стоимость счёта"), amount(usd(hl.accountValue)), amount(t(`free to withdraw ${usd(hl.withdrawable)}`, `можно вывести ${usd(hl.withdrawable)}`))));
    for (const d of hl.dexes || []) out.push(fact(t(`${d.dex} exchange (stocks, commodities)`, `биржа ${d.dex} (акции, сырьё)`), amount(usd(d.accountValue)), amount(t(`free ${usd(d.withdrawable)}`, `свободно ${usd(d.withdrawable)}`))));
    if (Number(hl.spotUSDC) > 0) out.push(h("div", { class: "pf-d-row" }, logo("usdc", 24),
      h("div", { class: "pf-d-name" }, h("b", {}, t("Spot USDC", "USDC на споте")), h("span", { class: "secondary" }, t("separate from the perp margin", "отдельно от маржи фьючерсов"))),
      h("span", { class: "spacer" }), h("b", { class: "num" }, amount(String(hl.spotUSDC)))));
    if (Number(hl.hlpEquity) > 0) out.push(fact(t("HLP vault", "Хранилище HLP"), amount(usd(hl.hlpEquity)),
      hl.hlpLockedUntil ? t(`locked until ${when(hl.hlpLockedUntil)} · 4 days after a deposit`, `заблокировано до ${when(hl.hlpLockedUntil)} · 4 дня после депозита`)
        : t("earning · withdraw any time", "приносит доход · вывести можно в любой момент")));
  }
  for (const x of s.positions) out.push(h("button", { class: "pf-d-row pf-lit", title: t(`Open the ${x.coin} chart`, `Открыть график ${x.coin}`), onclick: () => act("openChart", { name: x.coin }) },
    logoDisc(x.coin, 24), h("div", { class: "pf-d-name" }, h("b", {}, `${x.side === "long" ? t("Long", "Лонг") : t("Short", "Шорт")} ${x.coin}`),
      h("span", { class: "secondary" }, masked(x.summary || `${x.size} @ ${x.entry}`)), x.guardian ? h("span", { class: "secondary" }, x.guardian) : null),
    h("span", { class: "spacer" }), h("b", { class: "num" }, masked(x.pnl)), icon("chevron.right", 10)));
  if (!s.positions.length) out.push(h("div", { class: "pf-d-note" }, t("No open positions.", "Открытых позиций нет.")));
  if (key?.possible) out.push(h("div", { class: "pf-d-note pf-d-key" }, icon(key.active ? "key" : "lock", 11),
    key.active ? (key.ledger ? t("Trading key on: orders skip the Ledger, withdrawals still need it", "Торговый ключ включён: ордера идут без Ledger, выводу он по-прежнему нужен")
      : t("Trading key on: orders skip Windows Hello, withdrawals still need it", "Торговый ключ включён: ордера идут без Windows Hello, выводу он по-прежнему нужен"))
      : key.ledger ? t("Trading key off: every order asks for the Ledger's buttons", "Торговый ключ выключен: каждый ордер ждёт кнопок Ledger")
      : t("Trading key paused: every order asks for Windows Hello", "Торговый ключ на паузе: каждый ордер спрашивает Windows Hello")));
  return out;
}

function detailCoin(r, n) {
  const price = r.usd !== null && r.usd !== undefined && Number(r.value) > 0 ? Number(r.usd) / Number(r.value) : null;
  return h("button", { class: "pf-d-row pf-lit", onclick: () => openCoin(r, n) },
    logoDisc(r.symbol, 24),
    h("div", { class: "pf-d-name" }, h("b", { class: "one-line" }, r.symbol + (r.verified === false ? t(" · unverified", " · не проверен") : "")),
      h("span", { class: "secondary num" }, amount(r.amount) + " · " + (price === null ? t("no price", "нет цены") : "$" + (price >= 1 ? price.toFixed(2) : price.toPrecision(3))))),
    h("span", { class: "spacer" }), h("b", { class: "num" }, r.usd !== null && r.usd !== undefined ? amount(usd(r.usd)) : "—"), icon("chevron.right", 10));
}

function detailDeposit(r, n) {
  const place = depositPlace(r.symbol);
  return h(place && place !== detail ? "button" : "div", { class: "pf-d-row" + (place && place !== detail ? " pf-lit" : ""), onclick: place && place !== detail ? () => toggleDetail(place) : null },
    logo(r.symbol, 24),
    h("div", { class: "pf-d-name" }, h("b", { class: "one-line" }, r.symbol + (n ? ` · ${n.name}` : "")), r.note ? h("span", { class: "secondary one-line" }, r.note) : null),
    h("span", { class: "spacer" }), h("b", { class: "num" }, amount(usd(r.usd ?? 0))), place && place !== detail ? icon("chevron.right", 10) : null);
}
