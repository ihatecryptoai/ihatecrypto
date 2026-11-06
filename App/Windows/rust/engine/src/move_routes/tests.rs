//! The Mac's route for each move Windows drew as one card, and the footer under it (CardsTests, Cards13Tests,
//! Cards14Tests): each route's steps in order, a quote on the step it names, the cautions under the card, what each
//! footer says. In-process: this test's own data folder, balances set by hand, nothing goes to the network (every
//! contract named here is marked as asked, so no symbol is looked up).

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use ihatecrypto_core::plan::Market;
use signer_api::{SignerReply, SignerWallet};
use trade::executor::Position;

use super::*;
use crate::canvas::Board;
use crate::engine::{Card, ChainStep};
use crate::from_core::MppChallenge;
use crate::multisig::{Multisig, MultisigDraft, MultisigKey, MultisigPayment};
use crate::outbox::Outbox;
use crate::payments::PaymentQuote;
use crate::portfolio::{Network as PlaceRows, PortfolioView, Row};
use crate::signer_link::SignerLink;
use crate::store;

// Public test data only: Hardhat's addresses, a token's public contract, Base's USDC, XRP's account zero.
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
const ME: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const PHONE: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const AERO: &str = "0x940181a94A35A4569E4529A3CDfB74e38FD98631";
const FAKE: &str = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";
const BASE_USDC: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
const XRP_ZERO: &str = "rrrrrrrrrrrrrrrrrrrrrhoLvTp";

/// Route notes read Settings › Wallet's one-transaction switch, a process-wide flag: the tests that read them take turns.
static NOTES: Mutex<()> = Mutex::new(());

fn d(s: &str) -> Decimal { s.parse().expect("a decimal") }

/// An engine on this test's own folder and thread, in English, its window locked (a state push sends the lock alone,
/// so no Snapshot is drawn), and no contract here looked up.
fn with_engine(test: impl FnOnce(&Engine)) {
    let _dir = store::test_folder();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    tokio::task::LocalSet::new().block_on(&rt, async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the tests keep the window locked"));
        e.st_mut().lock.locked = true;
        e.set_settings(|s| s.language = Some("en".into()));
        e.st_mut().contract_names.asked.extend([AERO, FAKE, BASE_USDC].map(str::to_lowercase));
        test(&e);
    });
}

fn row(symbol: &str, value: &str, usd: Option<&str>, contract: Option<&str>) -> Row {
    Row { usd: usd.map(d), contract: contract.map(str::to_string), ..Row::new(symbol, value, d(value)) }
}

fn usdc(x: &str) -> Row { row("USDC", x, Some(x), None) }

/// A fresh wallet: these balances, nothing quoted or picked, Tesla's and NEAR's marks.
fn wallet(e: &Engine, networks: Vec<(&str, Vec<Row>)>) {
    let mut st = e.st_mut();
    st.portfolio = PortfolioView {
        networks: networks.into_iter().map(|(k, rows)| PlaceRows { key: k.into(), name: WalletCommand::place_name(k), usd: Decimal::ZERO, rows, note: None,
                                                                   hidden: vec![], hidden_more: 0 }).collect(),
        updated_at: Some(Utc::now()), ..Default::default()
    };
    st.way_quotes = None;
    st.route_way = None;
    st.status = None;
    st.markets = HashMap::from([("xyz:TSLA".into(), Market::new("xyz:TSLA", Decimal::from(250), 3, 10)), ("NEAR".into(), Market::new("NEAR", d("2.5"), 1, 10))]);
}

fn quoted(e: &Engine, c: &WalletCommand, ways: &[(&str, &str, &str)]) {
    e.st_mut().way_quotes = Some(WayQuotes { command: c.clone(), ways: ways.iter().map(|(w, g, worth)| way(w, g, worth)).collect() });
}

fn way(w: &str, gets: &str, worth: &str) -> Way { Way { way: w.into(), gets: gets.into(), worth: worth.into(), less: None, usd: None } }

fn labels(e: &Engine, c: &WalletCommand) -> Vec<String> { e.card_route(c).unwrap_or_default().into_iter().map(|s| s.label).collect() }

fn step(e: &Engine, c: &WalletCommand, label: &str) -> Option<Step> { e.card_route(c)?.into_iter().find(|s| s.label == label) }

fn buy_on(place: &str, token: &str, usdc: &str) -> WalletCommand {
    WalletCommand::BuyOn { place: place.into(), token: token.into(), usdc: Some(d(usdc)), from: None }
}

fn swap_on(place: &str, from: &str, to: &str, amount: &str) -> WalletCommand {
    WalletCommand::SwapOn { place: place.into(), from: from.into(), to: to.into(), amount: Some(d(amount)) }
}

fn cross(from: &str, on: &str, to: &str, at: &str, amount: &str) -> WalletCommand {
    WalletCommand::CrossSwap { from: from.into(), on: on.into(), to: to.into(), at: at.into(), amount: Some(d(amount)) }
}

fn entry(name: &str, network: &str, address: &str) -> SignerAddress {
    SignerAddress { name: name.into(), network: network.into(), address: address.into(), memo: String::new() }
}

/// A board on the open canvas and the card waiting on it, as show() leaves them.
fn waiting(e: &Engine, pending: Pending, title: &str, lines: &[&str]) -> String {
    let board = Board { state: Some("waiting".into()), steps: Some(vec![]), ..Board::new("move", title) };
    let id = board.id.clone();
    e.set_boards(|b| b.insert(0, board));
    let mut st = e.st_mut();
    st.pending = Some(pending);
    st.card = Some(Card { id: id.clone(), title: title.into(), lines: lines.iter().map(|l| l.to_string()).collect(), warnings: vec![], confirm: "Confirm".into(),
                          ticks: vec![] });
    id
}

fn board(e: &Engine, id: &str) -> Board { e.st().boards().iter().find(|b| b.id == id).cloned().expect("the board") }

/// Svc.example's price: a cent of USDC on Tempo to Hardhat's account #1.
fn payment(e: &Engine) -> WalletCommand {
    let pay = WalletCommand::Pay { method: "GET".into(), url: "https://svc.example/price".into(), body: None };
    let challenge = MppChallenge { params: HashMap::from([("realm".into(), "svc.example".into())]), amount: 10_000, payee: PHONE.into(), memo: None, expires: None };
    e.st_mut().payment = Some(PaymentQuote { command: pay.clone(), challenge, ask: String::new(), service: "Example".into(), probed: false, task: None });
    pay
}

fn keys() -> Vec<MultisigKey> {
    vec![MultisigKey { name: "This computer".into(), address: ME.to_lowercase(), kind: "pc".into() },
         MultisigKey { name: "Ledger".into(), address: PHONE.to_lowercase(), kind: "ledger".into() }]
}

fn multisig_send(e: &Engine, held: &str) -> WalletCommand {
    let send = WalletCommand::MultisigSend { token: "USDC".into(), amount: Some(Decimal::from(5)), to: "Mom".into(), network: Some("base".into()) };
    e.st_mut().multisig_payment = Some(MultisigPayment {
        command: send.clone(), multisig: Multisig { address: MOM.to_lowercase(), keys: keys(), threshold: 2, salt: 1, networks: vec!["base".into()] },
        net: "base".into(), to: entry("Mom", "base", MOM), symbol: "USDC".into(), token: None, amount: Decimal::from(5), units: Decimal::from(5_000_000), threshold: 2,
        signers: keys(), held: d(held),
    });
    send
}

// MARK: CardsTests

#[test]
fn a_stock_buy_bridges_approves_and_takes_the_best_way() {
    with_engine(|e| {
        wallet(e, vec![("solana", vec![usdc("50")]), ("robinhood", vec![row("USDG", "0", Some("0"), None)])]);
        let buy = WalletCommand::BuyStock { symbol: "TSLA".into(), usdc: Some(Decimal::TEN) };
        assert_eq!(labels(e, &buy), ["FROM · SOLANA", "BRIDGE · RELAY", "APPROVE · EXACT", "BUY · COMPARING"]);
        quoted(e, &buy, &[("v4", "0.04 TSLA", "$9.95"), ("relay", "0.039 TSLA", "$9.80")]);
        let steps = e.card_route(&buy).unwrap();
        assert_eq!(steps.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["FROM · SOLANA", "BRIDGE · RELAY", "APPROVE · EXACT", "BUY · UNISWAP V4 · BEST"]);
        assert!(steps[2].lines[0].contains("Permit2"), "{:?}", steps[2]);
        assert_eq!(steps[3].lines[0], "≈ 0.04 TSLA · Relay swap ≈ 0.039 TSLA");
        assert_eq!(steps[3].money.as_ref().unwrap().after, "≈ 0.04");
        // The USDG already there pays: no bridge.
        wallet(e, vec![("solana", vec![usdc("50")]), ("robinhood", vec![row("USDG", "10", Some("10"), None)])]);
        assert_eq!(labels(e, &buy), ["FROM · ROBINHOOD CHAIN", "APPROVE · EXACT", "BUY · COMPARING"]);
    });
}

#[test]
fn a_stock_sale_is_approved_sold_and_lands_as_usdg() {
    with_engine(|e| {
        wallet(e, vec![("robinhood", vec![row("TSLA", "1", Some("250"), None)])]);
        let sale = WalletCommand::SellStock { symbol: "TSLA".into(), amount: None };
        assert_eq!(labels(e, &sale), ["APPROVE · EXACT", "SELL · UNISWAP", "TO · USDG"]);
        assert_eq!(e.card_route(&sale).unwrap().last().unwrap().title, "≈ 250 USDG");
        quoted(e, &sale, &[("v3", "249.1 USDG", "$249.10")]);
        assert_eq!(labels(e, &sale), ["APPROVE · EXACT", "SELL · UNISWAP V3 · BEST", "TO · USDG"]);
        assert_eq!(e.card_route(&sale).unwrap().last().unwrap().title, "≈ 249.1 USDG");
    });
}

#[test]
fn an_evm_token_is_bought_here_or_from_solana_the_way_that_leaves_more() {
    with_engine(|e| {
        let buy = buy_on("base", "AERO", "5");
        // The USDC on Base pays: approve and Uniswap there.
        wallet(e, vec![("base", vec![usdc("20")]), ("solana", vec![usdc("50")])]);
        assert_eq!(labels(e, &buy), ["FROM · BASE", "APPROVE · EXACT", "BUY · UNISWAP ON BASE"]);
        quoted(e, &buy, &[("here", "3.2 AERO", "$4.98")]);
        let bought = step(e, &buy, "BUY · UNISWAP ON BASE").unwrap();
        assert_eq!(bought.lines, ["≈ 3.2 AERO"]);
        assert_eq!(bought.money.unwrap().after, "≈ 3.2");
        // None on Base: from Solana, the two ways compared, and once quoted the better one with its approve.
        wallet(e, vec![("base", vec![usdc("0")]), ("solana", vec![usdc("50")])]);
        assert_eq!(labels(e, &buy), ["FROM · SOLANA", "RELAY OR UNISWAP · COMPARING", "BUY · ON BASE"]);
        quoted(e, &buy, &[("uniswap", "3.1 AERO", "$4.90"), ("relay", "3.0 AERO", "$4.80")]);
        assert_eq!(labels(e, &buy), ["FROM · SOLANA", "RELAY + UNISWAP · BEST NOW", "APPROVE · EXACT", "BUY · ON BASE"]);
        assert_eq!(step(e, &buy, "RELAY + UNISWAP · BEST NOW").unwrap().lines, ["$0.10 more than Relay swap"]);
        assert_eq!(step(e, &buy, "BUY · ON BASE").unwrap().lines, ["≈ 3.1 AERO arrives"]);
        // Picked in the step's details: Relay's swap, no approve.
        e.st_mut().route_way = Some(crate::engine::RouteWay { key: "base:aero".into(), way: "relay".into() });
        assert_eq!(labels(e, &buy), ["FROM · SOLANA", "RELAY SWAP", "BUY · ON BASE"]);
        // Sold back to Solana, or for USDC that stays.
        wallet(e, vec![("base", vec![usdc("0"), row("AERO", "4", Some("6"), Some(AERO))])]);
        let sell = |keep| WalletCommand::SellOn { place: "base".into(), token: "AERO".into(), amount: Some(Decimal::TWO), keep };
        assert_eq!(labels(e, &sell(false)), ["FROM · BASE", "RELAY OR UNISWAP · COMPARING", "TO · SOLANA"]);
        assert_eq!(labels(e, &sell(true)), ["FROM · BASE", "APPROVE · EXACT", "TO · USDC ON BASE"]);
        assert_eq!(e.card_route(&sell(false)).unwrap()[0].money.as_ref().unwrap().after, "2");
    });
}

#[test]
fn swaps_off_solana_and_across_networks_have_their_routes() {
    with_engine(|e| {
        wallet(e, vec![("base", vec![usdc("20"), row("ETH", "0.01", Some("30"), Some("native"))]), ("solana", vec![usdc("50")])]);
        // The network's own coin goes in as itself; a token is approved exactly first.
        assert_eq!(labels(e, &swap_on("base", "ETH", AERO, "0.001")), ["FROM · BASE", "SWAP · UNISWAP ON BASE"]);
        let pair = swap_on("base", "USDC", "AERO", "5");
        assert_eq!(labels(e, &pair), ["FROM · BASE", "APPROVE · EXACT", "SWAP · UNISWAP ON BASE"]);
        quoted(e, &pair, &[("Uniswap v3", "3.2 AERO", "")]);
        assert_eq!(labels(e, &pair).last().unwrap(), "SWAP · UNISWAP V3 ON BASE");
        assert_eq!(step(e, &pair, "SWAP · UNISWAP V3 ON BASE").unwrap().lines, ["≈ 3.2 AERO"]);
        assert_eq!(labels(e, &swap_on("near", "NEAR", "USDC", "1")), ["FROM · NEAR", "SWAP · NEAR INTENTS", "TO · NEAR"]);
        assert_eq!(labels(e, &swap_on("hyperliquid", "HYPE", "PURR", "1")), ["FROM · SPOT BALANCE", "SELL · SPOT ORDER", "BUY · SPOT ORDER"]);
        let tempo = swap_on("tempo", "USDC", "PATHUSD", "1");
        assert_eq!(labels(e, &tempo), ["FROM · TEMPO", "APPROVE · EXACT", "SWAP · TEMPO'S EXCHANGE"]);
        quoted(e, &tempo, &[("Relay's swap", "0.99 USDT0", "")]);
        assert_eq!(labels(e, &tempo).last().unwrap(), "SWAP · RELAY ON TEMPO");
        // Across: Relay's swap, an approve for a token on an EVM network; NEAR's side through NEAR Intents.
        assert_eq!(labels(e, &cross("USDC", "solana", "ETH", "base", "5")), ["FROM · SOLANA", "SWAP ACROSS · RELAY", "TO · BASE"]);
        assert_eq!(labels(e, &cross("USDC", "base", "USDC", "arbitrum", "5")), ["FROM · BASE", "APPROVE · EXACT", "SWAP ACROSS · RELAY", "TO · ARBITRUM"]);
        assert_eq!(labels(e, &cross("NEAR", "near", "USDC", "solana", "1")), ["FROM · NEAR", "SWAP ACROSS · NEAR INTENTS", "TO · SOLANA"]);
        let revoke = WalletCommand::RevokeApproval { place: "base".into(), token: AERO.into(), spender: PHONE.into(), permit2: false };
        assert_eq!(labels(e, &revoke), ["TOKEN · BASE", "REVOKE · TO ZERO"]);
    });
}

#[test]
fn near_moves_and_coins_sent_through_near_intents() {
    with_engine(|e| {
        let friend = entry("Friend", "near", &"a".repeat(64));
        let exchange = entry("Exchange", "xrp", XRP_ZERO);
        wallet(e, vec![("solana", vec![usdc("50")]), ("near", vec![row("NEAR", "3", Some("7.5"), None)])]);
        e.st_mut().book = vec![friend, exchange.clone()];
        let buy = WalletCommand::BuyNear { usdc: Some(Decimal::from(5)) };
        assert_eq!(labels(e, &buy), ["FROM · SOLANA", "SWAP · NEAR INTENTS", "TO · NEAR"]);
        assert_eq!(e.card_route(&buy).unwrap().last().unwrap().title, "≈ 2 NEAR");
        assert_eq!(labels(e, &WalletCommand::SellNear { amount: Some(Decimal::ONE), to: None }), ["FROM · NEAR", "SWAP · NEAR INTENTS", "ARRIVES · ≈ 30–40 S", "TO · SOLANA"]);
        assert_eq!(labels(e, &WalletCommand::SendNear { name: "Friend".into(), amount: Some(d("0.5")) }), ["FROM · NEAR", "TO · SAVED ADDRESS"]);
        assert_eq!(labels(e, &WalletCommand::CloseNear { name: "Friend".into() }), ["FROM · NEAR", "TO · SAVED ADDRESS"]);

        // XRP to an exchange's address with no destination tag: the card says the money would be lost.
        let xrp = WalletCommand::SendToken { name: "Exchange".into(), token: "XRP".into(), amount: Some(Decimal::from(20)), network: None };
        let r = (exchange.clone(), "XRP".to_string(), XRP_ZERO.to_string());
        let across = |e: &Engine| e.across_steps(&xrp, &r, Some(Decimal::from(20)));
        assert_eq!(across(e).iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["FROM · SOLANA", "SWAP · NEAR INTENTS", "TO · SAVED ADDRESS"]);
        assert!(e.xrp_tag_warning(&exchange).unwrap().starts_with("An exchange’s XRP address needs its destination tag"));
        quoted(e, &xrp, &[("Arbitrum", "≈ 41.2 USDC for $40.00 of it · ≈ 30 s", "")]);
        let first = across(e).remove(0);
        assert_eq!((first.label.as_str(), first.lines), ("FROM · ARBITRUM", vec!["≈ 41.2 USDC for $40.00 of it · ≈ 30 s · the rest comes back".to_string()]));
        // With its tag saved as the memo: nothing to warn of.
        assert_eq!(e.xrp_tag_warning(&SignerAddress { memo: "12345".into(), ..exchange }), None);
    });
}

#[test]
fn a_payment_a_multisig_and_the_bridges_have_their_steps() {
    with_engine(|e| {
        wallet(e, vec![("solana", vec![usdc("50")]), ("tempo", vec![usdc("0")])]);
        let pay = payment(e);
        assert_eq!(labels(e, &pay), ["FROM · SOLANA → TEMPO", "PAY · MPP", "TO · EXAMPLE"]);
        wallet(e, vec![("tempo", vec![usdc("5")])]);
        assert_eq!(labels(e, &pay), ["FROM · TEMPO", "PAY · MPP", "TO · EXAMPLE"]);
        e.st_mut().payment = None;

        e.st_mut().multisig_draft = Some(MultisigDraft { keys: keys(), threshold: 2, networks: vec!["base".into(), "arbitrum".into()], salt: 1, address: None });
        let create = WalletCommand::MultisigCreate { threshold: Some(2), of: Some(2), keys: vec![], networks: vec!["base".into(), "arbitrum".into()] };
        assert_eq!(labels(e, &create), ["CHECK · KEYS", "MULTISIG · BASE", "MULTISIG · ARBITRUM"]);
        e.st_mut().multisig_draft = None;
        let send = multisig_send(e, "0");
        assert_eq!(labels(e, &send), ["SIGN · THIS COMPUTER", "SIGN · LEDGER", "TO · MOM"]);
        e.st_mut().multisig_payment = None;

        wallet(e, vec![("base", vec![usdc("20")]), ("arbitrum", vec![row("USDT", "10", Some("10"), None)])]);
        let cctp = WalletCommand::Cctp { from: "base".into(), to: "arbitrum".into(), amount: Some(Decimal::from(5)) };
        assert_eq!(labels(e, &cctp), ["FROM · BASE", "BURN & MINT · CIRCLE'S CCTP", "TO · ARBITRUM"]);
        let usdt = |layer_zero| WalletCommand::Usdt { from: "arbitrum".into(), to: "polygon".into(), amount: Some(Decimal::from(5)), layer_zero };
        assert_eq!(labels(e, &usdt(false)), ["FROM · ARBITRUM", "BRIDGE · RELAY", "TO · POLYGON"]);
        let lz = usdt(true);
        assert_eq!(labels(e, &lz), ["FROM · ARBITRUM", "BRIDGE · LAYERZERO", "TO · POLYGON"]);
        quoted(e, &lz, &[("layerzero", "4.99 USDT · $0.01 · ≈ 2 min", "$4.98")]);
        assert_eq!(step(e, &lz, "BRIDGE · LAYERZERO").unwrap().title, "4.99 USDT");
        assert_eq!(e.card_route(&lz).unwrap().last().unwrap().title, "4.99 USDT");
        // A bridge between EVM networks names the one it goes by once quoted, with each bridge's cost and time.
        let ways = [way("cctp", "4.99 USDC · $0.01 · ≈ 1 min", "$4.99"), way("relay", "4.97 USDC · $0.03 · ≈ 4 s", "$4.97")];
        let bridged = e.bridge_step(Some(&ways), "5 USDC", ("BRIDGE · RELAY", "Base → Arbitrum · no stop on Solana"));
        assert_eq!((bridged.label.as_str(), bridged.title.as_str(), bridged.lines[0].as_str()), ("BRIDGE · CCTP", "4.99 USDC", "CCTP\t$0.01 · ≈ 1 min"));
        assert_eq!(bridged.lines.len(), 3);
        assert_eq!(e.bridge_step(None, "5 USDC", ("BRIDGE · RELAY", "Base → Arbitrum")).lines, ["Base → Arbitrum"]);
    });
}

#[test]
fn aave_debt_shows_the_health_factor_and_the_liquidation_price() {
    with_engine(|e| {
        wallet(e, vec![("base", vec![usdc("20"), row("ETH", "0.01", Some("30"), Some("native"))])]);
        let borrow = WalletCommand::AaveBorrow { amount: Some(Decimal::from(5)), network: "base".into() };
        assert_eq!(labels(e, &borrow), ["COLLATERAL · AAVE", "BORROW · AAVE", "TO · BASE"]);
        let ways = [("health", "∞ → 2.1", ""), ("liquidation", "ETH ≤ $1,500", ""), ("collateral", "$20.00", "")];
        quoted(e, &borrow, &ways);
        assert_eq!(step(e, &borrow, "BORROW · AAVE").unwrap().lines, ["health factor ∞ → 2.1"]);
        assert_eq!(step(e, &borrow, "COLLATERAL · AAVE").unwrap().title, "$20.00");
        assert_eq!(labels(e, &WalletCommand::AaveRepay { amount: Some(Decimal::from(5)), network: "base".into() }), ["FROM · BASE", "APPROVE · EXACT", "REPAY · AAVE"]);
        assert_eq!(labels(e, &WalletCommand::AaveCollateral { amount: Some(d("0.005")), network: "base".into() }), ["FROM · BASE", "WRAP · WETH", "COLLATERAL · AAVE"]);

        // The liquidation price is said on the card, and a new quote replaces it.
        let id = waiting(e, Pending::Wallet(borrow.clone()), &borrow.summary(), &[]);
        e.liquidation_caution(&id, &ways.map(|(w, g, worth)| way(w, g, worth)));
        e.liquidation_caution(&id, &[way("liquidation", "ETH ≤ $1,400", "")]);
        let said: Vec<String> = e.st().card.as_ref().unwrap().warnings.iter().filter(|w| w.starts_with("Liquidation if")).cloned().collect();
        assert_eq!(said, ["Liquidation if ETH ≤ $1,400: Aave sells your collateral at a penalty. Keep an eye on it, or borrow less."]);
        assert_eq!(board(e, &id).lines.iter().filter(|l| l.contains("Liquidation if")).count(), 1);
    });
}

#[test]
fn rewards_a_transfer_and_a_close_are_drawn_step_by_step() {
    with_engine(|e| {
        wallet(e, vec![("base", vec![usdc("2")]), ("solana", vec![usdc("50")])]);
        let reward = Reward { chain_id: 8453, token: AERO.into(), symbol: "AERO".into(), decimals: 18, amount: d("2000000000000000000"), claimed: Decimal::ZERO,
                              proofs: vec![], root: String::new(), price: Some(d("1.5")) };
        let claim = |c: &WalletCommand| e.claim_steps(std::slice::from_ref(&reward), c).into_iter().map(|s| s.label).collect::<Vec<_>>();
        assert_eq!(claim(&WalletCommand::ClaimRewards), ["CLAIM · MERKL ON BASE", "TO · BASE"]);
        assert_eq!(claim(&WalletCommand::SweepRewards { min_usd: None, into: None }), ["CLAIM · MERKL ON BASE", "TO · BASE", "SELL · ON BASE"]);
        assert_eq!(e.claim_steps(std::slice::from_ref(&reward), &WalletCommand::ClaimRewards)[0].lines, ["worth\t≈ $3.00", "from Merkl's distributor"]);

        let transfer = e.transfer_steps(&entry("Mom", "base", MOM), Decimal::from(5));
        assert_eq!(transfer.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["FROM · BASE", "TO · SAVED ADDRESS"]);
        // The Mac's FROM box: this network's balance and, under it, Solana's going down for what's short (its `bridged`).
        assert_eq!(transfer[0].lines, ["3 USDC short: it comes from Solana first", "Solana\t50 → ≈ 47 USDC"]);

        e.st_mut().positions = vec![Position { coin: "ETH".into(), size: d("0.5"), entry: Decimal::from(2000), unrealized_pnl: Decimal::from(25), liquidation: None }];
        let close = e.close_steps(Decimal::ONE, "ETH").unwrap();
        assert_eq!(close.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["CLOSE · ETH LONG", "TO · HYPERLIQUID"]);
        assert!(close[0].title.starts_with("+$25"), "{:?}", close[0]);
        assert_eq!(e.close_steps(Decimal::ONE, "BTC"), None);
    });
}

#[test]
fn tokens_by_contract_and_stock_tokens_say_what_they_are() {
    with_engine(|e| {
        wallet(e, vec![]);
        assert_eq!(e.move_cautions(&buy_on("base", AERO, "5")), [e.by_contract()]);
        assert_eq!(e.move_cautions(&swap_on("base", "USDC", AERO, "5")), [e.by_contract()]);
        assert_eq!(e.move_cautions(&WalletCommand::BuyStock { symbol: AERO.into(), usdc: Some(Decimal::from(5)) }), [e.by_contract()]);
        assert!(e.move_cautions(&WalletCommand::BuyStock { symbol: "TSLA".into(), usdc: Some(Decimal::from(5)) })[0]
            .starts_with("Robinhood Stock Tokens give economic exposure, not the shares"));
        // A ticker, a sale, a Solana swap: nothing to say.
        assert!(e.move_cautions(&buy_on("base", "AERO", "5")).is_empty());
        assert!(e.move_cautions(&WalletCommand::SellOn { place: "base".into(), token: AERO.into(), amount: Some(Decimal::ONE), keep: false }).is_empty());
        assert!(e.move_cautions(&swap_on("solana", "USDC", "JUP", "5")).is_empty());
    });
}

#[test]
fn kaminos_rent_offers_jupiter_lend_beside_send_anyway() {
    with_engine(|e| {
        let instead = e.rent_free_instead(&Pending::Wallet(WalletCommand::Kamino { deposit: true, symbol: "USDC".into(), amount: Some(Decimal::from(5)) }));
        assert_eq!(instead.iter().map(|o| o.title.as_str()).collect::<Vec<_>>(), ["Jupiter Lend instead · no rent"]);
        assert_eq!(WalletCommand::parse(&instead[0].command), Some(WalletCommand::LendDeposit("USDC".into(), Some(Decimal::from(5)))));
        assert!(e.rent_free_instead(&Pending::Wallet(WalletCommand::LendDeposit("USDC".into(), Some(Decimal::from(5))))).is_empty());
    });
}

#[test]
fn the_routes_speak_russian() {
    with_engine(|e| {
        wallet(e, vec![("base", vec![usdc("20")])]);
        e.set_settings(|s| s.language = Some("ru".into()));
        let cctp = WalletCommand::Cctp { from: "base".into(), to: "arbitrum".into(), amount: Some(Decimal::from(5)) };
        assert_eq!(labels(e, &cctp), ["ОТКУДА · BASE", "СЖЕЧЬ И ВЫПУСТИТЬ · CCTP CIRCLE", "КУДА · ARBITRUM"]);
        assert!(e.move_cautions(&buy_on("base", AERO, "5"))[0].starts_with("Токен по контракту"));
    });
}

#[test]
fn a_quotes_amount_is_read_from_our_own_wording() {
    assert_eq!(Engine::quoted_amount(Some("≈ 2.41 AERO")), Some(d("2.41")));
    assert_eq!(Engine::quoted_amount(Some("1,234.5 USDC · 0.3% impact")), Some(d("1234.5")));
    assert_eq!(Engine::quoted_amount(None), None);
    assert_eq!(Engine::quoted_amount(Some("quoting…")), None);
    assert_eq!((Engine::bridge_name("1click"), Engine::bridge_name("other")), ("NEAR Intents".to_string(), "other".to_string()));
    assert_eq!((Engine::way_name("relay"), Engine::way_name("uniswap")), ("Relay swap".to_string(), "Relay + Uniswap".to_string()));
    assert!(Engine::genuine_contracts().contains(&BASE_USDC.to_lowercase()) && !Engine::genuine_contracts().contains(&FAKE.to_lowercase()));
}

// MARK: Cards13Tests: each route's footer

#[test]
fn each_route_has_the_macs_footer() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        // A buy with the USDC already on Base: Uniswap there, not Relay's ways from Solana.
        wallet(e, vec![("base", vec![usdc("20")]), ("solana", vec![usdc("50")])]);
        assert_eq!(e.route_note(&buy_on("base", "AERO", "5")).unwrap(), "Two Base transactions: an exact approve and the Uniswap swap, which may only pay this wallet. Press Confirm.");
        // A stock by its contract, with the USDG already there.
        wallet(e, vec![("robinhood", vec![row("USDG", "20", Some("20"), None)])]);
        let stock = |s: &str, u: Option<Decimal>| e.route_note(&WalletCommand::BuyStock { symbol: s.into(), usdc: u }).unwrap();
        let by_contract = stock(FAKE, Some(Decimal::TEN));
        assert!(by_contract.starts_with("Two signatures on Robinhood Chain") && by_contract.contains("taken by its contract"), "{by_contract}");
        assert!(stock("TSLA", Some(Decimal::TEN)).contains("Robinhood's registered contract"));
        assert!(!stock(FAKE, None).contains("registered"));
        // Into NEAR: the 1Click deposit, never «Relay's solver».
        wallet(e, vec![("base", vec![usdc("20")])]);
        let near = e.route_note(&cross("USDC", "base", "NEAR", "near", "5")).unwrap();
        assert!(near.contains("1Click") && !near.contains("Relay"), "{near}");
        let relay = e.route_note(&cross("USDC", "base", "SOL", "solana", "5")).unwrap();
        assert!(relay.contains("Relay's solver") && relay.contains("exact approve to Relay"), "{relay}");
        // Tempo's own exchange until Relay's swap is the quote taken.
        wallet(e, vec![("tempo", vec![usdc("5")])]);
        let tempo = swap_on("tempo", "USDC", "pathUSD", "1");
        assert!(e.route_note(&tempo).unwrap().contains("Tempo's own exchange"));
        quoted(e, &tempo, &[("Relay's swap", "0.99 USDT0", "")]);
        assert!(e.route_note(&tempo).unwrap().contains("Relay's swap on Tempo"));
        // A Russian footer for a Russian chat.
        wallet(e, vec![("base", vec![usdc("20")])]);
        e.set_settings(|s| s.language = Some("ru".into()));
        assert!(e.route_note(&buy_on("base", "AERO", "5")).unwrap().starts_with("Две транзакции в Base"));
    });
}

#[test]
fn a_paid_service_names_the_payee_and_where_the_money_comes_from() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        wallet(e, vec![("solana", vec![usdc("50")]), ("tempo", vec![usdc("0")])]);
        let pay = payment(e);
        let (start, end) = address_check::ends(PHONE);
        let short = e.route_note(&pay).unwrap();
        assert!(short.contains(&format!("svc.example named ({start} … {end})")) && short.contains("from Solana over Relay"), "{short}");
        wallet(e, vec![("tempo", vec![usdc("5")])]);
        assert!(!e.route_note(&pay).unwrap().contains("from Solana"));
    });
}

#[test]
fn a_multisig_send_shows_its_balance_before_and_after() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        wallet(e, vec![("base", vec![usdc("20")])]);
        let send = multisig_send(e, "12.3412");
        let money = e.card_route(&send).unwrap().pop().unwrap().money.unwrap();
        assert_eq!((money.place.as_str(), money.was.as_str(), money.after.as_str()), ("Multisig", "12.34", "7.34"));
        assert!(e.route_note(&send).unwrap().contains("money only to Mom"));
    });
}

#[test]
fn a_saved_address_says_when_its_your_own_wallet_and_ethereums_gas() {
    with_engine(|e| {
        wallet(e, vec![]);
        let main = SignerWallet { id: "1".into(), label: "Main".into(), kind: "phrase".into(), solana: None, evm: Some(ME.into()), near: None, test: None, automation: None };
        e.st_mut().status = Some(SignerReply { ok: true, wallets: Some(vec![main]), ..Default::default() });
        assert_eq!(e.transfer_note(&entry("Main", "base", ME), Decimal::from(5)), "To your own wallet «Main», one transfer on Base. Press Confirm.");
        let eth = e.transfer_note(&entry("Mom", "ethereum", MOM), Decimal::from(5));
        assert!(eth.starts_with("Only this saved address may receive") && eth.contains("the fee is in ETH"), "{eth}");
    });
}

#[test]
fn an_address_to_save_says_where_it_came_from_and_saves() {
    with_engine(|e| {
        wallet(e, vec![]);
        let pending = Pending::Address(entry("Mom", "base", MOM));
        let id = waiting(e, pending.clone(), "Save “Mom” to Addresses", &[]);
        e.note_route(&id, &pending);
        let note = board(e, &id).note.unwrap();
        assert!(note.starts_with("Taken from your own message and checked for Base"), "{note}");
        assert_eq!(e.st().card.as_ref().unwrap().confirm, "Save");
        // Resolved from a domain just now: where from.
        e.st_mut().saving_extras.insert(Engine::extra_key("Mom", "base"), crate::blocks::AddressExtra { note: Some("mom.base.eth — ENS".into()), ..Default::default() });
        assert!(e.address_note(&entry("Mom", "base", MOM)).starts_with("Resolved from mom.base.eth on Base just now"));
    });
}

#[test]
fn a_lookalike_ticker_gets_its_warning() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        wallet(e, vec![]);
        e.st_mut().contract_names.known.extend([(FAKE.to_lowercase(), "USDC".to_string()), (BASE_USDC.to_lowercase(), "USDC".to_string())]);
        assert_eq!(e.token_name(FAKE, "base"), "⚠ USDC (0x3C44…93BC)");
        assert_eq!(e.token_name(BASE_USDC, "base"), "USDC (0x8335…2913)");
        assert_eq!(e.token_name("aero", "base"), "AERO");
        assert_eq!(e.token_name("usdt", "arbitrum"), "USDT");
        assert_eq!(e.token_symbol(FAKE, "base").as_deref(), Some("USDC"));
        // Not known yet: shortened, and asked once (marked asked here, so nothing goes out).
        assert_eq!(e.token_name(AERO, "base"), "0x9401…8631");
        // The card's title names it so too, never the raw contract.
        wallet(e, vec![("base", vec![usdc("20")])]);
        let buy = buy_on("base", FAKE, "5");
        let id = waiting(e, Pending::Wallet(buy.clone()), &buy.summary(), &[]);
        e.note_route(&id, &Pending::Wallet(buy));
        let title = board(e, &id).title;
        assert!(title.contains("⚠ USDC (0x3C44…93BC)") && !title.contains(FAKE), "{title}");
        assert_eq!(e.st().card.as_ref().unwrap().title, title);
    });
}

#[test]
fn a_chains_footer_is_its_first_moves_then_its_own_words() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        wallet(e, vec![("solana", vec![usdc("50")]), ("base", vec![usdc("0")])]);
        let first = WalletCommand::SolanaTo("base".into(), Some(Decimal::TEN));
        let pending = Pending::Chain(vec![ChainStep::Wallet(first), ChainStep::Wallet(buy_on("base", "AERO", "5"))]);
        let order = "They run in order. The first that fails stops the rest. Windows Hello asks for each move that signs.";
        let id = waiting(e, pending.clone(), "2 steps, one Confirm", &["1. move 10 USDC to Base", "2. buy AERO on Base for 5 USDC", order]);
        e.note_route(&id, &pending);
        let note = board(e, &id).note.unwrap();
        assert!(note.ends_with(order) && !note.contains("1. move") && note.starts_with("One Solana signature for the Relay bridge"), "{note}");
    });
}

#[test]
fn the_one_transaction_switch_is_said_under_an_approve() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        wallet(e, vec![("base", vec![usdc("20")])]);
        // Settings › Wallet's switch is process-wide, and any engine made meanwhile mirrors its own settings over it: asked
        // again until a read lands with it on.
        let mut said = (String::new(), String::new());
        for _ in 0..1000 {
            trade::BATCHES_ON.store(true, Ordering::Relaxed);
            let note = e.route_note(&buy_on("base", "AERO", "5")).unwrap();
            let none = e.route_note(&WalletCommand::MultisigCreate { threshold: Some(2), of: Some(2), keys: vec![], networks: vec!["base".into()] }).unwrap();
            let on = Executor::batches_on();
            trade::BATCHES_ON.store(false, Ordering::Relaxed);
            if on {
                said = (note, none);
                break;
            }
        }
        assert!(said.0.ends_with("go out as one transaction under one Windows Hello."), "{}", said.0);
        // No approve, no such line.
        assert!(!said.1.contains("as one transaction"), "{}", said.1);
    });
}

// MARK: Cards14Tests

#[test]
fn a_deposit_with_no_gas_there_says_its_bridge_from_solana() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        // The USDC on Arbitrum, no ETH: ensure_evm bridges half a dollar and gas from Solana first, and the footer says so.
        wallet(e, vec![("solana", vec![usdc("50")]), ("arbitrum", vec![usdc("20")])]);
        let lend = WalletCommand::Aave { deposit: true, amount: Some(Decimal::from(5)), network: "arbitrum".into() };
        assert!(e.route_note(&lend).unwrap().starts_with("What's missing on Arbitrum comes from Solana over Relay, with ETH for gas"));
        // With the gas there too: the money is already there.
        wallet(e, vec![("solana", vec![usdc("50")]), ("arbitrum", vec![usdc("20"), row("ETH", "0.01", None, Some("native"))])]);
        assert!(e.route_note(&lend).unwrap().starts_with("Two Arbitrum transactions"));
        // Base, as the Mac's aaveRoute: its own footer.
        wallet(e, vec![("solana", vec![usdc("50")]), ("base", vec![usdc("20")])]);
        let morpho = WalletCommand::Morpho { deposit: true, amount: Some(Decimal::from(5)), network: "base".into() };
        assert!(e.route_note(&morpho).unwrap().starts_with("One Solana signature for the bridge, then two on Base"));
        wallet(e, vec![("base", vec![usdc("20"), row("ETH", "0.001", None, Some("native"))])]);
        assert!(e.route_note(&morpho).unwrap().starts_with("Two on Base"));
        // Ethereum says its gas.
        wallet(e, vec![("ethereum", vec![usdc("20"), row("ETH", "0.01", None, Some("native"))])]);
        let eth = e.route_note(&WalletCommand::Aave { deposit: true, amount: Some(Decimal::from(5)), network: "ethereum".into() }).unwrap();
        assert!(eth.starts_with("Two Ethereum transactions") && eth.contains("«how much is gas?»"), "{eth}");
    });
}

#[test]
fn a_paid_request_names_the_network_its_dollars_come_from() {
    let _turn = NOTES.lock().unwrap_or_else(|e| e.into_inner());
    with_engine(|e| {
        // Solana can't pay and Base can: ensure_evm sends it from Base, a dollar over for Relay.
        wallet(e, vec![("solana", vec![usdc("0")]), ("base", vec![usdc("20")]), ("tempo", vec![usdc("0")])]);
        let pay = payment(e);
        let first = e.card_route(&pay).unwrap().remove(0);
        assert_eq!((first.label.as_str(), first.lines.last().unwrap().as_str()), ("FROM · BASE → TEMPO", "20 USDC on Base"));
        assert_eq!(first.money.unwrap().place, "Base");
        assert!(e.route_note(&pay).unwrap().contains("What's short comes from Base over Relay first"));
        // Solana holds enough: it pays, as before.
        wallet(e, vec![("solana", vec![usdc("50")]), ("base", vec![usdc("20")]), ("tempo", vec![usdc("0")])]);
        assert_eq!(e.card_route(&pay).unwrap()[0].label, "FROM · SOLANA → TEMPO");
        assert!(e.route_note(&pay).unwrap().contains("from Solana over Relay"));
        assert_eq!(e.tempo_need(d("0.01")), Some(d("1.01")));
    });
}
