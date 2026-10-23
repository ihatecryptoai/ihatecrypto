// The chat floating over the canvas, as the Mac's FloatingChat (Studio.swift) and its replies (ChatReplies.swift): a
// glass panel docked at the bottom right that the header moves anywhere and the left edge widens; the coin with its
// mood; the conversation; the field with the model, the networks and the context over it. It never moves money: a
// move waits as a card, and only Confirm on it (here or on the canvas) runs it.

import { ui, act, render, t, h, svg, icon, addIcons, mascot, logo, toast, invoke, store, suggest, autosize, link, presence } from "./core.js";
import { railEdge } from "./rail.js";
import { blockTitle } from "./boards.js";

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
  const small = Math.min(full, Math.max(260, r.h * 0.5));
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
  if (listening) return "listening";
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
      return t("Reading…", "Читаю…");
    case "listening": return t("Listening…", "Слушаю…");
    case "waiting": return t("Waiting for your Confirm", "Ждёт твоего «Подтвердить»");
    case "done": return t("Done", "Готово");
    default: return s.model.connected ? t("Waiting for you", "Жду тебя") : t("No model: plain commands only", "Без модели: только простые команды");
  }
}

// MARK: The conversation

function conversation(s) {
  const items = [];
  const last = s.chat.length - 1;
  s.chat.forEach((m, i) => {
    items.push(m.fromUser ? userMessage(m) : reply(m));
    // Ways to go on live under the reply that offered them.
    if (i === last && !m.fromUser && s.options.length && !s.thinking) items.push(answers(m.id));
  });
  if (s.running) items.push(stepsCard(s.running));
  if (s.thinking && (last < 0 || s.chat[last].fromUser)) {
    items.push(s.draft ? reply({ fromUser: false, text: s.draft, draft: true }) : thinkingDots());
  }
  if (s.options.length && !s.thinking && (last < 0 || s.chat[last].fromUser)) items.push(answers("start"));
  if (!s.chat.length && !s.thinking) items.push(...ideas());
  return items;
}

/** Yours: an orange outline, no fill; its time beside it, at the bubble's foot. */
function userMessage(m) {
  return h("div", { class: "chat-msg-user" },
    m.context ? sentContext(m.context) : null,
    h("div", { class: "chat-msg-user-row" },
      m.at ? h("span", { class: "chat-msg-time num" }, stamp(m.at)) : null,
      h("div", { class: "chat-bubble selectable" }, m.text)));
}

/** What a block the person pointed at told the model with this message: a chip over it; a click shows the words. */
function sentContext(text) {
  const chip = h("button", { class: "chat-context-chip", title: t("What the model got with this message", "Что модель получила с этим сообщением"),
    onclick: (e) => { e.stopPropagation(); popover(chip, h("div", { class: "chat-pop-context" },
      h("div", { class: "eyebrow" }, t("Sent to the model", "Ушло модели")), h("div", { class: "selectable" }, text))); } },
  icon("square.on.square", 11), h("span", { class: "one-line" }, text.split("\n")[0].slice(0, 60)), icon("chevron.down", 9));
  return h("div", { class: "chat-context-row" }, h("span", { class: "chat-context-label" }, icon("paperclip", 11), t("Context", "Контекст")), chip);
}

/** «Claude Opus 5.5» from «claude-opus-5-5», «GPT-5.4» from «gpt-5.4». */
export function modelName(id) {
  if (!id) return "";
  if (/\s/.test(id)) return id;
  const out = [];
  for (const w of id.replace(/^.*\//, "").split(/[-_:]/).filter(Boolean)) {
    const prev = out[out.length - 1];
    if (/^\d+$/.test(w) && prev && /^\d+(\.\d+)*$/.test(prev)) out[out.length - 1] = prev + "." + w;
    else if (/^\d/.test(w) && prev === "GPT") out[out.length - 1] = "GPT-" + w;
    else out.push(/^gpt$/i.test(w) ? "GPT" : /^\d/.test(w) ? w : w[0].toUpperCase() + w.slice(1));
  }
  return out.join(" ");
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
const quietNotes = new Set(["guard", "guard: sends go to saved addresses", "Confirm is a button", "schedule", "price alert", "browser", "incoming"]);

/** One answer: a header line, then its words; a result keeps a card, a failure a red one (ChatReplies.Reply). */
function reply(m) {
  const head = header(m.note);
  const emoji = (m.text.match(/^(\p{Extended_Pictographic}️?)\s*/u) || [])[1];
  const text = emoji ? m.text.slice(m.text.indexOf(emoji) + emoji.length).trimStart() : m.text;
  const failed = text.startsWith("Failed: ");
  const boxed = !failed && (emoji || head?.result);
  const done = boxed && !emoji && !quietNotes.has(m.note);
  let body;
  if (failed) {
    body = h("div", { class: "chat-reply-failed" }, icon("xmark.octagon", 15), h("div", { class: "chat-words" }, words(text.slice(8))));
  } else if (done || emoji) {
    body = h("div", { class: "chat-reply-row" },
      h("span", { class: "chat-reply-tile" + (done ? " chat-done" : "") }, done ? icon("check", 14) : emoji),
      h("div", { class: "chat-words" }, words(text)));
  } else {
    body = h("div", { class: "chat-words" }, words(text));
  }
  return h("div", { class: "chat-reply" + (boxed ? " chat-boxed" : "") + (failed ? " chat-failed" : "") + (m.draft ? " chat-draft" : "") },
    head || m.at ? h("div", { class: "chat-reply-head" + (m.note === "incoming" ? " chat-incoming" : ""), title: head?.detail || "" },
      mascot(14), m.note === "incoming" ? icon("arrow.down.circle", 12) : null,
      head ? h("span", { class: "one-line" }, head.label) : null,
      m.at ? h("span", { class: "num" }, (head ? "· " : "") + stamp(m.at)) : null) : null,
    body,
    m.chart ? sparkCard(m.chart) : null);
}

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

/** Inline marks as text nodes: **bold**, links as «Solscan ↗», long hex short, amounts bold, signed numbers colored. */
function rich(text) {
  const out = [];
  const re = /`([^`]+)`|\*\*([^*]+)\*\*|(https?:\/\/[^\s]+[^\s.,;:)])|(0x[0-9a-fA-F]{40,64})|([+−-]\$?\d[\d,]*(?:\.\d+)?%?)|(\$\d[\d,]*(?:\.\d+)?|\d[\d,]*(?:\.\d+)? [A-Z]{2,6}\b)/g;
  let at = 0, m;
  while ((m = re.exec(text))) {
    const before = text.slice(at, m.index);
    if (m[5]) {
      // Signed numbers start the text or follow a space or «(»: «+25», not «gpt-6»; «−15.5% away» is a distance.
      const prev = text[m.index - 1];
      if ((m.index > 0 && !" (".includes(prev)) || text.slice(re.lastIndex).startsWith(" away")) continue;
    }
    out.push(before);
    if (m[1]) out.push(h("code", {}, m[1]));
    else if (m[2]) out.push(h("b", {}, m[2]));
    else if (m[3]) {
      let name = m[3];
      try { const labels = new URL(m[3]).hostname.split("."); name = labels.length >= 2 ? labels[labels.length - 2] : labels[0]; } catch {}
      out.push(link(m[3], name.charAt(0).toUpperCase() + name.slice(1) + " ↗"));
    } else if (m[4]) out.push(h("code", { title: m[4] }, m[4].slice(0, 6) + "…" + m[4].slice(-4)));
    else if (m[5]) out.push(h("span", { class: m[5].startsWith("+") ? "profit" : "loss" }, m[5]));
    else out.push(h("b", {}, m[6]));
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

/** A move being carried out, its lines as a checklist with a rail (StepsCard). */
function stepsCard(r) {
  const lines = r.lines.length ? r.lines : [r.title];
  return h("div", { class: "chat-steps-card" },
    h("div", { class: "chat-steps-head" }, icon("bolt", 11), t("Executing", "Выполняется"), " · ", r.title),
    lines.map((line, i) => {
      const last = i === lines.length - 1;
      const failed = /failed|refused|не вышло|отказ/i.test(line);
      const cut = line.indexOf(". ");
      return h("div", { class: "chat-step-line" },
        h("div", { class: "chat-step-rail" }, last ? h("span", { class: "chat-spinner" }) : h("span", { class: "chat-step-dot" + (failed ? " chat-failed" : "") }, icon(failed ? "xmark" : "check", 9)),
          last ? null : h("span", { class: "chat-step-track" })),
        h("div", { class: "chat-step-text selectable" }, cut > 0 ? [h("div", {}, rich(line.slice(0, cut))), h("div", { class: "small secondary" }, line.slice(cut + 2))] : rich(line)));
    }));
}

function thinkingDots() {
  return h("div", { class: "chat-dots", "aria-label": t("The assistant is thinking", "Ассистент думает") }, h("span"), h("span"), h("span"));
}

// MARK: Ways to go on

/** Folded by hand, by the reply they came with. */
const folded = new Set();
/** The card ↑↓ from an empty field moved to; Return picks it. */
let cardKey = null;

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

/** The cards the last reply offered, under its words (Answers + OptionCard): a tap carries one out. */
function answers(key) {
  const s = ui.state;
  const shut = folded.has(key);
  return h("div", { class: "chat-answers" },
    shut ? null : s.options.map((o, i) => h("button", {
      class: "chat-option" + (o.recommended && cardKey === null ? " chat-rec" : "") + (cardKey === i ? " chat-keyed" : ""),
      disabled: !!s.runningBoard, title: o.comment || o.title, onclick: () => { cardKey = null; act("pick", { index: i }); },
    },
    h("span", { class: "chat-option-icon" }, icon(optionIcon(o.title), 13)),
    h("span", { class: "chat-option-text" },
      h("span", { class: "chat-option-title" }, o.title, o.recommended ? h("span", { class: "chat-rec-chip" }, t("Recommended", "Рекомендую")) : null),
      o.comment ? h("span", { class: "chat-option-comment" }, o.comment) : null))),
    h("div", { class: "chat-answers-foot" },
      h("button", { class: "link-btn chat-fold", onclick: () => { shut ? folded.delete(key) : folded.add(key); render(); } },
        shut ? t(`Options: ${s.options.length}`, `Варианты: ${s.options.length}`) : t("Fold", "Свернуть"), icon(shut ? "chevron.down" : "chevron.up", 10)),
      shut ? null : h("span", { class: "small secondary" }, t("Or write your own. ↑ ↓ to choose, ⏎ to answer.", "Или напиши свой вариант. ↑ ↓ — выбрать, ⏎ — ответить."))));
}

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

function ideas() {
  const four = [0, 1, 2, 3].map((i) => templates[(ideasStart + i) % templates.length]);
  return [
    h("div", { class: "chat-intro" }, t("Your portfolio is on the canvas. Tell me what to do with your money — in any language — and I’ll build the plan next to it. Nothing moves without your signature.",
      "Твой портфель — на холсте. Скажи, что сделать с деньгами, на любом языке, — я построю план рядом. Без твоей подписи ничего не двигается.")),
    h("div", { class: "chat-ideas" },
      h("div", { class: "chat-ideas-head" }, h("button", { class: "icon-btn", title: t("Other starting points", "Другие идеи"), onclick: () => { ideasStart += 4; render(); } }, icon("arrow.clockwise", 14))),
      four.map(([en, ru, phrase, ico, lineEn, lineRu]) => h("button", { class: "chat-idea", title: t(`Puts “${phrase}” in the field to edit and send`, `Вставит «${phrase}» в поле — поправь и отправь`), onclick: () => suggest(phrase) },
        h("span", { class: "chat-idea-icon" }, icon(ico, 14)),
        h("span", { class: "chat-idea-text" }, h("b", { class: "one-line" }, t(en, ru)), h("span", { class: "one-line" }, t(lineEn, lineRu))),
        icon("arrow.up.left", 12)))),
  ];
}

// MARK: The field

let listening = false;
let listenStart = 0;
let listenTimer = null;
/** The field's text when Esc closed the «/ @ $» list: it stays closed until the text changes. */
let dismissed = null;
let pick = 0;
/** The sent message ↑ brought back into the field, by its place in the sent list. */
let recalled = null;

function composer(s) {
  const field = h("textarea", {
    id: "composer", rows: "1", spellcheck: "false",
    placeholder: s.card ? t("Ask about it, change it («make it 0.02»), or something else", "Спроси о нём, измени («сделай 0.02») или что-то другое")
      : ui.attached ? t("Ask about it, or say what to do with it…", "Спроси о нём или скажи, что с ним сделать…")
      : t("What should we do? / blocks, @ names, $ tokens", "Что сделаем? / блоки, @ имена, $ токены"),
    oninput: (e) => { ui.composerText = e.target.value; recalled = null; cardKey = null; autosize(e.target, 140); refreshSuggestions(e.target); sendLit(); },
    onkeydown: keys,
    onclick: (e) => refreshSuggestions(e.target),
    onkeyup: (e) => { if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) refreshSuggestions(e.target); },
  });
  field.value = ui.composerText;
  setTimeout(() => autosize(field, 140), 0);
  const pointed = ui.attached && s.boards.find((b) => b.id === ui.attached);
  if (ui.attached && !pointed) ui.attached = null;
  const waiting = s.card;
  return h("div", { class: "chat-composer" },
    listening ? listeningRow() : h("div", { class: "chat-chips" }, modelChip(s), networksChip(), h("span", { class: "spacer" }), contextChip(s)),
    pointed && !listening ? h("div", { class: "chat-attached" }, icon("square.on.square", 11), h("span", { class: "one-line" }, blockTitle(pointed) || pointed.title),
      h("button", { class: "icon-btn", title: t("Take it out of the message", "Убрать из сообщения"), onclick: () => { ui.attached = null; render(); } }, icon("xmark", 11))) : null,
    s.model.connected || listening ? null : connectBar(),
    h("div", { id: "chat-suggest", class: "chat-suggest-slot" }),
    listening ? voicePanel() : h("div", { class: "chat-field" + (waiting ? " chat-waiting" : "") },
      waiting ? waitingBar(waiting) : null,
      h("div", { class: "chat-entry" },
        h("button", { class: "chat-round-btn chat-glassy", title: t("Plan templates", "Шаблоны планов"), onclick: (e) => templatesMenu(e.currentTarget) }, icon("square.grid", 15)),
        field,
        h("button", { class: "icon-btn chat-mic", title: t("Speak instead of typing (the keyboard layout's language)", "Сказать голосом (язык раскладки)"), onclick: speak }, icon("mic", 16)),
        h("button", { id: "chat-send", class: "chat-round-btn chat-send" + (ui.composerText.trim() ? " chat-lit" : ""), title: t("Send", "Отправить"), disabled: s.thinking, onclick: send }, icon("arrow.up", 15)))),
  );
}

function sendLit() { document.getElementById("chat-send")?.classList.toggle("chat-lit", !!ui.composerText.trim()); }

/** While a move waits, the field carries it: what it is, Cancel and Confirm in one place; its steps stay on the canvas. */
function waitingBar(card) {
  const s = ui.state;
  const title = card.title.charAt(0).toUpperCase() + card.title.slice(1);
  const line = s.runningBoard ? (s.signer.waiting ? t(`${presence()} is asking for you…`, `${presence()} спрашивает тебя…`) : t("Signing…", "Подписываю…"))
    : t(`Its steps are on the canvas · signs after ${presence()}`, `Шаги — на холсте · подпись после ${presence()}`);
  const busy = s.thinking || !!s.runningBoard;
  return h("div", { class: "chat-waiting-bar" },
    h("div", { class: "chat-waiting-what" }, h("span", { class: "chat-wait-dot" }),
      h("div", {}, h("div", { class: "chat-waiting-title" }, title), h("div", { class: "chat-waiting-line" }, line))),
    h("div", { class: "chat-waiting-buttons" },
      h("button", { class: "chat-pill-outline", disabled: busy, onclick: () => act("cancel") }, t("Cancel", "Отмена")),
      h("button", { class: "chat-pill-accent", disabled: busy, title: title, onclick: () => act("confirm") }, icon("check", 13), card.confirm)));
}

/** No model reads the chat yet: plain commands still work, and here is how to get one (ConnectModelBar). */
function connectBar() {
  const way = (name, file) => h("button", { class: "chat-way-chip", onclick: openSettings }, logo(file, 20), name);
  return h("div", { class: "chat-connect-bar" },
    h("b", {}, t("Connect a model to chat", "Подключи модель для чата")),
    h("div", { class: "small secondary" }, t("Plain commands work without one: «send 5 USDC to Mom».", "Простые команды работают и без неё: «отправь 5 USDC маме».")),
    h("div", { class: "chat-way-chips" }, way("ChatGPT", "llm-openai"), way("Claude", "llm-claude"), way("OpenRouter", "llm-openrouter")));
}

function openSettings() { document.querySelector(".menu")?.remove(); ui.view = "settings"; render(); }

function keys(e) {
  const s = ui.state;
  const rows = suggestionRows(e.target);
  if (rows.length) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); pick = (pick + (e.key === "ArrowDown" ? 1 : rows.length - 1)) % rows.length; drawSuggestions(rows); return; }
    if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); choose(rows[Math.min(pick, rows.length - 1)], e.target); return; }
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); dismissed = e.target.value; drawSuggestions([]); return; }
  }
  // ↑↓ from an empty field walk the cards under the last reply; then what was sent, as a terminal's history.
  const empty = !e.target.value;
  if ((e.key === "ArrowUp" || e.key === "ArrowDown") && empty && s.options.length && !s.thinking) {
    e.preventDefault();
    cardKey = e.key === "ArrowUp" ? Math.max(0, (cardKey ?? s.options.length) - 1) : cardKey !== null && cardKey + 1 < s.options.length ? cardKey + 1 : null;
    render();
    return;
  }
  if ((e.key === "ArrowUp" && (empty || recalled !== null)) || (e.key === "ArrowDown" && recalled !== null)) {
    const sent = s.chat.filter((m) => m.fromUser).map((m) => m.text);
    if (!sent.length) return;
    e.preventDefault();
    recalled = e.key === "ArrowUp" ? Math.max(0, (recalled ?? sent.length) - 1) : recalled + 1 < sent.length ? recalled + 1 : null;
    ui.composerText = recalled === null ? "" : sent[recalled];
    e.target.value = ui.composerText;
    autosize(e.target, 140);
    sendLit();
    return;
  }
  if (e.key === "Escape" && cardKey !== null) { e.stopPropagation(); cardKey = null; render(); return; }
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    if (empty && cardKey !== null && s.options[cardKey]) { const i = cardKey; cardKey = null; act("pick", { index: i }); return; }
    send();
  }
}

function send() {
  const text = ui.composerText.trim();
  if (!text || ui.state.thinking) return;
  ui.composerText = "";
  recalled = null;
  const board = ui.attached || undefined;
  ui.attached = null;
  act("send", { text, board });
  render();
}

// «/» lists the blocks (a pick adds it to the canvas), «@» the saved names and favorite tokens, «$» the tokens held.
const blockKinds = [
  ["receive", "Receive", "Получить", "your addresses"],
  ["addresses", "Address book", "Адресная книга", "names money may go to"],
  ["history", "History", "История", "what was signed"],
  ["favorites", "Favorites", "Избранное", "tokens and wallets you watch"],
  ["alerts", "Price alerts", "Ценовые оповещения", "pings at a price"],
  ["schedule", "Schedule", "Расписание", "tasks and the automation wallet"],
  ["positions", "Open positions", "Открытые позиции", "Hyperliquid"],
  ["multisig", "Multisig", "Мультисиг", "a vault several keys sign"],
  ["ledger", "Ledger", "Ledger", "the hardware wallet"],
];

/** The word at the caret when it starts with «/», «@» or «$», with where it starts. */
function wordAtCaret(el) {
  const at = el.selectionStart ?? el.value.length;
  const before = el.value.slice(0, at);
  const m = before.match(/(^|\s)([/@$])([^\s/@$]*)$/);
  return m ? { sign: m[2], word: m[3].toLowerCase(), start: at - m[3].length - 1, end: at } : null;
}

function suggestionRows(el) {
  if (!el || el.value === dismissed) return [];
  const w = wordAtCaret(el);
  if (!w) return [];
  const s = ui.state;
  const hit = (x) => x.toLowerCase().includes(w.word);
  if (w.sign === "/") {
    return blockKinds.filter(([kind, en, ru]) => hit(kind) || hit(en) || hit(ru)).map(([kind, en, ru, detail]) => ({ icon: "square.grid", title: t(en, ru), detail, block: kind }));
  }
  if (w.sign === "@") {
    const names = s.book.filter((e) => hit(e.name)).map((e) => ({ icon: "person", title: e.name, detail: e.network, insert: e.name }));
    const favs = s.favorites.filter((f) => hit(f.symbol)).map((f) => ({ logo: f.symbol, src: f.image, title: f.symbol, detail: f.name + " · " + f.chain, insert: f.symbol }));
    return [...names, ...favs].slice(0, 30);
  }
  const held = new Map();
  for (const n of s.portfolio.networks) for (const r of n.rows) if (r.section !== "defi" && !held.has(r.symbol) && hit(r.symbol)) held.set(r.symbol, { logo: r.symbol, src: r.icon, title: r.symbol, detail: `${r.amount} · ${n.name}`, insert: r.symbol });
  for (const f of s.favorites) if (!held.has(f.symbol) && hit(f.symbol)) held.set(f.symbol, { logo: f.symbol, src: f.image, title: f.symbol, detail: f.name, insert: f.symbol });
  return [...held.values()].slice(0, 30);
}

function refreshSuggestions(el) { pick = 0; drawSuggestions(suggestionRows(el)); }

function drawSuggestions(rows) {
  const slot = document.getElementById("chat-suggest");
  if (!slot) return;
  if (!rows.length) { slot.replaceChildren(); return; }
  const field = document.getElementById("composer");
  slot.replaceChildren(h("div", { class: "chat-suggest" },
    h("div", { class: "chat-suggest-rows" }, rows.map((r, i) => h("button", { class: "chat-suggest-row" + (i === pick ? " chat-lit" : ""), onmousedown: (e) => { e.preventDefault(); choose(r, field); } },
      r.logo ? logo(r.logo, 16, r.src) : icon(r.icon, 13), h("b", { class: "one-line" }, r.title), h("span", { class: "one-line secondary" }, r.detail)))),
    h("div", { class: "chat-suggest-foot" }, t("↑↓ choose · Return puts it in · Esc closes", "↑↓ — выбрать · Return — вставить · Esc — закрыть"))));
  slot.querySelector(".chat-suggest-row.chat-lit")?.scrollIntoView({ block: "nearest" });
}

function choose(row, el) {
  const w = wordAtCaret(el);
  if (!w) return;
  const text = el.value;
  if (row.block) {
    // A block goes straight onto the canvas; its «/word» leaves the field.
    ui.composerText = (text.slice(0, w.start) + text.slice(w.end)).replace(/\s{2,}/g, " ");
    act("addBoard", { name: row.block });
  } else {
    ui.composerText = text.slice(0, w.start) + row.insert + " " + text.slice(w.end).replace(/^\s+/, "");
  }
  el.value = ui.composerText;
  const caret = row.block ? w.start : w.start + row.insert.length + 1;
  el.setSelectionRange(caret, caret);
  autosize(el, 140);
  sendLit();
  drawSuggestions([]);
}

function templatesMenu(anchor) {
  popover(anchor, h("div", { class: "chat-tpl-panel" },
    h("b", {}, t("Plan templates", "Шаблоны планов")),
    h("div", { class: "chat-tpl-grid" }, templates.map(([en, ru, phrase, , , , colors]) => h("button", { class: "chat-tpl", onclick: () => { document.querySelector(".menu")?.remove(); suggest(phrase); } },
      h("span", { class: "chat-tpl-colors" }, colors.map((c, i) => [i ? h("span", { class: "chat-tpl-joint" }) : null, h("span", { class: "chat-tpl-swatch", style: `background:${c}` })])),
      h("b", {}, t(en, ru)), h("span", { class: "small secondary" }, "«" + phrase + "»"))))), "up");
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
  const chip = h("button", { class: "chat-chip-btn chat-model", title: t(`${s.model.name || ""} · ${s.model.model || ""}. The model for this canvas; it never sees your wallet key.`,
      `${s.model.name || ""} · ${s.model.model || ""}. Модель этого холста; ключ кошелька она никогда не видит.`), onclick: () => modelMenu(chip) },
    c ? companyTile(c, 20) : null, h("span", { class: "one-line" }, name), icon("chevron.down", 9));
  return chip;
}

/** Two columns, so nothing moves under the pointer: the companies, then the chosen one's ways in and models. */
function modelMenu(anchor) {
  const s = ui.state;
  let shown = companyOf(s.model.provider)?.name || companies[0].name;
  const right = h("div", { class: "chat-ways-col" });
  const left = h("div", { class: "chat-companies-col" });
  const draw = () => {
    const list = companiesNow();
    const row = (c) => {
      const ways = s.providers.filter((p) => c.ids.includes(p.id));
      const current = ways.find((p) => p.id === s.model.provider);
      const detail = current ? (s.model.model ? modelName(s.model.model) : current.name) + t(" · in use", " · сейчас")
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
  const way = (p) => {
    const kindIcon = p.kind === "subscription" ? "person.circle" : p.kind === "local" ? "desktop" : "key";
    const current = p.id === s.model.provider;
    if (current && s.model.connected) {
      const models = s.model.models.includes(s.model.model) || !s.model.model ? s.model.models : [...s.model.models, s.model.model];
      return [h("div", { class: "chat-way-head" }, icon(kindIcon, 13), h("b", {}, p.name), h("span", { class: "spacer" }), h("span", {}, t("connected", "подключено"))),
        models.map((id) => h("button", { class: "chat-model-row", onclick: () => { document.querySelector(".menu")?.remove(); act("setModel", { model: id }); } },
          h("span", { class: "one-line" }, modelName(id)), id === s.model.model ? icon("check", 13) : null))];
    }
    // Another way in: a connected one (its key or login already saved, a model on this PC) switches here; one not yet
    // connected goes to Settings, where the key is pasted.
    const quick = p.connected;
    const detail = p.unavailable ? t("not open to apps yet", "пока закрыто для приложений")
      : p.connected ? t("connected", "подключено") + (p.existingLogin ? " · " + p.existingLogin : "") : t("sign-in needed", "нужен вход");
    return h("button", { class: "chat-model-row chat-way", disabled: !!p.unavailable, title: p.unavailable || p.note || "",
      onclick: () => { document.querySelector(".menu")?.remove(); quick ? act("connectModel", { provider: p.id }) : openSettings(); } },
    icon(kindIcon, 13), h("span", { class: "chat-way-text" }, h("b", {}, p.name), h("span", { class: "small secondary" }, detail)));
  };
  draw();
  popover(anchor, h("div", { class: "chat-model-menu" },
    h("div", { class: "chat-model-cols" }, left, right),
    h("div", { class: "chat-model-foot" }, icon("lock.shield", 12), h("span", {}, t("The model never sees your wallet key.", "Модель никогда не видит ключ кошелька.")),
      h("span", { class: "spacer" }), h("button", { class: "link-btn", onclick: openSettings }, t("Sign-ins…", "Входы…")))), "up");
}

/** The networks the assistant works on, as on the Mac: three logos and «+N»; a click lists what it does on each. */
const networkList = [
  ["Solana", "sol", "Jupiter swaps · Jupiter Lend · Kamino · JitoSOL"],
  ["Hyperliquid", "hype", "Perps · oil and stocks (xyz) · HLP vault"],
  ["Base", "base", "Aave · Morpho · USDC"],
  ["NEAR", "near", "NEAR Intents swaps · 1Click"],
  ["Arbitrum", "arb", "Aave · Morpho · USDC in and out"],
  ["Robinhood", "robinhood", "Stock tokens · USDG · Uniswap"],
  ["Polygon", "pol", "Aave · USDC"],
  ["Ethereum", "eth", "Aave · Morpho · mainnet gas varies"],
  ["Tempo", "tempo", "USDC transfers · pay for services (MPP) · fees in USDC"],
  ["HyperEVM", "hyperevm", "USDC in and out · HYPE for gas"],
  ["Optimism", "op", "USDC in and out · Circle's CCTP"],
  ["BNB Chain", "bnb", "USDC in and out · BNB for gas · swaps"],
];
function networksChip() {
  const chip = h("button", { class: "chat-chip-btn", title: t("Networks the assistant works on: ", "Сети, где работает ассистент: ") + networkList.map((n) => n[0]).join(", "),
    onclick: () => popover(chip, h("div", { class: "chat-networks-menu" },
      h("div", { class: "chat-caption chat-split" }, h("span", {}, t("Networks", "Сети")), h("span", {}, `${networkList.length}`)),
      networkList.map(([name, file, uses]) => h("div", { class: "chat-network-row" }, logo(file, 28), h("div", {}, h("b", {}, name), h("div", { class: "small secondary" }, uses))))), "up") },
  h("span", { class: "chat-logo-stack" }, networkList.slice(0, 3).map(([, file]) => logo(file, 20))), h("b", { class: "secondary" }, "+" + (networkList.length - 3)), icon("chevron.down", 9));
  return chip;
}

/** «950», «9.6k», «22k», «1M»: tokens as the Mac's ContextMeter.short writes them. */
function short(n) {
  const trim = (v, d) => String(Number(v.toFixed(d)));
  if (n >= 995000) return trim(n / 1e6, 2) + "M";
  if (n >= 9950) return Math.round(n / 1000) + "k";
  if (n >= 1000) return trim(n / 1000, 1) + "k";
  return String(n);
}

/** How much of the model's window the chat fills (ContextChip), after the first answer. */
function contextChip(s) {
  const c = s.context;
  if (!c || !c.used) return null;
  const share = c.window ? Math.min(1, c.used / c.window) : null;
  const tight = (share || 0) >= 0.8;
  const r = 6.5, len = 2 * Math.PI * r;
  const ring = share === null ? null : svg("svg", { viewBox: "0 0 18 18", class: "chat-ring" },
    svg("circle", { cx: 9, cy: 9, r, fill: "none", "stroke-width": 2.2, class: "chat-ring-back" }),
    svg("circle", { cx: 9, cy: 9, r, fill: "none", "stroke-width": 2.2, class: "chat-ring-fill", "stroke-linecap": "round",
      "stroke-dasharray": `${Math.max(0.5, share * len)} ${len}`, transform: "rotate(-90 9 9)" }));
  const chip = h("button", { class: "chat-chip-btn chat-ctx" + (tight ? " chat-near" : ""),
    title: t("What the model read with your last message, of what its window holds.", "Что модель прочитала с последним сообщением — из того, что вмещает её окно."),
    onclick: () => popover(chip, h("div", { class: "chat-context-pop" },
      h("div", { class: "chat-caption chat-split" }, h("span", {}, t("Context", "Контекст")), share === null ? null : h("span", {}, share < 0.01 ? t("under 1%", "меньше 1%") : Math.round(share * 100) + "%")),
      h("div", { class: "chat-context-big" }, h("b", {}, short(c.used)), h("span", { class: "secondary" }, c.window ? t(`of ${short(c.window)} tokens`, `из ${short(c.window)} токенов`) : t("tokens", "токенов"))),
      share === null ? null : h("div", { class: "chat-context-bar" }, h("span", { style: `width:${Math.max(2, share * 100)}%` })),
      h("div", { class: "small secondary" }, t("The whole chat of this canvas goes with each message. A new canvas starts it empty.", "С каждым сообщением уходит весь чат этого холста. Новый холст начинает его с нуля."))), "up") },
  ring, h("span", {}, short(c.used)), c.window ? h("span", { class: "secondary" }, "/ " + short(c.window)) : null);
  return chip;
}

/** A card of our own beside an anchor, above it by default, kept in the window; Escape or a click outside closes it. */
function popover(anchor, content, side = "up") {
  document.querySelector(".menu")?.remove();
  const pop = h("div", { class: "menu chat-pop" }, content);
  pop.addEventListener("click", (e) => e.stopPropagation());
  document.body.append(pop);
  const a = anchor.getBoundingClientRect(), w = pop.offsetWidth, hgt = pop.offsetHeight;
  const left = Math.max(8, Math.min(a.left, window.innerWidth - w - 8));
  const top = side === "up" && a.top - hgt - 8 > 8 ? a.top - hgt - 8 : Math.min(a.bottom + 8, window.innerHeight - hgt - 8);
  pop.style.left = left + "px";
  pop.style.top = Math.max(8, top) + "px";
  setTimeout(() => document.addEventListener("click", function off(ev) { if (!pop.contains(ev.target)) { pop.remove(); document.removeEventListener("click", off); } }), 0);
  return pop;
}

// MARK: The voice

/** Windows' recognizer listens until a pause; the words go into the field, never sent by themselves. */
async function speak() {
  listening = true;
  listenStart = Date.now();
  clearInterval(listenTimer);
  listenTimer = setInterval(() => {
    const el = document.getElementById("voice-time");
    const sec = Math.floor((Date.now() - listenStart) / 1000);
    if (el) el.textContent = `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, "0")}`;
  }, 1000);
  render();
  try {
    const text = (await invoke("listen")).trim();
    if (text) ui.composerText = ui.composerText ? ui.composerText.trimEnd() + " " + text : text;
  } catch (e) {
    toast(String(e));
  } finally {
    listening = false;
    clearInterval(listenTimer);
    render();
    document.getElementById("composer")?.focus();
  }
}

function listeningRow() {
  return h("div", { class: "chat-listening-row" }, h("span", { class: "chat-rec-dot" }), h("b", {}, t("Listening", "Слушаю")),
    h("span", { class: "small secondary" }, t("· in the keyboard layout's language · recognized by Windows", "· на языке раскладки · распознаёт Windows")));
}

function voicePanel() {
  return h("div", { class: "chat-voice" },
    h("div", { class: "chat-voice-heard secondary" }, t("Say what to do… it stops at a pause.", "Скажи, что сделать… остановится на паузе.")),
    h("div", { class: "chat-voice-row" },
      h("div", { class: "chat-voice-bars", "aria-hidden": "true" }, Array.from({ length: 24 }, (_, i) => h("span", { style: `animation-delay:${(i * 0.07).toFixed(2)}s` }))),
      h("span", { id: "voice-time", class: "num secondary" }, "0:00")));
}
