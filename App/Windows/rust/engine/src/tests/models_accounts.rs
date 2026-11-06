//! Models.swift and LocalModels.swift (Chat6Tests, Chat13Tests, Chat14Tests, Model7Tests, Settings4Tests): a model per
//! account, who the chat goes through and whether it's connected, Jev's key, Settings › AI model's switches and Check,
//! Ollama's state for the bar, and the tools run to their end. No model, Ollama or installer runs: state only, and a
//! stand-in script for Shell.

use llm::catalog::{self, Kind};
use llm::{Credential, OAuthTokens};
use serde_json::json;

use super::{Lines, engine};
use crate::engine::{Engine, Request};
use crate::from_core::ExamResult;
use crate::local_models::LocalSetup;
use crate::store;

fn request(op: &str, test: Option<bool>) -> Request { Request { op: op.into(), test, ..Default::default() } }

#[tokio::test]
async fn each_account_keeps_its_model_and_a_subscription_stands_in_for_a_dropped_one() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        e.set_model(Some("some-model"), "openrouter");
        assert_eq!(e.model_of("openrouter"), "some-model");
        e.set_model(None, "openrouter");
        assert_eq!(e.model_of("openrouter"), e.provider("openrouter").unwrap().info().default_model);
        assert!(e.st().settings.models.is_none());
        // A subscription that no longer lists the choice: its first listed model stands in, and the choice comes back.
        let sub = catalog::all().into_iter().map(|p| p.info()).find(|i| i.kind == Kind::Subscription && i.unavailable.is_none()).unwrap().id;
        e.set_model(Some("gone-model"), &sub);
        e.st_mut().live_models.insert(sub.clone(), vec!["live-a".into(), "live-b".into()]);
        assert_eq!(e.model_of(&sub), "live-a");
        e.st_mut().live_models.insert(sub.clone(), vec!["live-a".into(), "gone-model".into()]);
        assert_eq!(e.model_of(&sub), "gone-model");
        // The account the chat is on: a model kept before the per-account choice reads until one is made, and the menu
        // lists it when the catalog doesn't.
        e.set_settings(|s| { s.provider = Some("openrouter".into()); s.model = Some("older/choice".into()) });
        assert_eq!(e.model(), "older/choice");
        assert_eq!(e.models("openrouter").last().map(String::as_str), Some("older/choice"));
        e.set_model(Some("newer/choice"), "openrouter");
        assert!(e.st().settings.model.is_none() && e.model() == "newer/choice");
        assert_eq!(e.model_of("no-such-provider"), "");
    }).await;
}

#[tokio::test]
async fn only_an_account_with_a_login_or_on_this_pc_is_switched_to() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        let before = e.provider_id();
        e.use_account("no-such-provider", None);
        e.use_account("openrouter", Some("x/y"));
        e.use_account("grok-subscription", None);
        assert_eq!(e.provider_id(), before);
        assert_eq!(e.model_of("openrouter"), e.provider("openrouter").unwrap().info().default_model);
    }).await;
}

/// Chat6Tests' rest: a model on this PC is switched to with no login, and a downloaded chat model stands in.
#[tokio::test]
async fn a_model_on_this_pc_is_switched_to_and_a_downloaded_one_stands_in() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        e.use_account("ollama", Some("local-model"));
        assert!(e.provider_id() == "ollama" && e.model() == "local-model");
        e.st_mut().live_models.insert("ollama".into(), vec!["qwen3:32b".into()]);
        e.use_account("ollama", None);
        assert_eq!(e.model(), "qwen3:32b");
    }).await;
}

#[tokio::test]
async fn each_accounts_chosen_model_is_named_as_the_mac_names_it() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        e.set_model(Some("gpt-5.4"), "openrouter");
        let views = e.provider_views();
        let view = views.iter().find(|v| v.id == "openrouter").unwrap();
        assert_eq!(view.chosen, "gpt-5.4");
        assert_eq!(view.names["gpt-5.4"], catalog::display_name("gpt-5.4"));
        for m in &view.models { assert_eq!(view.names[m], catalog::display_name(m)) }
        assert!(view.kind == "apiKey" && view.takes_key && !view.connected && view.sign_in.as_deref() == Some("Sign in with OpenRouter"));
        // Windows' words: this PC, never this Mac; a model here needs no login.
        assert!(views.iter().all(|v| !v.name.contains("this Mac") && !v.note.contains("this Mac") && v.id != "apple"));
        let ollama = views.iter().find(|v| v.id == "ollama").unwrap();
        assert!(ollama.name == "Ollama on this PC" && ollama.kind == "local" && ollama.connected);
        // Its JSON as the window reads it.
        let sent = serde_json::to_value(view).unwrap();
        assert_eq!(sent["defaultModel"], json!(view.default_model));
        assert!(sent.get("existingLogin").is_none() && sent.get("unavailable").is_none());
    }).await;
}

#[tokio::test]
async fn the_chat_is_connected_only_through_a_kept_login_or_a_server_here_with_its_model() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        let view = e.model_view();
        assert!(view.provider.as_deref() == Some("anthropic") && view.model.as_deref() == Some("claude-opus-5-5") && !view.connected);
        assert!(e.llm_client(None).is_none() && e.llm_client(Some("no-such-provider")).is_none());
        // A key kept for OpenRouter: connected, and a client is made without asking anything.
        store::set_credential(Some(Credential::ApiKey("sk-or-test".into())), "openrouter");
        e.set_settings(|s| s.provider = Some("openrouter".into()));
        assert!(e.has_model() && e.model_view().connected && e.llm_client(None).is_some());
        e.disconnect("openrouter");
        assert!(!e.has_model() && store::credentials().is_empty());
        // Ollama here: connected while its server answers and has the model.
        e.set_settings(|s| s.provider = Some("ollama".into()));
        assert!(e.has_model());
        e.st_mut().local_servers.insert("ollama".into(), false);
        assert!(!e.has_model());
        e.st_mut().local_servers.insert("ollama".into(), true);
        e.st_mut().live_models.insert("ollama".into(), vec!["qwen3:8b".into()]);
        assert!(!e.has_model());
        e.set_model(Some("qwen3:8b"), "ollama");
        assert!(e.has_model());
        // The menu: what the server has, then the suggested ones it hasn't.
        let listed = e.models("ollama");
        assert_eq!(listed[0], "qwen3:8b");
        assert_eq!(listed.len(), 1 + e.provider("ollama").unwrap().info().models.len());
    }).await;
}

#[tokio::test]
async fn jev_gets_openrouters_key_only_when_the_chat_is_there_or_it_was_turned_on() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        assert!(e.second_check() && e.jev_key().is_none());
        store::set_credential(Some(Credential::ApiKey("sk-or-test".into())), "openrouter");
        assert!(e.jev_key().is_none());
        e.set_settings(|s| s.jev_open_router = Some(true));
        assert_eq!(e.jev_key().as_deref(), Some("sk-or-test"));
        e.set_settings(|s| s.second_check = Some(false));
        assert!(!e.second_check() && e.jev_key().is_none());
        e.set_settings(|s| { s.second_check = None; s.jev_open_router = None; s.provider = Some("openrouter".into()) });
        store::set_credential(Some(Credential::OAuth(OAuthTokens { access: "oauth-access".into(), refresh: "r".into(), expires: 0.0, account_id: None })), "openrouter");
        assert_eq!(e.jev_key().as_deref(), Some("oauth-access"));
    }).await;
}

#[tokio::test]
async fn what_cant_be_checked_isnt_connected() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        let said = |r: Result<(), wallet::WalletError>| r.unwrap_err().0;
        assert_eq!(said(e.connect("nope", None, None).await), "No model provider «nope».");
        assert!(said(e.connect("grok-subscription", Some("key"), None).await).starts_with("xAI opens subscription sign-in only to partners"));
        assert_eq!(said(e.connect("openrouter", Some("  "), None).await), "Paste the key for OpenRouter.");
        assert_eq!(said(e.sign_in("anthropic").await), "«anthropic» has no browser sign-in: paste its key.");
        // Model7Tests: no login another tool keeps for OpenRouter.
        assert_eq!(said(e.use_login("openrouter")), "No sign-in found on this PC.");
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(said(e.use_login("openrouter")), "На этом компьютере входа не нашлось.");
        assert!(store::credentials().is_empty());
    }).await;
}

#[test]
fn the_checks_line_says_what_it_means() {
    let good: ExamResult = serde_json::from_str(r#"{"passed":9,"total":10,"seconds":2.5,"misses":["a"]}"#).unwrap();
    let bad: ExamResult = serde_json::from_str(r#"{"passed":5,"total":10,"seconds":4,"misses":["send 5 USDC to Mom","b","c","d"]}"#).unwrap();
    let en = |a: &str, _: &str| a.to_string();
    assert_eq!(Engine::exam_line(&good, "Qwen", en), "Qwen understood 9 of 10 requests, 2.5 s a reply — good for the chat.");
    let line = Engine::exam_line(&bad, "Qwen", en);
    assert!(line.contains("only 5 of 10") && line.ends_with("send 5 USDC to Mom; b; c"), "{line}");
    assert_eq!(Engine::exam_line(&good, "Qwen", |_, b| b.to_string()), "Qwen понял 9 из 10 запросов, 2.5 с на ответ — для чата подходит.");
}

/// Settings4Tests: «Think before answering» is kept; Check without a connected account says what's missing; Keep always
/// running shows what Windows said back, not the wish.
#[tokio::test]
async fn the_ai_model_and_background_switches() {
    let dir = store::test_folder();
    let lines = Lines::default();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&lines);
        e.local_op(&request("thinking", Some(true))).await.unwrap();
        assert!(e.local_setup_view().thinking);
        e.check_model("anthropic", None).await;
        let view = e.local_setup_view();
        assert!(view.status.as_deref() == Some("Connect Anthropic API first.") && view.exam_progress.is_none() && view.exam_total == 10);
        e.local_op(&request("keepRunning", Some(true))).await.unwrap();
        assert_eq!(e.local_setup_view().keep_running, None);
        e.local_op(&request("keepRunningState", Some(true))).await.unwrap();
        assert_eq!(e.local_setup_view().keep_running, Some(true));
        // A model's score as Settings shows it.
        let score: ExamResult = serde_json::from_str(r#"{"passed":9,"total":10,"seconds":2.5,"misses":[]}"#).unwrap();
        e.set_settings(|s| { s.model_scores.get_or_insert_default().insert("ollama/qwen3:8b".into(), score); });
        let shown = serde_json::to_value(e.local_setup_view()).unwrap();
        assert_eq!(shown["scores"]["ollama/qwen3:8b"], json!({"passed": 9, "total": 10, "seconds": 2.5, "good": true, "short": "9/10 · 2.5 s", "misses": []}));
        assert_eq!(shown["ollamaState"], "missing");
    }).await;
    let wished: Vec<_> = lines.all().into_iter().filter(|l| l["event"] == "keepRunning").collect();
    assert_eq!(wished, [json!({"event": "keepRunning", "data": {"on": "1"}})]);
    // Restarted on the same settings, the choice is still there.
    let again: crate::store::Settings = store::load("settings.json").unwrap();
    assert_eq!(again.thinking, Some(true));
    drop(dir);
}

#[tokio::test]
async fn an_ollama_only_behind_its_newest_is_not_too_old_for_the_bar() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        {
            let mut st = e.st_mut();
            st.local_servers.insert("ollama".into(), true);
            st.live_models.insert("ollama".into(), vec![]);
            st.local_setup.ollama_version = Some("0.30.0".into());
            st.local_setup.ollama_latest = Some("0.35.1".into());
        }
        // Settings' line offers the update (the Mac's localState) …
        assert_eq!(e.ollama_state(), "outdated");
        // … but the chat's bar keeps «isn't on this PC yet · Download», which updates Ollama first (the Mac's ollamaOutdated).
        assert!(!e.local_setup_view().ollama_outdated);
        // A download refused for an old Ollama: now the bar says it's too old.
        e.st_mut().local_setup.ollama_outdated = true;
        assert!(e.local_setup_view().ollama_outdated);
    }).await;
}

#[tokio::test]
async fn a_download_refused_for_an_old_ollama_offers_the_update() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        {
            let mut st = e.st_mut();
            st.local_servers.insert("ollama".into(), true);
            st.live_models.insert("ollama".into(), vec!["qwen3:8b".into()]);
            st.local_setup = LocalSetup { ollama_updated: true, ollama_outdated: true, ..Default::default() };
        }
        assert!(e.ollama_state() == "outdated" && e.needs_new_ollama());
        e.st_mut().local_setup.ollama_outdated = false;
        assert_eq!(e.ollama_state(), "missing");
        // The chat on Ollama with its model here: ready.
        e.set_settings(|s| s.provider = Some("ollama".into()));
        e.set_model(Some("qwen3:8b"), "ollama");
        assert_eq!(e.ollama_state(), "ready");
    }).await;
}

#[cfg(not(windows))]
#[tokio::test]
async fn the_installers_run_on_windows_only() {
    let said = |r: Result<(), wallet::WalletError>| r.unwrap_err().0;
    assert_eq!(said(crate::local_models::ollama_install(&|_: &str| panic!("no step off Windows")).await), "Ollama installs from the Windows app.");
    assert_eq!(said(crate::local_models::claude_code_install(&|_: &str| panic!("no step off Windows")).await), "Claude Code installs from the Windows app here.");
    assert_eq!(said(crate::local_models::claude_code_sign_in("claude")), "Claude Code signs in from the Windows app here.");
}

/// Shell.run: what a tool printed, or its last line as the error.
#[cfg(unix)]
#[tokio::test]
async fn a_tool_runs_to_its_end_and_its_last_line_is_the_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = store::test_folder();
    let script = dir.0.join("tool.sh");
    std::fs::write(&script, "#!/bin/sh\nif [ \"$1\" = ok ]; then echo done; exit 0; fi\necho working\necho \"it broke: $1\" >&2\nexit 3\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let tool = script.to_string_lossy().to_string();
    assert_eq!(crate::local_models::run(&tool, &["ok"]).await.unwrap(), "done\n");
    assert_eq!(crate::local_models::run(&tool, &["badly"]).await.unwrap_err().0, "it broke: badly");
    assert!(crate::local_models::run(&dir.0.join("none").to_string_lossy(), &[]).await.is_err());
}
