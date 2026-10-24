// The canvas, as the Mac's (StudioCanvas, CanvasViewport, CanvasHost): the portfolio and the blocks beside it on the
// dotted ground, scaled to fit left of the chat and then by the person's zoom; dragged or scrolled to move, Ctrl and
// the wheel (or a pinch) to zoom about the pointer; the zoom pill at the bottom left, «Add block» at the top right.

import { ui, h, act, icon, addIcons, t, store, render, suggest, logo, mascot } from "./core.js";
import { portfolioFrame, placeDetail } from "./portfolio.js";
import { planColumn } from "./boards.js";
import { chatPanel } from "./chat.js";
import { railEdge } from "./rail.js";

addIcons({
  "arrow.left.to.line": "M4 5v14M20 12H9M14 7l-5 5 5 5",
  "arrow.right.to.line": "M20 5v14M4 12h11M10 7l5 5-5 5",
  bell: "M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0",
  "chart.line": "M4 19h16M5 15l4-5 4 3 6-7",
  "lock.square.stack": "M5 9h14v11H5zM7.5 6h9M9.5 3.5h5M9.5 13.5v-1a2.5 2.5 0 0 1 5 0v1M9 13.5h6v4H9z",
  "person.crop.rectangle": "M3.5 5h17v14h-17zM9 12a2.2 2.2 0 1 0 0-4.4A2.2 2.2 0 0 0 9 12zM5.5 16.5a3.5 3.5 0 0 1 7 0M14.5 9.5h3.5M14.5 13h3.5",
  "cpu": "M7 7h10v10H7zM10 3.5V7M14 3.5V7M10 17v3.5M14 17v3.5M3.5 10H7M3.5 14H7M17 10h3.5M17 14h3.5",
});

/** Portfolio frame + gap + plan + leading padding: at 100 % it fits left of the chat (StudioCanvas.contentWidth). */
const CONTENT_WIDTH = 28 + 270 + 36 + 470 + 24;
/** Room the canvas keeps for the docked chat: its default width and margins (StudioView.chatColumn). */
const CHAT_COLUMN = 350 + 32;
const Z_MIN = 0.4, Z_MAX = 2.5;
/** The Windows caption buttons' corner at the top right (chrome.js): «Add block» keeps left of it. */
const CAPTION_WIDTH = 138;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

// MARK: The camera

/** Where the canvas is looked at, per canvas: the pan past the rail's edge and the person's zoom on top of the fit. */
let cam = null, camFor = null;
function camera() {
  const id = ui.state.canvas;
  if (camFor !== id) {
    camFor = id;
    const kept = store("cam:" + id);
    cam = kept && Number.isFinite(kept.x) ? { x: kept.x, y: kept.y, z: clamp(kept.z || 1, Z_MIN, Z_MAX) } : { x: 0, y: 0, z: 1 };
  }
  return cam;
}
let saveTimer = null;
function saveCam() { clearTimeout(saveTimer); const id = camFor, c = { ...cam }; saveTimer = setTimeout(() => store("cam:" + id, c), 150); }

/** What fits left of the docked chat at 100 %, from the window's width (CanvasViewport's fit). */
const fit = () => Math.min(1, Math.max(0.55, (window.innerWidth - CHAT_COLUMN - railEdge()) / CONTENT_WIDTH));
const scale = () => fit() * cam.z;
/** On whole device pixels: a drag moves by fractions, and the boards then shimmered against each other. */
const snap = (v) => Math.round(v * devicePixelRatio) / devicePixelRatio;

let world = null, percent = null;
function place() {
  if (!world) return;
  world.style.transform = `translate(${snap(railEdge() + cam.x)}px, ${snap(cam.y)}px) scale(${scale()})`;
  if (percent) percent.textContent = Math.round(scale() * 100) + "%";
}

/** Changes the zoom by `factor`, keeping the point (in the canvas, past the rail) where it is. */
function zoomAbout(px, py, factor) {
  const next = clamp(cam.z * factor, Z_MIN, Z_MAX);
  const k = next / cam.z;
  cam.x = px - (px - cam.x) * k;
  cam.y = py - (py - cam.y) * k;
  cam.z = next;
  place();
  saveCam();
}

function setZoom(z) { cam.z = clamp(Math.round(z * 10) / 10, Z_MIN, Z_MAX); place(); saveCam(); }
function fitHome() { Object.assign(cam, { x: 0, y: 0, z: 1 }); place(); saveCam(); }

/** The rail's Portfolio button: back to the portfolio at 100 %; already there, the window's edge pulses once instead. */
export function goHome() {
  camera();
  if (cam.x === 0 && cam.y === 0 && cam.z === 1) { pulseEdge(); return; }
  fitHome();
  if (!world) render();
}

function pulseEdge() {
  document.querySelector(".edge-pulse")?.remove();
  const el = h("div", { class: "edge-pulse" });
  document.body.append(el);
  el.addEventListener("animationend", () => el.remove(), { once: true });
}

// MARK: The canvas

let blocksOpen = false;
let blocksOnLeft = store("blocksOnLeft") === true;
let listening = false;

export function canvasView() {
  camera();
  // A place opened from the portfolio is its own block above the plan, covering nothing (StudioCanvas.plan).
  world = h("div", { class: "world" }, h("div", { class: "world-row" }, portfolioFrame(), h("div", { class: "cv-plan" }, placeDetail(), planColumn())));
  percent = h("span", { class: "zoom-percent num" });
  const surface = h("div", { class: "canvas" },
    world,
    zoomPill(),
    addBlockButton(),
    chatPanel(),
    blocksOpen ? blocksPanel() : null);
  place();
  wire(surface);
  if (!listening) {
    listening = true;
    let pending = false;
    window.addEventListener("resize", () => {
      if (pending) return;
      pending = true;
      requestAnimationFrame(() => { pending = false; place(); keepAddBlockClear(); });
    });
  }
  requestAnimationFrame(keepAddBlockClear);
  return surface;
}

/** − 80% + Fit, in a glass capsule beside the rail (CanvasHost's zoom bar). A step, not animated. */
function zoomPill() {
  return h("div", { class: "zoom-pill glass", style: `left:${railEdge() + 18}px` },
    h("button", { class: "zoom-glyph", title: t("Zoom out · or pinch, or Ctrl + wheel", "Отдалить · или щипок, или Ctrl + колесо"), onclick: () => setZoom(cam.z - 0.1) }, icon("minus", 13)),
    percent,
    h("button", { class: "zoom-glyph", title: t("Zoom in · or pinch, or Ctrl + wheel", "Приблизить · или щипок, или Ctrl + колесо"), onclick: () => setZoom(cam.z + 0.1) }, icon("plus", 13)),
    h("button", { class: "zoom-fit", title: t("100 % and back to the start", "100 % и к началу"), onclick: fitHome }, t("Fit", "Вписать")));
}

/** Drag anywhere that isn't a control to move the canvas (past 6 points, so a click stays a click); the wheel moves it,
 * Ctrl + wheel and a touchpad's pinch zoom about the pointer. What scrolls by itself (the chat, a long list) keeps its wheel. */
function wire(surface) {
  const touches = new Map();
  let pinch = null;
  const middle = () => { const [a, b] = [...touches.values()]; return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, d: Math.hypot(a.x - b.x, a.y - b.y) }; };
  surface.addEventListener("pointerdown", (e) => {
    if (e.pointerType !== "touch") return;
    touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (touches.size === 2) { pinch = middle(); ui.dragging = true; }
  });
  // On the surface, not the window: a touch is captured by where it began, so its moves come here, and a redraw drops
  // these with the old surface instead of piling them up.
  surface.addEventListener("pointermove", (e) => {
    if (!touches.has(e.pointerId)) return;
    touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (!pinch || touches.size !== 2) return;
    const now = middle(), r = surface.getBoundingClientRect();
    cam.x += now.x - pinch.x;
    cam.y += now.y - pinch.y;
    if (pinch.d > 0 && now.d > 0) zoomAbout(now.x - r.left - railEdge(), now.y - r.top, now.d / pinch.d);
    else place();
    pinch = now;
  });
  const lift = (e) => {
    if (!touches.delete(e.pointerId) || touches.size >= 2 || !pinch) return;
    pinch = null;
    ui.dragging = false;
    saveCam();
    if (ui.renderWaiting) { ui.renderWaiting = false; render(); }
  };
  surface.addEventListener("pointerup", lift);
  surface.addEventListener("pointercancel", lift);
  surface.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || e.target.closest("button, a, input, textarea, select, label, [contenteditable], .chat-panel, .blocks-panel, .zoom-pill, .add-block, .selectable, .address, [data-no-pan]")) return;
    const start = { x: e.clientX, y: e.clientY, cx: cam.x, cy: cam.y };
    let panning = false;
    const move = (ev) => {
      // A second finger makes it a pinch, which moves the canvas itself.
      if (pinch) return;
      const dx = ev.clientX - start.x, dy = ev.clientY - start.y;
      if (!panning) {
        if (Math.hypot(dx, dy) < 6) return;
        panning = true;
        ui.dragging = true;
        surface.classList.add("grabbing");
        try { surface.setPointerCapture(e.pointerId); } catch {}
      }
      cam.x = start.cx + dx;
      cam.y = start.cy + dy;
      place();
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (!panning) return;
      surface.classList.remove("grabbing");
      // The click that ends a drag isn't one.
      const eat = (ev) => { ev.stopPropagation(); ev.preventDefault(); };
      window.addEventListener("click", eat, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", eat, true), 0);
      ui.dragging = false;
      saveCam();
      if (ui.renderWaiting) { ui.renderWaiting = false; render(); }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  });
  surface.addEventListener("wheel", (e) => {
    if (e.ctrlKey && e.target.closest(".chat-panel, .blocks-panel")) { e.preventDefault(); return; }
    if (!e.ctrlKey && (e.target.closest(".chat-panel, .blocks-panel") || scrollsItself(e.target, surface, e))) return;
    e.preventDefault();
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? window.innerHeight : 1;
    if (e.ctrlKey || e.metaKey) {
      const r = surface.getBoundingClientRect();
      zoomAbout(e.clientX - r.left - railEdge(), e.clientY - r.top, Math.exp(-clamp(e.deltaY * unit, -25, 25) * 0.008));
    } else {
      cam.x -= e.deltaX * unit;
      cam.y -= e.deltaY * unit;
      place();
      saveCam();
    }
  }, { passive: false });
}

/** An element under the pointer that can still scroll the wheel's way keeps it (a long history, a row of cards). */
function scrollsItself(target, surface, e) {
  for (let el = target; el && el !== surface; el = el.parentElement) {
    const s = getComputedStyle(el);
    if (e.deltaY && /(auto|scroll)/.test(s.overflowY) && el.scrollHeight > el.clientHeight + 1) {
      if ((e.deltaY > 0 && el.scrollTop + el.clientHeight < el.scrollHeight - 1) || (e.deltaY < 0 && el.scrollTop > 0)) return true;
    }
    if (e.deltaX && /(auto|scroll)/.test(s.overflowX) && el.scrollWidth > el.clientWidth + 1) {
      if ((e.deltaX > 0 && el.scrollLeft + el.clientWidth < el.scrollWidth - 1) || (e.deltaX < 0 && el.scrollLeft > 0)) return true;
    }
  }
  return false;
}

// MARK: Add block and the block library

function addBlockButton() {
  return h("button", { class: "add-block glass", style: `right:${16 + CAPTION_WIDTH + 8}px`,
    title: blocksOpen ? t("Close the blocks", "Закрыть блоки") : t("The blocks the agent builds a request from", "Блоки, из которых агент собирает запрос"),
    onclick: () => { blocksOpen = !blocksOpen; render(); } },
    icon(blocksOpen ? "xmark" : "plus", 14), blocksOpen ? t("Close blocks", "Закрыть блоки") : t("Add block", "Добавить блок"));
}

/** A chat moved up to the top right pushes «Add block» to its left, as on the Mac. */
function keepAddBlockClear() {
  const button = document.querySelector(".add-block"), chat = document.querySelector(".chat-panel");
  if (!button) return;
  let right = 16 + CAPTION_WIDTH + 8;
  if (chat) {
    const r = chat.getBoundingClientRect();
    if (r.top < 70 && r.right > window.innerWidth - 200) right = Math.max(right, window.innerWidth - r.left + 12);
  }
  button.style.right = right + "px";
  const panel = document.querySelector(".blocks-panel:not(.left)");
  if (panel) panel.style.right = right - CAPTION_WIDTH - 8 + "px";
}

/** The blocks on the canvas this window has: a pick adds the block. */
const canvasBlocks = [
  ["receive", "arrow.down.circle", () => t("Receive", "Получить"), () => t("Your addresses on every network, whole, with Copy", "Твои адреса во всех сетях, целиком, с кнопкой «Копировать»")],
  ["addresses", "person.crop.rectangle", () => t("Address book", "Адресная книга"), () => t("Saved addresses · money goes only to them", "Сохранённые адреса · деньги уходят только на них")],
  ["history", "clock", () => t("History", "История"), () => t("Everything signed, with its explorer link", "Всё подписанное, со ссылкой в обозреватель")],
  ["favorites", "star", () => t("Favorites", "Избранное"), () => t("Watched tokens and wallets, starred first", "Отслеживаемые токены и кошельки, отмеченные первыми")],
  ["alerts", "bell", () => t("Price alerts", "Ценовые оповещения"), () => t("When a price crosses a level", "Когда цена пересекает уровень")],
  ["schedule", "calendar", () => t("Schedule", "Расписание"), () => t("Tasks on a clock, and the automation wallet", "Задачи по времени и кошелёк автоматизации")],
  ["positions", "chart.line", () => t("Open positions", "Открытые позиции"), () => t("Hyperliquid's positions, P/L and liquidation", "Позиции на Hyperliquid, P/L и ликвидация")],
  ["multisig", "lock.square.stack", () => t("Multisig", "Мультисиг"), () => t("A Safe that moves money only when enough keys sign", "Safe, который двигает деньги, только когда подпишет достаточно ключей")],
  ["ledger", "cpu", () => "Ledger", () => t("Every move shown on its screen, approved with its buttons", "Каждое движение на его экране, подтверждение его кнопками")],
];

/** The Mac's library (BlockCatalog in IhatecryptoCore): each block is one of the chat's forms; a pick puts its example
 * into the chat field to finish and send. The automatic ones the agent adds to every trade itself: shown, not picked. */
const B = (group, label, title, form, example, fill, logoName, automatic = false) => ({ group, label, title, form, example, fill, logo: logoName, automatic });
const catalog = [
  B("Trade", "OPEN LONG · HYPERLIQUID", "Long or short", "long|short ASSET · risk $N · stop N% · take N% · leverage Nx", "long SOL by the book", "hyperliquid", "hype"),
  B("Trade", "PROTECT", "Stop and take", "take NR · take in parts · trailing N% · move stop to breakeven", "take in parts", "hyperliquid", "hype"),
  B("Trade", "CHECK RULES", "Risk rules", "risk ≤ 2% of capital · reward/risk ≥ 1.5 · leverage ≤ 5×", "", "check", null, true),
  B("Trade", "SECOND CHECK", "Second check", "the plan against your words, then every signature", "", "check", null, true),
  B("Trade", "RESULT · ESTIMATE", "Loss or profit", "at the stop and at the take, in $ and % of capital", "", "check", null, true),
  B("Triggers", "WHEN · PRICE FALLS TO", "Price alert", "when ASSET is above|below PRICE: a trade or a move", "alert me when SOL is below 115", "check", "sol"),
  B("Triggers", "LATER · SCHEDULE", "Later or on repeat", "in N days: … · at HH:MM: … · every week: … · every month on the 1st: …", "in 1 week: rebalance USDC", "check", null),
  B("Move money", "BRIDGE · RELAY", "Between networks", "move N USDC between Solana, Base, Arbitrum, Polygon, Ethereum, Robinhood, Tempo, HyperEVM, Optimism, BNB Chain — any two, straight", "move 5 USDC from Solana to Base", "relay", "relay"),
  B("Move money", "GAS · TOP UP", "Gas on any network", "add gas on Base|Arbitrum|Polygon|Ethereum|Robinhood for N USDC · moves top it up by themselves", "add gas on robinhood for 1 USDC", "robinhood", "eth"),
  B("Move money", "FROM · HYPERLIQUID", "Back to Solana", "withdraw N USDC to Solana", "withdraw 1 USDC to Solana", "hyperliquid", "hype"),
  B("Move money", "TO · ADDRESS BOOK", "Send", "send N USDC|SOL|NEAR to NAME from the address book", "send 1 USDC to Bybit", "solana", "usdc"),
  B("Move money", "ANY TOKEN · AS IT IS", "Send a token", "send N|all TICKER|0xCONTRACT|MINT on NETWORK to NAME from the address book", "send all RAY on solana to Bybit", "solana", "sol"),
  B("Buy and sell", "SWAP · JUPITER", "Tokens on Solana", "buy|sell SOL, BTC, ETH, JUP, BONK… for N USDC", "buy SOL for 2 USDC", "jupiter", "jup"),
  B("Buy and sell", "EVERY DAY · JUPITER DCA", "DCA on Jupiter", "Jupiter DCA N USDC into SOL, BTC, JUP, BONK… in K orders every day|week · on-chain, from 100 USDC, 50 an order", "Jupiter DCA 100 USDC into SOL in 2 orders every day", "jupiter", "jup"),
  B("Buy and sell", "BUY · ON BASE", "Base and Arbitrum", "buy TICKER on base|arbitrum for N USDC · Relay or Uniswap, the better", "buy AERO on base for 2 USDC", "base", "base"),
  B("Buy and sell", "SWEEP · DUST", "Dust into USDC", "sweep dust [under $N] · leftovers on Base, Arbitrum, Polygon", "sweep dust under $5", "base", "usdc"),
  B("Buy and sell", "SWAP · UNISWAP", "Token for token", "swap N ETH|TICKER for TICKER on base|arbitrum|ethereum|polygon · straight, no USDC in the middle", "swap 0.001 ETH for AERO on base", "base", "uni"),
  B("Buy and sell", "SWAP · STABLECOINS", "Dollar for dollar", "USDT, USDC, USDS, USDe, DAI on every network they're on · Uniswap, Jupiter, Relay or Hyperliquid, whichever gives more", "buy USDe on base for 2 USDC from base", "base", "usdc"),
  B("Buy and sell", "BUY · ROBINHOOD CHAIN", "Stock tokens", "buy TSLA, NVDA, SpaceX… stock for N USDC", "buy TSLA stock for 2 USDC", "robinhood", "robinhood"),
  B("Buy and sell", "SWAP · NEAR INTENTS", "NEAR", "buy NEAR for N USDC · sell NEAR", "buy NEAR for 2 USDC", "near", "near"),
  B("Earn", "INTO · JUPITER LEND", "Lend on Solana", "lend N USDC|SOL · withdraw N USDC from the vault", "lend 1 USDC", "jupiter", "jup"),
  B("Earn", "INTO · KAMINO", "Kamino", "deposit N USDC|SOL to kamino · withdraw [N] USDC from kamino", "deposit 1 USDC to kamino", "kamino", "kamino"),
  B("Earn", "INTO · AAVE", "Aave", "lend N USDC on Aave on Base|Arbitrum|Polygon|Ethereum", "lend 2 USDC on Aave", "aave", "aave"),
  B("Earn", "INTO · MORPHO", "Morpho", "deposit N USDC into Morpho on Base|Arbitrum|Ethereum", "deposit 2 USDC into Morpho", "morpho", "morpho"),
  B("Earn", "INTO · HLP", "Hyperliquid's vault", "deposit N USDC to HLP · from $5, locked 4 days", "deposit 5 USDC to HLP", "hlp", "hype"),
  B("Earn", "STAKE · JITO", "Stake SOL", "stake N SOL · unstake N JitoSOL", "stake 0.01 SOL", "jito", "jitosol"),
  B("Earn", "SELL · REWARDS", "Sell rewards", "sweep rewards [over $N] [into TOKEN] · every hour: sweep rewards over $10 into USDC", "every day: sweep rewards over $10 into USDC", "morpho", "usdc"),
  B("Earn", "BORROW · AAVE", "Borrow USDC", "supply N ETH as collateral on Aave · borrow N USDC on Aave · repay all USDC on Aave — health factor kept at 1.5+", "borrow 5 USDC on Aave on base", "aave", "aave"),
  B("Earn", "INTO · BEST RATES", "The best rates", "spread N USDC across the best rates · rebalance USDC", "spread USDC across the best rates", "jupiter", "usdc"),
  B("Tempo", "BRIDGE · TO TEMPO", "USDC to Tempo", "move N USDC from Solana to Tempo · and back to Solana", "move 2 USDC from Solana to Tempo", "tempo", "tempo"),
  B("Tempo", "SEND · ON TEMPO", "Send on Tempo", "send N USDC to NAME saved on Tempo · fee ≈ $0.001 in USDC", "send 5 USDC to Mom", "tempo", "tempo"),
  B("Tempo", "SWAP · ON TEMPO", "Swap dollars", "swap N USDC for pathUSD or OUSD on tempo · Tempo's own exchange, ≈ 1–2 bp", "swap 1 USDC for pathUSD on tempo", "tempo", "tempo"),
  B("Tempo", "EVERY MONTH · 1ST", "Monthly transfer", "every month on the 1st…28th [at HH:MM]: send N USDC to NAME", "every month on the 1st at 10:00: send 20 USDC to Mom", "tempo", "tempo"),
  B("Tempo", "PAY · WEB SEARCH", "Search the web", "Brave Search · $0.035 a search · web, news, images, video", 'pay POST https://brave.mpp.paywithlocus.com/brave/web-search {"q":"Tempo MPP payments"}', "tempo", "tempo"),
  B("Tempo", "PAY · CRYPTO PRICE", "Crypto prices", "CoinGecko · $0.06 a request · prices, markets, coin data", 'pay POST https://coingecko.mpp.paywithlocus.com/coingecko/simple-price {"ids":"bitcoin","vs_currencies":"usd"}', "tempo", "tempo"),
  B("Tempo", "PAY · WEATHER", "Weather", "OpenWeather · $0.006 · now, 5 days, air quality by lat/lon", 'pay POST https://openweather.mpp.paywithlocus.com/openweather/current-weather {"lat":55.7558,"lon":37.6173}', "tempo", "tempo"),
  B("Tempo", "PAY · TRANSLATE", "Translate", "DeepL · priced per text · 30+ languages", 'pay POST https://deepl.mpp.paywithlocus.com/deepl/translate {"text":["Hello from ihatecrypto"],"target_lang":"RU"}', "tempo", "tempo"),
  B("Tempo", "PAY · EMAIL", "Send an email", "StableEmail · $0.02 a letter · to, subject, text", 'pay POST https://stableemail.dev/api/send {"to":["you@example.com"],"subject":"Hi","text":"Sent from ihatecrypto"}', "tempo", "tempo"),
  B("Tempo", "PAY · PHONE CALL", "AI phone call", "StablePhone · $0.54 a call · a number and what to say", 'pay POST https://stablephone.dev/api/call {"phone_number":"+14155551234","task":"Ask if the table for two at 8 pm is still free."}', "tempo", "tempo"),
  B("Tempo", "PAY · FLIGHTS", "Cheapest flights", "StableTravel · $0.02 · Google Flights by airports and date", "pay GET https://stabletravel.dev/api/google-flights/search?departure_id=SVO&arrival_id=IST&outbound_date=2026-10-15&type=2", "tempo", "tempo"),
  B("HyperEVM", "BRIDGE · TO HYPEREVM", "USDC to HyperEVM", "move N USDC from Solana to HyperEVM · HYPE for gas comes along", "move 2 USDC from Solana to HyperEVM", "hyperevm", "hyperevm"),
  B("HyperEVM", "BACK · HYPEREVM → SOLANA", "USDC back to Solana", "move N USDC from HyperEVM to Solana · no N = all of it", "move all USDC from HyperEVM to Solana", "hyperevm", "hyperevm"),
  B("HyperEVM", "GAS · HYPE", "HYPE for gas", "add gas on HyperEVM [for $N] · ≈ $0.001 a transaction", "add gas on hyperevm", "hyperevm", "hyperevm"),
  B("HyperEVM", "SEND · ON HYPEREVM", "Send on HyperEVM", "send N USDC to NAME on HyperEVM · a saved 0x address", "send 5 USDC to Mom on hyperevm", "hyperevm", "hyperevm"),
  B("Optimism", "BRIDGE · TO OPTIMISM", "USDC to Optimism", "move N USDC from Solana to Optimism · ETH for gas comes along", "move 2 USDC from Solana to Optimism", "relay", "op"),
  B("Optimism", "BACK · OPTIMISM → SOLANA", "USDC back to Solana", "move N USDC from Optimism to Solana · no N = all of it", "move all USDC from Optimism to Solana", "relay", "op"),
  B("Optimism", "CCTP · BASE", "To Base by Circle", "move N USDC from Optimism to Base via CCTP · Circle burns and mints", "move 100 USDC from Optimism to Base via CCTP", "relay", "op"),
  B("Optimism", "SEND · ON OPTIMISM", "Send on Optimism", "send N USDC to NAME on Optimism · a saved 0x address", "send 5 USDC to Mom on optimism", "relay", "op"),
  B("BNB Chain", "BRIDGE · TO BNB CHAIN", "USDC to BNB Chain", "move N USDC from Solana to BNB Chain · BNB for gas comes along", "move 2 USDC from Solana to BNB", "relay", "bnb"),
  B("BNB Chain", "BACK · BNB CHAIN → SOLANA", "USDC back to Solana", "move N USDC from BNB Chain to Solana · no N = all of it", "move all USDC from BNB to Solana", "relay", "bnb"),
  B("BNB Chain", "BUY · ON BNB CHAIN", "Buy a token there", "buy TICKER on BNB Chain for N USDC · CAKE, BTCB, ETH, LINK…", "buy CAKE on bnb for 2 USDC", "relay", "bnb"),
  B("BNB Chain", "SWAP · ON BNB CHAIN", "One token for another", "swap N BNB|TICKER for TICKER on BNB Chain · straight, no USDC between", "swap 0.001 BNB for CAKE on bnb", "relay", "bnb"),
  B("BNB Chain", "GAS · BNB", "BNB for gas", "add gas on BNB Chain [for $N] · ≈ a cent a transaction", "add gas on bnb", "relay", "bnb"),
  B("BNB Chain", "SEND · ON BNB CHAIN", "Send on BNB Chain", "send N USDC to NAME on BNB Chain · a saved 0x address", "send 5 USDC to Mom on bnb", "relay", "bnb"),
];
const groups = ["Trade", "Triggers", "Move money", "Buy and sell", "Earn", "Tempo", "HyperEVM", "Optimism", "BNB Chain"];
const groupRu = { Trade: "Торговля", Triggers: "Условия", "Move money": "Переводы", "Buy and sell": "Купить и продать", Earn: "Доход" };

/** With a trade plan on the canvas, the price alert is for that plan (the engine's planAlertExample). */
const example = (b) => (b.label.startsWith("WHEN") && ui.state.planAlertExample) || b.example;

/** A route fill's color (RouteFill.color); the light ones keep dark text. */
const fills = { solana: "var(--solana-tint)", relay: "#4615C8", hyperliquid: "#072723", hlp: "#0D3B35", jupiter: "#131D27", kamino: "#082A56",
  jito: "#1F9D6A", aave: "#2B2D3C", morpho: "#1D4FC4", base: "#0052FF", arbitrum: "#213147", robinhood: "#3A4A00", near: "#10231C",
  tempo: "#111111", hyperevm: "#0E2B26", check: "var(--check-fill)" };

/** «KAMINO LEND» → «Kamino Lend»: short words and tickers keep their capitals (RouteCard.pretty). */
function pretty(place) {
  const small = new Set(["ON", "OF", "TO", "OR", "AND", "IN", "AT", "FOR", "WITH", "FROM"]);
  const kept = { USDC: "USDC", USDG: "USDG", USDT: "USDT", NEAR: "NEAR", JITOSOL: "JitoSOL", "1CLICK": "1Click", HYPEREVM: "HyperEVM", CCTP: "CCTP" };
  return place.split(" ").map((w, i) => kept[w] || (small.has(w) ? (i ? w.toLowerCase() : w[0] + w.slice(1).toLowerCase())
    : w.length <= 3 || /\d/.test(w) || !/[A-Z]/i.test(w) ? w : w[0] + w.slice(1).toLowerCase())).join(" ");
}

/** A block as the small route card it becomes: its color with the coin in it, its label and title, its form in rows. */
function tile({ label, place: placeName, title, rows, fill, logoName, glyph, hint, onpick }) {
  const [verb, ...rest] = label.split("·").map((s) => s.trim());
  const place = placeName ?? (rest.length ? pretty(rest.join(" · ")) : null);
  const dark = fill !== "solana" && fill !== "check";
  return h(onpick ? "button" : "div", { class: "blk-tile" + (onpick ? " pick" : "") + (fill === "check" ? " plain" : ""), title: hint, onclick: onpick },
    h("div", { class: "blk-pic" + (dark ? " dark" : ""), style: `background:${fills[fill] || fills.check}` },
      mascot(40, "idle"),
      logoName ? h("span", { class: "blk-badge" }, logo(logoName, 26)) : glyph ? h("span", { class: "blk-badge glyph" }, icon(glyph, 18)) : null),
    h("div", { class: "blk-info" },
      h("div", { class: "blk-label" }, h("b", {}, verb), place ? [h("i", {}), logoName ? logo(logoName, 14) : null, h("span", { class: "one-line" }, place)] : null),
      h("div", { class: "blk-title" }, title),
      rows.length ? h("div", { class: "blk-box" }, rows.slice(0, 3).map((r) => h("div", { class: "blk-row" }, r))) : null,
      h("div", { class: "blk-foot one-line" }, hint)));
}

function blocksPanel() {
  const close = () => { blocksOpen = false; render(); };
  const left = blocksOnLeft;
  const live = canvasBlocks.map(([kind, glyph, title, about]) => tile({
    label: t("BLOCK", "БЛОК"), place: t("on the canvas", "на холсте"), title: title(), rows: [about()], fill: "check", glyph,
    hint: t("Adds it to this canvas", "Добавит его на этот холст"), onpick: () => { act("addBoard", { name: kind }); close(); } }));
  return h("div", { class: "blocks-panel" + (left ? " left" : ""), style: left ? `left:${railEdge() + 16}px` : "" },
    h("div", { class: "blocks-head" },
      h("div", {}, h("div", { class: "blocks-title" }, t("Blocks", "Блоки")),
        h("div", { class: "blocks-about" }, t("What the agent builds a request from. Pick one to start it in the chat, or just write — it picks them itself.",
          "Из чего агент собирает запрос. Выбери блок, чтобы начать его в чате, или просто напиши — агент подберёт сам."))),
      h("button", { class: "icon-btn", title: left ? t("Move the blocks back to the right", "Вернуть блоки направо") : t("Move the blocks next to the sidebar", "Перенести блоки к боковой панели"),
        onclick: () => { blocksOnLeft = !blocksOnLeft; store("blocksOnLeft", blocksOnLeft); render(); } }, icon(left ? "arrow.right.to.line" : "arrow.left.to.line", 14))),
    h("div", { class: "blocks-scroll", "data-keep": "blocks" },
      h("div", { class: "blocks-group" }, t("On the canvas", "На холсте")), h("div", { class: "blocks-grid" }, live),
      groups.map((g) => [
        h("div", { class: "blocks-group" }, ui.state.russian && groupRu[g] ? groupRu[g] : g),
        h("div", { class: "blocks-grid" }, catalog.filter((b) => b.group === g).map((b) => tile({
          label: b.label, title: b.title, rows: b.form.split(" · ").concat(b.automatic ? [t("added by the agent", "добавляет агент")] : []),
          fill: b.fill, logoName: b.logo,
          hint: b.automatic ? t("The agent adds it to every trade by itself", "Агент добавляет его к каждой сделке сам") : t(`Puts “${example(b)}” into the chat`, `Вставит в чат «${example(b)}»`),
          onpick: b.automatic ? null : () => { blocksOpen = false; suggest(example(b)); } })))])));
}
