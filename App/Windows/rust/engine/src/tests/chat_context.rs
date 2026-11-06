//! What the model reads and may use, as the Mac's (Chat.swift, TradingAgent.swift; Model7Tests, Engine11Tests,
//! Holds10Tests): a contract a lookup found only in a contract call or a block that shows something; the held tokens by
//! their contract; the waiting card's route and how the moves ended, never a saved address or this wallet's own; the
//! capital the rules measure, gas coins left out. In-process, balances set by hand.

use std::rc::Rc;

use ihatecrypto_core::foundation;
use ihatecrypto_core::intent::{Fmt, TradeIntent};
use ihatecrypto_core::normalize::ChatRouter;
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::pnl::CostBasis;
use rust_decimal::Decimal;
use signer_api::SignerAddress;

use super::{Lines, engine};
use crate::canvas::{Board, Money, Step};
use crate::engine::{Card, Engine, Message};
use crate::portfolio::{Network, Row};
use crate::store;

async fn on_engine(test: impl AsyncFnOnce(Rc<Engine>)) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async { test(engine(&Lines::default())).await }).await;
}

// Public test data only: Hardhat's deployer contract and account #3.
const VAULT: &str = "0x5FbDB2315678afecb367f032d93F642f64180aa3";
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";

fn d(s: &str) -> Decimal { foundation::decimal(s).expect("a number") }

/// `Engine.row` of the Swift tests: a held coin with its dollars and contract.
fn row(symbol: &str, amount: Decimal, usd: Decimal, contract: Option<&str>) -> Row {
    Row { usd: Some(usd), contract: contract.map(String::from), ..Row::new(symbol, &amount.to_string(), amount) }
}

fn network(key: &str, name: &str, rows: Vec<Row>) -> Network {
    let usd = rows.iter().filter_map(|r| r.usd).try_fold(Decimal::ZERO, |a, b| a.checked_add(b)).unwrap_or(Decimal::MAX);
    Network { key: key.into(), name: name.into(), usd, rows, note: None, hidden: vec![], hidden_more: 0 }
}

fn read(e: &Engine, networks: Vec<Network>) {
    let mut st = e.st_mut();
    st.portfolio.total = networks.iter().try_fold(Decimal::ZERO, |a, n| a.checked_add(n.usd)).unwrap_or(Decimal::MAX);
    st.portfolio.networks = networks;
    st.portfolio.updated_at = Some(chrono::Utc::now());
}

#[test]
fn a_found_contract_only_in_a_call_or_a_display() {
    let found = [VAULT.to_string()];
    let none = Default::default();
    let call = format!("call arbitrum {VAULT} deposit(uint256,address) 10e6 me approving 10 USDC");
    assert!(ChatRouter::addresses_allowed_in_steps(&call, "put 10 USDC into Fluid on Arbitrum", &[], &found, Engine::found_allowed, &none));
    // Before this, only displays could: the form's own path («find the contract first») ended refused.
    assert!(!ChatRouter::addresses_allowed_in_steps(&call, "put 10 USDC into Fluid on Arbitrum", &[], &found, Engine::displays, &none));
    // A buy at a contract the user never wrote stays refused, alone or as a chain's step after a call.
    let buy = format!("buy token {VAULT} on base for 5 USDC");
    assert!(!ChatRouter::addresses_allowed_in_steps(&buy, "buy something good", &[], &found, Engine::found_allowed, &none));
    assert!(!ChatRouter::addresses_allowed_in_steps(&format!("{call} then {buy}"), "x", &[], &found, Engine::found_allowed, &none));
    // What only shows something, or is kept and not done.
    for shows in ["chart ETH", "pnl week", "show yields for USDC", "chart token B5ANxuoDaCGJvWwhKDnxLz5S4rq3KTbHNydnMJu5ZSij on solana"] {
        assert!(Engine::displays(shows), "«{shows}»");
    }
    assert!(!Engine::displays("buy SOL for 5 USDC") && !Engine::displays("send 5 USDC to Mom"));
}

#[tokio::test]
async fn held_tokens_are_known_by_their_contract() {
    on_engine(async |e| {
        read(&e, vec![network("arbitrum", "Arbitrum", vec![row("fUSDC", Decimal::from(12), Decimal::from(12), Some(VAULT)),
                                                          row("ETH", Decimal::ONE, Decimal::from(3000), Some("native"))])]);
        let known = e.known_contracts();
        assert!(known.contains(&VAULT.to_string()) && !known.contains(&"native".to_string()));
        // Each network's own USDC is known too: a token, never a recipient.
        assert!(known.iter().any(|k| k.eq_ignore_ascii_case("0xaf88d065e77c8cC2239327C5EDb3A432268e5831")), "Arbitrum's USDC");
        assert_eq!(e.held_contracts().iter().map(|h| (h.0.as_str(), h.2.as_str())).collect::<Vec<_>>(), [("arbitrum", VAULT)]);
        let facts = e.portfolio_facts();
        assert!(facts.iter().any(|f| f.contains(&format!("fUSDC ({VAULT})"))), "{facts:?}");
        assert!(!facts.iter().any(|f| f.contains("ETH (native)")), "{facts:?}");
        assert!(facts[0].starts_with("Total: $3,012.00 (read ") && facts[0].ends_with(" s ago)."), "{facts:?}");
    }).await;
}

#[tokio::test]
async fn balances_not_read_or_empty_are_said_outright() {
    on_engine(async |e| {
        assert_eq!(e.portfolio_facts(), ["Balances: not read yet — say so, don't guess."]);
        read(&e, vec![]);
        let facts = e.portfolio_facts();
        assert!(facts.len() == 1 && facts[0].starts_with("Balances (read ") && facts[0].ends_with("nothing on any network — the wallet is empty."), "{facts:?}");
    }).await;
}

#[tokio::test]
async fn the_model_reads_the_cards_route_and_how_moves_ended() {
    on_engine(async |e| {
        e.st_mut().book = vec![SignerAddress { name: "Mom".into(), network: "base".into(), address: MOM.into(), memo: String::new() }];
        let done = Board { state: Some("done".into()), ..Board::new("move", "Send 1 USDC to Mom") };
        let unknown = Board { state: Some("unknown".into()), ..Board::new("move", "Send 2 USDC to Mom") };
        let step = |label: &str, title: &str, lines: Vec<String>, money| Step { label: label.into(), title: title.into(), lines, money, status: None };
        let card = Board {
            lines: vec![format!("To {MOM}"), "⚠ a warning line".into()], state: Some("waiting".into()),
            steps: Some(vec![
                step("FROM · BASE", "5 USDC", vec!["fee ≈ $0.01".into()], Some(Money { place: "Base".into(), symbol: "USDC".into(), was: "20".into(), after: "15".into() })),
                step("TO · MOM", "≈ 5 USDC", vec![], None),
                Step { status: Some("then".into()), ..step("NEXT", "the model's next step", vec![], None) },
            ]),
            ..Board::new("move", "Send 5 USDC to Mom")
        };
        let id = card.id.clone();
        // The newest on top, as pin() puts them.
        e.set_boards(|b| *b = vec![card, unknown, done]);
        e.st_mut().card = Some(Card { id, title: "send 5 USDC to Mom".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] });
        let facts = e.move_facts();
        let waiting = facts.iter().find(|f| f.starts_with("waiting for Confirm")).cloned().unwrap_or_default();
        assert!(waiting.contains("FROM · BASE 5 USDC (Base 20 → 15 USDC) (fee ≈ $0.01) → TO · MOM ≈ 5 USDC; its plan:"), "{waiting}");
        assert!(!waiting.contains("NEXT") && !waiting.contains("⚠"), "the model's own step and the warnings' lines aren't the plan: {waiting}");
        // A saved address is read as its name, never as the address.
        assert!(!facts.concat().contains(MOM) && waiting.contains("To Mom"), "{facts:?}");
        assert!(facts.contains(&"just finished: Send 2 USDC to Mom — sent, no outcome back".to_string()), "{facts:?}");
        assert!(facts.contains(&"done earlier on this canvas: Send 1 USDC to Mom — done".to_string()), "{facts:?}");
        // A move running now, with its last lines.
        e.st_mut().running = Some(crate::snapshot::Running { title: "Buy SOL".into(), lines: (1..=6).map(|i| format!("line {i}")).collect() });
        assert!(e.move_facts().contains(&"running now: Buy SOL (line 3; line 4; line 5; line 6)".to_string()));
    }).await;
}

#[tokio::test]
async fn this_wallets_own_address_is_your_wallet_to_the_model() {
    on_engine(async |e| {
        e.set_settings(|s| s.watch_evm = Some("0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65".into()));
        assert_eq!(e.readable("from 0x15d34aaf54267db7d7c367839aaf71a00a2c6a65 to 0xabc"), "from your wallet to 0xabc");
        e.st_mut().book = vec![SignerAddress { name: "Mom".into(), network: "base".into(), address: MOM.into(), memo: String::new() }];
        assert_eq!(e.readable(&format!("send to {} and {MOM}", MOM.to_lowercase())), "send to Mom and Mom");
    }).await;
}

#[tokio::test]
async fn the_model_is_told_the_rules_capital() {
    on_engine(async |e| {
        read(&e, vec![network("solana", "Solana", vec![row("USDC", Decimal::from(800), Decimal::from(800), None),
                                                      row("SOL", Decimal::from(5), Decimal::from(1000), Some(CostBasis::WSOL))])]);
        assert_eq!(e.rule_equity(), Some(Decimal::from(800)));
        assert_eq!(e.router_context().capital, Some(Fmt::usd(Decimal::from(800))));
    }).await;
}

#[tokio::test]
async fn the_rules_capital_leaves_out_the_gas_coins() {
    on_engine(async |e| {
        read(&e, vec![
            network("solana", "Solana", vec![row("USDC", Decimal::from(800), Decimal::from(800), None), row("SOL", Decimal::from(5), Decimal::from(1000), Some(CostBasis::WSOL))]),
            network("base", "Base", vec![row("USDC", Decimal::from(200), Decimal::from(200), None), row("ETH", d("0.01"), Decimal::from(30), Some("native"))]),
            network("near", "NEAR", vec![row("NEAR", Decimal::from(20), Decimal::from(50), Some("native"))]),
        ]);
        assert_eq!(e.rule_equity(), Some(Decimal::from(1050)));
        read(&e, vec![]);
        assert_eq!(e.rule_equity(), None);
        // A balance past the arithmetic (a node's answer) measures nothing, never a panic.
        read(&e, vec![network("base", "Base", vec![row("ETH", Decimal::ONE, Decimal::MAX, Some("native")), row("WETH", Decimal::ONE, Decimal::MAX, Some("native"))])]);
        e.st_mut().portfolio.total = Decimal::ONE;
        assert_eq!(e.rule_equity(), None);
    }).await;
}

#[tokio::test]
async fn the_model_reads_the_plan_the_book_the_positions_and_no_keys() {
    on_engine(async |e| {
        e.st_mut().intent = TradeIntent { asset: "SOL".into(), risk_usd: Some(Decimal::from(5)), ..TradeIntent::default() };
        e.st_mut().book = vec![SignerAddress { name: "Mom".into(), network: "base".into(), address: MOM.into(), memo: String::new() }];
        e.st_mut().positions = vec![trade::executor::Position { coin: "ETH".into(), size: d("-0.5"), entry: Decimal::from(2500), unrealized_pnl: d("-1.25"), liquidation: None }];
        e.set_memory(|m| m.push("I trade only BTC and ETH".into()));
        e.st_mut().messages_mut().push(Message::new(true, "what now", None));
        let c = e.router_context();
        assert_eq!(c.plan.as_deref(), Some("long SOL risk $5"));
        assert_eq!(c.address_book, ["Mom (Base)"]);
        assert_eq!(c.positions, ["short ETH 0.5 at 2,500, unrealized −$1.25"]);
        assert!(c.canvas.contains(&"schedule: nothing scheduled".to_string()), "{:?}", c.canvas);
        assert!(c.memory == ["I trade only BTC and ETH"] && c.safe_search && c.reply_language == "English");
        assert!(c.skills.iter().any(|s| s.starts_with("trading-study — Trading study: ")) && c.loaded_skills.is_empty());
        // The message itself isn't in the recent chat: it goes as the message.
        assert!(c.recent.is_empty(), "{:?}", c.recent);
        assert!(!format!("{c:?}").contains(MOM), "no address of the book reaches the model");
    }).await;
}

#[tokio::test]
async fn a_markets_line_has_its_price_change_and_funding() {
    on_engine(async |e| {
        assert_eq!(e.market_line("ETH"), None);
        let eth = Market {
            name: "ETH".into(), mark_px: Decimal::from(2500), sz_decimals: 4, max_leverage: 25, index: 1, prev_day_px: Some(Decimal::from(2400)),
            funding: Some(d("0.000013")), atr_pct: None, dex: None, volume24h: Decimal::ZERO,
        };
        e.st_mut().markets.insert("ETH".into(), eth);
        assert_eq!(e.market_line("ETH").as_deref(), Some("ETH: 2,500, 4.17% in 24 h, funding 0.0013% an hour"));
    }).await;
}
