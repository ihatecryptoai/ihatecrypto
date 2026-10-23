// The window's own title strip. The window has no system title bar (tauri.conf.json: decorations off), as the Mac's
// has none: the rail and the canvas go up to the top edge. Windows' minimize, maximize and close sit at the top right,
// drawn like Windows 11's; a thin strip along the top edge and the rail's first row move the window, and a double-click
// there maximizes it (Tauri's drag region).

import { h, svg, t } from "./core.js";

const win = window.__TAURI__.window.getCurrentWindow();
let maximized = false;
win.isMaximized().then((m) => { maximized = m; paint(); }).catch(() => {});
win.onResized(() => win.isMaximized().then((m) => { if (m !== maximized) { maximized = m; paint(); } })).catch(() => {});

/** The middle button's glyph follows the window: a square, or two for «restore». */
function paint() {
  const b = document.querySelector(".chrome-caption .max");
  if (!b) return;
  b.replaceChildren(glyph(maximized ? "restore" : "max"));
  b.title = maximized ? t("Restore down", "Свернуть в окно") : t("Maximize", "Развернуть");
}

/** Segoe Fluent's caption glyphs, drawn 10 px with a 1 px line so they look the same on every machine. */
function glyph(name) {
  const d = { min: "M0 5.5h10", max: "M.5.5h9v9h-9z", restore: "M.5 2.5h7v7h-7zM2.5 2.5v-2h7v7h-2", close: "M.5.5l9 9M9.5.5l-9 9" }[name];
  return svg("svg", { viewBox: "0 0 10 10", width: 10, height: 10, fill: "none", stroke: "currentColor", "stroke-width": "1", "shape-rendering": "geometricPrecision", "aria-hidden": "true" },
    svg("path", { d }));
}

/** Width the caption buttons take at the top right: the canvas keeps its own buttons left of it. */
export const captionWidth = 3 * 46;

export function windowControls() {
  const button = (cls, name, title, run) => h("button", { class: "chrome-btn " + cls, title, "aria-label": title, tabindex: "-1", onclick: run }, glyph(name));
  const controls = h("div", { class: "chrome-root" },
    h("div", { class: "chrome-drag", "data-tauri-drag-region": true }),
    h("div", { class: "chrome-caption" },
      button("min", "min", t("Minimize", "Свернуть"), () => win.minimize()),
      button("max", maximized ? "restore" : "max", maximized ? t("Restore down", "Свернуть в окно") : t("Maximize", "Развернуть"), () => win.toggleMaximize()),
      // Closing keeps ihatecrypto in the tray (main.rs), as the Mac's goes to the menu bar.
      button("close", "close", t("Close", "Закрыть"), () => win.close())));
  return controls;
}
