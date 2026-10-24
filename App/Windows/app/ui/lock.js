// The app's lock (the Mac's LockScreen, Passcode.swift). With a passcode set, the engine sends the window only the lock
// while it's locked, and this draws it over everything: the code field and Windows Hello; a wrong code; the code off
// after five wrong ones, when only Windows Hello opens it. Ctrl+L locks. A typed code goes to the engine, which hands
// it to the signer to check: nothing here keeps it after the try.

import { ui, act, render, t, h, icon, mascot, store } from "./core.js";

// Its own styles, so the lock needs nothing else from the page.
document.head.append(Object.assign(document.createElement("link"), { rel: "stylesheet", href: new URL("./lock.css", import.meta.url).href }));

const LENGTH = 6;
// What's typed so far survives a redraw (a state arrives while typing); it's cleared after each try.
let typed = "";

/** The lock screen while the window is locked, else null: app.js draws it instead of the window. */
let offered = false;
export function lockScreen() {
  const l = ui.state?.lock;
  // Windows Hello stood in for a code that's off: Settings › Wallet opens on making a new one, once.
  if (l?.offerNewCode && !l.locked && !offered) { offered = true; store("settingsTab", "wallet"); ui.view = "settings"; }
  // A later «Forgot the code?» opens it again: the offer is said once each time it's made.
  if (!l?.offerNewCode) offered = false;
  if (!l?.locked) return null;
  // An open dialog (an import, a rename) goes, as the Mac closes its import window: nothing typed there survives the lock.
  ui.overlay = null;
  const hello = h("button", { class: "lk-hello btn primary capsule", onclick: () => act("unlockHello") },
    icon("hello", 18), t("Unlock with Windows Hello", "Открыть через Windows Hello"));
  return h("div", { class: "lk-screen" },
    h("div", { class: "lk-center" },
      h("div", { class: "lk-mascot" }, mascot(108, l.problem ? "error" : "asleep"), h("span", { class: "lk-badge" }, icon("lock", 15))),
      h("div", { class: "lk-words" },
        h("h1", {}, l.codeOff ? t("The code is off", "Код выключен") : t("ihatecrypto is locked", "ihatecrypto заблокирован")),
        h("p", { class: "secondary" }, l.codeOff
          ? t("5 wrong tries. Only Windows Hello opens ihatecrypto now; then make a new code in Settings › Wallet.",
              "5 неверных попыток. Теперь ihatecrypto открывает только Windows Hello; потом задай новый код в «Настройки › Кошелёк».")
          : t("Balances, chat and history stay hidden until you unlock.", "Балансы, чат и история скрыты, пока не откроешь."))),
      l.codeOff
        ? h("div", { class: "lk-off" }, hello,
          h("div", { class: "secondary small" }, t("Windows asks in its own window: ihatecrypto never sees your PIN, face or fingerprint.",
            "Windows спрашивает в своём окне: ihatecrypto не видит ни PIN, ни лицо, ни отпечаток.")),
          l.problem ? h("div", { class: "lk-problem" }, l.problem) : null)
        : h("div", { class: "lk-form" }, field(l),
          l.problem ? h("div", { class: "lk-problem" }, l.problem) : null,
          h("div", { class: "lk-or" }, h("i"), h("span", { class: "secondary" }, t("or", "или")), h("i")),
          hello)),
    l.codeOff ? null : h("div", { class: "lk-forgot secondary" }, t("Forgot the code?", "Забыл код?"), " ",
      h("button", { class: "lk-link", title: t("Windows Hello opens ihatecrypto, then Settings › Wallet makes a new code", "Windows Hello откроет ihatecrypto, потом в «Настройки › Кошелёк» задашь новый код"),
        onclick: () => act("unlockHello", { test: true }) }, t("Unlock with Windows Hello and make a new one", "Открыть через Windows Hello и задать новый"))),
    l.tasksRun ? h("div", { class: "lk-foot secondary" }, icon("clock", 13), t("Scheduled tasks keep running", "Задачи по расписанию продолжают работать")) : null);
}

function field(l) {
  const submit = () => {
    if (typed.length < LENGTH || l.checking) return;
    const code = typed;
    typed = "";
    act("unlock", { text: code });
  };
  const input = h("input", { id: "lk-code", type: "password", inputmode: "numeric", autocomplete: "off", maxlength: LENGTH, value: typed,
    placeholder: t("Enter your passcode", "Введи код"), "aria-label": t("Passcode, 6 digits", "Код, 6 цифр"),
    oninput: (e) => {
      typed = e.target.value.replace(/\D/g, "").slice(0, LENGTH);
      if (e.target.value !== typed) e.target.value = typed;
      go.disabled = typed.length < LENGTH;
      if (typed.length === LENGTH) submit();
    },
    onkeydown: (e) => { if (e.key === "Enter") submit(); } });
  const go = h("button", { class: "lk-go", disabled: typed.length < LENGTH || l.checking, "aria-label": t("Unlock", "Открыть"), onclick: submit },
    l.checking ? h("span", { class: "lk-spin" }) : icon("arrow.right", 16));
  // The field takes the keys as soon as the lock shows.
  queueMicrotask(() => { if (document.activeElement?.id !== "lk-code") input.focus({ preventScroll: true }); });
  return h("div", { class: "lk-field" + (l.problem ? " wrong" : "") }, input, go);
}

// The code off: Return presses «Unlock with Windows Hello», the screen's only way in (the Mac's default action).
document.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && ui.state?.lock?.locked && ui.state.lock.codeOff) { e.preventDefault(); act("unlockHello"); }
});

// Ctrl+L locks, as ⌘L on the Mac: only with a code set, since without one there's nothing to open it with.
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && !e.shiftKey && !e.altKey && (e.key === "l" || e.key === "L") && ui.state?.lock?.codeSet && !ui.state.lock.locked) {
    e.preventDefault();
    act("lockApp");
  }
});

// MARK: Settings › Wallet › Passcode

const CHOICES = [1, 5, 15, 60];

/** 1 min · 5 min · 15 min · 1 hour (the Mac's LockAfterPicker). */
export function lockAfterPicker(l) {
  return h("div", { class: "lk-after", role: "radiogroup" }, CHOICES.map((m) =>
    h("button", { class: m === l.lockAfter ? "on" : "", role: "radio", "aria-checked": String(m === l.lockAfter),
      onclick: () => act("lockAfter", { seconds: m * 60 }) }, m === 60 ? t("1 hour", "1 час") : t(`${m} min`, `${m} мин`))));
}

// The two steps of making a code (the Mac's CodeMaker): type it, then once more; kept here between redraws.
const maker = { first: null, code: "", problem: null, busy: false };

/** Six boxes over a hidden field; the sixth digit moves on. `done` runs once the signer has the code. */
export function codeMaker(l, done) {
  const next = async () => {
    if (maker.code.length < LENGTH || maker.busy) return;
    if (maker.first === null) {
      if (/^(\d)\1{5}$/.test(maker.code)) return reset(t("All the same digit is the first code anyone tries.", "Одну и ту же цифру пробуют первой."));
      if ("0123456789012345".includes(maker.code) || "9876543210987654".includes(maker.code)) return reset(t("A straight run like 123456 is guessed first.", "Ряд вроде 123456 угадывают первым."));
      Object.assign(maker, { first: maker.code, code: "", problem: null });
      return redraw();
    }
    if (maker.code !== maker.first) return reset(t("The two codes differ: type a new one.", "Коды не совпали: введи новый."));
    maker.busy = true;
    redraw();
    // Windows Hello asks first; the signer keeps the code, the engine never stores it.
    const ok = await act("setCode", { text: maker.code });
    Object.assign(maker, { first: null, code: "", problem: null, busy: false });
    if (ok !== null) done(); else redraw();
  };
  const reset = (problem) => { Object.assign(maker, { first: null, code: "", problem }); redraw(); };
  const boxes = h("div", { class: "lk-boxes" + (maker.problem ? " wrong" : "") },
    Array.from({ length: LENGTH }, (_, i) => h("span", { class: (i < maker.code.length ? "filled" : "") + (i === maker.code.length ? " next" : "") })));
  const input = h("input", { id: "lk-make-" + (maker.first === null ? "1" : "2"), class: "lk-hidden", type: "password", inputmode: "numeric", autocomplete: "off",
    maxlength: LENGTH, value: maker.code, "aria-label": t("Passcode, 6 digits", "Код, 6 цифр"),
    oninput: (e) => { maker.code = e.target.value.replace(/\D/g, "").slice(0, LENGTH); e.target.value = maker.code; paint(); if (maker.code.length === LENGTH) next(); } });
  const paint = () => [...boxes.children].forEach((b, i) => { b.className = (i < maker.code.length ? "filled" : "") + (i === maker.code.length ? " next" : ""); });
  const wrap = h("div", { class: "lk-maker" },
    h("div", { class: "eyebrow" }, maker.first === null ? t("Type 6 digits", "Введи 6 цифр") : t("Type them again", "Введи их ещё раз")),
    h("label", { class: "lk-boxes-wrap" }, input, boxes, maker.busy ? h("span", { class: "lk-spin" }) : null),
    maker.problem ? h("div", { class: "lk-problem left" }, maker.problem)
      : maker.first !== null ? h("div", { class: "secondary small" }, icon("hello", 13), t(" Then Windows asks for Windows Hello once, to save the code.", " Потом Windows один раз спросит Windows Hello, чтобы сохранить код.")) : null,
    h("div", { class: "lk-maker-after" }, h("span", { class: "secondary" }, t("Lock after", "Блокировать через")), lockAfterPicker(l),
      h("span", { class: "secondary" }, t("away, and when the PC sleeps", "без действий и когда компьютер засыпает"))));
  queueMicrotask(() => { if (!document.activeElement || document.activeElement === document.body) input.focus({ preventScroll: true }); });
  return wrap;
}
const redraw = () => render();
