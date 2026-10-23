// A stand-in for Tauri and the engine, for drawing the window in a plain browser: the state comes from a fixture, and
// every request is answered «ok» with the same state again. Nothing here signs or reaches a network.
(function () {
  const q = new URLSearchParams(location.search);
  const theme = q.get("theme");
  if (theme) document.documentElement.dataset.theme = theme;
  try {
    localStorage.clear();
    const preset = JSON.parse(q.get("store") || "{}");
    for (const [k, v] of Object.entries(preset)) localStorage.setItem(k, JSON.stringify(v));
  } catch {}
  let listener = null;
  let state = null;
  const fixture = fetch("../preview/fixtures/" + (q.get("fixture") || "main") + ".json").then((r) => r.json()).then((s) => (state = s));
  const emit = (obj) => listener && listener({ payload: JSON.stringify(obj) });
  window.__previewLog = [];
  const win = {
    label: "main",
    minimize: async () => {}, toggleMaximize: async () => {}, maximize: async () => {}, unmaximize: async () => {}, close: async () => {},
    isMaximized: async () => q.get("maximized") === "1", startDragging: async () => {}, onResized: async () => () => {},
  };
  // &view=settings|history|topup opens that page; &call=onboarding.showWatch runs a part's function (a dialog), once the
  // first state is drawn. The modules are the page's own (the same URLs; a classic script resolves import() from its own place, preview/), so this changes the page's real `ui`.
  const view = q.get("view"), call = q.get("call");
  if (view || call) {
    fixture.then(() => setTimeout(async () => {
      const core = await import("../ui/core.js");
      if (view) { core.ui.view = view; core.render(); }
      if (call) { const [mod, fn] = call.split("."); (await import("../ui/" + mod + ".js"))[fn](); }
    }, 300));
  }
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        window.__previewLog.push([cmd, args]);
        if (cmd === "engine_send") {
          const req = JSON.parse(args.line);
          await fixture;
          setTimeout(() => {
            if (req.op === "hello") emit({ event: "state", data: state });
            else { emit({ id: req.id, ok: true, result: null }); emit({ event: "state", data: state }); }
          }, 0);
          return null;
        }
        return null;
      },
    },
    event: { listen: async (name, fn) => { if (name === "engine") listener = fn; return () => {}; } },
    window: { getCurrentWindow: () => win },
  };
})();
