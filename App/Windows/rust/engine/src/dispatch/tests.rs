//! The window's requests reach their methods (Engine.swift's `run`), each answered once and followed by a state unless it
//! only reads; the state a fresh engine draws has the keys App/Windows/app/ui/*.js reads; a locked one draws the lock
//! alone. Offline: no signer beside the test (`no-signer-here`), no model, a folder of the test's own.

use std::collections::BTreeSet;
use std::io::Write;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use ihatecrypto_core::alerts::PriceAlert;
use rust_decimal::Decimal;
use serde_json::{Value, json};

use crate::engine::{Engine, Request};
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::{json as wire, store};

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Write for Lines {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

impl Lines {
    fn all(&self) -> Vec<Value> {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }
    fn answer(&self, id: i64) -> Value { self.all().into_iter().find(|l| l["id"] == id).unwrap_or_else(|| panic!("no answer {id}")) }
    fn states(&self) -> Vec<Value> { self.all().into_iter().filter(|l| l["event"] == "state").map(|l| l["data"].clone()).collect() }
}

async fn on_engine(test: impl AsyncFnOnce(Rc<Engine>, Lines)) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = Engine::new(Outbox::writer(lines.clone()), Arc::new(SignerLink::new(store::url("no-signer-here"))), Engine::snapshot);
        test(e, lines).await
    }).await;
}

/// A line as the window writes it, handled, and the state push after it let out.
async fn ask(e: &Engine, line: Value) {
    e.handle(Request::read(&line.to_string()).expect("a request")).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
}

fn keys(v: &Value) -> BTreeSet<String> { v.as_object().expect("an object").keys().cloned().collect() }

/// Every key Swift's Snapshot writes; each is read by App/Windows/app/ui/*.js (`state.X`, `s.X`).
const ALL_KEYS: [&str; 83] = [
    "signer", "wallets", "active", "addresses", "phrase", "portfolio", "positions", "chat", "sent", "thinking", "draft", "card", "site", "options",
    "running", "plan", "model", "providers", "modelChoices", "secondCheck", "jevOpenRouter", "language", "russian", "book", "history", "alerts", "tasks",
    "recurring", "jupiterDCA", "automation", "skills", "memory", "favorites", "canvases", "folders", "canvas", "boards", "runningBoard", "multisigs",
    "multisigDraft", "candidates", "ledgerConnect", "starred", "watching", "watched", "automationUSD", "positionsPnl", "limitWaiting", "inTransit",
    "context", "modelRunning", "agentStep", "optionsMessage", "retryBoard", "quote", "models", "lock", "modules", "trading", "nodes", "stepEdit",
    "schedulePaused", "scheduleRuns", "multisigHeld", "scheduleKey", "moves", "movesDay", "recentRequests", "automationBudget", "stuck", "places",
    "notices", "openAtLogin", "topUp", "appLanguage", "localSetup", "recentlyClosed", "addressExtras", "planAlertExample", "chipsPosition",
    "lastBalance", "build", "updateReady",
];

/// The ones that aren't optional in Swift's Snapshot: always written.
const ALWAYS: [&str; 53] = [
    "signer", "wallets", "addresses", "portfolio", "positions", "chat", "sent", "thinking", "options", "model", "providers", "modelChoices",
    "secondCheck", "jevOpenRouter", "language", "russian", "book", "history", "alerts", "tasks", "recurring", "skills", "memory", "favorites",
    "canvases", "folders", "canvas", "boards", "multisigs", "candidates", "starred", "watching", "watched", "inTransit", "modelRunning",
    "models", "lock", "modules", "trading", "nodes", "schedulePaused", "scheduleRuns", "multisigHeld", "moves", "recentRequests", "stuck", "places",
    "notices", "openAtLogin", "appLanguage", "localSetup", "recentlyClosed", "addressExtras",
];

#[tokio::test]
async fn a_fresh_engines_state_has_the_keys_the_window_reads() {
    on_engine(async |e, _| {
        let s: Value = serde_json::from_slice(&wire::window(&e.snapshot()).unwrap()).unwrap();
        let (all, always): (BTreeSet<String>, BTreeSet<String>) = (ALL_KEYS.map(String::from).into(), ALWAYS.map(String::from).into());
        let have = keys(&s);
        assert!(have.is_subset(&all), "{:?}", have.difference(&all).collect::<Vec<_>>());
        assert!(always.is_subset(&have), "{:?}", always.difference(&have).collect::<Vec<_>>());
        // Nothing yet: Swift's encoder leaves out every nil.
        for k in ["active", "phrase", "draft", "card", "site", "running", "plan", "automation", "automationUSD", "positionsPnl", "context", "stepEdit",
                  "agentStep", "retryBoard", "runningBoard", "multisigDraft", "ledgerConnect", "limitWaiting", "chipsPosition"] {
            assert!(!have.contains(k), "{k} is written on a fresh engine");
        }
        assert_eq!(s["signer"], json!({"ready": false, "notes": [], "waiting": false}));
        assert_eq!((s["language"].clone(), s["appLanguage"].clone(), s["secondCheck"].clone()), (json!("auto"), json!("system"), json!(true)));
        assert_eq!(s["canvas"], s["canvases"][0]["id"]);
        assert_eq!(s["skills"][0]["builtIn"], true);
        assert!(s["modelChoices"]["anthropic"].is_string());
        assert_eq!(keys(&s["lock"]), ["checking", "codeOff", "codeSet", "lockAfter", "locked", "newCodeOffers", "offerNewCode", "tasksRun"].map(String::from).into());
    }).await;
}

#[tokio::test]
async fn a_locked_engine_answers_only_the_locks_requests_and_draws_the_lock_alone() {
    on_engine(async |e, lines| {
        e.st_mut().lock.locked = true;
        ask(&e, json!({"id": 1, "op": "newCanvas"})).await;
        assert_eq!(lines.answer(1), json!({"id": 1, "ok": false, "result": {}, "error": "ihatecrypto is locked."}));
        let state = lines.states().pop().expect("a state");
        assert_eq!(keys(&state), ["language", "lock", "russian"].map(String::from).into());
        assert_eq!(state["lock"]["locked"], true);
        // The lock's own requests go through.
        ask(&e, json!({"id": 2, "op": "signerPid"})).await;
        assert_eq!(lines.answer(2), json!({"id": 2, "ok": true, "result": {"pid": ""}}));
    }).await;
}

#[tokio::test]
async fn each_request_reaches_its_method() {
    on_engine(async |e, lines| {
        // Settings.
        ask(&e, json!({"id": 1, "op": "language", "language": "ru"})).await;
        ask(&e, json!({"id": 2, "op": "secondCheck", "test": false})).await;
        ask(&e, json!({"id": 3, "op": "appLanguage", "language": "system"})).await;
        ask(&e, json!({"id": 4, "op": "lockAfter", "seconds": 900})).await;
        ask(&e, json!({"id": 5, "op": "batches", "test": true})).await;
        {
            let st = e.st();
            assert_eq!((st.settings.language.as_deref(), st.settings.second_check, st.settings.app_language.as_deref()), (Some("ru"), Some(false), None));
            assert_eq!((st.settings.lock_after, st.settings.batches), (Some(15), Some(true)));
        }
        assert_eq!(lines.answer(1), json!({"id": 1, "ok": true, "result": {}}));
        ask(&e, json!({"id": 6, "op": "language", "language": "auto"})).await;
        assert_eq!(e.st().settings.language, None);

        // The lock: a code is checked before the signer, in the chat's language.
        ask(&e, json!({"id": 7, "op": "setCode", "text": "123456"})).await;
        assert_eq!(lines.answer(7)["error"], "A straight run like 123456 is guessed first.");
        // No signer here: its call is refused, and said.
        ask(&e, json!({"id": 8, "op": "clearCode"})).await;
        assert!(lines.answer(8)["error"].as_str().unwrap().contains("The signer didn't start"));

        // The window's own flags.
        ask(&e, json!({"id": 9, "op": "onboarding", "test": true})).await;
        ask(&e, json!({"id": 10, "op": "windowFocus", "test": false})).await;
        ask(&e, json!({"id": 11, "op": "composing", "test": true})).await;
        assert!(e.st().onboarding && !e.st().window_front && e.st().composing);

        // Canvases and blocks.
        let first = e.st().canvas_id.clone();
        ask(&e, json!({"id": 12, "op": "newCanvas"})).await;
        let second = e.st().canvas_id.clone();
        assert!(second != first && e.st().canvases.len() == 2);
        ask(&e, json!({"id": 13, "op": "renameCanvas", "wallet": second, "label": "Work"})).await;
        assert_eq!(e.st().canvases.iter().find(|c| c.id == second).unwrap().name.as_deref(), Some("Work"));
        ask(&e, json!({"id": 14, "op": "addBoard", "name": "portfolio"})).await;
        let board = e.st().boards().iter().find(|b| b.kind == "portfolio").map(|b| b.id.clone()).expect("the block");
        ask(&e, json!({"id": 15, "op": "addBoard", "name": "rocket"})).await;
        assert_eq!(lines.answer(15)["error"], "Unknown block «rocket».");
        // Swift compares the optionals: no block named while no card waits reads as the card's.
        ask(&e, json!({"id": 16, "op": "closeBoard"})).await;
        assert_eq!(lines.answer(16)["error"], "Confirm or cancel the move first.");
        ask(&e, json!({"id": 17, "op": "closeBoard", "board": board})).await;
        assert!(e.st().boards().iter().all(|b| b.id != board));
        ask(&e, json!({"id": 18, "op": "openCanvas", "wallet": first})).await;
        assert_eq!(e.st().canvas_id, first);

        // Lists kept in files: alerts, skills, memory.
        let alert = PriceAlert::new("BTC", true, Decimal::from(120_000), None);
        let id = alert.id.to_string();
        e.set_alerts(|a| a.push(alert));
        ask(&e, json!({"id": 19, "op": "removeAlert", "wallet": id.to_lowercase()})).await;
        assert_eq!(e.st().alerts.len(), 1, "Swift's uuidString is upper case");
        ask(&e, json!({"id": 19, "op": "removeAlert", "wallet": id})).await;
        assert!(e.st().alerts.is_empty());
        ask(&e, json!({"id": 20, "op": "skillOn", "wallet": "trading-study", "test": false})).await;
        assert!(e.st().skills_off.contains("trading-study"));
        ask(&e, json!({"id": 21, "op": "skillOn", "wallet": "trading-study", "test": true})).await;
        assert!(e.st().skills_off.is_empty());
        e.set_memory(|m| m.extend(["a".to_string(), "b".to_string()]));
        ask(&e, json!({"id": 22, "op": "forget", "text": "a"})).await;
        assert_eq!(e.st().memory, vec!["b".to_string()]);

        // Answers that only read: a result, no state after them.
        let before = lines.states().len();
        ask(&e, json!({"id": 23, "op": "blockCatalog"})).await;
        assert!(lines.answer(23)["result"]["list"].as_str().unwrap().starts_with('['));
        ask(&e, json!({"id": 24, "op": "checkAddress", "address": "nope", "network": "base"})).await;
        assert_eq!(lines.answer(24)["ok"], true);
        assert_eq!(lines.states().len(), before);

        // An address that isn't one: refused before anything is watched.
        ask(&e, json!({"id": 25, "op": "watch", "evm": "0x123"})).await;
        assert_eq!(lines.answer(25)["ok"], false);
        assert_eq!(e.st().settings.watch_evm, None);

        // The chat cleared, and an op the engine doesn't know: answered with an error, as Swift's default case.
        e.reply("Hello", None);
        ask(&e, json!({"id": 26, "op": "clearChat"})).await;
        assert!(e.st().messages().is_empty());
        let before = lines.states().len();
        ask(&e, json!({"id": 27, "op": "teleport"})).await;
        assert_eq!(lines.answer(27), json!({"id": 27, "ok": false, "result": {}, "error": "Unknown request «teleport»."}));
        assert_eq!(lines.states().len(), before + 1);
        assert_eq!(lines.all().iter().filter(|l| l.get("id").is_some()).count(), 28, "each request answered once");
    }).await;
}
