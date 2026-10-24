// The tray's panel, the Mac's menu-bar popover (AppMain.swift MenuBarContent): what keeps working with the window closed —
// the balance and the positions' P/L, the positions and the stops the guardian manages, the schedule's next tasks with
// how each last ran, the price alerts — and Open / Quit Completely. It reads the engine's state and sends nothing to it.

import { ui, t, h, icon, addIcons, mascot, usd, masked, when } from "./core.js";

addIcons({
  bell: "M6 16v-5a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0",
  shield: "M12 3.5 19 6v5.5c0 4.5-3 7.8-7 9-4-1.2-7-4.5-7-9V6z",
});

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
const root = document.getElementById("tray");

const signed = (text) => (/^[-−]/.test(text || "") ? "loss" : "profit");

function draw() {
  const s = ui.state;
  if (!s) { root.replaceChildren(h("div", { class: "tray-head" }, mascot(28, "thinking"), h("b", {}, "ihatecrypto"))); return measure(); }
  // Locked, the state carries only the lock and the language: nothing of the wallet shows here either.
  if (s.lock?.locked) {
    root.replaceChildren(h("div", { class: "tray-head" }, mascot(28, "asleep"),
      h("div", { class: "tray-title" }, h("b", {}, t("ihatecrypto is locked", "ihatecrypto заблокирован")),
        h("span", { class: "secondary" }, t("Scheduled tasks keep running", "Задачи по расписанию продолжают работать"))),
      h("span", { class: "spacer" }), h("button", { class: "btn small", onclick: open }, t("Open", "Открыть"))));
    return measure();
  }
  const total = (Number(s.portfolio?.total) || 0) + (Number(s.automationUSD) || 0);
  const positions = s.positions || [];
  const scheduled = (s.tasks || []).filter((task) => task.next && !task.paused);
  const alerts = s.alerts || [];
  // As the Mac's: anything on the Schedule counts, a paused task too.
  const quiet = !positions.length && !(s.tasks || []).length && !alerts.length;
  const runs = s.scheduleRuns || [];
  // By the task's id, as the Mac's matches its schedule's id (a run keeps its task, Schedule.swift); by its words only for
  // runs older than that.
  const lastRun = (task) => runs.filter((r) => r.task ? r.task === task.id : (task.steps || []).join(" then ") === r.title).sort((a, b) => (a.at < b.at ? 1 : -1))[0];
  root.replaceChildren(...[
    h("div", { class: "tray-head" }, mascot(28, "idle"),
      h("div", { class: "tray-title" }, h("b", { class: "tray-brand" }, "ihatecrypto"),
        h("span", { class: "secondary" }, quiet ? t("Nothing running", "Ничего не работает") : t("ihatecrypto keeps working here", "ihatecrypto работает здесь"))),
      h("span", { class: "spacer" }),
      h("div", { class: "tray-money" },
        s.portfolio?.updatedAt ? h("b", { class: "num" }, masked(usd(total))) : h("span", { class: "tray-skeleton" }),
        s.positionsPnl != null ? h("span", { class: "tray-pnl num " + (Number(s.positionsPnl) < 0 ? "loss" : "profit") },
          masked((Number(s.positionsPnl) < 0 ? "" : "+") + usd(s.positionsPnl)) + t(" in positions", " в позициях")) : null)),
    positions.map((p) => h("div", { class: "tray-position" },
      h("div", { class: "tray-row" }, h("b", {}, (p.side === "long" ? t("Long ", "Лонг ") : t("Short ", "Шорт ")) + p.coin), h("span", { class: "spacer" }),
        h("b", { class: "num tray-pos-pnl " + signed(p.pnl) }, masked(p.pnl))),
      h("div", { class: "tray-pos-line" }, p.summary || `${p.size} @ ${p.entry}`))),
    positions.filter((p) => p.guardian).map((p) => h("div", { class: "tray-line" }, icon("shield", 12),
      t(`ihatecrypto manages the stop: ${p.coin}`, `Стопом управляет ihatecrypto: ${p.coin}`))),
    scheduled.length ? h("div", { class: "eyebrow tray-eyebrow" }, t("Schedule", "Расписание")) : null,
    scheduled.slice(0, 6).map((task) => {
      const last = lastRun(task);
      const failed = last?.result === "failed";
      return h("button", { class: "tray-task", onclick: () => open("task:" + task.id) },
        icon(task.automatic ? "bolt" : "clock", 12),
        h("div", { class: "tray-title" }, h("b", { class: "one-line" }, task.text),
          h("span", { class: "one-line " + (failed ? "loss" : "secondary") },
            t("next ", "дальше ") + when(task.next) + (last ? (failed ? t(" · last failed ", " · в прошлый раз не вышло ") : t(" · last ran ", " · последний раз ")) + when(last.at) : ""))));
    }),
    scheduled.length > 6 ? h("div", { class: "secondary tray-more" }, t(`+ ${scheduled.length - 6} more on the Schedule`, `+ ещё ${scheduled.length - 6} в «Расписании»`)) : null,
    alerts.map((a) => h("div", { class: "tray-line" }, icon("bell", 12), a.text)),
    h("div", { class: "tray-divider" }),
    h("div", { class: "tray-row" },
      h("button", { class: "btn primary capsule", onclick: open }, t("Open ihatecrypto", "Открыть ihatecrypto")), h("span", { class: "spacer" }),
      h("button", { class: "link-btn", onclick: () => invoke("tray_quit").catch(() => {}) }, t("Quit Completely", "Выйти совсем"))),
    h("div", { class: "secondary tray-note" }, t("Ctrl+Q keeps ihatecrypto here: the schedule, alerts and the guardian go on. Quit Completely stops them until you open it again; the stop and take-profit on the exchange stay.",
      "Ctrl+Q оставляет ihatecrypto здесь: расписание, оповещения и защита позиций продолжают работать. «Выйти совсем» останавливает их до следующего запуска; стоп и тейк на бирже остаются.")),
  ].flat(Infinity).filter(Boolean));
  measure();
}

/** Opens the window; a task's row opens its own canvas too. */
function open(schedule) { invoke("tray_open", typeof schedule === "string" ? { schedule } : {}).catch(() => {}); }

/** The window takes the panel's height, kept by the icon. */
function measure() {
  requestAnimationFrame(() => invoke("tray_layout", { height: Math.ceil(root.getBoundingClientRect().height) + 2 }).catch(() => {}));
}

function take(line) {
  try { const msg = JSON.parse(line); if (msg.event === "state") { ui.state = msg.data; draw(); } } catch {}
}

tauri.event.listen("engine", (e) => take(e.payload));
// Opened again: the last state, and the times («next in 5 min») drawn afresh.
tauri.event.listen("opened", () => invoke("tray_state").then(take).catch(() => {}));
invoke("tray_state").then(take).catch(() => {}).finally(() => { if (!ui.state) draw(); });
// Esc closes the panel, as the Mac's popover; nothing else opens.
document.addEventListener("keydown", (e) => { if (e.key === "Escape") invoke("tray_hide").catch(() => {}); });
