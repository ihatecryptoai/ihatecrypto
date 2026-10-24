// The chat floating over the canvas, as the Mac's FloatingChat (Studio.swift) and its replies (ChatReplies.swift): a
// glass panel docked at the bottom right that the header moves anywhere and the left edge widens; the coin with its
// mood; the conversation; the field with the model, the networks and the context over it. It never moves money: a
// move waits as a card, and only Confirm on it (here or on the canvas) runs it.

import { ui, act, engine, render, t, h, svg, icon, addIcons, mascot, logo, toast, invoke, store, suggest, autosize, link, presence, reportComposing } from "./core.js";
import { railEdge } from "./rail.js";
import { blockTitle } from "./boards.js";
import { templatePreview } from "./trade.js";
import { goHome } from "./canvas.js";

addIcons({
  bell: "M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5h4",
  cart: "M3.5 4.5h2.5l2 11h10l2-8H7M10 20h.01M17 20h.01",
  percent: "M18 6 6 18M7.5 9.5a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM16.5 18.5a2 2 0 1 0 0-4 2 2 0 0 0 0 4z",
  "building.columns": "M3.5 9.5 12 4l8.5 5.5zM5.5 10v7M10 10v7M14 10v7M18.5 10v7M3.5 20h17",
  "building.2": "M4 20.5V6l8-2.5v17M12 9h8v11.5M7 9h2M7 12.5h2M7 16h2M15 12.5h2M15 16h2M2.5 20.5h19",
  "arrow.up.right": "M7 17 17 7M9 7h8v8",
  "arrow.down.right": "M7 7l10 10M17 9v8H9",
  "arrow.up.left": "M17 17 7 7M7 15V7h8",
  "arrow.uturn.left": "M9 14 4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3",
  "arrow.left.arrow.right": "M4 8h15M15 4l4 4-4 4M20 16H5M9 12l-4 4 4 4",
  "arrow.right.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 12h8M13 8.5l3.5 3.5-3.5 3.5",
  "chart.line.uptrend": "M3.5 18.5l5.5-6 4 3.5 7.5-8.5M15 7.5h5.5V13",
  "chart.xyaxis.line": "M4 3.5V20h16.5M7.5 15l3.5-4.5 3 2.5 4.5-6",
  "chart.bar": "M4 20.5h16M6.5 17V11M11 17V6M15.5 17v-8M20 17v-4",
  shield: "M12 3.5 19 6v5.5c0 4.5-3 7.8-7 9-4-1.2-7-4.5-7-9V6z",
  leaf: "M5 19c0-8 5-13 15-14-1 10-6 15-14 15M5 19l6.5-6.5",
  banknote: "M3 7h18v10H3zM12 14.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5zM6 10v4M18 10v4",
  flame: "M12 21c-4 0-6.5-2.7-6.5-6.3 0-4 3-6 4-9.7 2.5 1.6 3 4 3 5.5 1-1 1.5-2.2 1.5-3.5 2.5 2 4.5 4.8 4.5 7.7 0 3.6-2.5 6.3-6.5 6.3z",
  pawprint: "M12 13c3 0 5 3.5 5 5.5S15 21 12 21s-5-.5-5-2.5S9 13 12 13zM6 11.5a1.8 2.2 0 1 0 0-4.4 1.8 2.2 0 0 0 0 4.4zM18 11.5a1.8 2.2 0 1 0 0-4.4 1.8 2.2 0 0 0 0 4.4zM9.5 8a1.8 2.2 0 1 0 0-4.4 1.8 2.2 0 0 0 0 4.4zM14.5 8a1.8 2.2 0 1 0 0-4.4 1.8 2.2 0 0 0 0 4.4z",
  sparkles: "M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
  gift: "M4 10h16v3H4zM5.5 13v7.5h13V13M12 10v10.5M12 10S10.5 4.5 8 5s-1 5 4 5zM12 10s1.5-5.5 4-5 1 5-4 5z",
  drop: "M12 3.5s6.5 7 6.5 11a6.5 6.5 0 0 1-13 0c0-4 6.5-11 6.5-11z",
  medal: "M8 3.5h8l-2 6h-4zM12 21a5.5 5.5 0 1 0 0-11 5.5 5.5 0 0 0 0 11zM12 13.5l1 2 2 .3-1.5 1.4.4 2-1.9-1-1.9 1 .4-2L9 15.8l2-.3z",
  key: "M8.5 15.5a4 4 0 1 1 3.4-6.1L20.5 9.5V13h-2.5v2.5h-3V13h-2.6a4 4 0 0 1-3.9 2.5zM7.5 12.5h.01",
  "calendar.badge.clock": "M4 6h16v6M4 10h16M8 3.5V7M16 3.5V7M4 6v14h7M17 21a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM17 15.5v1.8l1.2.8",
  "text.bubble": "M4 5h16v11H9l-5 4zM8 9h8M8 12.5h5",
  "dollarsign.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM14.5 9c-.4-1-1.3-1.5-2.5-1.5-1.6 0-2.5.8-2.5 2 0 2.8 5.2 1.5 5.2 4.3 0 1.2-1 2.2-2.7 2.2-1.3 0-2.3-.6-2.7-1.7M12 6v1.5M12 16v2",
  creditcard: "M3 6h18v12H3zM3 10h18M6.5 14.5h4",
  paperclip: "M16.5 7.5l-7 7a2 2 0 0 0 2.8 2.8l7.4-7.4a4 4 0 0 0-5.7-5.7l-7.4 7.4a6 6 0 0 0 8.5 8.5l6.4-6.4",
  "square.on.square": "M8 8h12v12H8zM4 16V4h12",
  "xmark.octagon": "M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3zM9 9l6 6M15 9l-6 6",
  "lock.shield": "M12 3.5 19 6v5.5c0 4.5-3 7.8-7 9-4-1.2-7-4.5-7-9V6zM9.5 11.5h5v4h-5zM10.5 11.5v-1.3a1.5 1.5 0 0 1 3 0v1.3",
  "person.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 12.5a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6.5 18a6 6 0 0 1 11 0",
  desktop: "M3.5 4.5h17v11h-17zM9 20h6M12 15.5V20",
  "stop.fill": "M7.5 7.5h9v9h-9z",
  curlybraces: "M9 4.5c-2 0-2.5 1-2.5 2.5v2c0 1.5-.7 2.5-2 2.5 1.3.5 2 1.5 2 3v2c0 1.5.5 2.5 2.5 2.5M15 4.5c2 0 2.5 1 2.5 2.5v2c0 1.5.7 2.5 2 2.5-1.3.5-2 1.5-2 3v2c0 1.5-.5 2.5-2.5 2.5",
  hourglass: "M7 3.5h10M7 20.5h10M8 3.5c0 4 8 5 8 8.5s-8 4.5-8 8.5M16 3.5c0 4-8 5-8 8.5s8 4.5 8 8.5",
  "play.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM10 8.5v7l5.5-3.5z",
  "exclamationmark.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7.5v5.5M12 16.5h.01",
  magnifyingglass: "M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5 20 20",
  "circle.dotted": "M12 3v1M12 20v1M3 12h1M20 12h1M5.6 5.6l.7.7M17.7 17.7l.7.7M5.6 18.4l.7-.7M17.7 6.3l.7-.7",
  "xmark.circle": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 9l6 6M15 9l-6 6",
  "shield.lefthalf": "M12 3.5 19 6v5.5c0 4.5-3 7.8-7 9-4-1.2-7-4.5-7-9V6zM12 3.5v17",
  "circle.lefthalf": "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 3v18",
});

// MARK: Where the panel sits

/** The Mac's @AppStorage: the width the left edge set, the offset from the docked corner (both ≤ 0), full height or not. */
const place = {
  width: store("chatWidth") ?? 350,
  x: store("chatX") ?? 0,
  y: store("chatY") ?? 0,
  full: store("chatFull") === true,
};
const MIN = 320, NARROW = 350;
/** Room kept at the top for Windows' minimize, maximize and close, which sit at the canvas's top right. */
const TOP = 44;

/** The canvas beside the rail: the room the chat lives in. */
function room() { return { w: Math.max(0, window.innerWidth - railEdge()), h: window.innerHeight }; }
const maxWidth = () => Math.max(MIN, room().w / 2);

/** Size and offset for the panel now, kept inside the canvas: never past its left or top edge. */
function frame(pull = { x: 0, y: 0 }, width = place.width) {
  const r = room();
  const w = Math.min(width, maxWidth());
  const full = Math.max(200, r.h - 16 - TOP);
  // The Mac's half of the canvas, but never so short that the field and its chips fall off a small screen (a 768-line
  // laptop at 150%, a VM): the Mac's window is never that short.
  const small = Math.min(full, Math.max(360, r.h * 0.5));
  const hgt = place.full ? full : small;
  const clamp = (v, span) => Math.min(0, Math.max(-span, v));
  const x = clamp(place.x + pull.x, Math.max(0, r.w - w - 32));
  const y = clamp(place.y + pull.y, Math.max(0, r.h - hgt - 16 - TOP));
  return { w, h: hgt, x, y, docked: x > -40 && y > -40 };
}

function apply(panel, f) {
  panel.style.width = f.w + "px";
  panel.style.height = f.h + "px";
  panel.style.transform = `translate(${f.x}px, ${f.y}px)`;
  panel.classList.toggle("chat-moved", !f.docked);
}

// A window made smaller keeps the chat inside it, without drawing everything again.
window.addEventListener("resize", () => { const p = document.querySelector(".chat-panel"); if (p) apply(p, frame()); });

function save() { store("chatWidth", place.width); store("chatX", place.x); store("chatY", place.y); store("chatFull", place.full); }

/** A drag that holds new states back while it lasts: a redraw would drop the panel being moved. */
function drag(e, move, end) {
  if (e.button !== 0) return;
  e.preventDefault();
  const start = { x: e.clientX, y: e.clientY };
  const target = e.currentTarget;
  target.setPointerCapture(e.pointerId);
  ui.dragging = true;
  const onMove = (ev) => move({ x: ev.clientX - start.x, y: ev.clientY - start.y });
  const onUp = (ev) => {
    target.removeEventListener("pointermove", onMove);
    ui.dragging = false;
    end({ x: ev.clientX - start.x, y: ev.clientY - start.y });
    save();
    if (ui.renderWaiting) { ui.renderWaiting = false; render(); }
  };
  target.addEventListener("pointermove", onMove);
  target.addEventListener("pointerup", onUp, { once: true });
  target.addEventListener("pointercancel", onUp, { once: true });
}

function moveBy(e, panel) {
  if (e.target.closest("button")) return;
  drag(e, (pull) => apply(panel, frame(pull)), (pull) => {
    const f = frame(pull);
    // Close to its corner it snaps home.
    [place.x, place.y] = f.docked ? [0, 0] : [f.x, f.y];
    apply(panel, frame());
  });
}

function resizeBy(e, panel) {
  const base = Math.min(place.width, maxWidth());
  const widthFor = (pull) => Math.min(maxWidth(), Math.max(MIN, base - pull.x));
  drag(e, (pull) => apply(panel, frame({ x: 0, y: 0 }, widthFor(pull))), (pull) => {
    place.width = widthFor(pull);
    apply(panel, frame());
  });
}

const wide = () => Math.min(place.width, maxWidth()) >= maxWidth() - 1;
function toggleWide() { place.width = wide() ? NARROW : maxWidth(); save(); render(); }
function toggleFull() { place.full = !place.full; save(); render(); }
function dock() { place.x = 0; place.y = 0; save(); render(); }

// MARK: The panel

export function chatPanel() {
  const s = ui.state;
  const panel = h("div", { class: "chat-panel glass" });
  // The model menu's lists are read when the chip first appears (and after the menu closes), never while it's open;
  // the open menu draws again from each state, so a download's percent or a finished sign-in shows in it.
  if (!modelsAsked) { modelsAsked = true; act("refreshModels"); }
  if (liveMenu) liveMenu();
  // The networks menu and the context popover follow the state while open too («N of M on» as a switch flips).
  const live = document.querySelector(".menu");
  if (live?._live) { const top = live.scrollTop; live.replaceChildren(live._live()); live.scrollTop = top; }
  // Cards come under a reply already shown: when they don't all fit, the reply they answer stays in view from its top
  // (the Mac's onChange(of: nextSteps?.message)) — after app.js has put a following chat at its end; not at launch.
  const cardsFor = liveMessage()?.id || null;
  if (cardsShownFor !== undefined && cardsFor && cardsFor !== cardsShownFor) queueMicrotask(() => replyInView(cardsFor));
  cardsShownFor = cardsFor;
  const mood = moodNow();
  const head = h("div", { class: "chat-head", title: t("Drag to move · double-click: back to its corner", "Перетащи, чтобы подвинуть · двойной клик — на место"),
      onpointerdown: (e) => moveBy(e, panel), ondblclick: (e) => { if (!e.target.closest("button")) dock(); } },
    mascot(30, mood),
    h("div", { class: "chat-title" }, h("b", {}, t("Assistant", "Ассистент")), h("span", { class: "chat-status one-line" }, statusLine(mood))),
    h("span", { class: "spacer" }),
    h("button", { class: "icon-btn", title: t("New canvas", "Новый холст"), onclick: () => act("newCanvas") }, icon("square.and.pencil", 15)),
    h("button", { class: "icon-btn", title: wide() ? t("Narrow chat", "Узкий чат") : t("Chat on half the window", "Чат на половину окна"), onclick: toggleWide },
      icon(wide() ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right", 15)),
    h("button", { class: "icon-btn", title: place.full ? t("Compact chat", "Компактный чат") : t("Full-height chat", "Чат во всю высоту"), onclick: toggleFull },
      icon(place.full ? "minus" : "chevron.up", 15)));
  const edge = h("div", { class: "chat-edge", title: t("Drag to resize the chat · double-click: half the window", "Перетащи, чтобы изменить ширину · двойной клик — половина окна"),
    onpointerdown: (e) => resizeBy(e, panel), ondblclick: toggleWide });
  panel.append(edge, head,
    h("div", { class: "chat-scroll", "data-keep": "chat", "data-stick": s.chat.length || s.thinking ? "bottom" : null }, h("div", { class: "chat-list" }, conversation(s))),
    composer(s));
  apply(panel, frame());
  return panel;
}

/** The coin's face in the header, in step with the line under «Assistant». */
function moodNow() {
  const s = ui.state;
  if (s.thinking || s.runningBoard) return "thinking";
  // A menu of the chat's open: the coin raises its brows and looks at it (the Mac's ws.tuning).
  if (tuning) return "tuning";
  if (voice.on) return "listening";
  if (s.card) return "waiting";
  const newest = s.boards.find((b) => b.kind === "move");
  if (newest?.state === "failed") return "error";
  if (newest?.state === "done") return "done";
  return "idle";
}

function statusLine(mood) {
  const s = ui.state;
  switch (mood) {
    case "error": return t("Something went wrong", "Что-то пошло не так");
    case "thinking":
      if (s.runningBoard) return s.signer.waiting ? t(`${presence()} is asking for you`, `${presence()} спрашивает тебя`) : t("Carrying it out", "Выполняю");
      return t("Working on it", "Работаю над этим");
    case "listening": return t("Listening…", "Слушаю…");
    case "tuning": return tuning === "networks" ? t("Choosing networks", "Выбор сетей") : t("Picking a model", "Выбор модели");
    case "waiting": return t("Waiting for your Confirm", "Ждёт твоего «Подтвердить»");
    case "done": return t("Done", "Готово");
    default: return s.model.connected ? t("Waiting for you", "Жду тебя") : t("No model: plain commands only", "Без модели: только простые команды");
  }
}

// MARK: The conversation

function conversation(s) {
  const items = [];
  s.chat.forEach((m, i) => {
    // A move's steps as it ran: one card; the reply right after it is what the move came to.
    if (m.steps) { items.push(stepsCard(m.steps, false)); return; }
    const el = m.fromUser ? userMessage(m) : reply(m, i > 0 && !!s.chat[i - 1].steps);
    if (m.id) el.dataset.msg = m.id;
    items.push(el);
    // The cards a reply offered stay under it: the last reply's live, older ones folded with their pick.
    if (!m.fromUser && m.options?.length) items.push(answers(m));
  });
  if (s.running) items.push(stepsCard(s.running.lines.length ? s.running.lines : [s.running.title], true));
  const last = s.chat.length - 1;
  if (s.thinking && (last < 0 || s.chat[last].fromUser)) {
    items.push(s.draft ? reply({ fromUser: false, text: s.draft, draft: true }) : thinkingDots(s.agentStep));
  }
  // Cards offered with no reply above them (the start's).
  if (s.options.length && !s.optionsMessage && !s.thinking) items.push(answers(null));
  if (!s.chat.length && !s.thinking) items.push(...ideas());
  return items;
}

/** Yours: an orange outline, no fill; its time beside it, at the bubble's foot; what went with it as chips over it. */
function userMessage(m) {
  const chips = m.contexts?.length ? m.contexts : m.context ? [{ title: m.context.split(":")[0].replace(/[«»]/g, ""), about: m.context, bare: false }] : [];
  return h("div", { class: "chat-msg-user" },
    chips.length ? h("div", { class: "chat-context-row" }, h("span", { class: "chat-context-label" }, icon("paperclip", 11), t("Context", "Контекст")), chips.map(sentContext)) : null,
    h("div", { class: "chat-msg-user-row" },
      m.at ? h("span", { class: "chat-msg-time num" }, stamp(m.at)) : null,
      h("div", { class: "chat-bubble selectable" }, mentions(m.text))));
}

/** One block a message went with: a chip; a click shows the words it went as, and says so when only its name went. */
function sentContext(c) {
  const chip = h("button", { class: "chat-context-chip", title: t("What the model got with this message", "Что модель получила с этим сообщением"),
    onclick: (e) => { e.stopPropagation(); popover(chip, h("div", { class: "chat-pop-context" },
      h("div", { class: "eyebrow" }, t("Sent to the model", "Ушло модели")), h("div", { class: "selectable" }, c.about),
      c.bare ? h("div", { class: "chat-bare" }, t("Only the block's name went: its numbers don't reach the model yet.", "Ушло только название блока: его цифры модели пока не передаются.")) : null)); } },
  icon("square.on.square", 11), h("span", { class: "one-line" }, c.title), icon(c.bare ? "exclamationmark.circle" : "chevron.down", 9));
  if (c.bare) chip.classList.add("chat-bare-chip");
  return chip;
}

/** «$SOL», «$<mint>», «$0x…» as the token's chip, as the field had them; «$5» stays dollars (the Mac's TokenMentions). */
const mentionRe = /(^|[\s(«"'])\$(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9]{0,11})(?=$|[\s,.;:!?)»"'])/g;
function mentions(text) {
  const out = [];
  let at = 0, m;
  mentionRe.lastIndex = 0;
  while ((m = mentionRe.exec(text))) {
    const start = m.index + m[1].length;
    out.push(text.slice(at, start), tokenChip(m[2]));
    at = mentionRe.lastIndex;
  }
  out.push(text.slice(at));
  return out;
}

function tokenChip(token) {
  const address = /^(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44})$/.test(token);
  const found = address ? marketOf(token) : null;
  if (address) lookUp(token);
  const label = address ? found?.symbol || `${token.slice(0, 5)}…${token.slice(-4)}` : token.toUpperCase();
  const held = !address && heldRow(token.toUpperCase());
  return h("span", { class: "chat-token-chip", title: address ? token : label }, logo(address ? (found?.symbol || "?") : token, 14, found?.image || held?.icon, !address), label);
}

/** The field's «$TOKEN» mentions as chips on the layer under it: the same text, glyph for glyph, so each chip sits on its
 *  letters and the caret stays where the textarea puts it; a pill behind «$TICKER» and the token's logo over the «$». */
function drawMirror(field) {
  const mirror = field?.previousElementSibling;
  if (!mirror?.classList.contains("chat-mirror")) return;
  const text = field.value, parts = [];
  let at = 0, m;
  mentionRe.lastIndex = 0;
  while ((m = mentionRe.exec(text))) {
    const start = m.index + m[1].length;
    parts.push(text.slice(at, start), fieldToken(m[2]));
    at = mentionRe.lastIndex;
  }
  field.classList.toggle("chat-mirrored", parts.length > 0);
  if (!parts.length) { mirror.replaceChildren(); return; }
  // A last line break keeps the last line's height, as the textarea has it.
  parts.push(text.slice(at) + "\n");
  mirror.replaceChildren(...parts);
  mirror.scrollTop = field.scrollTop;
}

function fieldToken(token) {
  const address = /^(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44})$/.test(token);
  // A pasted contract is looked up at once, as the Mac's field does, and its market's logo goes over the «$» (the
  // symbol itself can't replace the address here: this layer must match the field glyph for glyph).
  const found = address ? marketOf(token) : null;
  if (address) lookUp(token);
  const held = !address && heldRow(token.toUpperCase());
  const mark = logo(address ? (found?.symbol || "?") : token, 13, found?.image || held?.icon, !address);
  return h("span", { class: "chat-field-token" }, h("span", { class: "chat-field-dollar" }, "$", mark), token);
}

/** «Claude Opus 5.5» from «claude-opus-5-5», «GPT-5.4» from «gpt-5.4». */
export function modelName(id) {
  if (!id) return "";
  if (/\s/.test(id)) return id;
  // The engine's names first (LLMCatalog.displayName, as the Mac shows them).
  for (const p of ui.state?.providers || []) if (p.names?.[id]) return p.names[id];
  // The same rule here, for a model the engine hasn't named yet: «gpt-5.4» → «GPT 5.4», «qwen3:8b» → «Qwen3 8B».
  const words = [];
  for (const part of id.split("/").pop().split(/[-_: ]/).filter(Boolean)) {
    const last = words[words.length - 1];
    if (/^\d+$/.test(part) && last && /^[\d.]+$/.test(last)) words[words.length - 1] = last + "." + part;
    else if (part === "deepseek") words.push("DeepSeek");
    else if (part.length <= 3 && /^[a-z]+$/i.test(part) && !["pro", "max"].includes(part)) words.push(part.toUpperCase());
    else if (/^[\d.]*b$/.test(part)) words.push(part.toUpperCase());
    else words.push(part[0].toUpperCase() + part.slice(1));
  }
  return words.join(" ");
}

/** A reply's note as its header says it (ChatText.header): the model by name, or where a move ran — then a result card. */
function header(note) {
  if (!note) return null;
  const first = (x) => x.charAt(0).toUpperCase() + x.slice(1);
  const parts = note.split(" · ");
  if (note.startsWith("understood as ")) {
    const cut = note.lastIndexOf(" · ");
    if (cut > 0) return { label: modelName(note.slice(cut + 3)), detail: first(note.slice(0, cut)), result: false };
  }
  if (note === "from your portfolio and history") return { label: first(note), result: false };
  if (parts.length > 1 && (parts[1].startsWith("looked at ") || parts[1] === "from your portfolio and history")) {
    return { label: modelName(parts[0]), detail: first(parts.slice(1).join(" · ")), result: false };
  }
  // A bare model id («claude-opus-5-5», «qwen3:8b»): no spaces and a digit; places and protocols have neither.
  if (!/\s/.test(note) && /\d/.test(note)) return { label: modelName(note), result: false };
  return { label: first(note), result: true };
}

// ponytail: the Mac marks a move's outcome by the steps before it; here a result card gets the check unless its note is
// one of these, which say something without a move behind it.
const quietNotes = new Set(["guard", "guard: sends go to saved addresses", "Confirm is a button", "schedule", "price alert", "browser", "incoming", "from the route"]);

/** A service's raw JSON after the words (MPP.splitData): folded into a card, not a wall of braces. */
function splitData(text) {
  const m = text.match(/\n\s*\n(?=\s*[{[]\s*\n?\s*")/);
  if (!m) return [text, null];
  const data = text.slice(m.index + m[0].length).trim();
  if (data.length <= 40 || !(data.includes('":') || data.includes('" :'))) return [text, null];
  return [text.slice(0, m.index).trim(), data];
}

/** «✓ Market: …», «✗ Stop: not set — …»: a plan checked item by item, three marked lines at least (the Mac's Checklist). */
function checklist(text) {
  const lines = text.split("\n");
  const mark = (l) => (/^[✓✗⚠·] /.test(l) ? l[0] : null);
  const first = lines.findIndex((l) => mark(l));
  if (first < 0) return null;
  let last = lines.length - 1;
  while (last > first && !mark(lines[last])) last--;
  const marked = lines.slice(first, last + 1).filter(mark);
  if (marked.length < 3) return null;
  return { intro: lines.slice(0, first).join("\n").trim(), items: marked.map((l) => ({ mark: l[0], text: l.slice(2) })), next: lines.slice(last + 1).join("\n").trim() };
}

/** One answer: a header line, then its words; a result keeps a card, a failure a red one, a checklist its own (Reply). */
function reply(m, outcome = false) {
  const head = header(m.note);
  const emoji = (m.text.match(/^(\p{Extended_Pictographic}️?)\s*/u) || [])[1];
  const full = emoji ? m.text.slice(m.text.indexOf(emoji) + emoji.length).trimStart() : m.text;
  const [text, data] = splitData(full);
  const failed = text.startsWith("Failed: ");
  const list = failed ? null : checklist(text);
  const boxed = !failed && !list && (outcome || emoji || head?.result);
  const done = boxed && (outcome || (!emoji && !quietNotes.has(m.note)));
  let body;
  if (failed) {
    body = h("div", { class: "chat-reply-failed" }, icon("xmark.octagon", 15), h("div", { class: "chat-words" }, words(text.slice(8))));
  } else if (list) {
    body = checklistCard(list);
  } else if (done || emoji) {
    body = h("div", { class: "chat-reply-row" },
      h("span", { class: "chat-reply-tile" + (done ? " chat-done" : "") }, done ? icon("check", 14) : emoji),
      h("div", { class: "chat-words" }, words(done && emoji ? `${emoji} ${text}` : text)));
  } else {
    body = h("div", { class: "chat-words" }, words(text));
  }
  return h("div", { class: "chat-reply" + (boxed ? " chat-boxed" : "") + (failed ? " chat-failed" : "") + (m.draft ? " chat-draft" : "") },
    head || m.at ? h("div", { class: "chat-reply-head" + (m.note === "incoming" ? " chat-incoming" : ""), title: head?.detail || "" },
      mascot(14), m.note === "incoming" ? icon("arrow.down.circle", 12) : null,
      head ? h("span", { class: "one-line" }, head.label) : null,
      m.at ? h("span", { class: "num" }, (head ? "· " : "") + stamp(m.at)) : null) : null,
    body,
    data ? dataCard(data) : null,
    m.chart ? sparkCard(m.chart) : null);
}

function checklistCard(list) {
  const marks = { "✓": ["check", "chat-ok", "OK"], "✗": ["xmark", "chat-no", t("Missing or blocked", "Нет или заблокировано")],
    "⚠": ["exclamationmark.triangle", "chat-warn", t("Warning", "Предупреждение")], "·": ["circle.dotted", "chat-opt", t("Optional", "Необязательно")] };
  return h("div", { class: "chat-checklist" },
    list.intro ? h("div", { class: "chat-words" }, words(list.intro)) : null,
    h("div", { class: "chat-checklist-box" }, list.items.map((it) => {
      const [ico, cls, label] = marks[it.mark];
      return h("div", { class: "chat-check-row" }, h("span", { class: "chat-check-mark " + cls, "aria-label": label }, icon(ico, 12)), h("span", { class: "selectable" }, rich(it.text)));
    })),
    list.next ? h("div", { class: "chat-check-next selectable" }, rich(list.next)) : null);
}

/** What a service's JSON found (titles with their sites, or its first values), and the JSON itself behind «Show JSON». */
function dataCard(json) {
  const values = (keys) => [...json.matchAll(new RegExp(`"(?:${keys})"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)"`, "g"))].map((m) => m[1].replace(/\\(.)/g, "$1")).filter(Boolean);
  const titles = values("title|name|headline"), urls = values("url|link").filter((u) => u.startsWith("http"));
  const rows = titles.length ? titles.slice(0, 4).map((title, i) => ({ title, url: urls[i] }))
    : [...json.matchAll(/"(\w+)"\s*:\s*"((?:[^"\\]|\\.)+)"/g)].slice(0, 4).map((m) => ({ title: `${m[1]}: ${m[2]}` }));
  const raw = h("pre", { class: "chat-data-json selectable", hidden: true }, json);
  const toggle = h("button", { class: "link-btn", title: t("The service's answer as it came", "Ответ сервиса как он пришёл"),
    onclick: () => { raw.hidden = !raw.hidden; toggle.lastChild.textContent = raw.hidden ? t("Show JSON", "Показать JSON") : t("Hide JSON", "Скрыть JSON"); } },
  icon("chevron.down", 9), h("span", {}, t("Show JSON", "Показать JSON")));
  return h("div", { class: "chat-data-card" },
    h("div", { class: "chat-data-head" }, icon("curlybraces", 11), h("span", { class: "eyebrow" }, t("Service answer", "Ответ сервиса")), h("span", { class: "spacer" }), toggle),
    rows.map((r) => h("div", { class: "chat-data-row" }, r.url ? [link(r.url, r.title), h("div", { class: "small chat-data-host" }, hostOf(r.url))] : h("div", { class: "small" }, r.title))),
    raw);
}

function hostOf(url) { try { return new URL(url).hostname; } catch { return url; } }

/** «14:57» today, «Oct 16, 22:38» before. */
function stamp(iso) {
  const d = new Date(iso), now = new Date();
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  return d.toDateString() === now.toDateString() ? time : d.toLocaleDateString(undefined, { month: "short", day: "numeric" }) + ", " + time;
}

/** The words as the model laid them out: paragraphs apart, «- » and «1. » lines as a list, «## Title» bold. */
function words(text) {
  const blocks = [];
  let para = [];
  const flush = () => { if (para.length) { blocks.push({ text: para.join("\n") }); para = []; } };
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line) { flush(); continue; }
    const item = line.match(/^(?:[-•*]|(\d{1,2})[.)])\s+(.+)$/);
    const title = line.match(/^#{1,4}\s+(.+)$/);
    if (item) { flush(); blocks.push({ mark: item[1] ? item[1] + "." : "•", text: item[2] }); }
    else if (title) { flush(); blocks.push({ text: "**" + title[1] + "**" }); }
    else para.push(raw);
  }
  flush();
  return blocks.map((b, i) => {
    const gap = i === 0 ? "" : b.mark && blocks[i - 1].mark ? " chat-near" : " chat-apart";
    return b.mark ? h("div", { class: "chat-para chat-list-item" + gap }, h("span", { class: "chat-list-mark num" }, b.mark), h("span", { class: "selectable" }, rich(b.text)))
      : h("div", { class: "chat-para selectable" + gap }, rich(b.text));
  });
}

/** A held or favourite token's ticker by its contract, as the Mac's ChatText.tokenName: «AI 0x2E8c…1e18». */
function tokenName(contract) {
  const c = contract.toLowerCase();
  const fav = (ui.state.favorites || []).find((f) => (f.address || "").toLowerCase() === c);
  if (fav?.symbol) return fav.symbol;
  for (const n of ui.state.portfolio?.networks || []) for (const r of n.rows) if ((r.contract || "").toLowerCase() === c && r.symbol) return r.symbol;
  return null;
}

/** «Solscan ↗» from a link's host, as the Mac names a bare link. */
function hostName(url) {
  let name = url;
  try { const labels = new URL(url).hostname.split("."); name = labels.length >= 2 ? labels[labels.length - 2] : labels[0]; } catch {}
  return name.charAt(0).toUpperCase() + name.slice(1) + " ↗";
}

/** Inline marks as text nodes, as the Mac's inline markdown reads them: `code`, [title](link), **bold**, *italics*,
 *  bare links as «Solscan ↗», long hex short (a known token's ticker before it), amounts bold, signed numbers colored. */
function rich(text) {
  const out = [];
  const re = /`([^`]+)`|\[([^\]\n]+)\]\((https?:\/\/[^)\s]+)\)|\*\*([^*]+)\*\*|(?<![\w*])\*(?![\s*])([^*\n]+?)(?<!\s)\*(?![\w*])|(?<![\w_])_(?![\s_])([^_\n]+?)(?<!\s)_(?![\w_])|(https?:\/\/[^\s]+[^\s.,;:)])|(0x[0-9a-fA-F]{40,64})|([+−-]\$?\d[\d,]*(?:\.\d+)?%?)|(\$\d[\d,]*(?:\.\d+)?|\d[\d,]*(?:\.\d+)? [A-Z]{2,6}\b)/g;
  let at = 0, m;
  while ((m = re.exec(text))) {
    const before = text.slice(at, m.index);
    if (m[9]) {
      // Signed numbers start the text or follow a space or «(»: «+25», not «gpt-6»; «−15.5% away» is a distance.
      const prev = text[m.index - 1];
      if ((m.index > 0 && !" (".includes(prev)) || text.slice(re.lastIndex).startsWith(" away")) continue;
    }
    out.push(before);
    if (m[1]) out.push(h("code", {}, m[1]));
    else if (m[2]) out.push(link(m[3], m[2]));
    else if (m[4]) out.push(h("b", {}, m[4]));
    else if (m[5] || m[6]) out.push(h("i", {}, m[5] || m[6]));
    else if (m[7]) out.push(link(m[7], hostName(m[7])));
    else if (m[8]) {
      const code = h("code", { title: m[8] }, m[8].slice(0, 6) + "…" + m[8].slice(-4));
      const symbol = m[8].length === 42 ? tokenName(m[8]) : null;
      out.push(symbol ? symbol + " " : "", code);
    } else if (m[9]) out.push(h("span", { class: m[9].startsWith("+") ? "profit" : "loss" }, m[9]));
    else out.push(h("b", {}, m[10]));
    at = re.lastIndex;
  }
  out.push(text.slice(at));
  return out;
}

/** A market's closes as a line, drawn from numbers only. */
function sparkCard(c) {
  const w = 300, hgt = 90, pts = c.closes || [];
  const min = Math.min(...pts), max = Math.max(...pts), span = max - min || 1;
  const down = String(c.change).startsWith("-") || String(c.change).startsWith("−");
  const line = svg("polyline", { points: pts.map((p, i) => `${(i / (pts.length - 1 || 1)) * w},${hgt - 4 - ((p - min) / span) * (hgt - 8)}`).join(" "),
    class: down ? "chat-fall" : "chat-rise", fill: "none", "stroke-width": "2", "vector-effect": "non-scaling-stroke", "stroke-linejoin": "round" });
  return h("div", { class: "chat-spark-card" },
    h("div", { class: "chat-spark-head" }, h("b", {}, c.title), h("span", { class: "num " + (down ? "loss" : "profit") }, c.change)),
    svg("svg", { viewBox: `0 0 ${w} ${hgt}`, preserveAspectRatio: "none", class: "chat-spark" }, line),
    h("div", { class: "small secondary num" }, `${t("low", "мин")} ${c.low} · ${t("high", "макс")} ${c.high}`));
}

/** A move's steps as a checklist with a rail (StepsCard): «Executing» with a spinner on the last while it runs, then
 *  «Executed · N steps», kept in the chat. */
function stepsCard(lines, running) {
  const count = lines.length;
  const done = t(`Executed · ${count} ${count === 1 ? "step" : "steps"}`, `Выполнено · шагов: ${count}`);
  return h("div", { class: "chat-steps-card" },
    h("div", { class: "chat-steps-head" }, icon("bolt", 11), running ? t("Executing", "Выполняется") : done),
    lines.map((raw, i) => {
      const line = raw.replace(": done.", "");
      const last = i === lines.length - 1;
      const failed = /failed|refused|не вышло|отказ/i.test(raw);
      const cut = line.indexOf(". ");
      return h("div", { class: "chat-step-line" },
        h("div", { class: "chat-step-rail" }, running && last ? h("span", { class: "chat-spinner" }) : h("span", { class: "chat-step-dot" + (failed ? " chat-failed" : "") }, icon(failed ? "xmark" : "check", 9)),
          last ? null : h("span", { class: "chat-step-track" })),
        h("div", { class: "chat-step-text selectable" }, cut > 0 ? [h("div", {}, rich(line.slice(0, cut))), h("div", { class: "small secondary", title: line.slice(cut + 2) }, line.slice(cut + 2))] : rich(line)));
    }));
}

/** Three dots breathing where the reply will appear, with what the model is looking up now («Checking gas…»). */
function thinkingDots(step) {
  return h("div", { class: "chat-dots-row" },
    h("div", { class: "chat-dots", "aria-label": t("The assistant is thinking", "Ассистент думает") }, h("span"), h("span"), h("span")),
    step ? h("span", { class: "chat-dots-step one-line" }, step + "…") : null);
}

// MARK: Ways to go on

/** Folded or opened by hand, by the reply they came with: true folded. Otherwise the live set is open and older ones folded. */
const folds = new Map();
/** The card ↑↓ from an empty field moved to; Return picks it. */
let cardKey = null;
const LIVE_FOR = 30 * 60 * 1000;

/** NextSteps.icon, read from the card's words: the engine sends no command with them. */
function optionIcon(title) {
  const c = (title || "").toLowerCase();
  return !c ? "text.bubble" : c.startsWith("jupiter dca") || c.includes(" every ") ? "calendar.badge.clock" : c.startsWith("chart") ? "chart.xyaxis.line"
    : c.startsWith("watch") ? "star" : c.startsWith("alert") ? "bell" : c.startsWith("buy") ? "cart" : c.startsWith("sell") ? "arrow.down.circle"
    : c.startsWith("long") ? "arrow.up.right" : c.startsWith("short") ? "arrow.down.right" : c.startsWith("withdraw") || c.startsWith("unstake") ? "arrow.uturn.left"
    : c.startsWith("move") || c.startsWith("send") ? "arrow.left.arrow.right" : c.startsWith("where") ? "percent"
    : /lend|deposit|stake|spread|rebalance/.test(c) ? "building.columns" : c.startsWith("what") ? "flame" : c.startsWith("profit") ? "dollarsign.circle"
    : c.startsWith("pay") ? "creditcard" : "arrow.right.circle";
}

/** The reply whose cards just came, at the top of the chat, only while the chat follows its end (scrolled up, it stays). */
let cardsShownFor;
function replyInView(id) {
  const list = document.querySelector(".chat-scroll"), el = list?.querySelector(`[data-msg="${CSS.escape(id)}"]`);
  if (!el || list.scrollHeight - list.scrollTop - list.clientHeight > 40) return;
  list.scrollTop += el.getBoundingClientRect().top - list.getBoundingClientRect().top;
}

/** The live set: the last reply's cards, under 30 minutes old, while nothing is under way. */
function liveMessage() {
  const s = ui.state;
  if (!s.options.length || s.thinking || !s.optionsMessage) return null;
  const m = s.chat.find((x) => x.id === s.optionsMessage);
  return m && Date.now() - new Date(m.at).getTime() < LIVE_FOR ? m : null;
}

/** The cards a reply offered, under its words (Answers + OptionCard): a tap carries one out while they're the live ones; old
 *  ones fold to one line, the pick staying in view with its check; any set folds or opens by hand. `m` null: cards offered
 *  with no reply above them. */
function answers(m) {
  const s = ui.state;
  const key = m ? m.id : "start";
  const live = m ? liveMessage()?.id === m.id : !s.thinking;
  const options = m ? m.options : s.options;
  const picked = m?.picked ?? null;
  const folded = folds.has(key) ? folds.get(key) : !live;
  const shown = folded ? (picked !== null && options[picked] ? [picked] : []) : options.map((_, i) => i);
  if (live && !folded && !shown.length) return null;
  return h("div", { class: "chat-answers" },
    shown.map((i) => {
      const o = options[i];
      const card = h("button", {
        class: "chat-option" + (o.recommended && cardKey === null && live ? " chat-rec" : "") + (live && cardKey === i ? " chat-keyed" : "")
          + (live ? "" : " chat-past") + (picked === i ? " chat-picked" : ""),
        disabled: live && !!s.runningBoard, tabindex: live ? null : "-1",
        // On hover, what it runs (the Mac's .help(o.command)); a card that answers in words says so.
        title: o.command ? o.command : o.command === "" ? t("Answer with this", "Ответить этим") : o.comment || o.title,
        onclick: () => { if (!live) return; cardKey = null; act("pick", { index: i }); },
      },
      h("span", { class: "chat-option-icon" }, icon(picked === i ? "check" : optionIcon(o.title), 13)),
      h("span", { class: "chat-option-text" },
        h("span", { class: "chat-option-title" }, o.title, o.recommended && picked === null ? h("span", { class: "chat-rec-chip" }, t("Recommended", "Рекомендую")) : null),
        o.comment ? h("span", { class: "chat-option-comment" }, o.comment) : null));
      return card;
    }),
    shown.length < options.length || !folded ? h("div", { class: "chat-answers-foot" },
      h("button", { class: "link-btn chat-fold", onclick: () => { folds.set(key, !folded); render(); },
        title: live ? t("Fold the cards; they stay pickable when opened", "Свернуть варианты; раскрытые снова можно выбрать")
          : t("Offered earlier: a newer message or 30 minutes have passed, so their numbers may be old", "Предлагались раньше: был новый вопрос или прошло 30 минут, цифры в них могли устареть") },
        !folded ? t("Fold", "Свернуть") : picked === null ? t(`Options: ${options.length}`, `Варианты: ${options.length}`) : t(`Other options: ${options.length - 1}`, `Другие варианты: ${options.length - 1}`),
        icon(folded ? "chevron.down" : "chevron.up", 10)),
      live && !folded ? h("span", { class: "small secondary" }, t("Or write your own. ↑ ↓ to choose, ⏎ to answer.", "Или напиши свой вариант. ↑ ↓ — выбрать, ⏎ — ответить.")) : null) : null);
}

// A live set folds by itself at 30 minutes, the chat otherwise untouched.
setInterval(() => { if (ui.state?.optionsMessage) render(); }, 60 * 1000);

// The start screen's ideas: four of the plan templates at a time (TemplatesPanel), a different four each launch.
const templates = [
  ["Trade by the book", "Сделка по правилам", "long ETH by the book", "chart.line.uptrend", "long ETH with a stop", "лонг ETH со стопом", ["#9945FF", "#4615C8", "#072723"]],
  ["Short on a pullback", "Шорт на откате", "short oil on a 2% pullback by the book", "arrow.down.right", "short oil on a bounce", "шорт нефти на отскоке", ["#072723", "#97FCE4"]],
  ["Best rates", "Лучшие ставки", "spread USDC across the best rates", "percent", "free USDC where it earns most", "свободные USDC туда, где больше платят", ["#2775CA", "#131D27", "#082A56"]],
  ["Where to keep it", "Где держать", "where to keep USDC", "building.columns", "compare where USDC earns", "сравнить, где USDC приносит доход", ["#2775CA", "#C7F284"]],
  ["Every week", "Каждую неделю", "buy SOL for 2 USDC every week", "calendar", "a small SOL buy every week", "небольшая покупка SOL каждую неделю", ["#2775CA", "#9945FF"]],
  ["Back to Solana", "Обратно в Solana", "withdraw 1 USDC to Solana", "arrow.uturn.left", "1 USDC home from Hyperliquid", "1 USDC домой с Hyperliquid", ["#072723", "#4615C8", "#9945FF"]],
  ["Protect a position", "Защитить позицию", "move stop to breakeven", "shield", "move your stop to the entry", "стоп на точку входа", ["#072723", "#F59E0B"]],
  ["Stake SOL", "Застейкать SOL", "stake 0.01 SOL", "leaf", "earn the staking yield on SOL", "доход от стейкинга SOL", ["#9945FF", "#1F9D6A"]],
  ["Aave on Base", "Aave на Base", "lend 2 USDC on Aave", "banknote", "lend 2 USDC on Aave", "дать 2 USDC в долг на Aave", ["#2775CA", "#0052FF", "#B6509E"]],
  ["Price alert", "Ценовое оповещение", "alert me when ETH is below 2,500", "bell", "a ping when ETH drops below 2,500", "сигнал, когда ETH упадёт ниже 2 500", ["#627EEA", "#F59E0B"]],
  ["Buy a stock token", "Купить акцию-токен", "buy SpaceX stock for 2 USDC", "building.2", "SpaceX stock for 2 USDC", "акция SpaceX на 2 USDC", ["#2775CA", "#4615C8", "#CCFF00"]],
  ["Profit and loss", "Прибыль и убыток", "profit and loss week", "chart.bar", "what you made or lost this week", "сколько заработал или потерял за неделю", ["#0B7A55", "#B42318"]],
  ["What's moving", "Что движется", "what's moving", "flame", "the biggest movers right now", "самые сильные движения сейчас", ["#F59E0B", "#072723"]],
  ["Chart a coin", "График монеты", "chart SOL week", "chart.xyaxis.line", "SOL's last week on a chart", "неделя SOL на графике", ["#9945FF", "#072723"]],
  ["Buy a memecoin", "Купить мемкоин", "buy BONK on solana for 1 USDC", "pawprint", "BONK for 1 USDC on Solana", "BONK на 1 USDC в Solana", ["#2775CA", "#F7931A"]],
  ["Swap on Base", "Обмен на Base", "swap 2 USDC for ETH on base", "arrow.left.arrow.right", "USDC for ETH on Uniswap", "USDC на ETH через Uniswap", ["#2775CA", "#0052FF", "#627EEA"]],
  ["Move to Base", "Перевести на Base", "move 2 USDC from Solana to Base", "arrow.right.circle", "2 USDC over Relay in seconds", "2 USDC через Relay за секунды", ["#9945FF", "#2775CA", "#0052FF"]],
  ["Sweep dust", "Собрать пыль", "sweep dust", "sparkles", "small leftovers back to USDC", "мелкие остатки обратно в USDC", ["#F59E0B", "#2775CA"]],
  ["Claim rewards", "Забрать награды", "claim rewards", "gift", "what Merkl owes your wallet", "что Merkl должен твоему кошельку", ["#1F9D6A", "#0052FF"]],
  ["Hyperliquid spot", "Спот на Hyperliquid", "buy HYPE on hyperliquid spot for 10 USDC", "drop", "HYPE itself, no leverage", "сам HYPE, без плеча", ["#072723", "#97FCE4"]],
  ["Trade gold", "Торговать золотом", "long gold by the book", "medal", "gold with stop and take set", "золото со стопом и тейком", ["#D4AF37", "#072723"]],
  ["Make a multisig", "Сделать мультисиг", "make a multisig", "key", "a vault that needs several keys", "сейф, которому нужно несколько ключей", ["#0052FF", "#072723"]],
];
let ideasStart = Math.floor(Math.random() * templates.length);

/** A template's steps as tooltip lines, «Bridge: Relay or 1Click — the cheaper», read off trade.js's scheme (its table isn't exported). */
function templateSteps(en, ru, colors) {
  const lines = [...templatePreview(en, ru, colors).querySelectorAll(".plan-tpl-text")].map((e) => e.firstChild.textContent + ": " + e.lastChild.textContent);
  return lines.length ? "\n\n" + lines.join("\n") : "";
}

function ideas() {
  const four = [0, 1, 2, 3].map((i) => templates[(ideasStart + i) % templates.length]);
  return [
    h("div", { class: "chat-intro" }, t("Your portfolio is on the canvas. Tell me what to do with your money — in any language — and I’ll build the plan next to it. Nothing moves without your signature.",
      "Твой портфель — на холсте. Скажи, что сделать с деньгами, на любом языке, — я построю план рядом. Без твоей подписи ничего не двигается.")),
    h("div", { class: "chat-ideas" },
      h("div", { class: "chat-ideas-head" }, h("button", { class: "icon-btn", title: t("Other starting points", "Другие идеи"), onclick: () => { ideasStart += 4; render(); } }, icon("arrow.clockwise", 14))),
      // The tooltip lists the scheme it builds, step by step, as the Mac's (TemplatesPanel.steps).
      four.map(([en, ru, phrase, ico, lineEn, lineRu, colors]) => h("button", { class: "chat-idea", title: t(`Puts “${phrase}” in the field to edit and send`, `Вставит «${phrase}» в поле — поправь и отправь`) + templateSteps(en, ru, colors), onclick: () => suggest(phrase) },
        h("span", { class: "chat-idea-icon" }, icon(ico, 14)),
        h("span", { class: "chat-idea-text" }, h("b", { class: "one-line" }, t(en, ru)), h("span", { class: "one-line" }, t(lineEn, lineRu))),
        icon("arrow.up.left", 12)))),
  ];
}

// MARK: The field

/** The field's text when Esc closed the «/ @ $» list: it stays closed until the text changes. */
let dismissed = null;
let pick = 0;
/** The sent message ↑ brought back into the field, by its place in the sent list. */
let recalled = null;
/** The blocks the next message goes with besides the newest (ui.attached, which the canvas lights), oldest first. */
let attachStack = [];
let lastAttached = null;
/** What DexScreener knows of an address typed after «@» or «$», by address; «pending» while it's asked. */
const lookups = new Map();
/** The «/» list (BlockCatalog), from the engine once. */
let catalogRows = null;
let modelsAsked = false;
/** The model menu's draw while it's open (redrawn with each state), and which of the chat's menus is open. */
let liveMenu = null, tuning = null, netsPop = null;
let quoteTimer = null;

/** Every block the next message is about, the newest last. Each right-click adds one; one taken out elsewhere goes. */
function attachedIds() {
  const s = ui.state;
  if (lastAttached && !ui.attached) attachStack = attachStack.filter((id) => id !== lastAttached);
  if (ui.attached && !attachStack.includes(ui.attached)) attachStack.push(ui.attached);
  lastAttached = ui.attached;
  attachStack = attachStack.filter((id) => s.boards.some((b) => b.id === id));
  return attachStack;
}

/** Whether a block is among the ones the next message goes with: for the canvas to light every one of them. */
export function isAttached(id) { return attachStack.includes(id) || ui.attached === id; }

/** «Ask in chat» on something that isn't a block — a coin, a frame — as the Mac's attach(StepContext): a chip over the
 *  field (the same title again replaces it), and the next message reaches the model with `about`, the facts it's told.
 *  `logo` is a token symbol for the chip's logo, or nothing for the bubble. The field takes the keys. */
export function attachAbout({ title, about, logo = null }) {
  if (!title || !about) return;
  const list = [...(ui.attachedAbout || [])], i = list.findIndex((a) => a.title === title);
  if (i >= 0) list[i] = { title, about, logo }; else list.push({ title, about, logo });
  ui.attachedAbout = list;
  render();
  const f = document.getElementById("composer");
  if (f) { f.focus(); f.setSelectionRange(f.value.length, f.value.length); }
}

/** One block out of the next message: the × on its chip, and the canvas's «Take out of the chat» on the one right-clicked. */
export function detach(id) {
  attachStack = attachStack.filter((x) => x !== id);
  if (ui.attached === id) ui.attached = attachStack[attachStack.length - 1] || null;
  lastAttached = ui.attached;
  render();
}

function composer(s) {
  const ids = attachedIds(), abouts = ui.attachedAbout || [], n = ids.length + abouts.length;
  const field = h("textarea", {
    id: "composer", rows: "1", spellcheck: "false",
    placeholder: s.card ? t("Ask about it, change it («make it 0.02»), or something else", "Спроси о нём, измени («сделай 0.02») или что-то другое")
      : n > 1 ? t(`Ask about these ${n}, or say what to do with them…`, `Спроси про эти ${n} или скажи, что с ними сделать…`)
      : n ? t("Ask about it, or say what to do with it…", "Спроси о нём или скажи, что с ним сделать…")
      : t("What should we do? / blocks, @ names, $ tokens", "Что сделаем? / блоки, @ имена, $ токены"),
    oninput: (e) => { ui.composerText = e.target.value; reportComposing(e.target.value); recalled = null; cardKey = null; autosize(e.target, 140); refreshSuggestions(e.target); sendLit(); drawHelpers(); },
    onkeydown: keys,
    onclick: (e) => refreshSuggestions(e.target),
    onkeyup: (e) => { if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) refreshSuggestions(e.target); },
    onscroll: (e) => { const m = e.target.previousElementSibling; if (m) m.scrollTop = e.target.scrollTop; },
  });
  field.value = ui.composerText;
  // The «/ @ $» list is the draft's at every draw (the Mac's `suggestions` in the body): a state's redraw keeps it, with
  // the row ↑↓ had lit, and a contract's rows that landed with a lookup's answer stay.
  setTimeout(() => {
    autosize(field, 140);
    drawHelpers();
    if (document.activeElement !== field) return;
    const rows = suggestionRows(field);
    pick = Math.min(pick, Math.max(0, rows.length - 1));
    drawSuggestions(rows);
  }, 0);
  const waiting = s.card, site = s.site;
  const listening = voice.on;
  return h("div", { class: "chat-composer" },
    !listening ? positionChips(s) : null,
    !listening ? planHint(s) : null,
    listening ? listeningRow() : h("div", { class: "chat-chips" }, modelChip(s), networksChip(), h("span", { class: "spacer" }), contextChip(s)),
    // Why the voice didn't work stays under the chips until the next try (the Mac's .failed line), not a passing toast.
    voice.failed && !listening ? h("div", { class: "chat-voice-failed small" }, icon("mic.slash", 12), h("span", {}, voice.failed)) : null,
    n && !listening ? h("div", { class: "chat-attached-row" }, ids.map((id) => {
      const b = s.boards.find((x) => x.id === id);
      // A route step sent to the chat goes by its name, «Step 2 · Bridge · Relay · 10 USDC» (the Mac's StepContextChip).
      const step = ui.attachedStep?.board === id ? b.steps?.[ui.attachedStep.index] : null;
      const name = step ? t(`Step ${ui.attachedStep.index + 1}`, `Шаг ${ui.attachedStep.index + 1}`) + " · " + stepLabel(step.label) + " · " + step.title : blockTitle(b) || b.title;
      return h("div", { class: "chat-attached" }, icon("square.on.square", 11), h("span", { class: "one-line" }, name),
        h("button", { class: "icon-btn", title: t("Take it out of the message", "Убрать из сообщения"), onclick: () => detach(id) }, icon("xmark", 11)));
    }), abouts.map((a) => h("div", { class: "chat-attached", title: t(`Your next message is about ${a.title}`, `Следующее сообщение — про ${a.title}`) },
      a.logo ? logo(a.logo, 13) : icon("bubble.left", 11), h("span", { class: "one-line" }, a.title),
      h("button", { class: "icon-btn", title: t("Take it out of the message", "Убрать из сообщения"), onclick: () => { ui.attachedAbout = abouts.filter((x) => x !== a); render(); } }, icon("xmark", 11))))) : null,
    h("div", { id: "chat-suggest", class: "chat-suggest-slot" }),
    h("div", { id: "chat-helper", class: "chat-helper-slot" }),
    // Without a model the bar says how to get one; plain commands still work in the field under it.
    s.model.connected || listening ? null : connectBar(s),
    listening ? voicePanel() : h("div", { class: "chat-field" + (waiting || site ? " chat-waiting" : "") },
      site ? siteBar(site) : null,
      waiting ? waitingBar(waiting) : null,
      h("div", { class: "chat-entry" },
        h("button", { class: "chat-round-btn chat-glassy", title: t("Plan templates", "Шаблоны планов"), onclick: (e) => templatesMenu(e.currentTarget) }, icon("square.grid", 15)),
        // «$SOL» drawn as its token on a layer under the field (the Mac's ComposerField): the textarea keeps the typing,
        // the caret and IME; only its letters go clear while a mention is in it.
        h("div", { class: "chat-field-wrap" }, h("div", { class: "chat-mirror", "aria-hidden": "true" }), field),
        h("button", { class: "icon-btn chat-mic", title: t("Speak instead of typing (the keyboard layout's language)", "Сказать голосом (язык раскладки)"), onclick: speak }, icon("mic", 16)),
        sendButton(s))),
  );
}

/** Send, or Stop while the model answers; a signed move can't be called back, so then it waits, saying why. */
function sendButton(s) {
  if (s.modelRunning || s.runningBoard) {
    return h("button", { id: "chat-send", class: "chat-round-btn chat-stop" + (s.runningBoard ? " chat-off" : ""), disabled: !!s.runningBoard, "aria-label": t("Stop", "Стоп"),
      title: s.runningBoard ? t("A signed move is going through: it can't be called back. The chat says when it lands.", "Подписанное действие уже идёт: его не отозвать. Чат скажет, когда оно пройдёт.")
        : t("Stop — the model stops, nothing runs (Esc)", "Стоп — модель останавливается, ничего не запускается (Esc)"),
      onclick: () => act("stopModel") }, icon("stop.fill", 12));
  }
  return h("button", { id: "chat-send", class: "chat-round-btn chat-send" + (ui.composerText.trim() ? " chat-lit" : ""), title: t("Send", "Отправить"), disabled: s.thinking, onclick: send }, icon("arrow.up", 15));
}

function sendLit() { document.getElementById("chat-send")?.classList.toggle("chat-lit", !!ui.composerText.trim()); }

/** While a move waits, the field carries it: what it is, Cancel and Confirm in one place; a priced swap counts down its
 *  15 s after the quote, then Refresh asks again (the Mac's waitingBar). */
function waitingBar(card) {
  const s = ui.state;
  const title = card.title.charAt(0).toUpperCase() + card.title.slice(1);
  const q = s.quote;
  const left = q?.state === "fresh" && q.at ? Math.min(15, Math.max(0, Math.ceil(15 - (Date.now() - new Date(q.at).getTime()) / 1000))) : 0;
  const stateNow = q?.state === "fresh" && left === 0 ? "stale" : q?.state;
  const line = s.runningBoard ? (s.signer.waiting ? t(`${presence()} is asking for you…`, `${presence()} спрашивает тебя…`) : t("Signing…", "Подписываю…"))
    : stateNow === "quoting" ? t("Getting the price…", "Узнаю цену…")
    : stateNow === "stale" ? (q.line ? q.line + " · " : "") + t("the price is over 15 s old", "цене больше 15 с")
    : stateNow === "fresh" ? `${q.line || t("Quoted", "Цена есть")} · ${left} ${t("s", "с")}`
    : t(`Its steps are on the canvas · signs after ${presence()}`, `Шаги — на холсте · подпись после ${presence()}`);
  clearInterval(quoteTimer);
  if (stateNow === "fresh") {
    quoteTimer = setInterval(() => {
      const n = Math.min(15, Math.max(0, Math.ceil(15 - (Date.now() - new Date(q.at).getTime()) / 1000)));
      const el = document.getElementById("chat-quote-line");
      if (n === 0) { clearInterval(quoteTimer); render(); return; }
      if (el) el.textContent = `${q.line || t("Quoted", "Цена есть")} · ${n} ${t("s", "с")}`;
    }, 1000);
  }
  const busy = s.thinking || !!s.runningBoard;
  const confirm = stateNow === "stale"
    ? h("button", { class: "chat-pill-accent", disabled: busy, title: t("Ask for the price again; Confirm comes back for 15 s", "Спросить цену заново; «Подтвердить» вернётся на 15 с"), onclick: () => act("requote") },
      icon("arrow.clockwise", 13), t("Refresh", "Обновить"))
    : stateNow === "quoting" ? h("span", { class: "chat-pill-quiet" }, icon("hourglass", 13), t("Quoting…", "Котирую…"))
    // A trade the second check doubts has its tick on its block (the Mac's Execute sheet); the engine refuses Confirm without it.
    : card.ticks?.length ? h("button", { class: "chat-pill-outline", title: t("Tick the second check's warning on its block first", "Сначала отметь предупреждение второй проверки на блоке"),
      onclick: () => { ui.view = "canvas"; render(); goHome(); } }, icon("eye", 13), t("Review on the canvas", "Посмотреть на холсте"))
    : h("button", { class: "chat-pill-accent", disabled: busy, title: title, onclick: () => act("confirm", { board: card.id }) }, icon("check", 13), card.confirm);
  return h("div", { class: "chat-waiting-bar" },
    h("div", { class: "chat-waiting-what" }, h("span", { class: "chat-wait-dot" }),
      h("div", {}, h("div", { class: "chat-waiting-title" }, title), h("div", { id: "chat-quote-line", class: "chat-waiting-line num" }, line))),
    h("div", { class: "chat-waiting-buttons" },
      h("button", { class: "chat-pill-outline", disabled: busy, onclick: () => act("cancel", { board: card.id }) }, t("Cancel", "Отмена")), confirm));
}

/** A site's request waiting, apart from the person's own card (the Mac's sheet for it): answered by its own id. With a
 *  risk to tick it's approved only on its block, where the ticks are. */
function siteBar(site) {
  const busy = ui.state.thinking || !!ui.state.runningBoard;
  const approve = site.ticks?.length
    ? h("button", { class: "chat-pill-outline", title: t("Each risk is ticked on its block before it can be approved", "Каждый риск отмечается на блоке, прежде чем можно одобрить"),
      // Its block is on the canvas it came to, which may not be the one open now.
      onclick: async () => {
        ui.view = "canvas";
        if (site.canvas && site.canvas !== ui.state.canvas) await act("openCanvas", { wallet: site.canvas });
        render();
        goHome();
      } }, icon("eye", 13), t("Review on the canvas", "Посмотреть на холсте"))
    : h("button", { class: "chat-pill-accent", disabled: busy, onclick: () => act("siteAnswer", { board: site.id, test: true, ticks: [] }) }, icon("check", 13), site.approve);
  return h("div", { class: "chat-waiting-bar chat-site-bar" },
    h("div", { class: "chat-waiting-what" }, h("span", { class: "chat-wait-dot" }),
      h("div", {}, h("div", { class: "chat-waiting-title" }, site.title), h("div", { class: "chat-waiting-line one-line", title: site.origin }, site.origin || site.host))),
    h("div", { class: "chat-waiting-buttons" },
      h("button", { class: "chat-pill-outline", disabled: busy, onclick: () => act("siteAnswer", { board: site.id, test: false }) }, t("Decline", "Отказать")), approve));
}

/** An open position under the chat: its usual changes, each put in the field to send (never sent by a button). */
function positionChips(s) {
  // Only while the canvas's plan is on an open position (the Mac's managesPosition): the phrase names no coin, and the
  // engine takes that position's (targetCoin), whatever else is open.
  if (!s.chipsPosition || s.card) return null;
  const chips = [["move stop to breakeven", "shield.lefthalf"], ["trail 2%", "arrow.up.right"], ["close half", "circle.lefthalf"], ["close", "xmark.circle"]];
  return h("div", { class: "chat-position-chips" }, chips.map(([phrase, ico]) =>
    h("button", { class: "chat-pos-chip" + (phrase === "close" ? " loss" : ""), title: t(`Puts “${phrase}” in the field`, `Вставит «${phrase}» в поле`), onclick: () => suggest(phrase) },
      icon(ico, 12), phrase)));
}

/** While a plan is open and not yet a position, how to change it: in words (the Mac's line under the plan, Studio.swift:2586). */
function planHint(s) {
  if (!s.plan || s.chipsPosition || s.card) return null;
  return h("div", { class: "chat-plan-hint small secondary" }, t("Edit it in words: “take like the pros”, “risk $0.20”, “entry on a 1% pullback”.",
    "Меняй словами: «тейк как у профи», «риск $0,20», «вход на откате 1%»."));
}

/** No model reads the chat yet, or the one on this PC can't answer: how to get one, or what it still needs (ConnectModelBar). */
function connectBar(s) {
  const m = s.models || {}, model = s.model.model || "", name = modelName(model);
  const way0 = s.providers.find((p) => p.id === s.model.provider);
  // Any way on this PC (Ollama, LM Studio…), as the Mac's ConnectModelBar: running, then the model downloaded.
  const local = way0?.kind === "local" && !way0.unavailable && !m.refusals?.[model];
  const down = local && m.servers?.[s.model.provider] === false;
  const ollama = s.model.provider === "ollama";
  const setup = s.localSetup || {};
  const outdated = local && ollama && setup.ollamaOutdated === true;
  // An install or update under way says how far it is, and its button waits (the Mac's ollamaInstall).
  const installing = setup.ollamaInstall || null;
  const size = m.sizes?.[model];
  const loading = m.downloads?.[model];
  const fits = (gb) => gb * 1.2 <= (m.memoryGB || 0) * 1.073741824 * 0.7;
  // A subscription or a key: Settings › AI model opens on that way, where it signs in (the Mac's way(…, id:)).
  const way = (title, file, id) => h("button", { class: "chat-way-chip", onclick: () => openSettings("model", id) }, logo(file, 20), title);
  const chip = (title, ico, run, accent = false, disabled = false) => h("button", { class: "chat-way-chip" + (accent ? " chat-accent" : ""), disabled, onclick: run }, icon(ico, 13), title);
  const latest = setup.ollamaLatest;
  let head, line, actions = [];
  if (outdated) {
    head = t(`Ollama ${setup.ollamaVersion || ""} is too old for ${name}`, `Ollama ${setup.ollamaVersion || ""} слишком старая для ${name}`);
    line = t(`Update it to ${latest ? "the newest, " + latest : "the newest"} — free, right here — then download the model.`,
      `Обнови её до ${latest ? "новейшей, " + latest : "новейшей"} — бесплатно, прямо здесь, — потом скачай модель.`);
    actions.push(chip(installing || (loading !== undefined ? t(`Downloading · ${Math.round(loading * 100)}%`, `Скачиваю · ${Math.round(loading * 100)}%`)
      : t(`Update Ollama${latest ? " to " + latest : ""} and download`, `Обновить Ollama${latest ? " до " + latest : ""} и скачать`)), "arrow.down.circle", () => act("setUpLocal"), true, !!installing));
    actions.push(chip(t("Check again", "Проверить снова"), "arrow.clockwise", () => act("refreshModels")));
  } else if (down) {
    head = ollama ? t("Ollama isn't running", "Ollama не запущена") : t(`${way0.name} isn't running`, `${way0.name} не запущена`);
    line = t("Open it to chat with a model on this PC — or connect a subscription instead.", "Открой её, чтобы общаться с моделью на этом ПК, — или подключи подписку.");
    if (ollama) actions.push(m.installed ? chip(t("Open Ollama", "Открыть Ollama"), "play.circle", () => act("startOllama"))
      : chip(installing || t(`Get Ollama${latest ? " " + latest : ""} and a model`, `Скачать Ollama${latest ? " " + latest : ""} и модель`), "arrow.down.circle", () => act("setUpLocal"), true, !!installing));
    actions.push(chip(t("Check again", "Проверить снова"), "arrow.clockwise", () => act("refreshModels")));
  } else if (local) {
    head = t(`${name} isn't on this PC yet`, `${name} пока нет на этом ПК`);
    line = size ? t(`≈ ${size.toFixed(1)} GB to download; it runs in ≈ ${Math.round(size * 1.2)} GB of this PC's ${m.memoryGB} GB`, `≈ ${size.toFixed(1)} ГБ скачать; работает в ≈ ${Math.round(size * 1.2)} ГБ из ${m.memoryGB} ГБ этого ПК`)
      + (fits(size) ? "." : t(" — too big for it: pick a smaller one or a subscription.", " — слишком большая: выбери поменьше или подписку."))
      : t("Download it, or use one this PC already has.", "Скачай её или выбери ту, что уже есть на этом ПК.");
    // Only Ollama downloads from here; LM Studio's models are fetched in LM Studio itself.
    if (ollama) {
      if (loading !== undefined) actions.push(chip(t(`Downloading · ${Math.round(loading * 100)}%`, `Скачиваю · ${Math.round(loading * 100)}%`), "arrow.down.circle", null, false, true));
      else if (!size || fits(size)) actions.push(chip(size ? t(`Download · ≈ ${size.toFixed(1)} GB`, `Скачать · ≈ ${size.toFixed(1)} ГБ`) : t("Download", "Скачать"), "arrow.down.circle", () => act("downloadModel", { model }), true));
    }
    for (const have of (m.live?.[s.model.provider] || []).filter((x) => !m.refusals?.[x]).slice(0, 2)) {
      actions.push(chip(t(`Use ${modelName(have)}`, `Взять ${modelName(have)}`), "desktop", () => act("useAccount", { provider: s.model.provider, model: have })));
    }
  } else if (m.refusals?.[model]) {
    head = t(`${name} is too small for the chat`, `${name} слишком мала для чата`);
    line = t("Pick a bigger model above, or connect a subscription.", "Выбери модель побольше или подключи подписку.");
  } else {
    head = t("Connect a model to chat", "Подключи модель для чата");
    line = t("The chat needs a model to understand you: a subscription you already pay for, an API key, or a free model on this PC. Plain commands work without one: «send 5 USDC to Mom».",
      "Чату нужна модель, чтобы понимать тебя: подписка, за которую ты уже платишь, API-ключ или бесплатная модель на этом ПК. Простые команды работают и без неё: «отправь 5 USDC маме».");
  }
  return h("div", { class: "chat-connect-bar" },
    h("b", {}, head), h("div", { class: "small secondary" }, line),
    h("div", { class: "chat-way-chips" }, actions, way("ChatGPT", "llm-openai", "chatgpt"), way("Claude", "llm-claude", "claude-code"), way("OpenRouter", "llm-openrouter", "openrouter"),
      local ? null : chip(installing || t("On this PC", "На этом ПК"), "desktop", () => act("setUpLocal"), false, !!installing)));
}

/** Settings on a tab — and, for the AI model tab, on a way the person picked (pages.js reads `ui.settingsProvider` once). */
function openSettings(tab, provider) {
  document.querySelector(".menu")?.remove();
  liveMenu = null;
  tuning = null;
  if (tab) store("settingsTab", tab);
  if (provider) ui.settingsProvider = provider;
  ui.view = "settings";
  render();
}

function keys(e) {
  const s = ui.state;
  const rows = suggestionRows(e.target);
  if (rows.length) {
    // ↑↓ stop at the ends, as the Mac's keyed(max(0, pick - 1)) and keyed(min(count - 1, pick + 1)).
    if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); pick = Math.max(0, Math.min(rows.length - 1, pick + (e.key === "ArrowDown" ? 1 : -1))); drawSuggestions(rows); return; }
    if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); choose(rows[Math.min(pick, rows.length - 1)], e.target); return; }
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); dismissed = e.target.value; drawSuggestions([]); return; }
  }
  // Esc stops the model, as the Stop button does.
  if (e.key === "Escape" && s.modelRunning) { e.preventDefault(); e.stopPropagation(); act("stopModel"); return; }
  // ↑↓ with the caret on the draft's amount: a step up or down, between none and all that's held.
  if ((e.key === "ArrowUp" || e.key === "ArrowDown") && nudge(e.target, e.key === "ArrowUp")) { e.preventDefault(); return; }
  // ↑↓ from an empty field walk the live cards under the last reply; then what was sent, as a terminal's history.
  const empty = !e.target.value;
  const live = liveMessage();
  const cards = live && folds.get(live.id) !== true ? s.options : null;
  if ((e.key === "ArrowUp" || e.key === "ArrowDown") && empty && cards) {
    e.preventDefault();
    cardKey = e.key === "ArrowUp" ? Math.max(0, (cardKey ?? cards.length) - 1) : cardKey !== null && cardKey + 1 < cards.length ? cardKey + 1 : null;
    render();
    // The highlighted card comes into view, so Return never picks one out of sight (the Mac's scrollTo on cardKey).
    document.querySelector(".chat-option.chat-keyed")?.scrollIntoView({ block: "nearest" });
    return;
  }
  if ((e.key === "ArrowUp" && (empty || recalled !== null)) || (e.key === "ArrowDown" && recalled !== null)) {
    const sent = s.sent || [];
    if (!sent.length) return;
    e.preventDefault();
    recalled = e.key === "ArrowUp" ? Math.max(0, (recalled ?? sent.length) - 1) : recalled + 1 < sent.length ? recalled + 1 : null;
    ui.composerText = recalled === null ? "" : sent[recalled];
    e.target.value = ui.composerText;
    reportComposing(ui.composerText);
    autosize(e.target, 140);
    sendLit();
    drawHelpers();
    return;
  }
  if (e.key === "Escape" && cardKey !== null) { e.stopPropagation(); cardKey = null; render(); return; }
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    if (empty && cardKey !== null && cards?.[cardKey]) { const i = cardKey; cardKey = null; act("pick", { index: i }); return; }
    send();
  }
}

function send() {
  const text = ui.composerText.trim();
  if (!text || ui.state.thinking) return;
  ui.composerText = "";
  reportComposing("");
  recalled = null;
  const ids = attachedIds().slice();
  attachStack = [];
  ui.attached = null;
  lastAttached = null;
  // A route step sent to the chat goes as that step alone (the engine's stepBoard), as the Mac's «Send to chat».
  // Only when a step was sent and it's that step's block going with the message: with nothing attached both sides are
  // undefined, and reading .index of nothing stopped every message from being sent.
  const step = ui.attachedStep && ids[0] && ui.attachedStep.board === ids[0] ? ui.attachedStep.index : undefined;
  ui.attachedStep = null;
  const abouts = (ui.attachedAbout || []).map(({ title, about }) => ({ title, about }));
  ui.attachedAbout = null;
  act("send", { text, board: ids[0], boards: ids.length > 1 ? ids : undefined, step, abouts: abouts.length ? abouts : undefined });
  render();
  // Your own message brings the end back, scrolled up or not, and the reply follows it there (the Mac's chat).
  const list = document.querySelector(".chat-scroll");
  if (list) list.scrollTop = list.scrollHeight;
}

// MARK: Over the field: the amount it says, a top-up of the automation wallet

/** «send 0.01 SOL to …», «buy SOL for 1 USDC»: each «<number> <ticker>» in the text (the Mac's DraftAmount). */
function draftAmounts(text) {
  return [...text.matchAll(/(\d+(?:\.\d+)?)\s*\$?([A-Za-z][A-Za-z0-9₮]{1,9})\b/g)].map((m) => ({ start: m.index, end: m.index + m[1].length, value: Number(m[1]), text: m[1], ticker: m[2].toUpperCase() }));
}

/** The network the draft names («on Base», «from Solana»), as a key. */
function draftPlace(text) {
  const tl = text.toLowerCase();
  const nets = ui.state.portfolio.networks || [];
  return nets.find((n) => [n.name.toLowerCase(), n.key].some((k) => tl.includes(" on " + k) || tl.includes(" from " + k)))?.key || null;
}

/** A row of the wallet's for a ticker, where most of it is (or on `place`). */
function heldRow(ticker, place) {
  let best = null;
  for (const n of ui.state.portfolio.networks || []) {
    if (place && n.key !== place) continue;
    for (const r of n.rows) if (r.section !== "defi" && r.symbol.toUpperCase() === ticker && (!best || r.value > best.row.value)) best = { row: r, net: n };
  }
  return best ? { ...best.row, net: best.net } : null;
}

/** What the draft says of a token the wallet holds: the amount, how much is held and where. */
function draftAmount() {
  const s = ui.state;
  if (s.thinking || s.card) return null;
  const text = ui.composerText, place = draftPlace(text);
  for (const a of draftAmounts(text)) {
    const r = heldRow(a.ticker, place);
    if (r && r.value > 0) return { a, held: r.value, exact: r.amount.replace(/,/g, ""), price: r.usd && r.value ? r.usd / r.value : null, place: r.net };
  }
  return null;
}

/** One step of ↑↓ and the slider: a tenth of the held amount's order — 0.01 of 0.17 SOL, 1 of 45 USDC. */
const stepScale = (held) => Math.max(0, 1 - (held > 0 ? Math.floor(Math.log10(held)) : 0));
const written = (v, scale) => v.toLocaleString("en-US", { maximumFractionDigits: scale, useGrouping: false });

/** The draft with its amount set to `text`, the caret after it. */
function setAmount(a, text) {
  const field = document.getElementById("composer");
  ui.composerText = ui.composerText.slice(0, a.start) + text + ui.composerText.slice(a.end);
  if (field) { field.value = ui.composerText; const c = a.start + text.length; field.setSelectionRange(c, c); }
  drawHelpers();
}

function nudge(el, up) {
  const d = draftAmount();
  if (!d) return false;
  const caret = el.selectionStart;
  if (caret < d.a.start || caret > d.a.end) return false;
  const scale = stepScale(d.held), step = 10 ** -scale;
  const factor = 10 ** scale;
  const next = Math.min(d.held, Math.max(0, up ? Math.floor((d.a.value + step) * factor + 1e-9) / factor : Math.ceil((d.a.value - step) * factor - 1e-9) / factor));
  setAmount(d.a, next >= d.held ? d.exact : written(next, scale));
  return true;
}

/** The draft's amount panel, or the automation top-up's balances, over the field; drawn on its own as the text changes. */
function drawHelpers() {
  drawMirror(document.getElementById("composer"));
  const slot = document.getElementById("chat-helper");
  if (!slot) return;
  // Nothing over the listening panel while dictating (the Mac's `if !voice.listening`).
  if (voice.on) { slot.replaceChildren(); return; }
  const top = topUpDraft();
  const d = top === null ? draftAmount() : null;
  slot.replaceChildren(top !== null ? topUpBalances(top) : d ? amountSlider(d) : "");
}

function amountSlider(d) {
  const scale = stepScale(d.held);
  const set = (v) => setAmount(d.a, v >= d.held ? d.exact : written(Math.floor(v * 10 ** scale) / 10 ** scale, scale));
  const range = h("input", { type: "range", min: "0", max: "1000", value: String(Math.round(Math.min(1, d.a.value / d.held) * 1000)), "aria-label": t("Amount", "Сумма"),
    oninput: (e) => set(d.held * Number(e.target.value) / 1000) });
  return h("div", { class: "chat-amount" },
    h("div", { class: "chat-amount-head" }, logo(d.a.ticker, 18),
      h("b", { class: "num" + (d.a.value > d.held ? " loss" : "") }, `${d.a.text} ${d.a.ticker}`),
      h("span", { class: "one-line secondary" }, t(`of ${written(d.held, 6)} on ${d.place.name}`, `из ${written(d.held, 6)} на ${d.place.name}`)),
      h("span", { class: "spacer" }),
      d.price ? h("span", { class: "num secondary" }, "≈ $" + (d.a.value * d.price).toFixed(2)) : null),
    h("div", { class: "chat-amount-row" }, range,
      [25, 50, 100].map((p) => h("button", { class: "chat-amount-pct", onclick: () => set(p === 100 ? d.held : d.held * p / 100) }, p === 100 ? "Max" : `${p}%`))),
    h("div", { class: "small secondary" }, t("Cursor on the number: ↑ ↓ change it · Return sends", "Курсор на числе: ↑ ↓ меняют сумму · Return — отправить")));
}

/** The draft is a top-up of the automation wallet: its amount (the Mac's topUpDraft). */
function topUpDraft() {
  const w = ui.state.automation;
  const m = w && ui.composerText.trim().match(/^send\s+(\d+(?:\.\d+)?)\s+USDC\s+to\s+(.+?)(?:\s+on\s+solana)?(?:,\s*then\s+send\s+[\d.]+\s+SOL\s+to\s+.+)?$/i);
  return m && m[2].toLowerCase() === (w.label || "").toLowerCase() ? Number(m[1]) : null;
}

/** Where the money comes from and the budget, each now → after, while the field holds a top-up (TopUpBalances). */
function topUpBalances(amount) {
  const s = ui.state;
  const solana = heldRow("USDC", "solana")?.value ?? null, budget = s.automationBudget?.total ?? null;
  const elsewhere = (s.portfolio.networks || []).filter((n) => n.key !== "solana").map((n) => ({ n, r: heldRow("USDC", n.key) })).filter((x) => x.r && x.r.value >= 0.01)
    .map((x) => `${x.n.name} ${written(x.r.value, 2)}`);
  const row = (ico, name, now, after) => h("div", { class: "chat-topup-row" }, ico, h("b", {}, name), h("span", { class: "spacer" }),
    h("span", { class: "num" + (after !== null && after < 0 ? " loss" : "") }, now === null ? "…" : `${written(now, 2)} → ${after === null ? "…" : written(Math.max(0, after), 2)} USDC`));
  return h("div", { class: "chat-amount" },
    h("div", { class: "eyebrow" }, t("Top up the automation wallet", "Пополнить кошелёк автоматизации")),
    row(logo("sol", 16), t("Your wallet · Solana", "Твой кошелёк · Solana"), solana, solana === null ? null : solana - amount),
    row(icon("clock.arrow", 14), t("Automation budget", "Бюджет автоматизации"), budget, budget === null ? null : budget + amount),
    solana !== null && solana < amount ? h("div", { class: "chat-topup-short" }, t(`Solana has ${written(solana, 2)} USDC, ${written(amount - solana, 2)} short.`, `На Solana ${written(solana, 2)} USDC, не хватает ${written(amount - solana, 2)}.`)
      + (elsewhere.length ? t(` There’s USDC on ${elsewhere.join(", ")}: move it to Solana first.`, ` Есть USDC в ${elsewhere.join(", ")}: сначала перенеси их на Solana.`) : "")) : null,
    h("div", { class: "small secondary" }, t(`Type another amount if you like, then Return. Confirm and ${presence()} come after.`, `Можно ввести другую сумму, потом Return. «Подтвердить» и ${presence()} — после.`)));
}

// MARK: «/» blocks, «@» names, «$» tokens

/** The word at the caret when it starts with «/», «@» or «$», with where it starts and ends. */
function wordAtCaret(el) {
  const at = el.selectionStart ?? el.value.length;
  const text = el.value;
  let start = at;
  while (start > 0 && !/\s/.test(text[start - 1])) start--;
  let end = at;
  while (end < text.length && !/\s/.test(text[end])) end++;
  const word = text.slice(start, end);
  return /^[/@$]/.test(word) ? { sign: word[0], word: word.slice(1), start, end, before: text.slice(0, start), after: text.slice(end) } : null;
}

/** The deepest market a looked-up contract trades on, or null while it's being looked up or none knows it. */
function marketOf(address) { const rows = lookups.get(address); return Array.isArray(rows) ? rows[0] || null : null; }

const isAddress = (x) => /^(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44})$/.test(x);

function lookUp(address) {
  if (lookups.has(address)) return;
  lookups.set(address, "pending");
  engine("lookUpContract", { address }).then((r) => {
    let rows = [];
    try { rows = JSON.parse(r?.list || "[]"); } catch {}
    lookups.set(address, rows);
    const f = document.getElementById("composer");
    if (f) { refreshSuggestions(f); drawMirror(f); }
    if (document.querySelector(".chat-token-chip")) render();
  }).catch(() => lookups.set(address, []));
}

function loadCatalog() {
  if (catalogRows) return;
  catalogRows = [];
  engine("blockCatalog").then((r) => {
    try { catalogRows = JSON.parse(r?.list || "[]"); } catch { catalogRows = []; }
    const f = document.getElementById("composer");
    if (f) refreshSuggestions(f);
  }).catch(() => { catalogRows = null; });
}

/** The «$» list's tokens, each once: SOL and USDC on Solana, the tokens shown by value, the usual ones, the favorites. */
function tokenList() {
  const s = ui.state, seen = new Set(), out = [];
  const add = (symbol, detail, image) => { if (!symbol || symbol.length > 12 || seen.has(symbol.toUpperCase())) return; seen.add(symbol.toUpperCase()); out.push({ symbol, detail, image }); };
  const sol = (s.portfolio.networks || []).find((n) => n.key === "solana");
  if (sol) for (const sym of ["SOL", "USDC"]) { const r = sol.rows.find((x) => x.symbol === sym); if (r) add(sym, `Solana · ${r.amount} ${sym}`, r.icon); }
  const rows = (s.portfolio.networks || []).flatMap((n) => n.rows.filter((r) => r.section !== "defi").map((r) => ({ r, n }))).sort((a, b) => (b.r.usd || 0) - (a.r.usd || 0));
  for (const { r, n } of rows) add(r.symbol, `${n.name} · ${r.usd !== null && r.usd !== undefined ? "$" + Number(r.usd).toFixed(2) : r.amount}`, r.icon);
  for (const [sym, name] of [["SOL", "Solana"], ["USDC", "USD Coin"], ["ETH", "Ether"], ["BTC", "Bitcoin"], ["HYPE", "Hyperliquid"], ["NEAR", "NEAR"], ["USDT", "Tether"], ["JUP", "Jupiter"]]) add(sym, name);
  for (const f of s.favorites || []) add(f.symbol, `${f.name} · ${networkName(f.chain)}`, f.image);
  return out;
}

function suggestionRows(el) {
  if (!el || el.value === dismissed) return [];
  const w = wordAtCaret(el);
  if (!w) return [];
  const s = ui.state;
  const q = w.word.toLowerCase();
  // A pick replaces the word; the rest of the text stays, the caret after what went in.
  const row = (r, put, space = false) => ({ ...r, put, space });
  if (w.sign === "/") {
    loadCatalog();
    return (catalogRows || []).filter((b) => !q || `${b.title} ${b.group} ${b.example}`.toLowerCase().includes(q))
      .map((b) => row({ icon: "square.grid", logo: b.logo || null, title: b.title, detail: b.example }, b.example));
  }
  // A contract or mint after «@» or «$»: one row per network it trades on.
  if (isAddress(w.word)) {
    const dollar = w.sign === "$", put = (dollar ? "$" : "") + w.word, short = `${w.word.slice(0, 6)}…${w.word.slice(-4)}`;
    lookUp(w.word);
    const found = lookups.get(w.word);
    if (found === "pending") return [row({ icon: "magnifyingglass", title: short, detail: t("looking it up…", "ищу…") }, put, dollar)];
    if (!found.length) return [row({ icon: "questionmark.circle", title: short, detail: t("no market knows it", "ни один рынок его не знает") }, put, dollar)];
    // One row per network, the deepest pool first (the engine's order), the network by its name.
    const seen = new Set();
    return found.filter((f) => !seen.has(f.chain) && seen.add(f.chain)).map((f) => row({ icon: "circle.dotted", pic: f.image || null, logo: f.image ? null : networkOf(f.chain)?.logo || null, title: f.symbol,
      detail: [f.name, networkName(f.chain), f.price, f.liquidity ? t(`liquidity ${f.liquidity}`, `ликвидность ${f.liquidity}`) : null].filter(Boolean).join(" · ") }, (dollar ? "$" : "") + f.address, dollar));
  }
  // «$» lists the tokens; «$5» is dollars.
  if (w.sign === "$") {
    if (/^\d/.test(w.word)) return [];
    return tokenList().filter((x) => !q || x.symbol.toLowerCase().startsWith(q) || x.detail.toLowerCase().includes(q)).slice(0, 40)
      .map((x) => row({ icon: "dollarsign.circle", logo: x.symbol, src: x.image, title: x.symbol, detail: x.detail }, "$" + x.symbol, true));
  }
  // The address book with each network's logo, then the favourite tokens; all of them, as the Mac lists (it scrolls).
  const names = s.book.map((e) => row({ icon: "person", logo: networkOf(e.network)?.logo || null, title: e.name, detail: networkName(e.network) }, e.name));
  const favs = (s.favorites || []).map((f) => row({ icon: "star", pic: f.image || null, logo: f.image ? null : networkOf(f.chain)?.logo || null, title: f.symbol, detail: f.name + " · " + networkName(f.chain) }, f.symbol));
  return [...names, ...favs].filter((r) => !q || r.title.toLowerCase().includes(q));
}

/** A network by its key («base»), from Settings › Modules' list: its name and logo, «Base», as the Mac's ModuleNetwork. */
function networkOf(key) { return (ui.state.modules?.networks || []).find((n) => n.id === "network." + key) || null; }
function networkName(key) { return networkOf(key)?.name || (key ? key.charAt(0).toUpperCase() + key.slice(1) : ""); }

function refreshSuggestions(el) { pick = 0; drawSuggestions(suggestionRows(el)); }

function drawSuggestions(rows) {
  const slot = document.getElementById("chat-suggest");
  if (!slot) return;
  if (!rows.length) { slot.replaceChildren(); return; }
  const field = document.getElementById("composer");
  slot.replaceChildren(h("div", { class: "chat-suggest" },
    h("div", { class: "chat-suggest-rows" }, rows.map((r, i) => h("button", { class: "chat-suggest-row" + (i === pick ? " chat-lit" : ""), onmousedown: (e) => { e.preventDefault(); choose(r, field); },
      // The pointer over a row picks it, as ↑↓ do (the Mac's onHover).
      onmouseenter: () => { if (pick !== i) { pick = i; for (const [n, b] of [...slot.querySelectorAll(".chat-suggest-row")].entries()) b.classList.toggle("chat-lit", n === i); } } },
      // A market's picture is its own only (`pic`): never the app's logo for the symbol it claims.
      r.pic ? logo(r.title, 16, r.pic, false) : r.logo ? logo(r.logo, 16, r.src) : icon(r.icon, 13), h("b", { class: "one-line" }, r.title), h("span", { class: "one-line secondary" }, r.detail)))),
    h("div", { class: "chat-suggest-foot" }, rows.length > 8
      ? t(`${rows.length} · scroll or ↑↓ · Return puts it in · Esc closes`, `${rows.length} · прокрути или ↑↓ · Return — вставить · Esc — закрыть`)
      : t("↑↓ choose · Return puts it in · Esc closes", "↑↓ — выбрать · Return — вставить · Esc — закрыть"))));
  slot.querySelector(".chat-suggest-row.chat-lit")?.scrollIntoView({ block: "nearest" });
}

/** A pick: its text in the word's place, a space after it unless it stands alone (a «$» chip is closed by its space). */
function choose(row, el) {
  const w = wordAtCaret(el);
  if (!w) return;
  const alone = !w.before && !w.after;
  const head = w.before + row.put + (alone && !row.space ? "" : " ");
  ui.composerText = head + w.after.replace(/^ /, "");
  el.value = ui.composerText;
  el.setSelectionRange(head.length, head.length);
  autosize(el, 140);
  sendLit();
  drawSuggestions([]);
  drawHelpers();
}

/** The preview's `&call=chat.openTemplates`: the templates menu over its button. */
export function openTemplates() {
  const button = [...document.querySelectorAll(".chat-round-btn")].find((b) => b.title === t("Plan templates", "Шаблоны планов"));
  if (button) templatesMenu(button);
}

function templatesMenu(anchor) {
  // Beside the grid, the scheme of the template under the pointer (trade.js templatePreview), the first one at first.
  const preview = h("div", {}, templatePreview(templates[0][0], templates[0][1], templates[0][6]));
  popover(anchor, h("div", { class: "plan-tpl-row" }, h("div", { class: "chat-tpl-panel" },
    h("b", {}, t("Plan templates", "Шаблоны планов")),
    h("div", { class: "chat-tpl-grid" }, templates.map(([en, ru, phrase, , , , colors]) => h("button", { class: "chat-tpl", onclick: () => { document.querySelector(".menu")?.remove(); suggest(phrase); },
      onmouseenter: () => preview.replaceChildren(templatePreview(en, ru, colors)) },
      h("span", { class: "chat-tpl-colors" }, colors.map((c, i) => [i ? h("span", { class: "chat-tpl-joint" }) : null, h("span", { class: "chat-tpl-swatch", style: `background:${c}` })])),
      h("b", {}, t(en, ru)), h("span", { class: "small secondary" }, "«" + phrase + "»"))))), preview), "up");
}

// MARK: Over the field: the model, the networks, the context

const companies = [
  { name: "OpenAI", logo: "llm-openai", tile: "#000000", ink: true, ids: ["chatgpt", "openai"] },
  { name: "Anthropic", logo: "llm-claude", tile: "#F4F3EE", ids: ["claude-code", "anthropic"] },
  { name: "Google", logo: "llm-gemini", tile: "#FFFFFF", ids: ["gemini-subscription", "gemini"] },
  { name: "xAI", logo: "llm-grok", tile: "#000000", ink: true, ids: ["grok-subscription", "xai"] },
  { name: "DeepSeek", logo: "llm-deepseek", tile: "#FFFFFF", ids: ["deepseek"] },
  { name: "OpenRouter", logo: "llm-openrouter", tile: "#6566F1", ink: true, ids: ["openrouter"] },
  { name: "Ollama", logo: "llm-ollama", tile: "#FFFFFF", ids: ["ollama"], local: true },
  { name: "LM Studio", logo: "llm-lmstudio", tile: "#4338CA", ink: true, ids: ["lmstudio"], local: true },
];
function companiesNow() {
  const known = new Set(companies.flatMap((c) => c.ids));
  return companies.concat(ui.state.providers.filter((p) => !known.has(p.id)).map((p) => ({ name: p.name, tile: "#8E8E93", ids: [p.id], local: p.kind === "local" })));
}
const companyOf = (id) => companiesNow().find((c) => c.ids.includes(id));

/** The company's logo on its brand tile, like an app icon. */
function companyTile(c, size = 28) {
  const box = h("span", { class: "chat-company-tile" + (c?.ink ? " chat-ink" : ""), style: `width:${size}px;height:${size}px;border-radius:${size * 0.26}px;background:${c?.tile || "#8E8E93"}` });
  if (c?.logo) box.append(logo(c.logo, Math.round(size * 0.64)));
  else box.append(h("b", { style: `font-size:${size * 0.5}px` }, (c?.name || "?").charAt(0)));
  return box;
}

function modelChip(s) {
  const c = companyOf(s.model.provider);
  const name = s.model.model ? modelName(s.model.model) : s.model.name || t("No model", "Нет модели");
  const chip = h("button", { class: "chat-chip-btn chat-model", "data-pop": "model", title: t(`${s.model.name || ""} · ${s.model.model || ""}. The model for this canvas; it never sees your wallet key.`,
      `${s.model.name || ""} · ${s.model.model || ""}. Модель этого холста; ключ кошелька она никогда не видит.`), onclick: () => modelMenu(chip) },
    c ? companyTile(c, 20) : null, h("span", { class: "one-line" }, name), icon("chevron.down", 9));
  return chip;
}

/** Two columns, so nothing moves under the pointer: the companies, then the chosen one's ways in and models. */
function modelMenu(anchor) {
  // Read again at every draw: a download's percent, a server coming up, a sign-in — the open menu follows (the Mac's
  // rows update live); chatPanel() redraws it with each state while it's open.
  let s = ui.state, m = s.models || {};
  let shown = companyOf(s.model.provider)?.name || companies[0].name;
  const right = h("div", { class: "chat-ways-col" });
  const left = h("div", { class: "chat-companies-col" });
  const draw = () => {
    // Closed some other way (another menu, Settings): it stops redrawing, and only its own face goes.
    if (!left.isConnected && liveMenu === draw) { liveMenu = null; if (tuning === "model") tuning = null; return; }
    s = ui.state; m = s.models || {};
    const list = companiesNow();
    const row = (c) => {
      const ways = s.providers.filter((p) => c.ids.includes(p.id));
      const current = ways.find((p) => p.id === s.model.provider);
      // Under the company: the model in use, or a single way's own state, or else how to get in (the Mac's summary).
      const detail = current ? (s.model.model ? modelName(s.model.model) : current.name) + t(" · in use", " · сейчас")
        : ways.length === 1 ? wayDetail(ways[0])
        : ways.filter((p) => !p.unavailable).map((p) => (p.kind === "subscription" ? t("Subscription", "Подписка") : p.kind === "local" ? t("On this PC", "На этом ПК") : t("API key", "API-ключ"))).join(t(" or ", " или "));
      return h("button", { class: "chat-company-row" + (shown === c.name ? " chat-selected" : ""), onclick: () => { shown = c.name; draw(); } },
        companyTile(c), h("span", { class: "chat-company-text" }, h("b", {}, c.name), h("span", { class: "one-line" + (current ? " accent-ink" : "") }, detail || "—")), icon("chevron.right", 11));
    };
    left.replaceChildren(...[
      h("div", { class: "chat-caption" }, t("Model for this canvas", "Модель для этого холста")),
      list.filter((c) => !c.local).map(row),
      h("div", { class: "chat-caption" }, t("On this PC · no internet", "На этом ПК · без интернета")),
      list.filter((c) => c.local).map(row)].flat());
    const c = list.find((x) => x.name === shown) || list[0];
    right.replaceChildren(h("div", { class: "chat-caption" }, c.name), ...s.providers.filter((p) => c.ids.includes(p.id)).map(way).flat(Infinity));
  };
  // The popover's watch calls `closed` once it's gone.
  const close = () => document.querySelector(".menu")?.remove();
  // Only while it's the live one: a model menu opened in its place keeps its face.
  const closed = () => { if (liveMenu === draw) { liveMenu = null; if (tuning === "model") tuning = null; } act("refreshModels"); render(); };
  // «<model> · connected», «any model by key · sign-in needed», «<model> · on this PC» (the Mac's detail(info)).
  const wayDetail = (p) => {
    const state = p.kind === "local" ? t("on this PC", "на этом ПК") : p.connected ? t("connected", "подключено") : t("sign-in needed", "нужен вход");
    const name = p.takesKey && !p.connected ? t("any model by key", "любая модель по ключу") : p.chosen ? modelName(p.chosen) : "";
    return [name, state].filter(Boolean).join(" · ");
  };
  const fits = (gb) => gb * 1.2 <= (m.memoryGB || 0) * 1.073741824 * 0.7;
  const way = (p) => {
    const kindIcon = p.kind === "subscription" ? "person.circle" : p.kind === "local" ? "desktop" : "key";
    const models = p.connected && !p.unavailable ? (p.models || []) : [];
    if (!models.length) {
      // Nothing to choose inside it: the way in is the choice, or its sign-in.
      const detail = p.unavailable ? t("not open to apps yet — use the API key", "пока закрыто для приложений — используй API-ключ") : wayDetail(p);
      // Not signed in: Settings › AI model opens on this way, where it signs in (the Mac's pick → openSettings(.signIns)).
      return h("button", { class: "chat-model-row chat-way", disabled: !!p.unavailable, title: p.unavailable || p.note || "",
        onclick: () => { close(); p.connected ? act("useAccount", { provider: p.id }) : openSettings("model", p.id); } },
      icon(kindIcon, 13), h("span", { class: "chat-way-text" }, h("b", {}, p.name), h("span", { class: "small secondary" }, detail)), p.id === s.model.provider ? icon("check", 13) : null);
    }
    const local = p.kind === "local";
    const live = m.live?.[p.id];
    const head = h("div", { class: "chat-way-head" + (p.kind === "subscription" ? " chat-sub" : "") }, icon(kindIcon, 13), h("b", {}, p.name), h("span", { class: "spacer" }),
      h("span", {}, local ? t(`this PC · ${m.memoryGB || "?"} GB memory`, `этот ПК · ${m.memoryGB || "?"} ГБ памяти`) : t("connected", "подключено")));
    const rows = [head];
    // Ollama that didn't answer: open it, or get it — models on this PC need it running.
    if (p.id === "ollama" && m.servers?.ollama === false) {
      rows.push(m.installed
        ? h("button", { class: "chat-model-row chat-sub-row", onclick: () => act("startOllama") }, icon("exclamationmark.triangle", 12), h("span", { class: "chat-way-text" }, h("b", {}, t("Ollama isn't running", "Ollama не запущена")), h("span", { class: "small secondary" }, t("start it, then open this menu again", "запусти её и открой меню снова"))))
        // Its install says how far it is and waits (the Mac's ollamaInstall); the menu stays open to show it.
        : h("button", { class: "chat-model-row chat-sub-row", disabled: !!s.localSetup?.ollamaInstall, onclick: () => act("setUpLocal") }, icon("arrow.down.circle", 12),
          h("span", { class: "chat-way-text" }, h("b", {}, s.localSetup?.ollamaInstall || t("Get Ollama", "Скачать Ollama")),
            h("span", { class: "small secondary" }, t("free · the newest release, then a model that fits this PC", "бесплатно · новейшая версия, потом модель под этот ПК")))));
    }
    for (const id of models) {
      const off = m.refusals?.[id], absent = local && live && !live.includes(id);
      const loading = m.downloads?.[id], size = m.sizes?.[id];
      const pullable = absent && p.id === "ollama" && m.servers?.ollama === true;
      const tooBig = pullable && size !== undefined && !fits(size);
      const detail = loading !== undefined ? t(`downloading · ${Math.round(loading * 100)}%`, `скачиваю · ${Math.round(loading * 100)}%`)
        : tooBig ? t(`≈ ${size.toFixed(1)} GB · too big for this PC`, `≈ ${size.toFixed(1)} ГБ · слишком велика для этого ПК`)
        : pullable ? t(`Download · ${size !== undefined ? `≈ ${size.toFixed(1)} GB` : "not on this PC yet"}`, `Скачать · ${size !== undefined ? `≈ ${size.toFixed(1)} ГБ` : "пока не на этом ПК"}`)
        : absent ? t("not downloaded", "не скачана") : off ? t("too small for the chat", "слишком мала для чата")
        : [id === m.recommended?.[p.id] ? t("recommended", "рекомендую") : null, local && size !== undefined ? t(`≈ ${Math.round(size * 1.2)} GB memory`, `≈ ${Math.round(size * 1.2)} ГБ памяти`) : null,
           s.localSetup?.scores?.[p.id + "/" + id] ? (s.localSetup.scores[p.id + "/" + id].good ? "✓ " : "⚠ ") + s.localSetup.scores[p.id + "/" + id].short : null].filter(Boolean).join(" · ");
      const chosen = p.id === s.model.provider && id === s.model.model;
      const disabled = !!off || (absent && (!pullable || tooBig || loading !== undefined));
      rows.push(h("button", { class: "chat-model-row chat-sub-row" + (absent ? " chat-absent" : "") + (disabled ? " chat-faded" : ""), disabled,
        title: tooBig ? t(`Needs about ${Math.round(size * 1.2)} GB of memory; this PC has ${m.memoryGB} GB.`, `Нужно около ${Math.round(size * 1.2)} ГБ памяти; у этого ПК ${m.memoryGB} ГБ.`)
          : pullable ? t(`Downloads ${id} into Ollama on this PC; then pick it here.`, `Скачает ${id} в Ollama на этом ПК; потом выбери её здесь.`) : off || id,
        onclick: () => { if (pullable) { act("downloadModel", { model: id }); return; } close(); act(p.id === s.model.provider ? "setModel" : "useAccount", p.id === s.model.provider ? { model: id } : { provider: p.id, model: id }); } },
      pullable && loading === undefined && !tooBig ? icon("arrow.down.circle", 12) : null,
      h("span", { class: "chat-way-text" }, h("span", { class: "one-line" }, modelName(id)), detail ? h("span", { class: "small secondary" }, detail) : null),
      chosen ? icon("check", 13) : null));
    }
    return rows;
  };
  draw();
  if (!popover(anchor, h("div", { class: "chat-model-menu" },
    h("div", { class: "chat-model-cols" }, left, right),
    h("div", { class: "chat-model-foot" }, icon("lock.shield", 12), h("span", {}, t("The model never sees your wallet key.", "Модель никогда не видит ключ кошелька.")),
      h("span", { class: "spacer" }), h("button", { class: "link-btn", onclick: () => openSettings("model") }, t("Sign-ins…", "Входы…")))), "up", closed)) return;
  // Open first, then the face: a redraw before the menu is in the page would take it for closed.
  liveMenu = draw;
  tuning = "model";
  render();
}

/** The networks the assistant may use, as on the Mac (NetworksChip): those switched on, three logos and «+N»; the menu
 *  switches each one, and says that money there still shows. */
function networksChip() {
  const all = ui.state.modules?.networks || [];
  const on = all.filter((n) => n.on);
  const chip = h("button", { class: "chat-chip-btn", "data-pop": "networks", title: t("Networks the assistant may use: ", "Сети, где работает ассистент: ") + on.map((n) => n.name).join(", "),
    onclick: () => {
      const pop = popover(chip, networksMenu(), "up", () => { if (tuning === "networks" && netsPop === pop) tuning = null; render(); });
      if (!pop) return;
      pop._live = networksMenu;
      netsPop = pop; tuning = "networks"; render();
    } },
  on.length ? [h("span", { class: "chat-logo-stack" }, on.slice(0, 3).map((n) => logo(n.logo, 20))), on.length > 3 ? h("b", { class: "secondary" }, "+" + (on.length - 3)) : null]
    : h("b", { class: "chat-no-nets" }, t("No networks", "Нет сетей")),
  icon("chevron.down", 9));
  return chip;
}

function networksMenu() {
  const all = ui.state.modules?.networks || [];
  return h("div", { class: "chat-networks-menu" },
    h("div", { class: "chat-caption chat-split" }, h("span", {}, t("Networks", "Сети")), h("span", {}, t(`${all.filter((n) => n.on).length} of ${all.length} on`, `включено ${all.filter((n) => n.on).length} из ${all.length}`))),
    all.map((n) => h("div", { class: "chat-network-row" }, logo(n.logo, 28),
      h("div", { class: "chat-network-text" }, h("b", {}, n.name), h("div", { class: "small secondary" }, n.uses || n.users || "")),
      h("label", { class: "chat-switch" }, h("input", { type: "checkbox", id: "chat-net-" + n.id, checked: n.on, onchange: (e) => act("module", { name: n.id, test: e.target.checked }) }), h("span", {})))),
    h("div", { class: "chat-networks-note small secondary" }, t("Off: the assistant won’t plan moves there. Money on that network still shows on the canvas.", "Выключено: ассистент не будет планировать там действия. Деньги в этой сети по-прежнему видны на холсте.")),
    h("button", { class: "link-btn chat-networks-link", onclick: () => openSettings("modules") }, t("DeFi, perps, protocols: Settings › Modules", "DeFi, перпы, протоколы: Настройки › Модули")));
}

/** «950», «9.6k», «22k», «1M»: tokens as the Mac's ContextMeter.short writes them. */
function short(n) {
  const trim = (v, d) => String(Number(v.toFixed(d)));
  if (n >= 995000) return trim(n / 1e6, 2) + "M";
  if (n >= 9950) return Math.round(n / 1000) + "k";
  if (n >= 1000) return trim(n / 1000, 1) + "k";
  return String(n);
}

/** «$1.24», «$0.04», «$0.004»; under a tenth of a cent «<$0.001». */
const dollars = (v) => (v >= 0.0095 ? "$" + v.toFixed(2) : v >= 0.00095 ? "$" + v.toFixed(3) : v > 0 ? "<$0.001" : "$0");

/** The context chip's popover, from the state now: drawn again with each state while it's open (the Mac's ContextMeter). */
function contextPop() {
  const c = ui.state.context;
  if (!c) return h("div", { class: "chat-context-pop" });
  const share = c.window ? Math.min(1, c.used / c.window) : null;
  const rest = Math.max(0, c.used - (c.instructions || 0) - (c.chat || 0));
  const part = (cls, n) => n > 0 && c.window ? h("span", { class: cls, style: `width:${Math.max(2, (n / c.window) * 100)}%` }) : null;
  const line = (cls, name, value) => h("div", { class: "chat-context-line" }, cls ? h("i", { class: "chat-dot " + cls }) : null, h("span", { class: "one-line" }, name), h("span", { class: "spacer" }), h("b", {}, value));
  return h("div", { class: "chat-context-pop" },
      h("div", { class: "chat-caption chat-split" }, h("span", {}, t("Context", "Контекст")), share === null ? null : h("span", {}, share < 0.01 ? t("under 1%", "меньше 1%") : Math.round(share * 100) + "%")),
      h("div", { class: "chat-context-big" }, h("b", {}, short(c.used)), h("span", { class: "secondary" }, c.window ? t(`of ${short(c.window)} tokens`, `из ${short(c.window)} токенов`) : t("tokens", "токенов"))),
      c.window ? h("div", { class: "chat-context-bar chat-split-bar" }, part("chat-part-brief", c.instructions || 0), part("chat-part-chat", c.chat || 0), part("chat-part-rest", rest)) : null,
      h("div", { class: "chat-context-lines" },
        line("chat-part-brief", t("Instructions", "Инструкции"), short(c.instructions || 0)),
        line("chat-part-chat", c.sent < c.lines ? t(`This chat · the last ${c.sent} of ${c.lines} messages`, `Этот чат · последние ${c.sent} из ${c.lines} сообщений`) : t(`This chat · ${c.lines || 0} messages`, `Этот чат · сообщений: ${c.lines || 0}`), short(c.chat || 0)),
        line("chat-part-rest", t("Canvas, balances, your message", "Холст, балансы, твоё сообщение"), short(rest))),
      c.spent > 0 ? [h("div", { class: "menu-divider" }),
        h("div", { class: "chat-context-lines" }, line(null, t("This chat’s answers", "Ответы этого чата"), dollars(c.spent)), c.last !== null && c.last !== undefined ? line(null, t("The last one", "Последний"), dollars(c.last)) : null),
        h("div", { class: "small secondary" }, c.billed ? t("As OpenRouter charged it.", "Как списал OpenRouter.") : t("Tokens at the service’s list price; its own invoice is the final word.", "Токены по прейскуранту сервиса; окончательная сумма — в его счёте."))] : null,
      h("div", { class: "menu-divider" }),
      h("div", { class: "small secondary" }, c.sent < c.lines
        ? t("This chat is long: only its newest messages go with each request, so it never runs out of room.", "Чат длинный: с каждым запросом уходят только его новые сообщения, поэтому место не кончается.")
        : t("Each message goes with the chat so far. A long chat sends only its newest messages, so it never runs out of room.", "Каждое сообщение уходит вместе с чатом. Длинный чат отправляет только новые сообщения, поэтому место не кончается.")));
}

/** How much of the model's window the chat fills, or what a key paid for its answers (ContextChip); a click opens its parts. */
function contextChip(s) {
  const c = s.context;
  if (!c || !c.used) return null;
  const share = c.window ? Math.min(1, c.used / c.window) : null;
  const tight = (share || 0) >= 0.8;
  const paid = s.providers.find((p) => p.id === s.model.provider)?.kind === "apiKey" && c.spent > 0;
  const r = 6.5, len = 2 * Math.PI * r;
  const ring = share === null ? null : svg("svg", { viewBox: "0 0 18 18", class: "chat-ring" },
    svg("circle", { cx: 9, cy: 9, r, fill: "none", "stroke-width": 2.2, class: "chat-ring-back" }),
    svg("circle", { cx: 9, cy: 9, r, fill: "none", "stroke-width": 2.2, class: "chat-ring-fill", "stroke-linecap": "round",
      "stroke-dasharray": `${Math.max(0.5, share * len)} ${len}`, transform: "rotate(-90 9 9)" }));
  const chip = h("button", { class: "chat-chip-btn chat-ctx" + (tight ? " chat-near" : ""), "data-pop": "context",
    title: paid ? t(`This chat's answers cost ${dollars(c.spent)} so far. Click for the context they read.`, `Ответы этого чата уже стоили ${dollars(c.spent)}. Нажми, чтобы увидеть, что они читали.`)
      : t("What the model read with your last message, of what its window holds.", "Что модель прочитала с последним сообщением — из того, что вмещает её окно."),
    onclick: () => { const pop = popover(chip, contextPop(), "up"); if (pop) pop._live = contextPop; } },
  ring, paid ? h("span", {}, dollars(c.spent)) : [h("span", {}, short(c.used)), c.window ? h("span", { class: "secondary" }, "/ " + short(c.window)) : null]);
  return chip;
}

/** A card of our own beside an anchor, above it by default, kept in the window; Escape or a click outside closes it. */
function popover(anchor, content, side = "up", onClose) {
  // A second click on the chip whose menu is open closes it (the Mac's chips toggle). A chip redrawn while its menu is
  // open is a new element: `data-pop` names it across redraws.
  const open = document.querySelector(".menu");
  if (open && (open._from === anchor || (anchor.dataset.pop && open._pop === anchor.dataset.pop))) { open.remove(); return null; }
  open?.remove();
  const pop = h("div", { class: "menu chat-pop" }, content);
  pop._from = anchor;
  pop._pop = anchor.dataset.pop;
  pop.addEventListener("click", (e) => e.stopPropagation());
  document.body.append(pop);
  const a = anchor.getBoundingClientRect(), w = pop.offsetWidth, hgt = pop.offsetHeight;
  const left = Math.max(8, Math.min(a.left, window.innerWidth - w - 8));
  const top = side === "up" && a.top - hgt - 8 > 8 ? a.top - hgt - 8 : Math.min(a.bottom + 8, window.innerHeight - hgt - 8);
  pop.style.left = left + "px";
  pop.style.top = Math.max(8, top) + "px";
  // However it goes — a click outside, Escape (app.js), a pick, another menu, Settings — it says so once and stops
  // listening, so the coin's «Picking a model» goes with it (the Mac's onChange(of: open)).
  const outside = (ev) => { if (!pop.contains(ev.target)) pop.remove(); };
  const gone = new MutationObserver(() => {
    if (pop.isConnected) return;
    gone.disconnect();
    document.removeEventListener("click", outside);
    onClose?.();
  });
  gone.observe(document.body, { childList: true });
  setTimeout(() => { if (pop.isConnected) document.addEventListener("click", outside); }, 0);
  return pop;
}

// MARK: The voice

/** Speaking into the field: what Windows hears shows as it goes (the settled words in ink, the rest grey); Done puts it in
 *  the field, Cancel drops it; nothing is sent by itself. A build whose window has no live recognizer listens to a pause. */
const voice = { on: false, heard: "", started: 0, timer: null, unlisten: null, language: "", failed: null };

/** «BRIDGE · RELAY» → «Bridge · Relay», tickers kept («USDC»), for a step's name over the field. */
function stepLabel(label) {
  return String(label || "").split(" ").map((w) => /^[A-Z0-9]{2,5}$/.test(w) && !/^(FROM|INTO|SWAP|SEND|BUY|SELL|ON|TO|OPEN|GAS|PAY|CHECK|SIGN)$/.test(w) ? w
    : w.charAt(0) + w.slice(1).toLowerCase()).join(" ");
}

async function speak() {
  Object.assign(voice, { on: true, heard: "", started: Date.now(), language: "", failed: null });
  clearInterval(voice.timer);
  voice.timer = setInterval(() => {
    const el = document.getElementById("voice-time");
    const sec = Math.floor((Date.now() - voice.started) / 1000);
    if (el) el.textContent = `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, "0")}`;
  }, 1000);
  render();
  try {
    voice.unlisten = await window.__TAURI__.event.listen("voice", (e) => {
      const said = String(e.payload || "");
      // The first word from Windows is the language it listens in (voice.rs LANGUAGE_MARK), for the line over the panel.
      if (said.startsWith("\u0001")) {
        voice.language = said.slice(1);
        const lang = document.getElementById("voice-lang");
        if (lang) lang.textContent = voice.language;
        return;
      }
      voice.heard = said;
      const el = document.getElementById("voice-heard");
      if (el) { el.textContent = voice.heard ? voice.heard + "…" : t("Say what to do…", "Скажи, что сделать…"); el.classList.toggle("secondary", !voice.heard); }
    });
  } catch {}
  let text = "";
  try {
    text = (await invoke("listen_live")).trim();
  } catch (e) {
    // No live recognizer in this build: the one that listens to a pause.
    if (/not found|unknown|not allowed|undefined/i.test(String(e))) {
      try { text = (await invoke("listen")).trim(); } catch (x) { voice.failed = String(x); }
    } else voice.failed = String(e);
  } finally {
    voice.unlisten?.();
    voice.unlisten = null;
    voice.on = false;
    clearInterval(voice.timer);
    if (text) ui.composerText = ui.composerText ? ui.composerText.trimEnd() + " " + text : text;
    reportComposing(ui.composerText);
    render();
    document.getElementById("composer")?.focus();
  }
}

function stopVoice(done) { invoke("listen_stop", { done }).catch(() => {}); }

// Esc cancels and Return finishes while listening, as on the Mac.
document.addEventListener("keydown", (e) => {
  if (!voice.on) return;
  if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); stopVoice(false); }
  if (e.key === "Enter") { e.preventDefault(); stopVoice(true); }
}, true);

// Ctrl+Return sends the message from anywhere in the window, as the Mac's ⌘Return on its send button; the field's own
// Return is the field's (it may be picking a suggestion).
document.addEventListener("keydown", (e) => {
  if (e.key !== "Enter" || !e.ctrlKey || e.shiftKey || e.altKey || e.defaultPrevented || voice.on) return;
  const field = document.getElementById("composer");
  if (!field || e.target === field) return;
  e.preventDefault();
  send();
});

/** «Listening · Russian · recognized online by Microsoft»: the language, and where the sound goes — Windows' dictation needs
 *  Online speech recognition, so it leaves the PC (the Mac says «on this Mac» or «by Apple»). */
function listeningRow() {
  return h("div", { class: "chat-listening-row", title: t("It listens in the keyboard's language: switch the layout before the microphone to speak another one. Windows sends the sound to Microsoft's online speech recognition, so Settings › Privacy & security › Speech › Online speech recognition has to be on; ihatecrypto keeps nothing of it.",
      "Слушает на языке раскладки: переключи раскладку до микрофона, чтобы говорить на другом. Windows отправляет звук в онлайн-распознавание Microsoft, поэтому в Параметрах › Конфиденциальность и защита › Речь должно быть включено «Распознавание голоса в сети»; ihatecrypto ничего из него не хранит.") },
    h("span", { class: "chat-rec-dot" }), h("b", {}, t("Listening", "Слушаю")),
    h("span", { class: "small secondary" }, "· ", h("span", { id: "voice-lang" }, voice.language || t("the keyboard's language", "язык раскладки")),
      t(" · recognized online by Microsoft", " · распознаёт Microsoft онлайн")));
}

function voicePanel() {
  return h("div", { class: "chat-voice" },
    h("div", { id: "voice-heard", class: "chat-voice-heard" + (voice.heard ? "" : " secondary") }, voice.heard ? voice.heard + "…" : t("Say what to do…", "Скажи, что сделать…")),
    h("div", { class: "chat-voice-row" },
      h("div", { class: "chat-voice-bars", "aria-hidden": "true" }, Array.from({ length: 24 }, (_, i) => h("span", { style: `animation-delay:${(i * 0.07).toFixed(2)}s` }))),
      h("span", { id: "voice-time", class: "num secondary" }, "0:00"),
      h("button", { class: "chat-round-btn chat-glassy", title: t("Cancel · Esc", "Отмена · Esc"), onclick: () => stopVoice(false) }, icon("xmark", 13)),
      h("button", { class: "chat-round-btn chat-send chat-lit", title: t("Done: put the words in the field · Return", "Готово: слова в поле · Return"), onclick: () => stopVoice(true) }, icon("check", 13))));
}

// For the preview (preview/index.html «&call=chat.…»): a menu open, or a draft in the field.
export function openModelMenu() { document.querySelector(".chat-model")?.click(); }
export function openNetworksMenu() { document.querySelectorAll(".chat-chips .chat-chip-btn")[1]?.click(); }
export function openContextMenu() { document.querySelector(".chat-ctx")?.click(); }
export function showDraft() { suggest("send 120 USDC to Mom"); drawHelpers(); }
/** The preview's and the console sweep's send: a message with nothing attached goes out (`&call=chat.sendDraft`). */
export function sendDraft() { ui.composerText = "send 1 USDC to Mom"; send(); }
/** The preview's look at a contract in the field (`&call=chat.contractDraft`): its chip and rows wear only its market's picture. */
export function contractDraft() {
  const f = document.getElementById("composer");
  if (!f) return;
  ui.composerText = f.value = "buy $0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
  f.focus();
  f.setSelectionRange(f.value.length, f.value.length);
  refreshSuggestions(f);
  drawMirror(f);
}
/** The preview's «$» chips in the field (`&call=chat.showMention`). */
/** The preview's `&call=chat.showAbout`: a coin attached from outside the canvas, its chip over the field. */
export function showAbout() { attachAbout({ title: "SOL · Solana", about: "About the coin «SOL on Solana; the wallet holds 0.0215»", logo: "SOL" }); }
export function showMention() { suggest("buy $SOL for 5 USDC, then swap half to $JUP"); drawHelpers(); }
