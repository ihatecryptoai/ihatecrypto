//! BlocksTests (the token card's numbers, each network's dollar, the address form's check and the address book's
//! extras, a token's card opened twice), Cards6Tests (a chart's live parts, a step to the chat, the plan's alert
//! example) and Cards14Tests' approvals board, offline. Public addresses only (Hardhat's first account, Circle's USDC).

use std::collections::HashSet;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::alerts::PriceAlert;
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::pnl::CostBasis;
use signer_api::SignerReply;

use super::*;
use crate::engine::Message;
use crate::outbox::Outbox;
use crate::portfolio::{Network, PortfolioView};
use crate::signer_link::SignerLink;

const EVM: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

/// An engine in a test folder of its own, locked so a state push never draws.
fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::writer(std::io::sink()), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("locked: no state is drawn"));
    e.st_mut().lock.locked = true;
    e
}

fn d(s: &str) -> Decimal { s.parse().unwrap() }

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(test));
}

#[test]
fn the_token_cards_numbers_read_as_the_macs() {
    assert_eq!(Engine::compact_usd(d("1234000000")), "$1.2B");
    assert_eq!(Engine::compact_usd(d("34500000")), "$34.5M");
    assert_eq!(Engine::compact_usd(d("12000")), "$12K");
    assert!(Engine::compact_usd(d("950")).starts_with("$950"));
    assert_eq!(Engine::short_name("Space Exploration Technologies Corp. Class A Common Stock"), "Space Exploration Technologies");
    assert_eq!(Engine::short_name("Bonk"), "Bonk");
    assert_eq!(Engine::day_month(ihatecrypto_core::foundation::unix(1_791_633_600.0)), "Oct 10");
}

#[test]
fn each_networks_own_dollar_is_known() {
    assert_eq!(Engine::wallet_dollar("solana").as_deref(), Some(MINT));
    assert_eq!(Engine::wallet_dollar("base").map(|a| a.to_lowercase()).as_deref(), Some("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"));
    assert_eq!(Engine::wallet_dollar("near"), None);
    assert_eq!(Engine::extra_key("Mom", "base"), "base:mom");
}

#[test]
fn an_address_is_checked_for_its_network_as_its_typed() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("en".into()));
        assert!(e.check_address("  ", "solana", None).is_empty());
        assert!(e.check_address(EVM, "nowhere", None).contains_key("problem"));
        // An EVM address under Solana, and a Solana one under Base, say which network they belong to.
        assert!(e.check_address(EVM, "solana", None)["problem"].contains("EVM address"));
        assert!(e.check_address(MINT, "base", None)["problem"].contains("pick Solana"));
        let ok = e.check_address(EVM, "base", None);
        assert!(!ok.contains_key("problem") && ok["start"] == "0xf3 9Fd6" && ok["end"] == "fFb9 2266", "{ok:?}");
        assert!(!ok.contains_key("duplicate") && !ok.contains_key("own"));
        // Saved already under another name, and this wallet's own.
        e.st_mut().book = vec![SignerAddress { name: "Mom".into(), network: "base".into(), address: EVM.to_lowercase(), memo: String::new() }];
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), evm_address: Some(EVM.into()), ..Default::default() });
        let again = e.check_address(EVM, "base", None);
        assert_eq!((again.get("duplicate").map(String::as_str), again.get("own").map(String::as_str)), (Some("Mom"), Some("true")));
        assert!(!e.check_address(EVM, "base", Some("mom")).contains_key("duplicate"));
    });
}

#[test]
fn a_saved_address_keeps_its_picture_category_and_note() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.set_address_extra("Mom", "base", Some(r#"{"kind":"emoji","value":"🦄"}"#), None, None).unwrap();
        e.set_address_extra("Mom", "base", None, Some("Friend"), Some("Birthday money")).unwrap();
        let extra = e.address_extras()["base:mom"].clone();
        assert_eq!(extra.icon.map(|i| i.value).as_deref(), Some("🦄"));
        assert_eq!((extra.category.as_deref(), extra.note.as_deref()), (Some("Friend"), Some("Birthday money")));
        // Kept in address-extras.json, read back after a restart.
        let saved: HashMap<String, AddressExtra> = store::load("address-extras.json").unwrap();
        assert_eq!(saved["base:mom"].category.as_deref(), Some("Friend"));
        // A picture the window shouldn't send is refused; clearing everything leaves nothing behind.
        assert!(e.set_address_extra("Mom", "base", Some(r#"{"kind":"image","value":"https://example.com/x.png"}"#), None, None).is_err());
        e.set_address_extra("Mom", "base", Some(""), None, None).unwrap();
        e.set_address_extra("Mom", "base", None, Some(""), Some("")).unwrap();
        assert!(!e.address_extras().contains_key("base:mom"));
        // A category keeps 40 characters, a note 200.
        e.set_address_extra("Bob", "solana", None, Some(&"é".repeat(50)), None).unwrap();
        assert_eq!(e.address_extras()["solana:bob"].category.as_ref().map(|c| c.chars().count()), Some(40));
    });
}

#[test]
fn a_tokens_card_opens_once_and_the_dollar_row_finds_its_contract() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        // A dollar row has no contract: the network's own dollar is opened. Nothing here yields, so the market's read
        // (spawned) never starts: no network.
        let first = e.open_token("", "solana").await;
        let id = first["board"].clone();
        let card = e.st().boards().iter().find(|b| b.id == id).and_then(|b| b.data.as_ref()?.token.clone()).unwrap();
        assert!(card.address == MINT && card.dollar && card.loading && card.subtitle == "Token · Solana");
        let again = e.open_token(MINT, "solana").await;
        assert_eq!(again["board"], id);
        let tokens: Vec<String> = e.st().boards().iter().filter(|b| b.kind == "token").map(|b| b.id.clone()).collect();
        assert_eq!(tokens, [id]);
        assert!(e.open_token("", "near").await.is_empty());
    });
}

#[test]
fn a_chart_board_reads_its_market_levels_and_what_is_held() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("en".into()));
        {
            let mut st = e.st_mut();
            st.markets = HashMap::from([("SOL".to_string(), Market::new("SOL", d("200"), 2, 20))]);
            st.alerts = vec![PriceAlert::new("SOL", false, d("180"), None)];
            st.portfolio = PortfolioView {
                networks: vec![Network { key: "solana".into(), name: "Solana".into(), usd: Decimal::ZERO, note: None, hidden: vec![], hidden_more: 0,
                                         rows: vec![Engine::row("SOL", d("2"), None, None, Some("solana"), Some(CostBasis::WSOL), None, None)] }],
                updated_at: Some(chrono::Utc::now()), ..Default::default()
            };
        }
        let x = e.chart_extras("SOL", None);
        assert_eq!(x.tiles.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Volume 24h", "Funding 1h", "Max leverage"]);
        assert_eq!(x.tiles.iter().map(|t| t.value.as_str()).collect::<Vec<_>>(), ["—", "—", "20×"]);
        assert!(x.levels.iter().map(|l| l.label.as_str()).eq(["Alert"]) && x.levels[0].dashed);
        // SOL's home is Solana: held there at Hyperliquid's price even from a chart the chat opened.
        let held = x.held.clone().unwrap();
        assert!(held.usd == Some(d("400")) && held.how == "valued at Hyperliquid's price", "{held:?}");
        assert_eq!(held.paid.as_deref(), Some("No price yet to count its P/L from"));
        // With what it cost: its P/L and the line under it.
        e.set_app_costs(|c| { c.insert(CostBasis::WSOL.into(), CostBasis { amount: d("2"), usd: d("300"), ..Default::default() }); });
        let held = e.chart_extras("SOL", None).held.unwrap();
        assert_eq!((held.pnl.as_deref(), held.gain), (Some("+$100 (+33.3%)"), Some(d("100"))));
        assert_eq!(held.paid.as_deref(), Some("Bought 2 SOL for $300.00 at $150.00 · worth $400.00 now"));
        // Only chart boards get them, read for the window, never kept.
        let mut chart = Board::new("chart", "SOL · 24 h");
        chart.chart = Some(ChartLine { title: "SOL · 24 h".into(), closes: vec![1.0, 2.0], times: None, low: "$1".into(), high: "$2".into(), change: "+1%".into(),
                                       coin: Some("SOL".into()), network: None, extras: None });
        let shown = e.with_chart_extras(vec![chart, Board::new("result", "x")]);
        assert!(shown[0].chart.as_ref().and_then(|c| c.extras.as_ref()?.held.as_ref()).is_some() && shown[1].chart.is_none());
    });
}

#[test]
fn a_step_goes_to_the_chat_on_its_own() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        let mut b = Board::new("move", "Move 5 USDC to Base");
        b.steps = Some(vec![step("FROM · SOLANA", "5 USDC", vec!["free\t20 USDC".into()]), step("TO · BASE", "≈ 4.99 USDC", vec![])]);
        let s = e.step_board(&b, 1).unwrap();
        assert_eq!(s.title, "Step 2 · TO · BASE · ≈ 4.99 USDC");
        assert!(s.lines[0].contains("of the route «Move 5 USDC to Base»"), "{:?}", s.lines);
        assert!(e.step_board(&b, 0).unwrap().lines[0].ends_with("(5 USDC · free 20 USDC)"));
        assert!(e.step_board(&b, 5).is_none() && e.step_board(&b, -1).is_none());
    });
}

#[test]
fn add_blocks_price_alert_is_for_the_plan_on_the_canvas() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.st_mut().markets = HashMap::from([("ETH".to_string(), Market::new("ETH", d("2000"), 4, 25))]);
        assert_eq!(e.plan_alert_example(), None);
        let intent = IntentParser::apply("long ETH risk $5", &TradeIntent::default(), &HashSet::from(["ETH".to_string()])).unwrap();
        e.st_mut().intent = intent.clone();
        let example = e.plan_alert_example().unwrap();
        assert!(example.starts_with("when ETH is below 1,960: long ETH"), "{example}");
        // Once traded, the plan is managed: no example.
        e.st_mut().traded_plan = Some(intent);
        assert_eq!(e.plan_alert_example(), None);
    });
}

#[test]
fn the_approvals_board_says_it_reads_before_the_read_is_back() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("en".into()));
        let id = e.reading_approvals();
        let approvals = |e: &Engine| e.st().boards().iter().find(|b| b.id == id).cloned().unwrap();
        assert_eq!(approvals(&e).data.unwrap().approvals.unwrap().loading, Some(true));
        e.fill_approvals(&id, &[], &["base".into()]);
        let b = approvals(&e);
        let card = b.data.unwrap().approvals.unwrap();
        assert!(card.loading.is_none() && b.lines == ["No token approvals."] && card.unread == ["Base"] && b.actions.is_none(), "{:?}", b.lines);
        // One unlimited approval: its line, its Revoke and its card row.
        let a = TokenApproval { chain: "base".into(), token: "0xtoken".into(), symbol: "USDC".into(), decimals: Some(6), spender: "0xspender".into(),
                                spender_name: Some("Uniswap".into()), amount: Decimal::MAX, permit2: false };
        e.fill_approvals(&id, &[a.clone(), TokenApproval { amount: d("2500000"), spender_name: None, ..a }], &[]);
        let b = approvals(&e);
        assert_eq!(b.lines, ["USDC on base → Uniswap · unlimited", "USDC on base → 0xspender"]);
        assert_eq!(b.actions.as_ref().unwrap()[0].title, "Revoke USDC · base");
        let card = b.data.unwrap().approvals.unwrap();
        assert_eq!(card.list.iter().map(|a| a.amount.as_str()).collect::<Vec<_>>(), ["unlimited", "2.5"]);
        assert_eq!(card.list[0].place, "Base");
    });
}

#[test]
fn a_board_keeps_the_request_not_the_yes_after_it() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        for (user, text) in [(true, "show my approvals"), (false, "Here."), (true, "yes"), (true, "cancel")] {
            e.st_mut().messages_mut().push(Message::new(user, text, None));
        }
        assert_eq!(e.last_request().as_deref(), Some("show my approvals"));
    });
}

#[test]
fn the_pnl_boards_periods_are_said_in_the_chats_language() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        assert_eq!(raw(Period::All), "all");
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(Period::ALL.map(|p| e.period_title(p)), ["24 ч", "7 дней", "30 дней", "всё время"]);
        e.set_settings(|s| s.language = Some("en".into()));
        assert_eq!(e.period_title(Period::Week), "7 days");
    });
}

#[test]
fn a_rebalance_and_a_spread_read_as_route_cards() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("en".into()));
        let m = rebalance::Move { from: "Kamino".into(), to: "Aave · Base".into(), amount: d("100"), from_apy: d("4"), to_apy: d("6.5"), cost: d("0.2") };
        let plan = rebalance::Plan { moves: vec![m], skipped: vec![], notes: vec!["n".into()] };
        let (steps, notes) = e.rebalance_steps(&plan, d("50"));
        assert_eq!(steps.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["REBALANCE · USDC", "FROM · KAMINO", "INTO · AAVE · BASE"]);
        assert_eq!(steps[0].title, "+$2.50 a year");
        assert_eq!(steps[2].lines, ["rate\t6.5%", "a year\t+$2.50", "costs\t≈ $0.20", "pays back\t30 days"]);
        assert!(notes[0].starts_with("Together ≈ +$2.50 a year for ≈ $0.20.") && notes[1] == "n");
    });
}
