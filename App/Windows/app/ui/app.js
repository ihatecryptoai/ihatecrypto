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

import { ui, answered, engine, invoke, render, setRoot, t, toast, h, suggest, store } from "./core.js";
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
    // A sign-in page the engine asks for (ChatGPT, OpenRouter), in the default browser (a web page only, main.rs open_link).
    invoke("open_link", { url: msg.data.url }).catch((x) => toast(String(x)));
  } else if (msg.event === "notify") {
    toast(msg.data.body);
  } else if (msg.event === "suggest") {
    // A phrase the engine hands the person (the guard's «save <address> as Name»): into the field, not sent.
    suggest(msg.data.text, msg.data.select);
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
  } else if (ui.view === "settings" && (onboardingRunning() || (!s.wallets.length && !s.watching && !welcomeDone()))) {
    // Ctrl+, during Welcome or the first run: Settings over them, as the Mac's opens over both; closing it goes back.
    setProtect(false);
    parts.push(h("div", { class: "ground" }), settingsPage(), windowControls());
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
  // What the window last showed in its own place, for Settings' «opened anew» (pages.js): the lock, a sheet and the
  // recovery words only cover it, as the Mac's lock is an overlay over Settings and its import is inline.
  if (!locked && !ui.overlay && !s.phrase) ui.settingsShown = !!app.querySelector(".pg-settings");
  // What's New (on the body) is out of sight while Settings fills the window and back after, as the Mac's is an overlay
  // of the Studio that Settings replaces (whatsnew.css, whatsnew.js's keys).
  document.body.classList.toggle("in-settings", ui.view === "settings");
  // A chat popover goes with the chat: Settings (Ctrl+, too) replaces it, as the Mac's Settings replaces the Studio.
  if (!app.querySelector(".chat-panel")) for (const m of document.querySelectorAll(".chat-pop")) m.remove();
  // The page panel's webviews sit over this page: hidden while Settings, a sheet, the lock or the first run fills it, as
  // the Mac's Settings replaces the Studio (panel-grip.js reads it) — History and Top up are drawn inside the Studio
  // there, beside its panel, so it stays beside them here too — and while a site's request waits, whose block (and its
  // ticks) would sit under the browser: the Mac's sheet covers the browser.
  const covered = parts.length !== 4 || ui.view === "settings" || !!s.site;
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

// A right-click inside a text field is the field's: its Cut / Copy / Paste (WebView2's menu, kept for fields in keys.rs),
// never a block's «Ask or act in chat», as a text field keeps its own menu on the Mac. Stopped before any block hears it.
const textField = "input:not([type]), input[type=text], input[type=search], input[type=number], input[type=password], input[type=email], input[type=url], textarea, [contenteditable]:not([contenteditable=false])";
document.addEventListener("contextmenu", (e) => { if (e.target.closest?.(textField)) e.stopPropagation(); }, true);

// A pinch (Ctrl+wheel in WebView2) zooms the canvas only, never the whole page.
document.addEventListener("wheel", (e) => { if (e.ctrlKey) e.preventDefault(); }, { passive: false });

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    // The topmost menu first: a row's own menu over the folded Canvases list closes before the list.
    const menus = document.querySelectorAll(".menu");
    if (menus.length) { menus[menus.length - 1].remove(); return; }
    // Under the lock screen or the recovery words nothing behind changes: the Mac's window is disabled under its lock, and
    // the words stay until they're written down (as F10). Settings is still there after unlocking.
    if (ui.state?.lock?.locked || ui.state?.phrase) return;
    // An Import sheet from Settings › Wallet: the Mac's import is inline in Settings, so Esc leaves Settings as from any
    // control and what was typed goes with the sheet (ImportWalletView's onDisappear). Elsewhere a sheet keeps Esc.
    if (ui.overlay) { if (ui.view === "settings") { ui.overlay = null; ui.view = "canvas"; render(); } return; }
    // A field that owns Esc (a node's address, a rename) stops it, as the Mac's sheet closes alone. Settings is left from
    // any other control, as the Mac's hidden cancel button does; elsewhere Esc in a field stays the field's.
    if (e.defaultPrevented || (ui.view !== "settings" && e.target.closest?.("input, textarea, select, [contenteditable]"))) return;
    if (ui.view !== "canvas") { ui.view = "canvas"; render(); }
  }
});

engine("hello").catch((e) => toast(e.message));
