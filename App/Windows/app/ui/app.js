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

import { ui, answered, engine, invoke, render, setRoot, t, toast, h, suggest } from "./core.js";
import { rail } from "./rail.js";
import { windowControls } from "./chrome.js";
import { canvasView } from "./canvas.js";
import { settingsPage, historyPage, topUpPage } from "./pages.js";
import { welcome, phraseScreen, setProtect, onboarding, onboardingRunning, welcomeDone } from "./onboarding.js";
import { lockScreen } from "./lock.js";
// Ctrl+Q to the tray, Ctrl+Shift+Q quits, Ctrl+, opens Settings.
import "./system.js";

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
  } else if (msg.event === "suggest") {
    // A phrase the engine hands the person (the guard's «save <address> as NAME on <network>»): into the field, not sent.
    suggest(msg.data.text);
  } else if (msg.event === "openView") {
    // Typed «top up» / «receive»: the Top up page, as the Mac's showReceive opens its board.
    ui.view = msg.data?.view || msg.view;
    render();
  } else if (msg.event === "whatsNew") {
    // «what's new» typed in the chat.
    import("./whatsnew.js").then((m) => m.openWhatsNew());
  } else if (msg.event === "checkUpdates") {
    // «check for updates» typed in the chat: what Settings › About does.
    import("./pages.js").then((m) => { if (m.checkUpdate) m.checkUpdate(); else { ui.view = "settings"; render(); } });
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
  // Locked: the engine sent the lock alone, and nothing else is drawn.
  const locked = lockScreen();
  if (locked) {
    parts.push(h("div", { class: "ground" }), locked, windowControls());
  } else if (ui.overlay) {
    parts.push(h("div", { class: "ground" }), ui.overlay, windowControls());
  } else if (s.phrase) {
    parts.push(h("div", { class: "ground" }), phraseScreen(), windowControls());
  } else if (onboardingRunning()) {
    // The Mac's staged first run, wallet or not: its step is kept until it's finished.
    setProtect(false);
    parts.push(h("div", { class: "ground" }), onboarding(), windowControls());
  } else if (!s.wallets.length && !s.watching && !welcomeDone()) {
    setProtect(false);
    parts.push(h("div", { class: "ground" }), welcome(), windowControls());
  } else {
    setProtect(false);
    const page = ui.view === "settings" ? settingsPage() : ui.view === "history" ? historyPage() : ui.view === "topup" ? topUpPage() : null;
    parts.push(h("div", { class: "ground" }), page || canvasView(), rail(), windowControls());
  }
  app.replaceChildren(...parts);
  // The page panel's webviews sit over this page: hidden while anything but the canvas fills it (Settings, a sheet,
  // the lock, the first run), as the Mac's WebPanel shows only beside the canvas (panel-grip.js reads it).
  const covered = parts.length !== 4 || ui.view === "settings" || ui.view === "history" || ui.view === "topup";
  if (store("panelHidden") !== covered) store("panelHidden", covered);
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

// A pinch (Ctrl+wheel in WebView2) zooms the canvas only, never the whole page.
document.addEventListener("wheel", (e) => { if (e.ctrlKey) e.preventDefault(); }, { passive: false });

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (document.querySelector(".menu")) { document.querySelector(".menu").remove(); return; }
    if (ui.view !== "canvas") { ui.view = "canvas"; render(); }
  }
});

engine("hello").catch((e) => toast(e.message));
