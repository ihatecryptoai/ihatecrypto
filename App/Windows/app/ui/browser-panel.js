// The header of the page beside the canvas, as the Mac's WebPanel has it: back, reload or stop, the page's title and
// host, Copy the link, open it with the wallet (ihatecrypto's browser), open it in the person's own browser, close. It
// moves the page through the app's own commands and never sees the page's contents.

import { h, icon, t, ui, store } from "./core.js";
import { loadError } from "./browser-words.js";

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
// WebView2 gives a page beside another webview no keyboard on a click: ask the app to move it here when it isn't.
addEventListener("pointerdown", () => { if (!document.hasFocus()) invoke("focus_me").catch(() => {}); }, true);
const head = document.getElementById("head");
let s = null;
let copied = false;
let problem = "";

async function go(action) {
  problem = "";
  try { await invoke("page_nav", { action }); } catch (e) { problem = String(e); draw(); }
}

async function copyLink() {
  try { await navigator.clipboard.writeText(s?.url || ""); copied = true; draw(); setTimeout(() => { copied = false; draw(); }, 1500); } catch {}
}

function draw() {
  const url = (() => { try { return new URL(s.url); } catch { return null; } })();
  const host = url?.host || "";
  const secure = url?.protocol === "https:";
  const sub = problem || (s?.error ? loadError(s.error) : host);
  head.replaceChildren(
    h("button", { class: "icon-btn", title: t("Back", "Назад"), disabled: s ? !s.back : true, onclick: () => go("back") }, icon("chevron.left")),
    s?.loading
      ? h("button", { class: "icon-btn", title: t("Stop", "Остановить"), onclick: () => go("stop") }, icon("xmark"))
      : h("button", { class: "icon-btn", title: t("Reload", "Обновить"), onclick: () => go("reload") }, icon("arrow.clockwise")),
    h("span", { class: "panel-lock", title: secure ? t("A secure connection", "Защищённое соединение") : t("Not a secure connection", "Соединение не защищено") },
      icon(secure ? "lock" : "lock.open", 11)),
    h("div", { class: "panel-name" },
      h("b", { class: "one-line" }, s?.title || host),
      h("span", { class: "one-line" + (problem || s?.error ? " panel-error" : "") }, sub)),
    h("button", { class: "icon-btn" + (copied ? " panel-copied" : ""), title: copied ? t("Copied", "Скопировано") : t("Copy the link", "Скопировать ссылку"),
      disabled: !url, onclick: copyLink }, icon(copied ? "check" : "link", 15)),
    h("button", { class: "icon-btn", title: t("Open with your wallet: the site can ask to connect and to sign", "Открыть с кошельком: сайт сможет попросить подключиться и подписать"),
      disabled: !secure, onclick: () => go("wallet") }, icon("wallet.pass", 15)),
    h("button", { class: "icon-btn", title: t("Open in your browser", "Открыть в своём браузере"), disabled: !secure, onclick: () => go("system") }, icon("globe", 15)),
    h("button", { class: "icon-btn", title: t("Close", "Закрыть"), onclick: () => go("close") }, icon("xmark")),
  );
}

let timer = null;
async function refresh() {
  try {
    s = await invoke("page_state");
    ui.state = { russian: s.russian };
    document.documentElement.lang = s.russian ? "ru" : "en";
  } catch {}
  draw();
}
tauri.event.listen("panel", () => { clearTimeout(timer); timer = setTimeout(refresh, 120); });

/** Where the main page has the rail and the chat: this page shares its storage, and hears when either moves. */
function place() {
  invoke("page_layout", { folded: store("railCollapsed"), chat: store("chatWidth"), x: store("chatX"), y: store("chatY") }).catch(() => {});
}
window.addEventListener("storage", (e) => { if (["railCollapsed", "chatWidth", "chatX", "chatY"].includes(e.key)) place(); });
document.addEventListener("keydown", (e) => { if (e.key === "Escape") go("close"); });

place();
draw();
refresh();
