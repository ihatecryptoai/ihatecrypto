// The window's page: it draws what the engine says (the «state» event after every change) and sends it what the person
// did. The layout is the Mac's Studio (Studio.swift): one dotted ground; the wallet rail floating on it as a glass panel
// that folds to a strip; the canvas with the portfolio and the blocks; the chat floating over the canvas. Each part is
// its own module:
//
//   rail.js        the rail (wide and folded), its balance, Top up, the canvases, the model accounts
//   chrome.js      the window's own title strip: Windows' minimize, maximize and close, the drag area
//   canvas.js      the canvas: pan and zoom, the zoom pill, «Add block» and the block library
//   portfolio.js   the portfolio frame and its network cards
//   boards.js      the blocks on the canvas: a move's route cards, results, charts, live blocks
//   chat.js        the floating chat and its composer
//   pages.js       Settings, History and Top up, which take the canvas's place
//   onboarding.js  the first run, the recovery phrase, importing, watching, small dialogs

import { ui, answered, engine, invoke, render, setRoot, t, toast, h } from "./core.js";
import { rail } from "./rail.js";
import { windowControls } from "./chrome.js";
import { canvasView } from "./canvas.js";
import { settingsPage, historyPage, topUpPage } from "./pages.js";
import { welcome, phraseScreen, setProtect } from "./onboarding.js";

const app = document.getElementById("app");

window.__TAURI__.event.listen("engine", (e) => {
  let msg;
  try { msg = JSON.parse(e.payload); } catch { return; }
  if (msg.event === "state") {
    ui.state = msg.data;
    render();
  } else if (msg.event === "openURL") {
    // A sign-in page the engine asks for (ChatGPT, OpenRouter): https only, in the default browser.
    invoke("open_link", { url: msg.data.url }).catch((x) => toast(String(x)));
  } else if (msg.event === "notify") {
    toast(msg.data.body);
  } else if (msg.event === "engineStopped") {
    toast(t("The engine stopped. Restart ihatecrypto.", "Движок остановился. Перезапусти ihatecrypto."));
  } else {
    answered(msg);
  }
});

/** Scroll positions of the parts marked `data-keep="<name>"`; `data-stick="bottom"` follows new lines when at the end. */
function keepScroll() {
  const kept = new Map();
  for (const el of app.querySelectorAll("[data-keep]")) {
    kept.set(el.dataset.keep, { top: el.scrollTop, left: el.scrollLeft, atBottom: el.scrollHeight - el.scrollTop - el.clientHeight < 40 });
  }
  return () => {
    for (const el of app.querySelectorAll("[data-keep]")) {
      const k = kept.get(el.dataset.keep);
      if (el.dataset.stick === "bottom" && (!k || k.atBottom)) { el.scrollTop = el.scrollHeight; continue; }
      if (k) { el.scrollTop = k.top; el.scrollLeft = k.left; }
    }
  };
}

function draw() {
  const s = ui.state;
  // What the person is in the middle of survives a new state: the field they type in, where the caret is, the scroll.
  const restore = keepScroll();
  const active = document.activeElement;
  const focused = active?.id;
  const selection = focused && "selectionStart" in active ? [active.selectionStart, active.selectionEnd] : null;
  const parts = [];
  if (ui.overlay) {
    parts.push(h("div", { class: "ground" }), ui.overlay, windowControls());
  } else if (s.phrase) {
    parts.push(h("div", { class: "ground" }), phraseScreen(), windowControls());
  } else if (!s.wallets.length && !s.watching) {
    setProtect(false);
    parts.push(h("div", { class: "ground" }), welcome(), windowControls());
  } else {
    setProtect(false);
    const page = ui.view === "settings" ? settingsPage() : ui.view === "history" ? historyPage() : ui.view === "topup" ? topUpPage() : null;
    parts.push(h("div", { class: "ground" }), page || canvasView(), rail(), windowControls());
  }
  app.replaceChildren(...parts);
  restore();
  if (focused) {
    const el = document.getElementById(focused);
    if (el) {
      el.focus({ preventScroll: true });
      if (selection && "setSelectionRange" in el) { try { el.setSelectionRange(selection[0], selection[1]); } catch {} }
    }
  }
}
setRoot(draw);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (document.querySelector(".menu")) { document.querySelector(".menu").remove(); return; }
    if (ui.view !== "canvas") { ui.view = "canvas"; render(); }
  }
});

engine("hello").catch((e) => toast(e.message));
