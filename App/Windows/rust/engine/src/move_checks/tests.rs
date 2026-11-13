//! What the Mac checks before a move's card (MoveChecksTests, and the parts of Moves6Tests, Cards14Tests and Refusals8Tests
//! that are this file's): dust is the verified small tokens, a move the wallet can't pay is said before any card, a saved
//! address written out is its name and the first send to it warns, a route is drawn step by step with the balances it
//! changes. In-process: the balances set by hand, the store a test folder, nothing to the network or the signer.

use std::rc::Rc;
use std::sync::Arc;

use execution::trade_signer::TradeError;
use ihatecrypto_core::foundation::decimal;
use ihatecrypto_core::position_command::WalletCommand;
use rust_decimal::Decimal;
use signer_api::{SignerAddress, SignerReply, SignerWallet};

use crate::canvas::{Board, Step};
use crate::engine::{Card, Engine, Pending};
use crate::outbox::Outbox;
use crate::portfolio::{Network, PortfolioView, Row};
use crate::signer_link::SignerLink;
use crate::{link_signer, store};

// Public test data only: Hardhat's account #3.
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";

fn d(s: &str) -> Decimal { decimal(s).unwrap() }

/// `Engine.row(_:_:usd:chain:contract:verified:)` as the Swift tests build them: a dollar has no contract, a token keeps it.
fn row(symbol: &str, amount: &str, usd: Option<&str>, contract: Option<&str>, verified: Option<bool>) -> Row {
    Row { usd: usd.map(d), contract: contract.map(Into::into), verified, ..Row::new(symbol, amount, d(amount)) }
}

fn usdc(x: &str) -> Row { row("USDC", x, Some(x), None, None) }

fn network(key: &str, rows: Vec<Row>) -> Network {
    Network { key: key.into(), name: WalletCommand::place_name(key), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 }
}

/// An engine in English whose state is never pushed (no window here): built inside the test's LocalSet.
fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("no window in these tests"));
    {
        let mut st = e.st_mut();
        st.settings.language = Some("en".into());
        // A push goes out as the lock alone, never through the draw this test has none of.
        st.lock.locked = true;
    }
    e
}

/// These balances, read now.
fn wallet(e: &Engine, networks: Vec<Network>) {
    e.st_mut().portfolio = PortfolioView { networks, updated_at: Some(chrono::Utc::now()), ..PortfolioView::default() };
}

/// `route_steps` as it draws with no Jupiter quote shown (`with_jupiter` then changes nothing).
fn route(e: &Engine, c: &WalletCommand) -> Vec<Step> { e.raw_route_steps(c).map(|s| e.amounts_for_all(s)).unwrap_or_default() }

fn labels(steps: &[Step]) -> Vec<&str> { steps.iter().map(|s| s.label.as_str()).collect() }

async fn local(test: impl std::future::Future<Output = ()>) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(test).await;
}

/// Sweep dust sells the verified tokens worth $0.20 up to the limit on Base, Arbitrum and Polygon, the most first — never
/// the gas coin, USDC itself, an unverified token or one on another network.
#[test]
fn dust_is_the_verified_small_tokens() {
    let base = network("base", vec![
        usdc("3"),
        row("ETH", "0.001", Some("3"), Some("native"), None),
        row("AERO", "2", Some("1.4"), Some("0x940181a94A35A4569E4529A3CDfB74e38FD98631"), Some(true)),
        row("FAKE", "9", Some("2"), Some("0x000000000000000000000000000000000000c0de"), Some(false)),
        row("DUST", "1", Some("0.1"), Some("0x000000000000000000000000000000000000beef"), Some(true)),
        row("BIG", "1", Some("40"), Some("0x000000000000000000000000000000000000f00d"), Some(true)),
    ]);
    let arbitrum = network("arbitrum", vec![row("ARB", "4", Some("3.2"), Some("0x912ce59144191c1204e64559fe8253a0e49e6548"), Some(true))]);
    let solana = network("solana", vec![row("JUP", "3", Some("2"), Some("JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN"), Some(true))]);
    let symbols = |dust: Vec<trade::dust::Dust>| dust.into_iter().map(|d| d.symbol).collect::<Vec<_>>();
    assert_eq!(symbols(Engine::dust(&[base.clone(), arbitrum.clone(), solana], None)), ["ARB", "AERO"]);
    assert_eq!(symbols(Engine::dust(&[base, arbitrum], Some(Decimal::TWO))), ["AERO"]);
}

/// An empty wallet asked to put 5 USDC into Jupiter Lend hears what there is and the ways on, and no card is drawn; one
/// asked to sweep dust hears there's none (MoveChecksTests.aShortfallIsSaidBeforeACard, by its parts).
#[tokio::test]
async fn a_shortfall_is_said_before_a_card() {
    local(async {
        let e = engine();
        wallet(&e, vec![]);
        let (line, options) = e.short_choices(&WalletCommand::LendDeposit("USDC".into(), Some(Decimal::from(5)))).expect("said before a card");
        assert!(line.contains("On Solana there's 0 USDC") && line.contains("asks 5"), "{line}");
        assert!(options.iter().any(|o| o.title == "Top up first" && o.command == "top up"), "{options:?}");
        let dust = e.refusal_before_card(&WalletCommand::SweepDust { under: None }).expect("no dust");
        assert_eq!(dust, "No verified token worth $0.20 to $5.00 on Base, Arbitrum or Polygon — no dust to sweep, nothing was prepared.");
        // Not read yet: left to the move.
        e.st_mut().portfolio = PortfolioView::default();
        assert!(e.short_choices(&WalletCommand::LendDeposit("USDC".into(), Some(Decimal::from(5)))).is_none());
        assert!(e.refusal_before_card(&WalletCommand::SweepDust { under: None }).is_none());
    }).await;
}

/// A move from a place that holds too little, or nothing, is said with what's there.
#[tokio::test]
async fn a_move_from_a_place_short_of_it_says_what_is_there() {
    local(async {
        let e = engine();
        wallet(&e, vec![network("base", vec![usdc("2")])]);
        assert_eq!(e.refusal_before_card(&WalletCommand::BaseToSolana(Some(Decimal::from(5)))).as_deref(),
                   Some("Only 2 USDC is on Base, less than 5. Say an amount up to that."));
        assert!(e.refusal_before_card(&WalletCommand::BaseToSolana(Some(Decimal::ONE))).is_none());
        assert_eq!(e.refusal_before_card(&WalletCommand::ArbitrumToSolana(None)).as_deref(),
                   Some("Arbitrum has 0 USDC, so there’s nothing to move from there — nothing was prepared."));
        e.st_mut().settings.language = Some("ru".into());
        assert_eq!(e.refusal_before_card(&WalletCommand::ArbitrumToSolana(None)).as_deref(), Some("На Arbitrum 0 USDC — оттуда нечего переводить, ничего не готовлю."));
    }).await;
}

/// «send 2 USDC to 0x…» with that address saved as Mom: the message names Mom, and the first send to her warns — once
/// (MoveChecksTests.aSavedAddressIsItsNameAndAFirstSendWarns, by its parts). One of the user's own wallets never warns.
#[tokio::test]
async fn a_saved_address_is_its_name_and_a_first_send_warns() {
    local(async {
        let e = engine();
        let mom = SignerAddress { name: "Mom".into(), network: "base".into(), address: MOM.into(), memo: String::new() };
        e.st_mut().book = vec![mom.clone()];
        assert_eq!(e.saved_names(&format!("send 2 USDC to {MOM} on base")), "send 2 USDC to Mom on base");
        // Not a send: as written.
        assert_eq!(e.saved_names(&format!("what is {MOM}")), format!("what is {MOM}"));
        assert_eq!(e.first_send_warning(MOM).as_deref(), Some("First transfer to this address. Sending a small test amount first is the safe habit."));
        e.note_sent(&Pending::Transfer(mom, Decimal::TWO));
        assert!(e.first_send_warning(MOM).is_none() && e.first_send_warning(&MOM.to_lowercase()).is_none());
        assert_eq!(store::load::<Vec<String>>("sent-to.json").unwrap(), [MOM.to_lowercase()]);
        let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        e.st_mut().status = Some(SignerReply {
            ok: true,
            wallets: Some(vec![SignerWallet { id: "1".into(), label: "Main".into(), kind: "phrase".into(), solana: None, evm: Some(me.into()), near: None, test: None, automation: None }]),
            ..Default::default()
        });
        assert!(e.is_own_wallet(&me.to_lowercase()) && e.first_send_warning(me).is_none());
        assert_eq!(e.other_network_warning("base", "base"), None);
        assert!(e.other_network_warning("base", "ethereum").unwrap().starts_with("Saved for Base, sent on Ethereum to the same address."));
    }).await;
}

/// A move between the user's own places is drawn as its route: from, through the bridge, to — each a step, with the
/// balance it changes (MoveChecksTests.aWithdrawalIsDrawnAsItsRoute).
#[tokio::test]
async fn a_withdrawal_is_drawn_as_its_route() {
    local(async {
        let e = engine();
        wallet(&e, vec![network("solana", vec![usdc("20")])]);
        let steps = route(&e, &WalletCommand::WithdrawToSolana(Some(Decimal::ONE)));
        assert_eq!(labels(&steps), ["FROM · HYPERLIQUID", "BRIDGE · RELAY", "TO · SOLANA"]);
        let to = steps[2].money.as_ref().unwrap();
        assert_eq!((to.place.as_str(), to.was.as_str(), to.after.as_str()), ("Solana", "20", "≈ 21"));
        assert_eq!(steps[1].lines, ["≈ 4 s · small Relay fee"]);
    }).await;
}

/// «all USDC» on a step reads as the amount held, «all available» under it; unread, the words stay (Moves6Tests).
#[tokio::test]
async fn all_is_the_amount_held() {
    local(async {
        let e = engine();
        wallet(&e, vec![network("solana", vec![usdc("20")])]);
        let steps = route(&e, &WalletCommand::LendDeposit("USDC".into(), None));
        assert_eq!(steps[0].title, "20 USDC");
        assert!(steps[0].lines[0].starts_with("all available"), "{:?}", steps[0].lines);
        e.st_mut().portfolio = PortfolioView::default();
        assert_eq!(route(&e, &WalletCommand::LendDeposit("USDC".into(), None))[0].title, "all USDC");
    }).await;
}

/// An Aave or Morpho deposit with no gas there shows its bridge from Solana as the executor runs it (Cards14Tests'
/// routes; the footers are move_routes.rs's).
#[tokio::test]
async fn a_deposit_with_no_gas_there_shows_its_bridge_from_solana() {
    local(async {
        let e = engine();
        let eth = |x: &str| row("ETH", x, None, Some("native"), None);
        // The USDC on Arbitrum, no ETH: half a dollar and gas come from Solana first.
        wallet(&e, vec![network("solana", vec![usdc("50")]), network("arbitrum", vec![usdc("20")])]);
        let lend = WalletCommand::Aave { deposit: true, amount: Some(Decimal::from(5)), network: "arbitrum".into() };
        let dry = route(&e, &lend);
        assert_eq!(labels(&dry[..2]), ["FROM · SOLANA", "BRIDGE · RELAY"]);
        assert_eq!(dry[0].title, "0.5 USDC");
        assert!(dry[0].lines[0].starts_with("20 already on Arbitrum"), "{:?}", dry[0].lines);
        assert_eq!(dry[1].lines, ["+ ETH for gas · ≈ 2 s"]);
        // With the gas there too: the money is already there.
        wallet(&e, vec![network("solana", vec![usdc("50")]), network("arbitrum", vec![usdc("20"), eth("0.01")])]);
        assert_eq!(route(&e, &lend)[0].label, "ON ARBITRUM");
        // Base, as the Mac's aaveRoute: what Solana sends, the gas coming along.
        wallet(&e, vec![network("solana", vec![usdc("50")]), network("base", vec![usdc("20")])]);
        let morpho = WalletCommand::Morpho { deposit: true, amount: Some(Decimal::from(5)), network: "base".into() };
        let base = route(&e, &morpho);
        assert_eq!(labels(&base), ["FROM · SOLANA", "BRIDGE · RELAY", "GAS · COMES ALONG", "APPROVE · EXACT", "MORPHO · GAUNTLET USDC PRIME"]);
        assert_eq!(base[0].lines, ["sends ≈ 0.64 USDC"]);
        assert_eq!(base[1].title, "0.5 USDC → Base");
        wallet(&e, vec![network("base", vec![usdc("20"), eth("0.001")])]);
        assert_eq!(route(&e, &morpho)[0].label, "ON BASE");
    }).await;
}

/// A balance a step changes: now → after, in the Mac's MoneyLine places.
#[tokio::test]
async fn a_step_shows_the_balance_it_changes() {
    local(async {
        let e = engine();
        wallet(&e, vec![network("solana", vec![usdc("20")])]);
        let m = e.moved("USDC", "solana", Some(-Decimal::from(5)), false).unwrap();
        assert_eq!((m.place.as_str(), m.symbol.as_str(), m.was.as_str(), m.after.as_str()), ("Solana", "USDC", "20", "15"));
        assert_eq!(e.moved("USDC", "solana", None, true).unwrap().after, "≈ 0");
        assert!(e.moved("SOL", "base", Some(Decimal::ONE), false).is_some_and(|m| m.was == "0"));
        e.st_mut().portfolio = PortfolioView::default();
        assert!(e.moved("USDC", "solana", Some(Decimal::ONE), false).is_none());
    }).await;
    assert_eq!(Engine::amount(d("2.4722"), "AERO"), "2.4722");
    assert_eq!(Engine::amount(d("0.074852"), "SOL"), "0.074852");
    assert_eq!(Engine::amount(d("3.14159"), "USDC"), "3.14");
}

/// The answer to «How much?» in another unit is asked again in the question's; «no» lets it go (MoveChecks.swift
/// answeredAmount).
#[tokio::test]
async fn an_amount_in_another_unit_is_asked_again() {
    local(async {
        let e = engine();
        e.st_mut().awaiting_amount = Some(WalletCommand::Stake(None));
        assert!(e.answered_amount("$5", false).await);
        let said = e.st().messages().last().unwrap().text.clone();
        assert_eq!(said, "The question is in SOL. How much SOL? Write a number in SOL, for example “0.01”.");
        assert!(e.st().awaiting_amount.is_some());
        assert!(e.answered_amount("no", true).await);
        assert_eq!(e.st().messages().last().unwrap().text, "OK, not doing that.");
        assert!(e.st().awaiting_amount.is_none());
        // Anything else is the chat's again.
        e.st_mut().awaiting_amount = Some(WalletCommand::Stake(None));
        assert!(!e.answered_amount("what's the weather", false).await);
        assert!(e.st().awaiting_amount.is_none());
    }).await;
}

/// A card left on a canvas waits there; back within a quarter of an hour it waits again, later it's said gone.
#[tokio::test]
async fn a_waiting_card_waits_on_its_canvas() {
    local(async {
        let e = engine();
        let mom = SignerAddress { name: "Mom".into(), network: "base".into(), address: MOM.into(), memo: String::new() };
        let board = Board::new("move", "Send 1 USDC to Mom");
        let card = Card { id: board.id.clone(), title: board.title.clone(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![], over_limit: false };
        e.set_boards(|b| b.push(board.clone()));
        let wait = |e: &Engine| {
            let mut st = e.st_mut();
            st.pending = Some(Pending::Transfer(mom.clone(), Decimal::ONE));
            st.card = Some(card.clone());
        };
        wait(&e);
        assert!(e.park_pending());
        assert!(e.st().pending.is_none() && e.st().card.is_none());
        assert!(!e.park_pending());
        e.unpark_pending();
        assert_eq!(e.st().card.as_ref().map(|c| c.id.clone()), Some(card.id.clone()));
        // Away longer than a quarter of an hour: gone, nothing signed.
        assert!(e.park_pending());
        let canvas = e.st().canvas_id.clone();
        e.st_mut().parked.get_mut(&canvas).unwrap().at = chrono::Utc::now() - chrono::TimeDelta::minutes(16);
        e.unpark_pending();
        assert!(e.st().pending.is_none());
        let st = e.st();
        let b = st.boards().iter().find(|b| b.id == card.id).unwrap();
        assert_eq!(b.state.as_deref(), Some("cancelled"));
        assert_eq!(b.lines.last().unwrap(), "Waited a quarter of an hour: nothing was signed.");
    }).await;
}

/// After a stop by the guard, the signer or a scam check nothing is offered around it (the Mac's a3f7edb); a move that
/// only lacked money or a route gets the way that works (Refusals8Tests).
#[test]
fn a_signers_refusal_is_a_stop_that_nothing_is_offered_around() {
    let reply = SignerReply { ok: false, error: Some("Windows Hello was declined.".into()), ..Default::default() };
    let refused = trade::Error::from(TradeError::Guard(link_signer::reason(&reply)));
    assert_eq!(refused.to_string(), "Windows Hello was declined.");
    assert!(!Engine::explains_after(&refused));
    assert!(!Engine::explains_after(&trade::Error::Guard("The guard stopped the swap: a program off the list.".into())));
    assert!(!Engine::explains_after(&trade::Error::Costly(trade::CostlyMove::new(Decimal::TWO, Decimal::TEN, "Relay"))));
    assert!(Engine::explains_after(&trade::Error::Execution("Only 3 USDC on Base.".into())));
}
