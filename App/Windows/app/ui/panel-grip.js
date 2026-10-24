// The panel beside the chat, moved and sized by its header as the Mac's WebPanel: the header drags it left of its
// place (within 40 px of it, it snaps home) and a double click docks it; its left edge drags its width, and a double
// click there gives it its own width again. Both the page's header and the browser's toolbar use it; the app lays the
// panel out (web.rs page_layout) and answers with the width and offset it took, which this page keeps in the main
// page's storage (the Mac's webWidth and webX).

import { store } from "./core.js";

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const KEYS = ["railCollapsed", "chatWidth", "chatX", "chatY", "webWidth", "webX", "panelHidden"];

/** Where the main page has the rail and the chat, and the panel's width and offset (`live` while a drag goes on). */
export function report(live = {}) {
  return invoke("page_layout", {
    folded: store("railCollapsed"), chat: store("chatWidth"), x: store("chatX"), y: store("chatY"),
    width: live.width ?? store("webWidth") ?? 0, offset: live.offset ?? store("webX") ?? 0, hidden: store("panelHidden") === true,
  }).catch(() => null);
}

/** The main page moved the rail or the chat, or something now fills its window: the panel follows. */
export function follow() {
  window.addEventListener("storage", (e) => { if (KEYS.includes(e.key)) report(); });
  report();
}

/** `handle` moves the panel (its buttons and fields keep their clicks), `edge` sizes it. */
export function grip(handle, edge) {
  const own = (e) => !!e.target.closest("button, input, label, a");
  let drag = null, frame = 0, live = null;
  const send = (next) => { live = next; if (!frame) frame = requestAnimationFrame(() => { frame = 0; report(live); }); };
  const start = (e, kind) => {
    if (e.button !== 0 || (kind === "move" && own(e))) return;
    // The screen's coordinates: the panel moves under the pointer while it drags.
    drag = { kind, x: e.screenX, width: window.innerWidth, offset: store("webX") || 0, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
    e.preventDefault();
  };
  const at = (e) => drag.kind === "resize" ? { width: Math.max(380, drag.width - (e.screenX - drag.x)) } : { offset: Math.min(0, drag.offset + e.screenX - drag.x) };
  const move = (e) => {
    if (!drag || (!drag.moved && Math.abs(e.screenX - drag.x) < 4)) return;
    drag.moved = true;
    send(at(e));
  };
  const end = async (e) => {
    if (!drag) return;
    // A cancelled pointer has no place of its own: where it last was.
    const d = drag, next = e.type === "pointercancel" ? live : at(e);
    drag = null;
    if (!d.moved || !next) return;
    cancelAnimationFrame(frame);
    frame = 0;
    // Kept as the app laid it out: no wider than the room, no further left than the rail; home again within 40 px.
    const took = await report(next);
    if (d.kind === "resize") store("webWidth", Math.round(took?.width ?? next.width));
    else {
      const o = took?.offset ?? next.offset;
      store("webX", o > -40 ? 0 : Math.round(o));
    }
    report();
  };
  for (const [el, kind] of [[handle, "move"], [edge, "resize"]]) {
    el.addEventListener("pointerdown", (e) => start(e, kind));
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
  }
  handle.addEventListener("dblclick", (e) => { if (!own(e)) { store("webX", 0); report(); } });
  edge.addEventListener("dblclick", () => { store("webWidth", 0); report(); });
}
