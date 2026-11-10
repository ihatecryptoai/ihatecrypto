//! ChatCommands.swift (ChatCommandsTests, Auto7Tests' removed wallet, the per-wallet files): the app's own commands read
//! before any model, a wallet named in the chat, a question answered without a model, what the chat says when the model
//! can't be reached, and each wallet's own tasks and costs. No model, no network.

use std::collections::{HashMap, HashSet};

use ihatecrypto_core::folders::FolderCommand;
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::pnl::CostBasis;
use ihatecrypto_core::trade_history::{Fill, Story};
use rust_decimal::Decimal;
use signer_api::{SignerReply, SignerWallet};
use trade::executor::Position;

use super::{Lines, engine, task};
use crate::engine::{Engine, Message};
use crate::ledger::LedgerAccount;
use crate::{store, task_seal};

fn wallet(id: &str, label: &str, automation: bool) -> SignerWallet {
    SignerWallet { id: id.into(), label: label.into(), kind: "phrase".into(), solana: Some("AN5bQnu1xEuN4Kd3pXzR1Nd7Hk3hJyz8xVq9ce8cL".into()),
                   evm: Some("0x1523a7C1F2c9F0a3E1a0B6dE2Bf5A2c3D4e5F656".into()), near: None, test: None, automation: automation.then_some(true) }
}

fn signer(e: &Engine, active: &str) {
    e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), wallets: Some(vec![wallet("2", "Main", false), wallet("3", "Trading", false), wallet("4", "Automation", true)]),
                                           active_wallet: Some(active.into()), ..Default::default() });
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

fn d(s: &str) -> Decimal { s.parse().unwrap() }

#[tokio::test]
async fn the_apps_own_commands_are_read_before_the_model() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = engine(&lines);
        signer(&e, "2");
        assert!(e.app_command("what's new").await);
        assert!(last(&e).starts_with("What's new is on screen"), "{}", last(&e));
        assert!(e.app_command("что нового в приложении?").await);
        assert!(e.app_command("Check for updates").await);
        assert_eq!(last(&e), "Checking for a new version: the window says what it finds. An update installs only when it's signed with ihatecrypto's key.");
        let events: Vec<String> = lines.all().iter().filter_map(|l| l["event"].as_str().map(str::to_string)).filter(|n| n != "state").collect();
        assert_eq!(events, ["whatsNew", "whatsNew", "checkUpdates"]);
        // A name that's no wallet's lists the ones there are, never the automation wallet.
        assert!(e.app_command("switch to wallet Nope").await);
        assert_eq!(last(&e), "No wallet has that name. Yours: «Main», «Trading».");
        // The wallet that signs already.
        assert!(e.app_command("use wallet main").await);
        assert_eq!(last(&e), "«Main» is already the wallet ihatecrypto signs with.");
        // Anything else goes on to the model; «clear the plan.» with its dot isn't the command (Swift's pattern).
        assert!(!e.app_command("hello").await && !e.app_command("what's new with SOL").await && !e.app_command("clear the plan.").await);
    }).await;
}

#[tokio::test]
async fn a_wallet_is_named_by_its_label_or_how_it_starts() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        signer(&e, "2");
        e.set_ledger(|l| *l = Some(LedgerAccount { evm: Some("0x00000000000000000000000000000000000000aa".into()), ..Default::default() }));
        let ids = |w: Vec<SignerWallet>| w.into_iter().map(|w| w.id).collect::<Vec<_>>();
        assert_eq!(ids(e.switchable_wallets()), ["2", "3", "ledger"]);
        assert_eq!(e.wallet_switch("switch to wallet trad").unwrap().unwrap().id, "3");
        assert_eq!(e.wallet_switch("переключись на кошелёк «Main»").unwrap().unwrap().id, "2");
        assert_eq!(e.wallet_switch("Use wallet Ledger.").unwrap().unwrap().id, "ledger");
        assert_eq!(e.wallet_switch("use wallet Automation"), Some(None));
        assert_eq!(e.wallet_switch("what is my wallet"), None);
    }).await;
}

#[test]
fn a_wallet_is_described_by_what_it_signs_for() {
    let en = |a: &str, _: &str| a.to_string();
    let ru = |_: &str, b: &str| b.to_string();
    assert_eq!(Engine::describe_wallet(&wallet("2", "Main", false), en), "recovery phrase · Solana AN5b…e8cL, EVM 0x15…F656");
    let key = SignerWallet { kind: "solana".into(), evm: None, ..wallet("5", "Key", false) };
    assert_eq!(Engine::describe_wallet(&key, ru), "только ключ Solana · Solana AN5b…e8cL");
    assert_eq!(Engine::describe_wallet(&LedgerAccount { evm: Some("0x1523a7C1F2c9F0a3E1a0B6dE2Bf5A2c3D4e5F656".into()), ..Default::default() }.wallet(), en),
               "Ledger, confirm on the device · EVM 0x15…F656");
}

fn fill(px: &str, sz: &str, side: &str, start: &str, pnl: &str, fee: &str) -> Fill {
    Fill { coin: "ETH".into(), px: d(px), sz: d(sz), side: side.into(), time: chrono::Utc::now(), start_position: d(start), dir: String::new(),
           closed_pnl: d(pnl), fee: d(fee), hash: None }
}

#[test]
fn a_question_without_a_model_is_answered_from_the_positions_and_the_closed_trades() {
    let en = |a: &str, _: &str| a.to_string();
    let ru = |_: &str, b: &str| b.to_string();
    let markets: HashSet<String> = ["ETH".to_string(), "SOL".to_string()].into();
    let open = [Position { coin: "ETH".into(), size: d("1.5"), entry: d("2000"), unrealized_pnl: d("10"), liquidation: None }];
    let closed = [Story { coin: "ETH".into(), long: true, fills: vec![fill("1900", "1", "B", "0", "0", "0.5"), fill("2000", "1", "A", "1", "100", "0.5")] }];
    assert_eq!(Engine::local_answer("what happened to my ETH position?", &open, &closed, &markets, en),
               "Your long ETH is open: 1.5 at 2,000, unrealized $10.00.");
    let answer = Engine::local_answer("what happened to my ETH position?", &[], &closed, &markets, en);
    assert!(answer.starts_with("The long ETH is closed: it closed ") && answer.ends_with(" at 2,000, realized $99.00 after fees. The whole story is in History › Positions."), "{answer}");
    assert!(Engine::local_answer("что с ETH?", &[], &closed, &markets, ru).starts_with("Лонг ETH закрыт: закрылся "));
    assert_eq!(Engine::local_answer("и что с SOL?", &[], &closed, &markets, en), "There is no SOL position, open or closed, on Hyperliquid.");
    assert_eq!(Engine::local_answer("how am I doing", &[], &closed, &markets, en), "No open positions. The last one: long ETH, realized $99.00.");
    assert_eq!(Engine::local_answer("how am I doing", &open, &closed, &markets, ru), "Открыты: лонг ETH $10.00. Для остального подключи модель в настройках.");
}

#[tokio::test]
async fn a_question_asked_without_a_model_is_answered_never_as_a_plan() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        e.st_mut().markets.insert("ETH".into(), Market::new("ETH", d("2000"), 4, 25));
        // No EVM address: the closed trades aren't asked for.
        e.answer_without_model("what happened to my ETH position?").await;
        let said = e.st().messages().last().cloned().unwrap();
        assert_eq!(said.text, "There is no ETH position, open or closed, on Hyperliquid.");
        assert_eq!(said.note.as_deref(), Some("from your portfolio and history"));
    }).await;
}

#[tokio::test]
async fn the_model_out_of_reach_says_why_and_nothing_moves() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        e.model_unreachable("buy SOL", "", true, true).await;
        let said = e.st().messages().last().cloned().unwrap();
        assert_eq!(said.text, "claude-opus-5-5 answered, but not in a form ihatecrypto can read, so nothing ran. Say it another way, or pick another model.");
        assert_eq!(said.note.as_deref(), Some("claude-opus-5-5"));
        // Typed, but nothing in it reads without a model: it isn't acted on.
        // A failure the engine knows is said in plain words with what to do, never the service's own text.
        e.model_unreachable("buy SOL for 5 USDC", &format!("HTTP 500: {}", "x".repeat(200)), false, true).await;
        assert_eq!(last(&e), "claude-opus-5-5 didn’t answer. Anthropic is overloaded or down right now. Send it again a little later. \
                              Or pick another model. I didn’t act on this — nothing moved.");
        e.model_unreachable("buy SOL for 5 USDC", r#"HTTP 402: {"error":{"message":"Insufficient Balance"}}"#, false, true).await;
        assert!(last(&e).starts_with("claude-opus-5-5 didn’t answer. Anthropic says this account has no credits"), "{}", last(&e));
        for (long, short) in [("Claude (subscription via Claude Code)", "Claude"), ("Ollama on this PC", "Ollama"), ("OpenAI API", "OpenAI"), ("NEAR AI Cloud (private)", "NEAR AI Cloud")] {
            assert_eq!(Engine::service_name(long), short);
        }
        // One it doesn't know keeps the service's own words, cut short.
        e.model_unreachable("buy SOL for 5 USDC", &format!("weird: {}", "x".repeat(200)), false, true).await;
        let text = last(&e);
        assert!(text.starts_with("I couldn’t reach claude-opus-5-5 (weird: xxx") && text.ends_with("x), so I didn’t act on this — nothing moved. Send it again, or pick another model."), "{text}");
        assert_eq!(text.chars().filter(|c| *c == 'x').count(), 140 - "weird: ".len());
        // A model on this PC that isn't downloaded: how to get it.
        e.set_settings(|s| s.provider = Some("ollama".into()));
        e.model_unreachable("hi", "model 'gemma4:26b' not found", false, false).await;
        assert_eq!(last(&e), "I couldn’t reach gemma4:26b (it isn't downloaded — run «ollama pull gemma4:26b» in a terminal), so I didn’t act on this — nothing moved. Send it again, or pick another model.");
        e.set_settings(|s| s.language = Some("ru".into()));
        e.model_unreachable("hi", "model not found", false, false).await;
        assert_eq!(last(&e), "Не достучался до gemma4:26b (она не скачана — выполни в терминале «ollama pull gemma4:26b»), поэтому ничего не сделал — ничего не ушло. Отправь ещё раз или выбери другую модель.");
    }).await;
}

#[tokio::test]
async fn what_reads_without_the_network_reads_so() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        assert_eq!(e.movers_facts(), "Markets aren't loaded yet.");
        assert!(e.book_plan_facts("ETH", ihatecrypto_core::intent::Side::Long).is_none());
        assert!(!e.read_displays("hello there").await);
    }).await;
}

#[tokio::test]
async fn each_wallet_loads_its_own_tasks_and_costs_and_a_changed_list_is_paused() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        task_seal::save(&[task(&["buy SOL for 5 USDC"])], "tasks-3.json", task_seal::KEY);
        store::save(&HashMap::from([("base:0xa".to_string(), CostBasis::default())]), "costs-3.json");
        e.st_mut().chain_cost.insert("x".into(), crate::engine::ChainCost { held: Decimal::ONE, cost: CostBasis::default() });
        signer(&e, "3");
        e.load_wallet_state();
        {
            let st = e.st();
            assert_eq!((st.wallet_scope_tasks.as_deref(), st.wallet_scope_costs.as_deref()), (Some("3"), Some("3")));
            assert!(st.tasks.len() == 1 && st.tasks[0].paused != Some(true) && st.app_costs.contains_key("base:0xa") && st.chain_cost.is_empty());
        }
        // A list written outside ihatecrypto: every task paused, and said.
        std::fs::write(store::url("tasks-4.json"), serde_json::to_vec(&[task(&["sell SOL"])]).unwrap()).unwrap();
        signer(&e, "4");
        e.load_wallet_state();
        assert!(e.st().tasks.iter().all(|t| t.paused == Some(true)));
        assert!(last(&e).starts_with("This wallet's scheduled tasks were changed outside ihatecrypto, so every one is paused."));
        // The same word again loads nothing twice.
        e.st_mut().messages_mut().clear();
        e.load_wallet_state();
        assert!(e.st().messages().is_empty());
    }).await;
}

#[tokio::test]
async fn a_removed_wallets_tasks_and_costs_go_with_it() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        // Wallet «1» loaded, its list left empty in the signer: it still reads as the wallet that's on.
        e.set_tasks(|t| t.push(task(&["buy SOL for 5 USDC"])));
        e.set_app_costs(|c| { c.insert("base:0xa".into(), CostBasis::default()); });
        std::fs::write(store::url("recurring-buys.json"), "[]").unwrap();
        task_seal::save(&[task(&["buy ETH"])], "tasks-94.json", task_seal::KEY);
        e.wallet_removed("94");
        assert!(!store::url("tasks-94.json").exists() && !store::url("tasks-94.seal").exists());
        assert_eq!(e.st().tasks.len(), 1);
        e.wallet_removed("");
        assert!(store::url("recurring-buys.json").exists());
        e.wallet_removed(SignerWallet::FIRST_ID);
        assert!(e.st().tasks.is_empty() && e.st().app_costs.is_empty() && !store::url("recurring-buys.json").exists());
        assert!(task_seal::load(task_seal::TASKS, task_seal::KEY).0.is_empty());
    }).await;
}

/// The rest of ChatCommandsTests: a note makes a fresh canvas its own, a note from a busy canvas gets a canvas of its own
/// and a card with «Open», the next one joins it, a typo is the folder there; «clear the plan».
#[tokio::test]
async fn notes_folders_and_the_plan_cleared() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        let first = e.st().canvas_id.clone();
        e.st_mut().messages_mut().push(Message::new(true, "write to folder Ideas: buy the dip on SOL", None));
        e.folder_command(FolderCommand::Note { folder: "Ideas".into(), text: "buy the dip on SOL".into() }, None);
        assert!(last(&e).contains("Saved the note to «Ideas»"));
        {
            let st = e.st();
            let own = st.boards().iter().find(|b| b.kind == "note").unwrap();
            assert!(own.title == "Ideas" && own.lines == ["buy the dip on SOL"] && own.opens.is_none());
            assert_eq!(st.canvases.iter().find(|c| c.id == first).unwrap().folder.as_deref(), Some("Ideas"));
        }
        e.new_canvas(false).unwrap();
        let second = e.st().canvas_id.clone();
        e.st_mut().messages_mut().push(Message::new(true, "write to folder Idaes: second idea", None));
        e.st_mut().messages_mut().push(Message::new(true, "and more", None));
        e.folder_command(FolderCommand::Note { folder: "Idaes".into(), text: "second idea".into() }, None);
        let target = e.st().boards().iter().find(|b| b.kind == "note").unwrap().opens.clone().unwrap();
        assert!(target != second && target != first);
        assert_eq!(e.st().canvases.iter().find(|c| c.id == target).unwrap().folder.as_deref(), Some("Ideas"));
        e.folder_command(FolderCommand::Note { folder: "Ideas".into(), text: "third".into() }, None);
        assert_eq!(e.st().boards().iter().find(|b| b.kind == "note").unwrap().lines, ["second idea", "third"]);
        e.folder_command(FolderCommand::Save { folder: "Trading".into() }, None);
        assert_eq!(e.st().canvases.iter().find(|c| c.id == second).unwrap().folder.as_deref(), Some("Trading"));
        assert!(e.app_command("clear the plan").await);
        assert_eq!(last(&e), "The plan is cleared. Tell me what to do next.");
    }).await;
}
