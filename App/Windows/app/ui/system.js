// The Mac's app-level keys, the Windows way, and the main page's keyboard: Ctrl+Q puts the window in the tray (the Mac's
// ⌘Q, Quit to Menu Bar), Ctrl+Shift+Q quits for real (⌥⌘Q, Quit Completely; not Ctrl+Alt, which is AltGr on many
// layouts), Ctrl+, opens Settings (⌘,). Imported once by app.js.

import { ui, render, invoke, toast } from "./core.js";

document.addEventListener("keydown", (e) => {
  if (!e.ctrlKey || e.altKey || e.metaKey) return;
  const key = e.key.toLowerCase();
  if (key === "q") {
    e.preventDefault();
    invoke(e.shiftKey ? "quit_app" : "hide_main").catch((x) => toast(String(x)));
  } else if (e.key === "," && !e.shiftKey) {
    e.preventDefault();
    ui.view = "settings";
    render();
  }
});

// With the page panel open beside the chat, the keyboard stays in it until moved back (WebView2's child webviews).
addEventListener("pointerdown", () => { if (!document.hasFocus()) invoke("focus_me").catch(() => {}); }, true);
