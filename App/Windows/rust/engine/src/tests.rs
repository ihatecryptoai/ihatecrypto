//! Layer 1, offline: every file the engine keeps reads in the shape Swift's JSONEncoder wrote and writes back the same;
//! the window's state with the keys App/Windows/app/ui reads; the lock's rules; TaskSeal (TaskSealTests); one state push
//! per step. Each test keeps its files in a folder of its own (`store::test_folder`), never the real one.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use ihatecrypto_core::foundation::{Date, Uuid};
use ihatecrypto_core::intent::TradeIntent;
use ihatecrypto_core::scheduled_task::ScheduledTask;
use rust_decimal::Decimal;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use signer_api::{SignerReply, SignerWallet};
use tokio::sync::oneshot;

use crate::automation::RecurringView;
use crate::blocks::*;
use crate::browser::SiteAsk;
use crate::canvas::{Board, CanvasDoc};
use crate::chat_turns::{ModelsView, QuoteView};
use crate::consent::SiteAskView;
use crate::engine::{ANSWERS_ONLY, Card, ChainStep, Engine, Message, Request};
use crate::favorites::FavoriteToken;
use crate::folders::{CanvasIcon, FolderStore, FolderView};
use crate::from_core::ExamResult;
use crate::hidden_tokens::TransitView;
use crate::history_moves::{MoveRow, MovesDay};
use crate::history_tabs::{Activity, HistoryStore, Saved};
use crate::ledger::{LedgerAccount, LedgerConnect};
use crate::local_models::{LocalSetupView, from_claude, ollama_older, signed_by_ollama};
use crate::lock::{LockView, localized_code_error, locks_now};
use crate::models::{ModelView, ProviderView};
use crate::modules::ModulesView;
use crate::moves::HistoryLine;
use crate::multisig::{Multisig, MultisigDraft, MultisigKey, MultisigNetHolding};
use crate::nodes::NodesView;
use crate::outbox::Outbox;
use crate::place_facts::PlaceFacts;
use crate::portfolio::PortfolioView;
use crate::profit::EquityPoint;
use crate::route_edit::{self, Edit};
use crate::schedule::{AutomationBudget, ScheduleRun};
use crate::signer_link::SignerLink;
use crate::snapshot::*;
use crate::store::{self, Settings};
use crate::top_up::{PageRequest, TopUpView};
use crate::trade_plan::PositionRuler;
use crate::trading_agent::{TradingView, trading_styles};
use crate::trading_key::TradingKeyView;
use crate::wallet_ops::StuckView;
use crate::{json as wire, task_seal};

// MARK: Helpers

/// Numbers as numbers: Swift writes a whole Double as «5», serde as «5.0»; both read back the same.
fn norm(v: Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().expect("a number")),
        Value::Array(a) => Value::Array(a.into_iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, norm(v))).collect()),
        v => v,
    }
}

/// `swift` (a file as Swift's JSONEncoder wrote it) reads as `T` and writes back the same JSON.
fn same<T: Serialize + DeserializeOwned>(swift: &str) -> T {
    let value: T = serde_json::from_str(swift).unwrap_or_else(|e| panic!("{e}: {swift}"));
    assert_eq!(norm(serde_json::to_value(&value).unwrap()), norm(serde_json::from_str(swift).unwrap()), "{swift}");
    value
}

/// The same through the store: written as Swift did, loaded, saved, read back as JSON.
fn through_store<T: Serialize + DeserializeOwned>(name: &str, swift: &str) -> T {
    std::fs::write(store::url(name), swift).unwrap();
    let value: T = store::load(name).unwrap_or_else(|| panic!("{name} didn't load"));
    store::save(&value, name);
    let back: Value = serde_json::from_slice(&std::fs::read(store::url(name)).unwrap()).unwrap();
    assert_eq!(norm(back), norm(serde_json::from_str(swift).unwrap()), "{name}");
    value
}

fn keys(v: &Value) -> BTreeSet<String> { v.as_object().expect("an object").keys().cloned().collect() }

fn set(list: &[&str]) -> BTreeSet<String> { list.iter().map(|s| s.to_string()).collect() }

/// 2025-11-06 12:00:00.5 UTC as a Swift Date's Double.
const AT: f64 = 784_123_200.5;

fn at() -> Date { wire::from_reference(AT).unwrap() }

/// What a test's Outbox wrote.
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
}

// MARK: The engine's files

#[test]
fn settings_read_and_write_back_as_swift_wrote_them() {
    let _dir = store::test_folder();
    let s: Settings = through_store("settings.json", r#"{"language":"ru","appLanguage":"en","provider":"anthropic","model":"claude-opus-5-5",
        "models":{"anthropic":"claude-opus-5-5"},"secondCheck":true,"jevOpenRouter":false,"canvas":"5D1F0B5E-0000-4000-8000-000000000001",
        "watchSolana":"So1aNaAddress","watchEVM":"0x00000000000000000000000000000000000000aa","tokenChoices":{"solana:Mint":false},
        "modules":"protocol.kamino=0,network.base=1","modulesKnown":["network.base","protocol.kamino"],
        "policy":{"maxRiskPct":2,"defaultRiskPct":1,"minRewardRisk":1.5,"defaultRewardRisk":2,"maxLeverage":5,"atrStopMultiple":1.5,"maxOpenRiskPct":6},
        "safeSearch":false,"batches":true,"contractCap":120.5,"tradingKeyOffered":["0x00000000000000000000000000000000000000aa"],"schedulePaused":true,
        "codeSet":true,"codeOff":false,"lockAfter":15,"noticesOff":["incoming","update"],"openAtLogin":true,"thinking":true,
        "modelScores":{"ollama/qwen3:8b":{"passed":8,"total":10,"seconds":2.5,"misses":["buy 5 SOL"]}},"keepRunning":false}"#);
    assert_eq!(s.watch_evm.as_deref(), Some("0x00000000000000000000000000000000000000aa"));
    let score = &s.model_scores.as_ref().unwrap()["ollama/qwen3:8b"];
    assert!(score.good() && score.short() == "8/10 · 2.5 s");
    // A first launch's settings: nothing set, nothing written but braces.
    assert_eq!(serde_json::to_string(&Settings::default()).unwrap(), "{}");
    assert_eq!(same::<Settings>("{}"), Settings::default());
}

/// A canvas with everything a Swift engine kept on it: a message with its chart and cards, a move with its route, a
/// trade plan's board, result boards of every kind, the canvas's own state.
const CANVAS: &str = r#"[{"id":"C1","title":"Buy SOL","name":"Mine","messages":[
    {"id":"M1","fromUser":true,"text":"buy sol for 5","at":784123200.5,"context":"Portfolio","contexts":[{"title":"Portfolio","about":"$5","bare":false}]},
    {"id":"M2","fromUser":false,"text":"Here","note":"Claude · checked","at":784123201.25,"understood":"buy SOL for 5 USDC",
     "chart":{"title":"SOL · day","closes":[1,2.5],"times":[1762430400,1762434000],"low":"$1","high":"$2.50","change":"+150%","coin":"SOL","network":"solana"},
     "options":[{"title":"Buy","comment":"Now","recommended":true,"command":"buy SOL for 5 USDC"},{"title":"Older","comment":"","recommended":false}],
     "picked":0,"steps":["Swap on Jupiter"]}],
  "boards":[
    {"id":"B1","kind":"move","title":"Buy SOL","lines":["5 USDC → SOL"],"state":"done","at":784123202.5,
     "steps":[{"label":"SWAP · Jupiter","title":"5 USDC → 0.03 SOL","lines":["route"],"money":{"place":"Solana","symbol":"USDC","was":"10","after":"5"},"status":"then"}],
     "actions":[{"title":"Again","command":"buy SOL for 5 USDC"}],"note":"Jupiter's route","request":"buy sol for 5"},
    {"id":"B2","kind":"move","title":"Long ETH","lines":[],"state":"plan","at":784123203.5,
     "trade":{"coin":"ETH","side":"long","leverage":"3","limit":false,"entry":2400.5,"stop":2300,"take":2600,"notional":"$30","size":"0.0125 ETH",
              "margin":"$10","liquidation":"≈ $1,700","lossAtStop":"−$1.25","lossPct":"1.2","profitAtTake":"+$2.50","alerts":[2500],"prepared":1762430400.5},
     "plan":{"market":"ETH","group":"crypto","stopped":"No stop","short":"stop","blocked":true,"fix":{"title":"Add a 2% stop","command":"stop 2%"},
             "fixDetail":"By the book","overLimit":false,"takes":[{"id":"half","title":"Half at 1R","detail":"d","note":"n","payoff":"+$2.00","recommended":true}],
             "footer":"Confirm opens it","preview":false}},
    {"id":"B3","kind":"result","title":"Rates","lines":["Kamino 5%"],"at":784123204.5,"data":{
      "rates":[{"place":"Kamino","asset":"USDC","apy":5.42,"how":"deposit 5 USDC to kamino","note":"earning"}],
      "token":{"chain":"base","address":"0xa","symbol":"AERO","name":"Aero","fullName":"Aerodrome","image":"https://i","subtitle":"AERO · Base",
               "verified":true,"verifiedBy":"CoinGecko","stock":false,"favorite":true,"dollar":false,"price":1.2345,"range":"week","closes":[1,2],
               "times":[1,2],"change":-3.5,"changes":[{"title":"1h","value":"+1%"}],"tiles":[{"title":"Liquidity","value":"$1M"}],
               "held":{"amount":"5 AERO","usd":6.17,"how":"wallet","pnl":"+$1","gain":1,"today":0.5,"paid":"Bought","since":"Oct 10"},
               "links":[{"label":"DexScreener","url":"https://dexscreener.com"}],"loading":false,"error":"none"},
      "pnl":{"coin":"ETH","period":"week","reports":[{"period":"week","title":"This week","trading":1.5,"realized":1,"funding":-0.25,"unrealized":0.75,
               "change":2.5,"changeSince":784123100.5,"flows":10,"closed":3,"wins":2,"byAsset":[{"coin":"ETH","name":"Ethereum","realized":1,"closed":3}],
               "best":{"coin":"ETH","name":"Ethereum","long":true,"closed":784123150.5,"realized":2}}],
             "held":[{"symbol":"SOL","network":"solana","place":"wallet","amount":"0.5","usd":80,"gain":5,"paid":75,"counted":"(0.5 of it)"}]},
      "movers":{"up":[{"coin":"SOL","name":"Solana","price":"$160","change":5.5}],"down":[]},
      "approvals":{"list":[{"chain":"base","place":"Base","symbol":"USDC","spender":"0xs","spenderName":"Uniswap","token":"0xt","unlimited":true,
                   "amount":"unlimited","permit2":false,"revoke":"revoke USDC on base for 0xs"}],"unread":["ethereum"],"loading":false},
      "dca":[{"coin":"SOL","perCycle":5,"every":"day","used":10,"deposited":50,"fills":2,"received":0.06,"cancel":"cancel jupiter dca SOL"}],
      "receive":{"network":"bitcoin","title":"Receive BTC","token":"BTC","amount":0.001,"address":"bc1q","minimum":0.0001,"until":784130000.5,
                 "arrives":"USDC on Solana","state":"PENDING_DEPOSIT"},
      "service":{"service":"httpbin.org","usdc":0.01,"proof":"0xhash","raw":"{}",
                 "answer":{"items":[{"title":"A","url":"https://x/a.png"},{"title":"B","detail":"d"}],"facts":[{"key":"k","value":"v"}],"text":"t",
                           "links":["https://x"]}}}},
    {"id":"B4","kind":"chart","title":"ETH · day","lines":[],"at":784123205.5,"opens":"C2",
     "chart":{"title":"ETH · day","closes":[2400],"low":"$2,400","high":"$2,400","change":"0%"}}],
  "at":784123100.5,"pinnedAt":784123300.5,"folder":"Work","icon":{"kind":"emoji","value":"🚀"},"provider":"anthropic","model":"claude-opus-5-5",
  "schedule":"task:T1","intent":{"asset":"ETH","side":"long","riskUSD":5,"stopPct":2},"planBoard":"B2","tradedPlan":{"asset":"BTC","side":"short"},
  "skills":["trading-study"],"offers":[{"title":"Buy","comment":"Now","command":"buy SOL for 5 USDC","recommended":true}]},
 {"id":"C2","title":"New canvas","messages":[],"boards":[],"at":784123000}]"#;

#[test]
fn canvases_read_and_write_back_as_swift_wrote_them() {
    let _dir = store::test_folder();
    let canvases: Vec<CanvasDoc> = through_store("canvases.json", CANVAS);
    let c = &canvases[0];
    assert_eq!(c.messages[1].options.as_ref().unwrap()[1].command, None);
    assert_eq!(c.intent.as_ref().unwrap().risk_usd, Some(Decimal::from(5)));
    assert_eq!(wire::to_reference(c.messages[0].at), AT);
    let data = c.boards[2].data.as_ref().unwrap();
    assert!(data.service.as_ref().unwrap().answer.items[0].is_image() && !data.service.as_ref().unwrap().answer.items[1].is_image());
    assert_eq!(data.token.as_ref().unwrap().price, Some("1.2345".parse().unwrap()));
}

#[test]
fn a_card_left_waiting_or_a_move_left_running_is_marked_at_launch() {
    let _dir = store::test_folder();
    let waiting = r#"[{"id":"C","title":"T","messages":[],"at":784123000,"boards":[
        {"id":"W","kind":"move","title":"Send","lines":[],"state":"waiting","at":784123000},
        {"id":"R","kind":"move","title":"Swap","lines":["a"],"state":"running","at":784123000}]}]"#;
    std::fs::write(store::url("canvases.json"), waiting).unwrap();
    let c = crate::canvas::load_canvases();
    assert_eq!((c[0].boards[0].state.as_deref(), c[0].boards[0].lines.last().map(String::as_str)),
               (Some("cancelled"), Some("ihatecrypto restarted before Confirm: nothing was signed.")));
    assert_eq!((c[0].boards[1].state.as_deref(), c[0].boards[1].lines.len()), (Some("failed"), 2));
    // Before canvases: the old chat.json becomes the first canvas, named by its first words.
    std::fs::remove_file(store::url("canvases.json")).unwrap();
    std::fs::write(store::url("chat.json"), r#"[{"id":"M","fromUser":true,"text":"what is my balance on every network today","at":784123000}]"#).unwrap();
    let c = crate::canvas::load_canvases();
    assert_eq!((c.len(), c[0].title.as_str(), c[0].messages.len()), (1, "what is my balance on every network toda", 1));
}

#[test]
fn every_other_stored_file_reads_and_writes_back_as_swift_wrote_it() {
    let _dir = store::test_folder();
    through_store::<HashMap<String, ihatecrypto_core::pnl::CostBasis>>("costs.json",
        r#"{"So11111111111111111111111111111111111111112":{"amount":1.5,"usd":200.25,"since":784123200.5,"received":0.5,"receivedUSD":60},
            "base:0xa":{"amount":10,"usd":12}}"#);
    through_store::<Vec<ihatecrypto_core::skills::Skill>>("skills.json", r#"[{"id":"s1","name":"Notes","about":"What","text":"Body","builtIn":false}]"#);
    through_store::<Vec<String>>("memory.json", r#"["I trade ETH","Russian"]"#);
    through_store::<Vec<FavoriteToken>>("favorites.json",
        r#"[{"chain":"base","address":"0xAb","name":"Aero","symbol":"AERO","image":"https://i","price":1.234,"change24h":-2.5,"liquidity":1000000},
            {"chain":"solana","address":"Mint","name":"Bonk","symbol":"BONK"}]"#);
    through_store::<FolderStore>("folders.json", r#"{"names":["Work","Home"],"icons":{"Work":{"kind":"emoji","value":"🚀"}}}"#);
    through_store::<HashMap<String, Vec<String>>>("live-models.json", r#"{"ollama":["qwen3:8b"],"chatgpt":["gpt-5.4"]}"#);
    through_store::<Vec<Multisig>>("multisigs.json",
        r#"[{"address":"0xsafe","keys":[{"name":"This PC","address":"0xab","kind":"pc"},{"name":"Phone","address":"0xcd","kind":"address"}],
             "threshold":2,"salt":18446744073709551615,"networks":["base","arbitrum"]}]"#);
    let ledger: Option<LedgerAccount> = through_store("ledger.json",
        r#"{"label":"Ledger","model":"Nano X","evm":"0xab","solana":"So1","evmPath":"m/44'/60'/0'/0/0","solanaPath":"m/44'/501'/0'","active":true,"bluetooth":false}"#);
    assert_eq!(ledger.unwrap().wallet().id, "ledger");
    through_store::<Option<LedgerAccount>>("ledger.json", "null");
    through_store::<Vec<ihatecrypto_core::alerts::PriceAlert>>("alerts.json",
        r#"[{"id":"E621E1F8-C36C-495A-93FC-0C247A3E6E5F","coin":"BTC","above":true,"price":120000,"created":784123200.5,"steps":["buy BTC for 5 USDC"],
             "plan":"E621E1F8-C36C-495A-93FC-0C247A3E6E50","paused":true}]"#);
    through_store::<Vec<ScheduleRun>>("schedule-runs.json",
        r#"[{"at":784123200.5,"title":"Buy SOL","result":"done","line":"Bought 0.03 SOL","canvas":"C1","task":"T1"},{"at":784123300,"title":"Check","result":"nothing"}]"#);
    through_store::<HashMap<String, AddressExtra>>("address-extras.json",
        r#"{"base:Mom":{"icon":{"kind":"logo","value":"eth"},"category":"family","note":"n","source":"chat"},"solana:Me":{}}"#);
    through_store::<Vec<EquityPoint>>("equity.json", r#"[{"at":784123200.5,"usd":12.5,"wallet":"1"}]"#);
    through_store::<HashMap<String, crate::automation::KeptAsset>>("recurring-assets.json", r#"{"Mint":{"symbol":"BONK","decimals":5,"token2022":false}}"#);
    through_store::<HashSet<Uuid>>("recurring-buys.json", r#"["E621E1F8-C36C-495A-93FC-0C247A3E6E5F"]"#);
    let saved: Saved = through_store("history.json", &format!(
        r#"{{"chats":{CANVAS},"automatic":[{{"kind":"automatic","at":784123200.5,"title":"Money arrived","coin":"SOL","bubbles":[],"lines":["+5 USDC"],
             "failed":false,"schedule":"perp:BTC"}}],
             "shown":{{"M1":{{"title":"Buy SOL","bubbles":[{{"kind":"move","label":"SWAP","title":"5 USDC","lines":["a"],"status":"done"}}]}}}}}}"#));
    assert_eq!(saved.chats.len(), 2);
    // A record from before the frozen canvases were kept.
    through_store::<Saved>("history.json", r#"{"chats":[],"automatic":[]}"#);
}

#[test]
fn sets_are_saved_sorted_as_swift_saves_them() {
    let _dir = store::test_folder();
    std::fs::write(store::url("skills-off.json"), r#"["b","a"]"#).unwrap();
    std::fs::write(store::url("starred.json"), r#"["solana:Z","base:A"]"#).unwrap();
    let st = crate::engine::State::load();
    assert_eq!(st.skills_off.iter().collect::<Vec<_>>(), ["a", "b"]);
    store::save(&st.starred, "starred.json");
    assert_eq!(std::fs::read_to_string(store::url("starred.json")).unwrap(), r#"["base:A","solana:Z"]"#);
}

#[test]
fn model_sign_ins_and_nodes_stay_encrypted_and_read_as_swift_wrote_them() {
    let _dir = store::test_folder();
    // Swift's credentials.bin: the JSON of every provider's credential, protected (off Windows the stand-in's XOR).
    let swift = br#"{"anthropic":{"apiKey":{"_0":"sk-ant-test"}},"claude-code":{"external":{"path":"C:\\claude.exe"}}}"#;
    std::fs::write(store::url("credentials.bin"), &*store::protect(swift).unwrap()).unwrap();
    let all = store::credentials();
    assert_eq!(all["anthropic"], llm::Credential::ApiKey("sk-ant-test".into()));
    store::set_credential(Some(llm::Credential::ApiKey("sk-or-test".into())), "openrouter");
    store::set_credential(None, "anthropic");
    let blob = std::fs::read(store::url("credentials.bin")).unwrap();
    assert!(!blob.windows(7).any(|w| w == b"sk-or-t"), "kept encrypted");
    assert_eq!(norm(serde_json::from_slice(&store::unprotect(&blob).unwrap()).unwrap()),
               norm(json!({"openrouter": {"apiKey": {"_0": "sk-or-test"}}, "claude-code": {"external": {"path": "C:\\claude.exe"}}})));
    crate::nodes::set_node_secret(Some("https://mainnet.helius-rpc.com/?api-key=k".into()), "solana");
    crate::nodes::set_node_secret(Some("partner".into()), "1click");
    assert_eq!(crate::nodes::node_secrets().len(), 2);
    assert!(!std::fs::read(store::url("nodes.bin")).unwrap().windows(7).any(|w| w == b"partner"));
    assert_eq!(crate::nodes::host("https://mainnet.helius-rpc.com/?api-key=k").as_deref(), Some("mainnet.helius-rpc.com"));
    assert_eq!(crate::nodes::host("nonsense"), None);
}

#[test]
fn a_wallets_files_are_named_as_swift_names_them() {
    assert_eq!(store::scoped("tasks.json", Some("7")), "tasks-7.json");
    assert_eq!(store::scoped("costs.json", Some(SignerWallet::FIRST_ID)), "costs.json");
    assert_eq!(store::scoped("recurring-buys.json", None), "recurring-buys.json");
    assert_eq!(task_seal::seal_name("tasks-7.json"), "tasks-7.seal");
}

// MARK: TaskSeal (TaskSealTests)

fn task(steps: &[&str]) -> ScheduledTask {
    serde_json::from_value(json!({"id": Uuid::new().to_string(), "steps": steps, "next": AT + 3600.0})).unwrap()
}

#[test]
fn a_task_list_changed_outside_loads_paused() {
    let _dir = store::test_folder();
    let (list, key) = ("tasks-t1.json", "seal-t1.key");
    task_seal::save(&[task(&["buy SOL for 5 USDC"])], list, key);
    // As saved: intact, running.
    let (tasks, tampered) = task_seal::load(list, key);
    assert!(!tampered && tasks[0].paused != Some(true));
    // Another program rewrites the list: every task comes back paused, and that's said.
    let changed = task(&["send 50 USDC to 0x000000000000000000000000000000000000dEaD"]);
    std::fs::write(store::url(list), serde_json::to_vec(std::slice::from_ref(&changed)).unwrap()).unwrap();
    let (tasks, tampered) = task_seal::load(list, key);
    assert!(tampered && tasks.iter().all(|t| t.paused == Some(true)));
    // Saved again by the app (the user switched one on): intact again.
    task_seal::save(&tasks, list, key);
    assert!(!task_seal::load(list, key).1);
    // Another program deletes the key and writes its own list: with no key to check it by, it loads paused too.
    std::fs::remove_file(store::url(key)).unwrap();
    std::fs::write(store::url(list), serde_json::to_vec(&[changed]).unwrap()).unwrap();
    let (tasks, tampered) = task_seal::load(list, key);
    assert!(tampered && tasks.iter().all(|t| t.paused == Some(true)));
}

/// The prices Confirmed in a task are kept under the seal and dropped with a broken one: the Schedule doesn't show them,
/// so one written in could pass unseen.
#[test]
fn a_tasks_confirmed_prices_keep_under_the_seal_and_go_when_it_breaks() {
    let _dir = store::test_folder();
    let (list, key) = ("tasks-t3.json", "seal-t3.key");
    let pay = ihatecrypto_core::position_command::WalletCommand::Pay { method: "GET".into(), url: "https://api.example.com/answer".into(), body: None };
    let mut t = task(&["pay GET https://api.example.com/answer"]);
    t.approve(60_000, &pay);
    task_seal::save(std::slice::from_ref(&t), list, key);
    let (tasks, tampered) = task_seal::load(list, key);
    assert!(!tampered && tasks[0].approves(60_000, &pay) && !tasks[0].approves(60_001, &pay));
    // Another program raises the price kept: the list loads paused, with no prices.
    t.approve(u64::MAX, &pay);
    std::fs::write(store::url(list), serde_json::to_vec(&[t]).unwrap()).unwrap();
    let (tasks, tampered) = task_seal::load(list, key);
    assert!(tampered && tasks[0].paused == Some(true) && tasks[0].prices.is_none() && !tasks[0].approves(1, &pay));
}

#[test]
fn each_wallets_tasks_have_their_own_list_and_seal() {
    let _dir = store::test_folder();
    let key = "seal-t2.key";
    let (mine, other) = ("tasks-a.json", "tasks-b.json");
    let t = task(&["buy SOL for 5 USDC"]);
    task_seal::save(std::slice::from_ref(&t), mine, key);
    task_seal::save(&[t.clone(), t], other, key);
    assert_eq!((task_seal::load(mine, key).0.len(), task_seal::load(other, key).0.len()), (1, 2));
    std::fs::write(store::url(other), serde_json::to_vec(&[task(&["send 50 USDC to 0xdead"])]).unwrap()).unwrap();
    assert!(task_seal::load(other, key).1 && !task_seal::load(mine, key).1);
}

/// A list the Swift engine sealed: its bytes as Swift's JSONEncoder wrote them, tagged with the key beside it.
#[test]
fn a_list_the_swift_engine_sealed_reads_intact() {
    let _dir = store::test_folder();
    let (key, made) = task_seal::key(task_seal::KEY).unwrap();
    assert!(made && !task_seal::key(task_seal::KEY).unwrap().1);
    let swift = br#"[{"id":"E621E1F8-C36C-495A-93FC-0C247A3E6E5F","steps":["buy SOL for 5 USDC"],"next":784123200.5,"every":{"days":{"_0":7}},"because":"weekly"}]"#;
    std::fs::write(store::url(task_seal::TASKS), swift).unwrap();
    std::fs::write(store::url("tasks.seal"), task_seal::tag(swift, &key)).unwrap();
    let (tasks, tampered) = task_seal::load(task_seal::TASKS, task_seal::KEY);
    assert!(!tampered && tasks.len() == 1 && tasks[0].paused.is_none() && tasks[0].because.as_deref() == Some("weekly"));
    // The engine starts with them.
    assert_eq!(crate::engine::State::load().tasks, tasks);
}

// MARK: The window's state

fn sample_snapshot() -> Snapshot {
    let canvases: Vec<CanvasDoc> = serde_json::from_str(CANVAS).unwrap();
    let board = canvases[0].boards[0].clone();
    let key = MultisigKey { name: "PC".into(), address: "0xab".into(), kind: "pc".into() };
    let activity = Activity { title: Some("Buy".into()), ..Activity::new("request", at()) };
    let wallet = SignerWallet { id: "1".into(), label: "Main".into(), kind: "phrase".into(), solana: Some("So1".into()), evm: None, near: None, test: None, automation: None };
    Snapshot {
        signer: Signer { ready: true, error: Some("e".into()), notes: vec![], waiting: false, session_length: Some(300), limits: Some(signer_api::SignerLimits::STANDARD),
                         ledger: Some("Confirm".into()) },
        wallets: vec![wallet.clone()], active: Some("1".into()), addresses: HashMap::from([("solana".into(), "So1".into())]), phrase: Some(zeroize::Zeroizing::new(vec!["abandon".into()])),
        portfolio: PortfolioView { updated_at: Some(at()), change7d: Some(Decimal::ONE), ..Default::default() }, positions: vec![Position {
            coin: "ETH".into(), side: "long".into(), size: "1".into(), entry: "$1".into(), pnl: "+$1".into(), liquidation: Some("$0".into()),
            summary: Some("s".into()), guardian: Some("g".into()), ruler: Some(PositionRuler { points: vec![], now: Some(1.0), now_text: None, guard_line: None }),
            market: Some("ETH".into()) }],
        chat: canvases[0].messages.clone(), sent: vec!["buy sol for 5".into()], thinking: false, draft: Some("d".into()),
        card: Some(Card { id: "B1".into(), title: "Buy".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] }),
        site: Some(SiteAskView { id: "S".into(), origin: "https://a".into(), host: "a".into(), check: "known".into(), real: None, title: "Sign".into(),
                                 lines: vec![], warnings: vec![], ticks: vec![], approve: "Sign".into(), canvas: "C1".into(), lead: None, balances: vec![],
                                 checks: vec![], note: None }),
        options: vec![OptionView { title: "Buy".into(), comment: "".into(), recommended: true, command: "buy".into() }],
        running: Some(Running { title: "Swap".into(), lines: vec![] }), plan: Some("long ETH".into()),
        model: ModelView { provider: Some("anthropic".into()), name: None, model: None, connected: true, models: vec![] },
        providers: vec![ProviderView { id: "anthropic".into(), name: "Anthropic".into(), kind: "api".into(), takes_key: true, existing_login: None,
                                       sign_in: None, note: "".into(), unavailable: None, connected: true, models: vec![], default_model: "m".into(),
                                       chosen: "m".into(), names: HashMap::new() }],
        model_choices: HashMap::new(), second_check: true, jev_open_router: false, language: "auto".into(), russian: false, book: vec![],
        history: vec![HistoryLine { at: at(), chain: "solana".into(), kind: "swap".into(), note: "n".into(), link: None }],
        alerts: vec![Alert { id: "A".into(), text: "t".into(), coin: Some("BTC".into()), when: None, now: None, steps: None,
                             blocks: Some(vec![AlertStep { plan: true, coin: Some("ETH".into()), title: "Long".into(), line: None }]) }],
        tasks: vec![TaskView { id: "T".into(), text: "t".into(), paused: false, automatic: true, next: Some(at()), steps: vec![], every: Some("3 days".into()),
                               pick: false, moves: true, buys: false, sells: false, free: Some(Decimal::TEN), free_other: None, untrusted: false, recurring: false }],
        recurring: vec![RecurringView { id: "R".into(), symbol: "SOL".into(), usdc: Decimal::from(5), days: 7, next: at(), paused: false, solana: None }],
        jupiter_dca: Some(board.clone()), automation: Some(wallet),
        skills: vec![SkillView { id: "s".into(), name: "n".into(), about: "a".into(), text: "t".into(), built_in: true, on: true, journal: Some(false) }],
        memory: vec![], favorites: vec![],
        canvases: vec![CanvasRow { id: "C1".into(), title: "t".into(), at: at(), pinned_at: Some(at()), folder: None, icon: None, provider: None, model: None,
                                   empty: false, blocks: true, coin: Some("ETH".into()), note: false }],
        folders: vec![FolderView { name: "Work".into(), icon: Some(CanvasIcon { kind: "emoji".into(), value: "🚀".into() }) }], canvas: "C1".into(),
        boards: vec![board], running_board: Some("B1".into()),
        multisigs: vec![Multisig { address: "0xsafe".into(), keys: vec![key.clone()], threshold: 1, salt: 7, networks: vec!["base".into()] }],
        multisig_draft: Some(MultisigDraft { keys: vec![key.clone()], threshold: 1, networks: vec![], salt: 7, address: None }), candidates: vec![key],
        ledger_connect: Some(LedgerConnect::default()), starred: vec![], watching: false, watched: HashMap::new(),
        automation_usd: Some(Decimal::from(12)), positions_pnl: Some(Decimal::ONE), limit_waiting: Some("Limit waiting".into()), in_transit: vec![TransitView {
            id: "o".into(), chain: "base".into(), what: "w".into(), bridge: "Relay".into(), logo: "relay".into(), receiving: false, at: at(), tx: None, links: vec![] }],
        context: Some(Context { used: 10, window: Some(200_000), ..Default::default() }), model_running: false, agent_step: Some("lookup".into()),
        options_message: Some("M2".into()), retry_board: Some("B1".into()), quote: Some(QuoteView { state: "fresh".into(), at: Some(at()), line: None }),
        models: ModelsView { live: HashMap::new(), servers: HashMap::new(), installed: false, sizes: HashMap::new(), downloads: HashMap::new(), memory_gb: 16,
                             recommended: HashMap::new(), refusals: HashMap::new() },
        lock: LockView::default(), modules: ModulesView { networks: vec![], kinds: vec![] },
        trading: TradingView { policy: Default::default(), styles: trading_styles(), safe_search: true, batches: false, contract_cap: 50.0,
                               key: TradingKeyView { address: None, active: false, paused: false, ledger: false, fee_approved: None, fee_rate: "0.05%".into(),
                                                     referral: "IHATECRYPTO".into(), possible: true } },
        nodes: NodesView { nodes: vec![], one_click_key: false, agent_key: false, agent_host: "api.demo.agentsonintents.com".into() },
        step_edit: Some(Edit { board: "B1".into(), amount: Some("5".into()), unit: "USDC".into(), ways: false, stock: false, way: None, quotes: None }),
        schedule_paused: false, schedule_runs: vec![], multisig_held: HashMap::from([("0xsafe".into(), vec![MultisigNetHolding { network: "base".into(), coins: None, usd: None }])]),
        schedule_key: Some("task:T".into()), moves: vec![MoveRow { at: at(), kind: "send".into(), text: "t".into(), amount: Some(Decimal::ONE), symbol: None, to: None,
                                                                  own: false, link: None, request: None, reply: "r".into(), canvas: None, activity: Some(activity.clone()) }],
        moves_day: Some(MovesDay { day: at(), lines: vec![], own: vec![] }), recent_requests: vec![activity],
        automation_budget: Some(AutomationBudget { evm: HashMap::from([("base".into(), Decimal::TEN)]), ..Default::default() }),
        stuck: vec![StuckView { network: "base".into(), key: "base".into(), coin: "ETH".into(), usd: Decimal::ONE, usdc: Decimal::TEN, ways: 2 }],
        places: PlaceFacts::default(), notices: vec![], open_at_login: true,
        top_up: Some(TopUpView { networks: vec![], network: "solana".into(), coin: "usdc".into(), held: HashMap::new(), gas_short: None, waiting: false, arrived: None, sent: None }),
        app_language: "system".into(),
        local_setup: LocalSetupView { thinking: false, exam_progress: None, exam_total: 10, exam_for: None, status: None, scores: HashMap::new(), ollama_version: None,
                                      ollama_latest: None, ollama_state: "noOllama".into(), ollama_outdated: false, ollama_install: None, claude_install: None,
                                      claude_found: false, keep_running: None },
        recently_closed: vec![ClosedTrade { coin: "ETH".into(), name: "Ethereum".into(), long: true, closed: at(), realized: Decimal::ONE }],
        address_extras: HashMap::new(), plan_alert_example: Some("alert".into()), chips_position: Some("ETH".into()), last_balance: Some(Decimal::from(100)),
        build: Some("20261106.1200".into()), update_ready: Some("0.9.0".into()),
        journey: Some(crate::journey::Journey::new("2 USDC → Mom".into(), "solana", "base", true, Some(30), 1)),
        privacy: Some(crate::journey::PrivacyChoice { is_private: true, rows: vec![] }),
        tee: Some(crate::journey::TeeView { model: "m".into(), gateway_key: "k".into(), at: 1, reply: None, why: None }),
    }
}

/// Every key Swift's Snapshot writes, in its order: what App/Windows/app/ui/*.js reads.
const SNAPSHOT_KEYS: [&str; 86] = [
    "signer", "wallets", "active", "addresses", "phrase", "portfolio", "positions", "chat", "sent", "thinking", "draft", "card", "site", "options",
    "running", "plan", "model", "providers", "modelChoices", "secondCheck", "jevOpenRouter", "language", "russian", "book", "history", "alerts", "tasks",
    "recurring", "jupiterDCA", "automation", "skills", "memory", "favorites", "canvases", "folders", "canvas", "boards", "runningBoard", "multisigs",
    "multisigDraft", "candidates", "ledgerConnect", "starred", "watching", "watched", "automationUSD", "positionsPnl", "limitWaiting", "inTransit",
    "context", "modelRunning", "agentStep", "optionsMessage", "retryBoard", "quote", "models", "lock", "modules", "trading", "nodes", "stepEdit",
    "schedulePaused", "scheduleRuns", "multisigHeld", "scheduleKey", "moves", "movesDay", "recentRequests", "automationBudget", "stuck", "places",
    "notices", "openAtLogin", "topUp", "appLanguage", "localSetup", "recentlyClosed", "addressExtras", "planAlertExample", "chipsPosition",
    "lastBalance", "build", "updateReady", "journey", "privacy", "tee",
];

#[test]
fn the_state_has_the_keys_the_window_reads_and_leaves_out_what_is_nil() {
    let full: Value = serde_json::from_slice(&wire::window(&sample_snapshot()).unwrap()).unwrap();
    assert_eq!(keys(&full), set(&SNAPSHOT_KEYS));
    // Dates as ISO 8601 to the window, decimals as numbers.
    assert_eq!(full["chat"][0]["at"], "2025-11-06T12:00:00Z");
    assert_eq!(full["tasks"][0]["next"], "2025-11-06T12:00:00Z");
    assert_eq!(full["boards"][0]["at"], "2025-11-06T12:00:02Z");
    assert_eq!(full["automationUSD"], json!(12));
    assert_eq!(full["automationBudget"]["evm"]["base"], json!(10));
    // The nested views the pages read.
    assert_eq!(keys(&full["signer"]), set(&["ready", "error", "notes", "waiting", "sessionLength", "limits", "ledger"]));
    assert_eq!(keys(&full["portfolio"]), set(&["networks", "total", "updatedAt", "ready", "errors", "equity", "change7d"]));
    assert_eq!(keys(&full["lock"]), set(&["codeSet", "locked", "codeOff", "lockAfter", "checking", "offerNewCode", "newCodeOffers", "tasksRun"]));
    assert_eq!(keys(&full["models"]), set(&["live", "servers", "installed", "sizes", "downloads", "memoryGB", "recommended", "refusals"]));
    assert_eq!(keys(&full["tasks"][0]),
               set(&["id", "text", "paused", "automatic", "next", "steps", "every", "pick", "moves", "buys", "sells", "free", "untrusted", "recurring"]));
    assert_eq!(keys(&full["canvases"][0]), set(&["id", "title", "at", "pinnedAt", "blocks", "empty", "coin", "note"]));
    assert_eq!(keys(&full["context"]), set(&["used", "window", "instructions", "chat", "sent", "lines", "spent", "billed"]));
    assert_eq!(keys(&full["trading"]), set(&["policy", "styles", "safeSearch", "batches", "contractCap", "key"]));
    assert_eq!(keys(&full["trading"]["key"]), set(&["active", "paused", "ledger", "feeRate", "referral", "possible"]));
    assert_eq!(keys(&full["ledgerConnect"]), set(&["status", "notes", "busy"]));
    assert_eq!(full["ledgerConnect"]["status"], "Looking for a Ledger on USB…");
    assert_eq!(keys(&full["localSetup"]),
               set(&["thinking", "examTotal", "scores", "ollamaState", "ollamaOutdated", "claudeFound"]));
    assert_eq!(keys(&full["stepEdit"]), set(&["board", "amount", "unit", "ways", "stock"]));
    assert_eq!(keys(&full["skills"][0]), set(&["id", "name", "about", "text", "builtIn", "on", "journal"]));
    assert_eq!(full["trading"]["styles"][0]["policy"]["maxRiskPct"], json!(1));
    // A first state: every optional left out, as Swift's encoder leaves out nil.
    let mut empty = sample_snapshot();
    (empty.active, empty.phrase, empty.draft, empty.card, empty.site, empty.running, empty.plan) = (None, None, None, None, None, None, None);
    (empty.jupiter_dca, empty.automation, empty.running_board, empty.multisig_draft, empty.ledger_connect) = (None, None, None, None, None);
    (empty.automation_usd, empty.positions_pnl, empty.limit_waiting, empty.context, empty.agent_step, empty.options_message) = (None, None, None, None, None, None);
    (empty.retry_board, empty.quote, empty.step_edit, empty.schedule_key, empty.moves_day, empty.automation_budget) = (None, None, None, None, None, None);
    (empty.top_up, empty.plan_alert_example, empty.chips_position, empty.last_balance, empty.build, empty.update_ready) = (None, None, None, None, None, None);
    (empty.journey, empty.privacy, empty.tee) = (None, None, None);
    let lean: Value = serde_json::from_slice(&wire::window(&empty).unwrap()).unwrap();
    assert_eq!(keys(&lean).len(), 86 - 33);
}

#[test]
fn a_file_gets_seconds_since_2001_and_the_window_iso_8601() {
    let b = Board { at: at(), ..Board::new("result", "R") };
    let file = serde_json::to_value(&b).unwrap();
    assert_eq!(file["at"], json!(AT));
    let window: Value = serde_json::from_slice(&wire::window(&b).unwrap()).unwrap();
    assert_eq!(window["at"], "2025-11-06T12:00:00Z");
    // Either reads back.
    assert_eq!(serde_json::from_value::<Board>(window).unwrap().at.timestamp(), at().timestamp());
    // The window's mode ends with its write.
    assert_eq!(serde_json::to_value(&b).unwrap()["at"], json!(AT));
}

#[test]
fn the_outbox_writes_one_line_per_answer_or_event() {
    let lines = Lines::default();
    let out = Outbox::writer(lines.clone());
    out.answer(Some(3), true, &HashMap::from([("pid".into(), "7".into())]), None);
    out.answer(None, false, &HashMap::new(), Some("ihatecrypto is locked."));
    out.event("locked", &HashMap::<String, String>::new());
    assert_eq!(lines.all(), vec![
        json!({"id": 3, "ok": true, "result": {"pid": "7"}}),
        json!({"ok": false, "result": {}, "error": "ihatecrypto is locked."}),
        json!({"event": "locked", "data": {}}),
    ]);
    Outbox::silent().event("state", &1);
}

#[test]
fn a_request_reads_as_the_window_sends_it() {
    let r = Request::read(r#"{"id":12,"op":"send","text":"buy $SOL","board":"B1","boards":["B0","B1"],"step":1,
        "abouts":[{"title":"SOL","about":"Solana's coin"}],"policy":{"maxRiskPct":2,"defaultRiskPct":1,"minRewardRisk":1.5,"defaultRewardRisk":2,
        "maxLeverage":5,"atrStopMultiple":1.5,"maxOpenRiskPct":6},"value":75.5,"ticks":["lookalike"],"unknownKey":true}"#).unwrap();
    assert_eq!((r.id, r.op.as_str(), r.step, r.value), (Some(12), "send", Some(1), Some(75.5)));
    assert_eq!(r.abouts.unwrap()[0].about, "Solana's coin");
    assert!(Request::read(r#"{"op":"lockApp"}"#).is_some());
    // A line that doesn't read is skipped, as main.swift skips it.
    assert!(Request::read(r#"{"op":"send","index":"one"}"#).is_none() && Request::read("not json").is_none());
    assert!(ANSWERS_ONLY.contains(&"siteState") && !ANSWERS_ONLY.contains(&"send"));
}

// MARK: The engine's state

fn engine(lines: &Lines) -> Rc<Engine> {
    Engine::new(Outbox::writer(lines.clone()), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| sample_snapshot())
}

fn states(lines: &Lines) -> Vec<Value> { lines.all().into_iter().filter(|l| l["event"] == "state").map(|l| l["data"].clone()).collect() }

#[tokio::test]
async fn one_state_goes_out_per_step_and_a_locked_window_gets_the_lock_alone() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = engine(&lines);
        e.changed();
        e.changed();
        e.reply("Hello", None);
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let s = states(&lines);
        assert_eq!(s.len(), 1);
        assert_eq!(keys(&s[0]), set(&SNAPSHOT_KEYS));
        // The reply is on the open canvas and in canvases.json.
        assert_eq!(e.st().messages().last().unwrap().text, "Hello");
        let saved: Vec<CanvasDoc> = store::load("canvases.json").unwrap();
        assert_eq!(saved[0].messages.last().unwrap().text, "Hello");
        // Locked: the lock and the language, nothing else.
        e.set_settings(|s| s.code_set = Some(true));
        e.st_mut().lock.locked = true;
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let s = states(&lines);
        assert_eq!(s.len(), 2);
        assert_eq!(keys(&s[1]), set(&["lock", "russian", "language"]));
        assert_eq!(s[1]["lock"]["codeSet"], true);
    }).await;
}

#[tokio::test]
async fn the_setters_save_what_swift_s_did_set_saved() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        e.set_memory(|m| m.push("I trade ETH".into()));
        e.set_skills_off(|s| { s.insert("b".into()); s.insert("a".into()); });
        e.set_favorites(|f| f.push(serde_json::from_value(json!({"chain": "base", "address": "0xAB", "name": "A", "symbol": "A"})).unwrap()));
        e.set_ledger(|l| *l = Some(LedgerAccount::default()));
        e.set_tasks(|t| t.push(task(&["buy SOL for 5 USDC"])));
        e.st_mut().wallet_scope_costs = Some("3".into());
        e.set_app_costs(|c| { c.insert("base:0xa".into(), Default::default()); });
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(store::load::<Vec<String>>("memory.json").unwrap(), ["I trade ETH"]);
        assert_eq!(std::fs::read_to_string(store::url("skills-off.json")).unwrap(), r#"["a","b"]"#);
        assert_eq!(store::load::<Vec<FavoriteToken>>("favorites.json").unwrap()[0].id(), "base:0xab");
        assert_eq!(store::load::<LedgerAccount>("ledger.json").unwrap().evm_path, "m/44'/60'/0'/0/0");
        assert!(!task_seal::load(task_seal::TASKS, task_seal::KEY).1 && task_seal::load(task_seal::TASKS, task_seal::KEY).0.len() == 1);
        assert!(store::url("costs-3.json").exists() && !store::url("costs.json").exists());
        assert_eq!(store::load::<Settings>("settings.json").unwrap().language.as_deref(), Some("ru"));
        assert!(e.st().russian() && e.say("Hi", "Привет") == "Привет");
        // A shown swap and quoted ways mark when they were priced.
        e.set_way_quotes(Some(crate::engine::WayQuotes { command: ihatecrypto_core::position_command::WalletCommand::Receive, ways: vec![] }));
        assert!(e.st().quoted_at.is_none());
    }).await;
}

#[tokio::test]
async fn the_wallet_on_screen_follows_the_ledger_then_the_signer_then_watched_addresses() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine(&Lines::default());
        e.set_settings(|s| { s.watch_solana = Some("WatchSol".into()); s.watch_evm = Some("0xwatch".into()) });
        assert!(e.st().watching() && e.st().solana_address().as_deref() == Some("WatchSol"));
        let main = SignerWallet { id: "2".into(), label: "Main".into(), kind: "phrase".into(), solana: Some("So1".into()), evm: Some("0xmain".into()), near: None, test: None, automation: None };
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), solana_address: Some("So1".into()), evm_address: Some("0xmain".into()),
                                               near_address: Some("me.near".into()), wallets: Some(vec![main]), active_wallet: Some("2".into()), ..Default::default() });
        {
            let st = e.st();
            assert!(!st.watching() && st.evm_address().as_deref() == Some("0xmain") && st.active_wallet().unwrap().id == "2");
            let tw = st.trade_wallet().unwrap();
            assert_eq!((tw.solana.as_str(), tw.evm.as_str(), st.near_address().as_deref()), ("So1", "0xmain", Some("me.near")));
        }
        e.set_ledger(|l| *l = Some(LedgerAccount { evm: Some("0xledger".into()), active: Some(true), ..Default::default() }));
        let st = e.st();
        assert!(st.ledger_active() && st.active_wallet().unwrap().id == "ledger" && st.near_address().is_none());
        // A Ledger without Solana: the move on Solana says why, the address is empty.
        assert_eq!(st.trade_wallet().unwrap().solana, "");
    }).await;
}

#[test]
fn the_chats_language_follows_the_setting_then_the_person() {
    let _dir = store::test_folder();
    let mut st = crate::engine::State::load();
    st.messages_mut().push(Message::new(true, "купи сол на 5 долларов", None));
    assert!(st.russian() && st.reply_language() == "Russian" && st.say3("a", "б", "c") == "б");
    st.settings.language = Some("es".into());
    assert!(!st.russian() && st.reply_language() == "Spanish" && st.say3("a", "б", "c") == "c");
    st.settings.language = Some("en".into());
    assert!(st.say("a", "б") == "a");
    st.settings.app_language = Some("ru".into());
    assert!(st.app_russian());
    // Message ids are Swift's uppercase UUIDs.
    let m = Message::new(false, "x", None);
    assert!(Uuid::parse(&m.id).is_some() && m.id == m.id.to_uppercase());
}

#[test]
fn a_chain_step_is_titled_as_swift_titles_it() {
    let address = signer_api::SignerAddress { name: "Mom".into(), network: "solana".into(), address: "So1".into(), memo: "".into() };
    assert_eq!(ChainStep::Transfer(address, "5.5".parse().unwrap()).title(), "send 5.5 USDC to Mom");
    assert_eq!(ChainStep::Said("chart ETH".into()).title(), "chart ETH");
}

// MARK: The lock

#[test]
fn the_lock_watch_locks_on_sleep_time_away_or_a_locked_session_only_with_a_code() {
    assert!(locks_now(true, false, true, 0, 5, false));
    assert!(locks_now(true, false, false, 300_000, 5, false));
    assert!(!locks_now(true, false, false, 299_999, 5, false));
    assert!(locks_now(true, false, false, 0, 5, true));
    assert!(!locks_now(false, false, true, 999_999, 1, true), "no code: never");
    assert!(!locks_now(true, true, true, 999_999, 1, true), "already locked");
    assert!(locks_now(true, false, false, 3_600_000, 60, false) && !locks_now(true, false, false, 3_599_999, 60, false));
}

#[test]
fn the_signers_code_words_are_said_in_russian() {
    assert_eq!(localized_code_error("Wrong code · 3 tries left", true), "Неверный код · осталось попыток: 3");
    assert_eq!(localized_code_error("Wrong code · 1 try left", true), "Неверный код · осталось попыток: 1");
    assert_eq!(localized_code_error("The code is 6 digits.", true), "Код — 6 цифр.");
    assert_eq!(localized_code_error("Something else.", true), "Something else.");
    assert_eq!(localized_code_error("Wrong code · 3 tries left", false), "Wrong code · 3 tries left");
}

#[tokio::test]
async fn the_lock_follows_the_settings_and_the_signers_word() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        // The last run had a code: locked before the signer answers; an odd «minutes away» reads as 5.
        store::save(&json!({"codeSet": true, "lockAfter": 7}), "settings.json");
        let e = engine(&Lines::default());
        assert!(e.st().lock.locked);
        let v = e.lock_view();
        assert!(v.code_set && v.lock_after == 5 && !v.tasks_run);
        e.set_lock_after(15);
        e.set_lock_after(2);
        assert_eq!(e.lock_view().lock_after, 15);
        // The code went while locked (five wrong ones): only Windows Hello opens it, after a restart too.
        e.lock_heard(false);
        assert!(e.st().settings.code_off == Some(true) && e.st().settings.code_set == Some(false) && e.st().lock_heard_signer);
        let locked = e.locked_snapshot();
        assert!(locked.lock.code_off && locked.language == "auto");
    }).await;
    tokio::task::LocalSet::new().run_until(async {
        // No code last run: open; the signer's first word that there is one locks it, a later one doesn't.
        store::save(&json!({}), "settings.json");
        let e = engine(&Lines::default());
        assert!(!e.st().lock.locked);
        e.lock_heard(true);
        assert!(e.st().lock.locked && e.st().settings.code_off.is_none());
        e.st_mut().lock.locked = false;
        e.lock_heard(true);
        assert!(!e.st().lock.locked);
    }).await;
}

#[tokio::test]
async fn locking_declines_a_sites_request_and_a_dead_signer_is_said_on_the_lock() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = engine(&lines);
        // No code: «lock» does nothing.
        e.lock_app().await;
        assert!(!e.st().lock.locked);
        e.set_settings(|s| s.code_set = Some(true));
        // A site's request waits in its block.
        let (tx, rx) = oneshot::channel();
        let view = SiteAskView { id: "S1".into(), origin: "https://app.example".into(), host: "app.example".into(), check: "unknown".into(), real: None,
                                 title: "Sign".into(), lines: vec![], warnings: vec![], ticks: vec![], approve: "Sign".into(), canvas: String::new(), lead: None,
                                 balances: vec![], checks: vec![], note: None };
        e.st_mut().site_ask = Some(SiteAsk::new(view, tx));
        e.set_boards(|b| b.insert(0, Board { id: "S1".into(), state: Some("waiting".into()), ..Board::new("site", "Sign") }));
        e.lock_app().await;
        assert!(e.st().lock.locked && e.st().site_ask.is_none());
        assert_eq!(rx.await, Ok(false));
        let b = e.st().boards()[0].clone();
        assert_eq!((b.state.as_deref(), b.lines.last().map(String::as_str)), (Some("cancelled"), Some("Locked before an answer: nothing was signed.")));
        assert!(lines.all().iter().any(|l| l["event"] == "locked"));
        // The code typed while the signer can't be reached: said on the lock, still locked, no longer checking.
        e.unlock("123456").await;
        let lock = e.st().lock.clone();
        assert!(lock.locked && !lock.checking && lock.problem.as_deref() == Some("The signer didn't answer: try again, or use Windows Hello."));
    }).await;
}

#[tokio::test]
async fn windows_hello_opens_the_lock_and_a_forgotten_code_offers_a_new_one() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        store::save(&json!({"codeOff": true}), "settings.json");
        let e = engine(&Lines::default());
        assert!(e.st().lock.locked);
        // Windows Hello not set up (the stand-in's default): it says how to set up a PIN.
        if cfg!(not(windows)) {
            e.unlock_with_hello(false).await;
            assert!(e.st().lock.locked && e.st().lock.problem.as_deref().is_some_and(|p| p.contains("set up a PIN")));
            // SAFETY: the only test that reads or sets it.
            unsafe { std::env::set_var("WINSEC_FAKE_HELLO", "1") }
            e.unlock_with_hello(true).await;
            unsafe { std::env::remove_var("WINSEC_FAKE_HELLO") }
            let lock = e.st().lock.clone();
            assert!(!lock.locked && lock.problem.is_none() && lock.offer_new_code && lock.new_code_offers == 1);
            assert!(e.st().settings.code_off.is_none());
        }
    }).await;
}

// MARK: The other files' small helpers

#[test]
fn a_picture_is_taken_only_in_a_shape_the_window_makes() {
    let icon = |kind: &str, value: &str| CanvasIcon { kind: kind.into(), value: value.into() }.checked();
    assert!(icon("logo", "usdc.e-2").is_some() && icon("symbol", "star").is_some());
    assert!(icon("logo", "").is_none() && icon("logo", "a/b").is_none() && icon("logo", &"a".repeat(61)).is_none());
    assert!(icon("emoji", "🚀").is_some() && icon("emoji", "👨‍👩‍👧‍👦").is_some() && icon("emoji", "").is_none());
    assert!(icon("image", "data:image/png;base64,AAAA").is_some() && icon("image", "https://x/a.png").is_none() && icon("svg", "x").is_none());
}

#[test]
fn a_step_field_reads_one_amount_or_none() {
    let a = |s: &str| route_edit::amount(s).map(ihatecrypto_core::foundation::text);
    assert_eq!(a("2").as_deref(), Some("2"));
    assert_eq!(a(" 0,5 ").as_deref(), Some("0.5"));
    assert_eq!(a("1 000").as_deref(), Some("1000"));
    assert_eq!(a("12 500,5").as_deref(), Some("12500.5"));
    assert_eq!(a("1,000.50").as_deref(), Some("1000.5"));
    assert_eq!(a("1.000,50").as_deref(), Some("1000.5"));
    assert_eq!(a("1,000"), None, "a thousand, or one with three decimals");
    assert_eq!(a("0,500").as_deref(), Some("0.5"));
    assert_eq!(a("5abc"), None);
    assert_eq!(route_edit::pretty("BRIDGE · RELAY OR CCTP"), "Bridge · Relay or CCTP");
    assert_eq!(route_edit::pretty("DEPOSIT · JITOSOL V3 USDC"), "Deposit · JitoSOL v3 USDC");
}

#[test]
fn ollama_is_installed_only_when_ollama_signed_it() {
    assert!(signed_by_ollama("Valid|CN=Ollama Inc., O=Ollama Inc., L=Palo Alto, S=California, C=US"));
    assert!(signed_by_ollama("Valid|CN=Ollama, O=Ollama, C=US\r\n"));
    assert!(!signed_by_ollama("NotSigned|"));
    assert!(!signed_by_ollama("HashMismatch|CN=Ollama Inc., O=Ollama Inc."));
    assert!(!signed_by_ollama("Valid|CN=Ollama Inc., O=OllamaFake Inc."));
    assert!(!signed_by_ollama("Valid|CN=Ollama Inc., O=Someone Else"));
    assert!(!signed_by_ollama("Valid"));
    assert!(ollama_older("0.12.3", "0.13.0") && ollama_older("0.9.9", "0.10.0"));
    assert!(!ollama_older("0.13.0", "0.13.0") && !ollama_older("0.13.1", "0.13.0"));
    assert!(from_claude(Some("claude.ai")) && from_claude(Some("downloads.claude.ai")));
    assert!(!from_claude(Some("evilclaude.ai")) && !from_claude(Some("claude.ai.example.com")) && !from_claude(None));
}

#[test]
fn the_top_up_page_reads_a_whole_request_only() {
    let r = PageRequest::parse(b"POST /sent?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 4\r\nX-Secret:  abc \r\n\r\nbody").unwrap();
    assert_eq!((r.method.as_str(), r.path.as_str(), r.headers["x-secret"].as_str(), r.body.as_slice()), ("POST", "/sent", "abc", &b"body"[..]));
    assert!(PageRequest::parse(b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nbody").is_none());
    assert!(PageRequest::parse(b"GET / HTTP/1.1\r\n").is_none());
}

#[test]
fn the_history_keeps_what_a_canvas_said_after_the_canvas_lets_it_go() {
    let _dir = store::test_folder();
    let mut h = HistoryStore::load();
    let mut canvases: Vec<CanvasDoc> = serde_json::from_str(CANVAS).unwrap();
    assert!(h.record(&canvases) && !h.record(&canvases));
    // Live blocks aren't kept; the chat cleared leaves the History as it was.
    assert_eq!(h.saved.chats[0].boards.len(), 4);
    canvases[0].messages.clear();
    assert!(!h.record(&canvases) && h.saved.chats[0].messages.len() == 2);
    // A trailing stop moving again is one more line on its record.
    h.automatic("Trailing stop", "moved to $2,400", Some("ETH".into()), false, false, None);
    h.automatic("Trailing stop", "moved to $2,450", Some("ETH".into()), false, true, None);
    assert_eq!(h.saved.automatic.last().unwrap().lines.len(), 2);
    assert_eq!(HistoryStore::load().saved.automatic.len(), 1);
}

#[test]
fn every_exam_score_and_trading_style_reads_as_on_the_mac() {
    let r = ExamResult { passed: 7, total: 10, seconds: 3.25, misses: vec![] };
    assert!(!r.good() && r.short() == "7/10 · 3.2 s");
    let styles = trading_styles();
    assert_eq!(styles.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["careful", "balanced", "bold"]);
    assert_eq!(styles[2].policy.max_leverage, Decimal::from(10));
    let tv = TradeIntent::default();
    assert_eq!(serde_json::to_value(&tv).unwrap(), json!({"asset": "ETH", "side": "long"}));
}

// Layer 2, the chat side: its tests, one file each in tests/.
mod browser_sites;
mod canvas_history;
mod canvas_switching;
mod chat_context;
mod chat_send;
mod chat_trading;
mod chat_turns;
mod commands_chat;
mod commands_skills;
mod models_accounts;
