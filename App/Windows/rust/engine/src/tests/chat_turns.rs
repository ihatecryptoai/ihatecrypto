//! The chat's turn as the Mac has it (ChatTurns.swift; ChatTurnsTests, Chat6Tests, Chat13Tests, Cards16Tests,
//! Cards17Tests): a message about several blocks carries one chip each, the cards offered go with their reply, a swap's
//! price line and its bar read as the Mac's, the model's next step shows on the waiting card, the context meter, and
//! Ollama's download read as it streams. In-process: state set by hand, no network, no model, no Ollama.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use ihatecrypto_core::blocks;
use ihatecrypto_core::foundation;
use ihatecrypto_core::intent::TradeIntent;
use ihatecrypto_core::normalize::{ChatRouter, Lookup};
use ihatecrypto_core::position_command::{PositionCommand, SpotAsset, WalletCommand};
use llm::usage::ModelFacts;
use llm::{Message as LlmMessage, Request, Role, Usage};
use rust_decimal::Decimal;
use trade::executor::Position;

use super::{Lines, engine};
use crate::canvas::{Board, Step};
use crate::chat_turns::{OllamaPull, QUOTE_LIFE};
use crate::engine::{Card, ChainStep, Engine, JupiterShown, Message, Pending, WayQuotes};
use crate::from_core::RouterOption;
use crate::route_edit::Way;
use crate::snapshot::Running;
use crate::store;

async fn on_engine(test: impl AsyncFnOnce(Rc<Engine>)) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async { test(engine(&Lines::default())).await }).await;
}

fn d(s: &str) -> Decimal { foundation::decimal(s).expect("a number") }

fn sol() -> SpotAsset { SpotAsset::all().iter().find(|a| a.symbol == "SOL").cloned().expect("SOL") }

fn position(coin: &str) -> Position { Position { coin: coin.into(), size: Decimal::ONE, entry: Decimal::from(100), unrealized_pnl: Decimal::ZERO, liquidation: None } }

fn option(title: &str, command: &str) -> RouterOption { RouterOption::new(title, "", command, false) }

/// A card waiting for Confirm, as show() leaves it.
fn waiting(e: &Engine, pending: Pending) {
    let mut st = e.st_mut();
    st.pending = Some(pending);
    st.card = Some(Card { id: "B1".into(), title: "2 steps, one Confirm".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] });
}

#[tokio::test]
async fn a_message_about_several_blocks_carries_a_chip_for_each() {
    on_engine(async |e| {
        let receive = Board::new("receive", "Receive");
        let alerts = Board::new("alerts", "Alerts");
        let ids = [receive.id.clone(), alerts.id.clone()];
        e.set_boards(|b| { b.push(receive); b.push(alerts.clone()) });
        // The window's other chips (Engine.handle's alsoAttached), then the message about the first.
        e.st_mut().also_attached = vec![alerts];
        e.send("what are these", None, Some(&ids[0])).await;
        let mine = e.st().messages().iter().rfind(|m| m.from_user).cloned().unwrap();
        let chips = mine.contexts.unwrap_or_default();
        assert_eq!(chips.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["receive", "alerts"]);
        // No alerts yet: only that block's name went, and the chip says so.
        assert!(chips[1].bare && mine.context.is_some());
        // The turn is over: no step under the dots, no Stop, nothing to try again, nothing attached.
        let st = e.st();
        assert!(st.agent_step.is_none() && !st.model_running && st.retry.is_none() && st.attached.is_none() && st.also_attached.is_empty());
        drop(st);
        // Stop with nothing running, Refresh with nothing priced: no-ops.
        e.stop_model();
        e.requote();
        assert!(!e.st().stop_requested);
    }).await;
}

#[tokio::test]
async fn the_slash_list_is_the_block_catalog() {
    on_engine(async |e| {
        let list = e.block_catalog()["list"].clone();
        let rows: Vec<HashMap<String, String>> = serde_json::from_str(&list).unwrap();
        assert_eq!(rows.len(), blocks::ALL.iter().filter(|b| !b.automatic && !b.example.is_empty()).count());
        assert!(rows.iter().all(|r| !r["example"].is_empty() && r.contains_key("logo") && r.contains_key("group")));
        // Not an address: nothing goes out.
        assert_eq!(e.look_up_contract("hello").await["list"], "[]");
    }).await;
}

#[test]
fn a_swaps_price_line_reads_as_the_macs() {
    assert_eq!(Engine::swap_line(&WalletCommand::Buy { asset: sol(), usdc: Some(Decimal::TEN) }, "0.05 SOL", None, None), "You get ≈ 0.05 SOL · 1 SOL ≈ $200");
    assert_eq!(Engine::swap_line(&WalletCommand::Sell { asset: sol(), amount: Some(Decimal::TWO) }, "300 USDC", None, None), "You get ≈ 300 USDC · 1 SOL ≈ $150");
    let eth = WalletCommand::BuyOn { place: "base".into(), token: "ETH".into(), usdc: Some(Decimal::from(25)), from: None };
    assert_eq!(Engine::swap_line(&eth, "a price only", None, None), "You get ≈ a price only");
}

#[test]
fn the_quote_line_says_a_near_send_in_its_own_words_and_warns_of_fees() {
    let near = "≈ 25.31 USDC for $24.80 of it · fees ≈ $0.51 (2%) · ≈ 2 min";
    let send = WalletCommand::SendToken { name: "Alice".into(), token: "BTC".into(), amount: Some(d("0.0003")), network: None };
    assert_eq!(Engine::swap_line(&send, near, None, None), near);
    let aero = |usdc: i64| WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: Some(Decimal::from(usdc)), from: None };
    // $1 in, $0.82 worth arriving: fees take more than a tenth.
    let small = Engine::swap_line(&aero(1), "0.5 AERO", Some(d("0.82")), None);
    assert!(small.contains("⚠️ fees take ≈ $0.18 (18%)"), "{small}");
    // A tenth or less: no warning; nor without a worth.
    assert!(!Engine::swap_line(&aero(10), "5 AERO", Some(d("9.5")), None).contains("fees take"));
    assert!(!Engine::swap_line(&WalletCommand::Buy { asset: sol(), usdc: Some(Decimal::ONE) }, "0.005 SOL", None, None).contains("fees take"));
    // A cross-network quote's own «≈ $X» is its worth.
    let cross = WalletCommand::CrossSwap { from: "USDC".into(), on: "solana".into(), to: "ETH".into(), at: "base".into(), amount: Some(Decimal::TWO) };
    assert!(Engine::swap_line(&cross, "0.0004 ETH ≈ $1.50", None, None).contains("fees take ≈ $0.50"));
    // A sale by contract names its ticker once known.
    let sale = WalletCommand::SellOn { place: "base".into(), token: "0x2e8c0000000000000000000000000000000e1e18".into(), amount: Some(Decimal::from(100)), keep: false };
    assert!(Engine::swap_line(&sale, "1.23 USDC", None, Some("AI")).contains("1 AI ≈ $0.0123"));
    // An amount past the arithmetic against a quote's: the price is left out, never a panic.
    let huge = WalletCommand::Buy { asset: sol(), usdc: Some(Decimal::MAX) };
    assert_eq!(Engine::swap_line(&huge, "0.0000000000000000000000001 SOL", None, None), "You get ≈ 0 SOL");
}

#[test]
fn the_position_chips_are_commands() {
    for phrase in ["move stop to breakeven", "trail 2%", "close half", "close"] {
        assert!(PositionCommand::parse(phrase).is_some(), "«{phrase}»");
        assert!(PositionCommand::parse(&format!("{phrase} ETH")).is_some(), "«{phrase} ETH»");
    }
}

#[tokio::test]
async fn position_chips_only_on_the_plans_open_position() {
    on_engine(async |e| {
        let plan = TradeIntent { asset: "SOL".into(), ..TradeIntent::default() };
        e.st_mut().intent = plan.clone();
        e.st_mut().positions = vec![position("ETH")];
        assert_eq!(e.chips_position(), None, "another coin's position isn't the plan's");
        e.st_mut().positions = vec![position("ETH"), position("SOL")];
        assert_eq!(e.chips_position().as_deref(), Some("SOL"));
        // Once traded the plan is cleared; the traded one stands in while its position is open.
        e.st_mut().intent = TradeIntent::default();
        e.st_mut().traded_plan = Some(plan);
        assert_eq!(e.chips_position().as_deref(), Some("SOL"));
        e.st_mut().positions = vec![];
        assert_eq!(e.chips_position(), None);
    }).await;
}

#[tokio::test]
async fn confirm_is_held_only_where_the_mac_holds_it() {
    on_engine(async |e| {
        // Hyperliquid's spot book: its quote shows on the card, Confirm isn't held for it.
        assert!(!e.priced(&WalletCommand::HlSpotBuy { symbol: "HYPE".into(), usdc: Some(Decimal::from(5)) }));
        assert!(!e.priced(&WalletCommand::HlSpotSell { symbol: "HYPE".into(), amount: Some(Decimal::ONE) }));
        assert!(!e.priced(&WalletCommand::SwapOn { place: "hyperliquid".into(), from: "HYPE".into(), to: "USDC".into(), amount: Some(Decimal::ONE) }));
        assert!(e.priced(&WalletCommand::SwapOn { place: "base".into(), from: "ETH".into(), to: "USDC".into(), amount: None }));
        assert!(!e.priced(&WalletCommand::SwapOn { place: "near".into(), from: "NEAR".into(), to: "USDC".into(), amount: None }));
        // A quote asked and not given: Refresh and «over 15 s old», not «Quoting…» for as long as it fails.
        let buy = WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: Some(Decimal::from(5)), from: None };
        waiting(&e, Pending::Wallet(buy.clone()));
        assert_eq!(e.quote_view().unwrap().state, "quoting");
        e.set_way_quotes(Some(WayQuotes { command: buy.clone(), ways: vec![] }));
        assert_eq!(e.quote_view().unwrap().state, "stale");
        // A swap Jupiter can't price: Confirm as always.
        e.st_mut().unpriced = Some(buy);
        assert!(e.quote_view().is_none());
    }).await;
}

#[tokio::test]
async fn a_chain_led_by_a_priced_swap_is_quoted_and_held_to_its_price() {
    on_engine(async |e| {
        let first = WalletCommand::Buy { asset: sol(), usdc: Some(Decimal::from(5)) };
        let then = WalletCommand::SolanaTo("arbitrum".into(), Some(Decimal::TWO));
        waiting(&e, Pending::Chain(vec![ChainStep::Wallet(first.clone()), ChainStep::Wallet(then)]));
        // Before Jupiter answers: «Quoting…», as a single swap's card.
        assert_eq!(e.quote_view().unwrap().state, "quoting");
        e.set_jupiter_shown(Some(JupiterShown { command: first, out: 51_200_000, title: "≈ 0.0512 SOL".into(), route: "Jupiter: USDC → SOL".into() }));
        let q = e.quote_view().unwrap();
        assert!(q.state == "fresh" && q.at.is_some() && q.line.as_deref().is_some_and(|l| l.starts_with("You get ≈ 0.0512 SOL")), "{q:?}");
        // 15 s on, the price is stale and Confirm waits for Refresh.
        e.st_mut().quoted_at = Some(chrono::Utc::now() - chrono::TimeDelta::milliseconds((QUOTE_LIFE * 1000.0) as i64 + 1));
        assert_eq!(e.quote_view().unwrap().state, "stale");
        // An EVM buy leading the chain: its ways, then Refresh drops the old price.
        let aero = WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: Some(Decimal::from(5)), from: None };
        waiting(&e, Pending::Chain(vec![ChainStep::Wallet(aero.clone()), ChainStep::Wallet(WalletCommand::Bridge { from: "base".into(), to: "arbitrum".into(), amount: Some(Decimal::ONE) })]));
        let way = |w: &str, gets: &str| Way { way: w.into(), gets: gets.into(), worth: "$4.90".into(), less: None, usd: None };
        e.set_way_quotes(Some(WayQuotes { command: aero, ways: vec![way("relay", "2.41 AERO"), way("uniswap", "2.38 AERO")] }));
        let q = e.quote_view().unwrap();
        assert!(q.state == "fresh" && q.line.as_deref().is_some_and(|l| l.contains("2.41 AERO")), "{q:?}");
        e.requote();
        assert_eq!(e.quote_view().unwrap().state, "quoting");
        assert!(e.st().way_quotes.is_none() && e.st().quoted_at.is_none());
        // A chain led by an unpriced move has no quote bar.
        waiting(&e, Pending::Chain(vec![ChainStep::Wallet(WalletCommand::SolanaTo("base".into(), Some(Decimal::from(5))))]));
        assert!(e.quote_view().is_none());
    }).await;
}

#[tokio::test]
async fn an_offers_card_carries_the_command_it_fills_and_keeps_its_pick() {
    on_engine(async |e| {
        e.st_mut().messages_mut().push(Message::new(false, "Which one?", None));
        e.set_options(vec![RouterOption::new("Long", "with a stop", "long ETH by the book", false), option("Answer", "")]);
        let last = e.st().messages().last().cloned().unwrap();
        let commands: Vec<Option<String>> = last.options.unwrap().into_iter().map(|o| o.command).collect();
        assert_eq!(commands, [Some("long ETH by the book".to_string()), Some(String::new())]);
        assert_eq!(e.st().options_message.as_ref(), Some(&last.id));
        // A card picked: the check stays on it once the set folds.
        e.mark_picked(1);
        assert_eq!(e.st().messages().last().unwrap().picked, Some(1));
        // The same cards again keep the pick; new ones clear it.
        e.set_options(vec![RouterOption::new("Long", "with a stop", "long ETH by the book", false), option("Answer", "")]);
        assert_eq!(e.st().messages().last().unwrap().picked, Some(1));
        e.set_options(vec![option("Short", "short ETH by the book")]);
        assert_eq!(e.st().messages().last().unwrap().picked, None);
        // Cards after the person's own message belong to no reply.
        e.st_mut().messages_mut().push(Message::new(true, "hmm", None));
        e.set_options(vec![option("Short", "short ETH by the book")]);
        assert!(e.st().options_message.is_none());
        e.set_options(vec![]);
        assert!(e.st().options_message.is_none());
    }).await;
}

#[tokio::test]
async fn the_forms_the_mac_offers_are_offered() {
    on_engine(async |e| {
        e.st_mut().messages_mut().push(Message::new(false, "What now?", None));
        e.offer(vec![option("Automation", "create an automation wallet"), option("Cancel", "cancel all scheduled tasks"), option("New", "new canvas"),
                     option("Nonsense", "fly to the moon"), option("Answer", "")], "what now", vec![]);
        let titles: Vec<String> = e.st().options.iter().map(|o| o.title.clone()).collect();
        assert_eq!(titles, ["Automation", "Cancel", "New", "Answer"]);
        let offered = e.st().offered_for.clone().unwrap();
        assert!(offered.text == "what now" && offered.options.len() == 4);
        // An address the user never wrote stays off a card, unless it's one of theirs.
        let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
        e.offer(vec![option("Save Mom", &format!("save {mom} as Mom on base"))], "keep it", vec![]);
        assert!(e.st().options.is_empty());
        e.offer(vec![option("Save Mom", &format!("save {mom} as Mom on base"))], &format!("keep {mom} as Mom"), vec![]);
        assert_eq!(e.st().options.len(), 1);
    }).await;
}

#[tokio::test]
async fn ask_in_chat_on_a_coin_goes_as_its_facts_with_a_chip() {
    on_engine(async |e| {
        let about = Board { lines: vec!["About the coin «SOL: 0.5 held»".into()], ..Board::new("result", "SOL · Solana") };
        e.st_mut().also_attached = vec![about];
        assert!(e.router_context().canvas.iter().any(|c| c.contains("About the coin «SOL: 0.5 held»")));
        assert_eq!(e.sent_contexts().unwrap().iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["SOL · Solana"]);
        assert!(!e.sent_contexts().unwrap()[0].bare);
        e.st_mut().also_attached = vec![];
        assert!(e.sent_contexts().is_none());
    }).await;
}

#[tokio::test]
async fn the_models_next_step_shows_pale_on_the_waiting_card() {
    on_engine(async |e| {
        let card = Board::new("move", "Buy SOL");
        let other = Board::new("result", "Rates");
        let id = card.id.clone();
        e.set_boards(|b| { b.push(card); b.push(other) });
        assert_eq!(e.boards_shown(), *e.st().boards());
        let mut goal = crate::agent::AgentGoal::new("buy SOL and set an alert", "buy SOL for 5 USDC", 0, 0);
        goal.next = Some("an alert 5% under the price you get".into());
        e.st_mut().agent_goal = Some(goal);
        e.st_mut().card = Some(Card { id: id.clone(), title: "Buy SOL".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] });
        let shown = e.boards_shown();
        let step: &Step = shown.iter().find(|b| b.id == id).and_then(|b| b.steps.as_ref()?.last()).unwrap();
        assert_eq!((step.label.as_str(), step.title.as_str(), step.status.as_deref()), ("NEXT · AFTER THIS", "an alert 5% under the price you get", Some("then")));
        assert!(shown.iter().filter(|b| b.id != id).all(|b| b.steps.is_none()));
        // The canvas itself keeps the card as it was.
        assert!(e.st().boards().iter().all(|b| b.steps.is_none()));
    }).await;
}

#[tokio::test]
async fn a_moves_steps_stay_in_the_chat_and_a_failed_move_can_be_tried_again() {
    on_engine(async |e| {
        e.st_mut().messages_mut().push(Message::new(true, "buy SOL", None));
        e.keep_steps(1);
        assert_eq!(e.st().messages().len(), 1, "nothing ran: nothing kept");
        e.st_mut().running = Some(Running { title: "Buy SOL".into(), lines: vec!["Quoted".into(), "Signed".into()] });
        e.keep_steps(5);
        assert_eq!(e.st().messages().len(), 1, "past the end: nowhere to keep it");
        e.keep_steps(1);
        let kept = e.st().messages()[1].clone();
        assert!(kept.text == "Quoted\nSigned" && kept.note.as_deref() == Some("execution") && kept.steps == Some(vec!["Quoted".into(), "Signed".into()]));
        let buy = WalletCommand::Buy { asset: sol(), usdc: Some(Decimal::from(5)) };
        e.note_retry(&Pending::Wallet(buy.clone()));
        assert!(e.st().retry.is_none(), "no move's block running");
        e.st_mut().running_board = Some("B7".into());
        e.note_retry(&Pending::Wallet(buy.clone()));
        let retry = e.st().retry.clone().unwrap();
        assert!(retry.board == "B7" && retry.command == buy);
    }).await;
}

#[tokio::test]
async fn all_is_the_held_amount_where_its_held_in_one_place() {
    on_engine(async |e| {
        let row = |symbol: &str, value: Decimal, contract: Option<&str>| crate::portfolio::Row { contract: contract.map(String::from), ..crate::portfolio::Row::new(symbol, &value.to_string(), value) };
        let network = |key: &str, rows| crate::portfolio::Network { key: key.into(), name: key.into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 };
        e.st_mut().portfolio.networks = vec![network("solana", vec![row("RAY", d("1.0412"), Some("4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R")), row("USDC", Decimal::from(5), None)])];
        assert_eq!(e.with_amounts("sell all RAY"), "sell 1.0412 RAY");
        // USDC is on every network: «all» stays words, as on the Mac.
        assert_eq!(e.with_amounts("send all USDC to Mom"), "send all USDC to Mom");
        assert_eq!(e.with_amounts("sell all BONK"), "sell all BONK");
        // Held in two places: words.
        e.st_mut().portfolio.networks.push(network("base", vec![row("RAY", Decimal::ONE, Some("0xray"))]));
        assert_eq!(e.with_amounts("sell all RAY"), "sell all RAY");
    }).await;
}

#[tokio::test]
async fn the_line_under_the_dots_names_what_is_looked_up() {
    on_engine(async |e| {
        assert_eq!(e.lookup_step(&Lookup::new("portfolio", "")).as_deref(), Some("Checking your portfolio"));
        assert_eq!(e.lookup_step(&Lookup::new("fetch", "https://api.example.com/v1 a b")).as_deref(), Some("Reading api.example.com"));
        assert_eq!(e.lookup_step(&Lookup::new("rpc", "base eth_blockNumber")).as_deref(), Some("Reading the chain"));
        assert_eq!(e.lookup_step(&Lookup::new("skill", "trading-study")).as_deref(), Some("Reading the skill «Trading study»"));
        assert_eq!(e.lookup_step(&Lookup::new("skill", "no-such")).as_deref(), Some("Reading the skill"));
        assert_eq!(e.lookup_step(&Lookup::new("nothing", "")), None);
    }).await;
}

#[tokio::test]
async fn the_meter_counts_what_the_chat_read_and_what_a_key_paid() {
    on_engine(async |e| {
        let said = "long ETH ".repeat(200);
        e.st_mut().messages_mut().push(Message::new(true, &said, None));
        let read = Request::new("claude-opus-5-5", vec![LlmMessage::new(Role::System, ChatRouter::system()), LlmMessage::new(Role::User, format!("user: {said}"))]);
        let usage = Usage { input: 10_000, window: Some(200_000), cost: Some(0.02), ..Usage::default() };
        // A subscription: the window's parts, no dollars.
        e.set_settings(|s| s.provider = Some("claude-code".into()));
        e.counted(usage.clone(), read.clone(), "claude-code").await;
        let canvas = e.st().canvas_id.clone();
        let m = e.st().meters[&canvas].clone();
        assert!(m.used == 10_000 && m.window == Some(200_000) && m.lines == 1 && m.sent == 1 && m.spent == 0.0 && !m.billed, "{m:?}");
        assert!(m.instructions > 9_000 && m.instructions + m.chat <= 10_000 && m.chat > 0, "{m:?}");
        // An API key: what the service says it cost, or the list's price for what was read.
        e.st_mut().model_facts = HashMap::from([(ModelFacts::key("claude-opus-5-5"), ModelFacts::new(1_000_000, 0.000_005, 0.0, 0.000_025, None))]);
        e.counted(usage, read.clone(), "anthropic").await;
        let m = e.st().meters[&canvas].clone();
        assert!(m.spent == 0.02 && m.last == Some(0.02) && m.billed);
        let other = Request::new("claude-opus-5-5", vec![LlmMessage::new(Role::User, "a check")]);
        e.counted(Usage { input: 1_000, output: 100, ..Usage::default() }, other, "anthropic").await;
        let m = e.st().meters[&canvas].clone();
        assert!((m.last.unwrap() - 0.0075).abs() < 1e-12 && !m.billed && m.used == 10_000, "another request leaves the window's parts: {m:?}");
        // A service's count past i64 doesn't take the engine down.
        e.counted(Usage { input: i64::MAX, ..Usage::default() }, read, "claude-code").await;
    }).await;
}

#[tokio::test]
async fn the_model_menu_knows_what_fits_this_pc() {
    on_engine(async |e| {
        // Off Windows the PC is a 16 GB stand-in.
        assert!(Engine::fits(8.0) && !Engine::fits(12.0));
        let ollama = e.provider("ollama").unwrap().info();
        let view = e.models_view();
        assert_eq!(view.recommended["ollama"], ollama.default_model);
        assert_eq!(view.memory_gb, Engine::memory_gb().round() as i64);
        assert!(view.refusals.is_empty() && !view.installed);
        // Sizes read: the biggest suggested model that fits.
        for m in &ollama.models { e.st_mut().model_sizes.insert(m.clone(), 40.0); }
        let small = ollama.models.last().unwrap().clone();
        e.st_mut().model_sizes.insert(small.clone(), 7.5);
        assert_eq!(e.local_pick().as_deref(), Some(small.as_str()));
        assert_eq!(e.models_view().recommended["ollama"], small);
        e.get_ollama();
    }).await;
}

#[test]
fn ollamas_download_is_read_as_it_streams() {
    let seen = RefCell::new(vec![]);
    let progress = |p: f64| seen.borrow_mut().push(p);
    let mut pull = OllamaPull::default();
    pull.heard(b"{\"status\":\"pulling manifest\"}\n{\"status\":\"pulling a\",\"digest\":\"a\",\"total\":100,\"completed\":0}\n{\"digest\":\"a\",\"total\":100,\"comp", &progress);
    pull.heard(b"leted\":50}\n{\"digest\":\"b\",\"total\":100,\"completed\":0}\n{\"digest\":\"b\",\"total\":100,\"completed\":0.5}\n", &progress);
    pull.heard(b"{\"digest\":\"b\",\"total\":100,\"completed\":100}\n{\"digest\":\"a\",\"total\":100,\"completed\":100}\n{\"status\":\"success\"}", &progress);
    assert!(pull.finish(200, &progress).is_ok());
    // Each layer counts by its size; a step under 1% isn't told.
    assert_eq!(*seen.borrow(), [0.0, 0.5, 0.25, 0.75, 1.0]);
    // Ollama's own refusal is what's said, whatever the status.
    let mut old = OllamaPull::default();
    old.heard(b"{\"error\":\"pull model manifest: 412: The model you are attempting to pull requires a newer version of Ollama.\"}\n", &progress);
    assert!(old.finish(412, &progress).unwrap_err().message().contains("newer version of Ollama"));
    assert_eq!(OllamaPull::default().finish(404, &progress).unwrap_err().message(), "Ollama didn't start the download (HTTP 404).");
    assert_eq!(OllamaPull::default().finish(200, &progress).unwrap_err().message(), "Ollama stopped before the download finished.");
}

#[tokio::test]
async fn the_cards_seen_go_with_the_answer_to_them() {
    on_engine(async |e| {
        e.st_mut().chat_seen = vec![option("Kamino", "deposit 1 USDC into Kamino")];
        assert_eq!(e.router_context().offered, ["Kamino → deposit 1 USDC into Kamino"]);
        // The live cards when there are any, an answer's card said as one.
        e.st_mut().options = vec![RouterOption::new("Yes", "", "", true)];
        assert_eq!(e.router_context().offered, ["Yes → (an answer to your question) (recommended)"]);
    }).await;
}
