// What's New, the Mac's (WhatsNew.swift): a panel over the dimmed window, opened from the rail's logo or the Profile
// menu. The versions by date on the left, the chosen one's changes on the right; a change opens the version as stories,
// one card at a time — ← → switch, Space pauses, the bars fill in 6 s and move on, Esc goes back to the list. Nothing
// opens by itself: the newest version not seen yet is `whatsNewUnseen()`, for the logo's ring and the menu's dot.

import { ui, t, h, icon, addIcons, suggest, store, render } from "./core.js";
import { releases } from "./whatsnew-data.js";

addIcons({
  sparkles: "M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
  gift: "M4 10h16v3H4zM5.5 13v7.5h13V13M12 10v10.5M12 10S10.5 4.5 8 5s-1 5 4 5zM12 10s1.5-5.5 4-5 1 5-4 5z",
  play: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM10 8.5v7l5.5-3.5z",
});

// Its own stylesheet, so the page's head needs no change: a link to 'self' is what the CSP allows.
if (!document.querySelector('link[href="whatsnew.css"]')) document.head.append(h("link", { rel: "stylesheet", href: "whatsnew.css" }));

const DURATION = 6000;
/** The Mac's SF Symbols as the icons this window draws; anything else gets the sparkles. */
const SYMBOLS = { bolt: "bolt", lock: "lock", "lock.shield": "lock", eye: "eye", "eye.slash": "eye.slash", calendar: "calendar", globe: "globe",
  "rectangle.stack": "rectangle.stack", "square.stack.3d.up": "rectangle.stack", star: "star", gearshape: "gear", "person.crop.circle": "person",
  "clock.arrow.circlepath": "clock", "clock.arrow.2.circlepath": "clock", "calendar.badge.clock": "calendar", "checkmark.seal": "checkmark.seal",
  "arrow.triangle.2.circlepath": "arrow.clockwise", "arrow.down.circle": "arrow.down.circle", "arrow.down.to.line.circle": "arrow.down.circle",
  "wallet.bifold": "wallet.pass", "folder": "rectangle.stack", "key": "lock" };
const symbol = (name) => { const base = (name || "").replace(/\.fill$/, ""); return SYMBOLS[base] || "sparkles"; };
const color = (hex) => "#" + (/^[0-9a-f]{6}$/i.test(hex || "") ? hex : "FF7A2F");

/** «Oct 1, 2026», Gregorian and English, as the Mac's. */
function day(r) {
  if (!r.date) return "";
  const d = new Date(r.date + "T12:00:00Z");
  return isNaN(d) ? "" : d.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
}

/** The newest version not opened yet, or null. */
export function whatsNewUnseen() {
  const newest = releases[0];
  return newest && store("whatsNewSeen") !== newest.version ? newest : null;
}

let panel = null;

export function openWhatsNew() {
  // It belongs over the canvas (the Mac's is an overlay of the Studio): opened from the rail beside Settings, it leaves
  // Settings, and one already open there comes back into sight.
  if (ui.view === "settings") { ui.view = "canvas"; render(); }
  if (panel) return;
  if (releases[0]) store("whatsNewSeen", releases[0].version);
  let chosen = releases[0]?.version;
  let playing = null; // { release, index, watched, resumed, paused, timer }
  const close = () => { stop(); panel?.remove(); panel = null; document.removeEventListener("keydown", keys, true); };

  function list() {
    const release = releases.find((r) => r.version === chosen);
    return h("div", { class: "wn-panel", onclick: (e) => e.stopPropagation() },
      h("div", { class: "wn-head" }, h("h2", {}, t("What's new", "Что нового")), h("span", { class: "spacer" }),
        h("button", { class: "icon-btn", title: t("Close · Esc", "Закрыть · Esc"), onclick: close }, icon("xmark", 14))),
      h("div", { class: "wn-body" },
        h("nav", { class: "wn-versions" }, releases.map((r) => h("button", { class: "wn-version" + (r.version === chosen ? " on" : ""), onclick: () => { chosen = r.version; draw(); } },
          h("b", {}, t("Version ", "Версия ") + r.version), day(r) ? h("span", { class: "secondary" }, day(r)) : null))),
        h("div", { class: "wn-stories" }, (release?.stories || []).map((s, i) => {
          const open = { title: t("Open as stories: ← → to switch", "Открыть историями: ← → — листать") };
          // «Try it» beside the change when a phrase shows it: the phrase into the chat's field, never sent. Any other click
          // in the card (its padding and gaps too, or Enter on its buttons) opens the stories.
          return h("div", { class: "wn-row", onclick: (e) => { if (!e.target.closest(".btn")) play(release, i); } },
            h("button", { class: "wn-row-main", ...open },
              h("span", { class: "wn-tile", style: `background:${color(s.color)}` }, icon(symbol(s.symbol), 18)),
              h("span", { class: "wn-row-text" }, h("b", {}, s.title), h("span", { class: "secondary one-line" }, s.text))),
            s.try ? tryIt(s.try, "btn primary capsule small") : null,
            h("button", { class: "wn-row-play", ...open }, icon("play", 16)));
        }))));
  }

  function tryIt(phrase, cls) {
    return h("button", { class: cls, title: t(`Puts «${phrase}» into the chat`, `Вставит «${phrase}» в чат`), onclick: () => { close(); suggest(phrase); } },
      t("Try it", "Попробовать"));
  }

  function card() {
    const { release, index } = playing;
    const s = release.stories[index];
    const bars = h("div", { class: "wn-bars" }, release.stories.map((_, i) => h("span", {}, h("i", { style: `width:${i < index ? 100 : i === index ? 0 : 0}%`, "data-now": i === index ? "" : null }))));
    return h("div", { class: "wn-stage", onclick: (e) => e.stopPropagation() },
      h("button", { class: "wn-arrow", disabled: index === 0, title: t("Previous · ←", "Назад · ←"), onclick: () => go(-1) }, icon("chevron.left", 18)),
      h("div", { class: "wn-card" },
        h("div", { class: "wn-art", style: `background:${color(s.color)}` }, icon(symbol(s.symbol), 76),
          h("div", { class: "wn-art-top" }, bars,
            h("div", { class: "wn-art-line" }, h("span", {}, t("What's new in ", "Что нового в ") + release.version), h("span", { class: "spacer" }),
              h("button", { class: "wn-close", title: t("Back to the list · Esc", "К списку · Esc"), onclick: back }, icon("xmark", 12))))),
        h("div", { class: "wn-card-text" }, h("h3", {}, s.title), h("p", { class: "secondary" }, s.text), h("span", { class: "spacer" }),
          h("div", { class: "wn-card-foot" },
            s.try ? tryIt(s.try, "btn primary capsule") : null,
            h("span", { class: "spacer" }), h("span", { class: "tertiary num" }, t(`${index + 1} of ${release.stories.length}`, `${index + 1} из ${release.stories.length}`))))),
      h("button", { class: "wn-arrow", title: index + 1 < release.stories.length ? t("Next · →", "Дальше · →") : t("Back to the list · →", "К списку · →"), onclick: () => go(1) }, icon("chevron.right", 18)));
  }

  function draw() {
    const inner = playing ? card() : list();
    panel.replaceChildren(inner);
    if (playing) tick();
  }

  // The card's bar fills while it plays; its time up, the next card.
  function tick() {
    stop();
    if (!playing || playing.paused) return;
    const step = () => {
      const part = Math.min(1, (playing.watched + Date.now() - playing.resumed) / DURATION);
      const bar = panel?.querySelector(".wn-bars [data-now]");
      if (bar) bar.style.width = part * 100 + "%";
      if (part >= 1) { go(1); return; }
      playing.timer = requestAnimationFrame(step);
    };
    playing.timer = requestAnimationFrame(step);
  }
  function stop() { if (playing?.timer) cancelAnimationFrame(playing.timer); }

  function play(release, index) { playing = { release, index, watched: 0, resumed: Date.now(), paused: false }; draw(); }
  function back() { stop(); playing = null; draw(); }
  function go(step) {
    const next = playing.index + step;
    if (next < 0) { playing.watched = 0; playing.resumed = Date.now(); return tick(); }
    if (next >= playing.release.stories.length) return back();
    Object.assign(playing, { index: next, watched: 0, resumed: Date.now(), paused: false });
    draw();
  }
  function pause() {
    if (!playing) return;
    if (playing.paused) { playing.paused = false; playing.resumed = Date.now(); tick(); }
    else { playing.watched += Date.now() - playing.resumed; playing.paused = true; stop(); }
  }

  function keys(e) {
    // Under the lock its keys are the lock screen's (the Mac's What's New is disabled under it), and while Settings fills
    // the window they are Settings' (Esc leaves Settings, and What's New is there again).
    if (ui.state?.lock?.locked || ui.view === "settings") return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); playing ? back() : close(); }
    else if (playing && e.key === "ArrowLeft") { e.preventDefault(); go(-1); }
    else if (playing && e.key === "ArrowRight") { e.preventDefault(); go(1); }
    else if (playing && e.key === " ") { e.preventDefault(); pause(); }
  }

  panel = h("div", { class: "wn-over", onclick: () => (playing ? back() : close()) });
  document.body.append(panel);
  document.addEventListener("keydown", keys, true);
  draw();
}
