//! Chains as the Mac runs them (ChainsTests, and the parts of Holds10Tests, Engine11Tests, Sends9Tests and Refusals8Tests
//! that are this file's): what one Confirm carries out, a card a chain stops at holds the steps after it, a move without
//! its amount asks «How much?» and holds the rest, a step after a move spends what arrived, a send no balance covers is
//! said before any card.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::foundation::decimal;
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::position_command::{ContractCall, PositionCommand, SpotAsset, WalletCommand};
use rust_decimal::Decimal;
use signer_api::SignerAddress;

use super::{AfterCall, ChainKind, Held};
use crate::canvas::Board;
use crate::engine::{Card, ChainStep, Engine, Pending};
use crate::from_core::MppChallenge;
use crate::outbox::Outbox;
use crate::payments::PaymentQuote;
use crate::portfolio::{Network, PortfolioView, Row};
use crate::signer_link::SignerLink;
use crate::store;

// Public test data only: Hardhat's accounts #3 and #1.
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
const PHONE: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";

fn d(s: &str) -> Decimal { decimal(s).unwrap() }

fn mom() -> SignerAddress { SignerAddress { name: "Mom".into(), network: "base".into(), address: MOM.into(), memo: String::new() } }

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("no window in these tests"));
    {
        let mut st = e.st_mut();
        st.settings.language = Some("en".into());
        // A push goes out as the lock alone, never through the draw this test has none of.
        st.lock.locked = true;
        for coin in ["SOL", "ETH", "BTC"] { st.markets.insert(coin.into(), Market::new(coin, Decimal::from(100), 2, 20)); }
    }
    e
}

async fn local(test: impl std::future::Future<Output = ()>) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(test).await;
}

/// A card waiting for Confirm, on the canvas.
fn waiting(e: &Engine, pending: Pending, title: &str) -> String {
    let board = Board::new("move", title);
    let id = board.id.clone();
    e.set_boards(|b| b.push(board));
    let mut st = e.st_mut();
    st.pending = Some(pending);
    st.card = Some(Card { id: id.clone(), title: title.into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![], over_limit: false });
    id
}

fn last_reply(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

#[test]
fn the_next_step_is_fitted_to_what_arrived() {
    let lend = WalletCommand::LendDeposit("USDC".into(), Some(Decimal::ONE_HUNDRED));
    let hundred = Decimal::ONE_HUNDRED;
    // Cents short after a bridge: the step spends what arrived, rounded down to the cent.
    assert_eq!(Engine::fitted(&lend, hundred, d("99.437"), true), Some(WalletCommand::LendDeposit("USDC".into(), Some(d("99.43")))));
    // More than 2 % (and a dollar) short is left for the step to refuse; enough is left as asked.
    assert_eq!(Engine::fitted(&lend, hundred, Decimal::from(97), true), None);
    assert_eq!(Engine::fitted(&lend, hundred, hundred, true), None);
    assert_eq!(Engine::fitted(&lend, hundred, Decimal::ZERO, true), None);
    // A small move gets a dollar of slack.
    let mv = WalletCommand::SolanaTo("arbitrum".into(), Some(Decimal::from(5)));
    assert_eq!(Engine::fitted(&mv, Decimal::from(5), d("4.2"), true), Some(WalletCommand::SolanaTo("arbitrum".into(), Some(d("4.2")))));
    // But never more than a quarter of the amount: «withdraw 1 then deposit 1» read the balance before the withdrawal
    // showed and deposited the 0.1 that was there (the Mac, 2026-11-10).
    let one = WalletCommand::LendDeposit("USDC".into(), Some(Decimal::ONE));
    assert_eq!(Engine::fitted(&one, Decimal::ONE, d("0.107186"), true), None);
    assert_eq!(Engine::fitted(&WalletCommand::LendDeposit("USDC".into(), Some(Decimal::TWO)), Decimal::TWO, d("1.1"), true), None);
    assert_eq!(Engine::fitted(&one, Decimal::ONE, d("0.8"), true), Some(WalletCommand::LendDeposit("USDC".into(), Some(d("0.8")))));
    // What comes out of a vault lands on Solana like a bridge's money: the step after it waits for the balance.
    assert_eq!(Engine::arrives(&WalletCommand::LendWithdraw("USDC".into(), Some(Decimal::ONE))).as_deref(), Some("solana"));
    assert_eq!(Engine::arrives(&WalletCommand::Kamino { deposit: false, symbol: "USDC".into(), amount: None }).as_deref(), Some("solana"));
    assert_eq!(Engine::arrives(&WalletCommand::LendWithdraw("SOL".into(), None)), None);
    assert_eq!(Engine::arrives(&one), None);
    // The wait for what a move brought: over with the step's amount there, and for a step that names none, or asks for
    // more than the move brought, once the finalized balance equals a confirmed one that stopped moving.
    let shown = |need: Option<&str>, finalized: &str, confirmed: Option<&str>, steady: u32, entry: &str| Engine::deposit_shown(need.map(d), d(finalized), confirmed.map(d), steady, d(entry));
    assert!(!shown(Some("1"), "0.107", Some("1.107"), 5, "0.107") && shown(Some("1"), "1.107", Some("1.107"), 0, "0.107") && shown(Some("1"), "1", None, 0, "0.107"));
    assert!(!shown(None, "0.107", Some("1.107"), 9, "0.107") && !shown(None, "1.107", Some("1.107"), 2, "0.107") && shown(None, "1.107", Some("1.107"), 3, "0.107"));
    // Nothing seen coming (it had landed already, or a node behind still answers the old balance): eight reads, not three.
    assert!(!shown(None, "0.107", Some("0.107"), 7, "0.107") && shown(None, "0.107", Some("0.107"), 8, "0.107"));
    assert!(!shown(Some("50"), "5", Some("6"), 9, "5") && shown(Some("50"), "6", Some("6"), 3, "5") && !shown(Some("50"), "6", None, 9, "5"));
    // Money an earlier step spent is confirmed before it is final: 20 final, 15 counted is not «caught up».
    assert!(!shown(None, "20", Some("15"), 9, "20") && shown(None, "15", Some("15"), 8, "20"));
    // Which steps wait: one that spends Solana's USDC, and a spread of the free USDC wherever it is; never another coin's deposit.
    assert!(Engine::spends_on_solana(&WalletCommand::LendDeposit("USDC".into(), None)) && Engine::spends_on_solana(&WalletCommand::SolanaTo("base".into(), None)));
    assert!(!Engine::spends_on_solana(&WalletCommand::LendDeposit("SOL".into(), Some(Decimal::TWO))) && !Engine::spends_on_solana(&WalletCommand::LendWithdraw("USDC".into(), Some(Decimal::ONE))));
    // A token a swap gave: within 2 % only.
    let send = |x: Decimal| WalletCommand::SendToken { name: "Bybit".into(), token: "USDT".into(), amount: Some(x), network: Some("solana".into()) };
    assert_eq!(Engine::fitted(&send(Decimal::from(750)), Decimal::from(750), d("749.8"), false), Some(send(d("749.8"))));
    assert_eq!(Engine::fitted(&send(Decimal::TEN), Decimal::TEN, d("9.5"), false), None);
    // Where a step's money lands and comes from: what each step waits for.
    assert_eq!(Engine::arrives(&WalletCommand::BaseToSolana(Some(Decimal::from(5)))).as_deref(), Some("solana"));
    assert_eq!(Engine::spends(&WalletCommand::LendDeposit("USDC".into(), Some(Decimal::from(5)))).as_deref(), Some("solana"));
    assert_eq!(Engine::source(&WalletCommand::Bridge { from: "base".into(), to: "arbitrum".into(), amount: Some(Decimal::from(5)) }).as_deref(), Some("base"));
    assert_eq!(Engine::solana_usdc(&WalletCommand::Kamino { deposit: true, symbol: "USDC".into(), amount: Some(Decimal::from(5)) }), Some(Decimal::from(5)));
    // Hyperliquid's own trading balance isn't a network a later step spends on.
    assert_eq!(Engine::arrives(&WalletCommand::Bridge { from: "base".into(), to: "hyperliquid".into(), amount: None }), None);
}

/// R8-1, T11 and G27: a confirmed run carries out only what the Mac's runSteps would — a move, a transfer, a position, an
/// address, a recurring buy's first buy — and leaves a contract call, a multisig and a trade plan waiting on their own cards.
#[test]
fn a_confirm_carries_out_only_what_the_macs_run_steps_would() {
    let call = WalletCommand::parse(&format!("call base {MOM} claim()")).expect("a contract call");
    assert!(matches!(call, WalletCommand::ContractCall(_)));
    assert!(!Engine::carried_on_chain(Some(&Pending::Wallet(call))));
    let usdc_buy = WalletCommand::Buy { asset: SpotAsset::usdc(), usdc: Some(Decimal::from(5)) };
    assert!(Engine::carried_on_chain(Some(&Pending::Recurring { command: usdc_buy, days: 7 })));
    assert!(!Engine::carried_on_chain(None));
    assert!(Engine::carried_on_chain(Some(&Pending::Wallet(WalletCommand::SendSol { name: "Mom".into(), amount: Decimal::ONE }))));
    assert!(Engine::carried_on_chain(Some(&Pending::Transfer(mom(), Decimal::from(5)))));
    assert!(Engine::carried_on_chain(Some(&Pending::Position { command: PositionCommand::Close(Decimal::ONE), coin: "ETH".into() })));
    assert!(Engine::carried_on_chain(Some(&Pending::Address(mom()))));
    let make = WalletCommand::parse("make a multisig 2 of 3").expect("a multisig");
    assert!(matches!(make, WalletCommand::MultisigCreate { .. }));
    assert!(!Engine::carried_on_chain(Some(&Pending::Wallet(make))));
    let pay_out = WalletCommand::MultisigSend { token: "USDC".into(), amount: Some(Decimal::TEN), to: MOM.into(), network: Some("base".into()) };
    assert!(!Engine::carried_on_chain(Some(&Pending::Wallet(pay_out))));
    assert!(!Engine::carried_on_chain(Some(&Pending::Chain(vec![]))));
    // A paid request's card shows a price no one has seen yet: it waits for its own Confirm (the Mac's paysUnseen).
    let pay = WalletCommand::Pay { method: "GET".into(), url: "https://api.example.com/answer".into(), body: None };
    assert!(!Engine::carried_on_chain(Some(&Pending::Wallet(pay))));
}

/// A paid request after a chain's first step isn't on its card's one Confirm: the card names it by its words, nothing is
/// asked or paid now, and once reached it shows its price on its own card (the Mac's paysUnseen).
#[tokio::test]
async fn a_later_paid_request_waits_for_its_own_card() {
    local(async {
        let e = engine();
        e.st_mut().book = vec![mom()];
        let steps = ["send 5 USDC to Mom".to_string(), "pay GET https://api.example.com/answer".into()];
        assert!(e.chain(&steps, None, None, false).await);
        let pending = e.st().pending.clone();
        assert!(matches!(&pending, Some(Pending::Chain(s)) if *s == [ChainStep::Transfer(mom(), Decimal::from(5)), ChainStep::Said(steps[1].clone())]), "{pending:?}");
        assert!(e.st().payment.is_none() && !e.st().chain_state.stopped);
    }).await;
}

/// R8-2: a card a chain stops at holds the steps after it (its Confirm names them, and they run once it's through) — any
/// card, not only a contract call's; with no card nothing is held (Holds10Tests).
#[tokio::test]
async fn any_card_a_chain_stops_at_holds_the_steps_after_it() {
    local(async {
        let e = engine();
        e.hold_after_call(&["chart SOL".into()]);
        assert!(e.st().chain_state.after_call.is_none());
        let id = waiting(&e, Pending::Address(mom()), "Save Mom");
        e.hold_after_call(&["chart SOL".into(), "send 1 USDC to Mom".into()]);
        let held = e.st().chain_state.after_call.clone().unwrap();
        assert_eq!((held.board.as_str(), held.rest.as_slice()), (id.as_str(), ["chart SOL".to_string(), "send 1 USDC to Mom".into()].as_slice()));
        let then = "Then, on this Confirm, once it’s through: chart SOL → send 1 USDC to Mom";
        assert_eq!(e.st().card.as_ref().unwrap().lines.last().unwrap(), then);
        assert_eq!(e.st().boards().iter().find(|b| b.id == id).unwrap().lines.last().unwrap(), then);
        assert_eq!(last_reply(&e), "It waits on its card. Its Confirm covers the steps after it too — they run once it’s through.");
        // No steps after it: nothing held.
        e.st_mut().chain_state.after_call = None;
        e.hold_after_call(&[]);
        assert!(e.st().chain_state.after_call.is_none());
    }).await;
}

/// A contract call in a chain: its card names the steps after it, which ride on its Confirm; another card's Confirm never
/// runs them (Sends9Tests).
#[tokio::test]
async fn a_contract_call_holds_the_steps_after_it_on_its_card() {
    local(async {
        let e = engine();
        let call = ContractCall::parse(&format!("call arbitrum {MOM} deposit(uint256,address) 10e6 me approving 10 USDC")).expect("a call");
        let id = waiting(&e, Pending::Wallet(WalletCommand::ContractCall(call)), "call");
        e.hold_after_call(&["send 0.01 SOL to Mom".into()]);
        let held = e.st().chain_state.after_call.clone().unwrap();
        assert_eq!((held.board, held.rest), (id.clone(), vec!["send 0.01 SOL to Mom".to_string()]));
        assert!(e.st().card.as_ref().unwrap().lines.last().unwrap().contains("Then, on this Confirm"));
        assert_eq!(last_reply(&e), "The call waits on its card. Its Confirm covers the steps after it too — they run once it’s through.");
        // Another board's Confirm: held as it was, and nothing runs.
        e.continue_after_call(Some("another")).await;
        assert!(e.st().chain_state.after_call.is_some() && !e.st().chain_state.running_held && !e.st().thinking);
        // Its own, from another canvas: taken, the run ends at once (the steps run on the canvas that held them).
        e.st_mut().chain_state.after_call = Some(AfterCall { canvas: "elsewhere".into(), ..held_for(&id) });
        e.continue_after_call(Some(&id)).await;
        assert!(e.st().chain_state.after_call.is_none() && !e.st().chain_state.running_held && !e.st().thinking);
    }).await;
}

fn held_for(board: &str) -> AfterCall { AfterCall { board: board.into(), canvas: String::new(), rest: vec!["chart SOL".into()], since: chrono::Utc::now() } }

/// A chain's paid first step names its price (Refusals8Tests).
#[tokio::test]
async fn a_chains_paid_first_step_names_its_price() {
    local(async {
        let e = engine();
        let pay = WalletCommand::Pay { method: "GET".into(), url: "https://api.example.com/answer".into(), body: None };
        assert_eq!(e.chain_line(&ChainStep::Wallet(pay.clone())), pay.summary());
        let challenge = MppChallenge { params: HashMap::new(), amount: 10_000, payee: PHONE.into(), memo: None, expires: None, method: "tempo".into(), near: None };
        e.st_mut().payment = Some(PaymentQuote { command: pay.clone(), challenge, ask: "the answer".into(), service: "Example".into(), probed: false, task: None,
                                                 x402: None, receives: None });
        assert_eq!(e.chain_line(&ChainStep::Wallet(pay)), "pay 0.01 USDC to Example");
        assert_eq!(e.chain_line(&ChainStep::Transfer(mom(), Decimal::from(5))), "send 5 USDC to Mom");
        assert_eq!(e.chain_line(&ChainStep::Said("chart SOL".into())), "chart SOL");
    }).await;
}

/// What each step is: what shows or sets something runs at once, a move goes on the chain's card, a position or a
/// recurring buy has a card of its own; and what isn't a command stops the chain before anything runs.
#[tokio::test]
async fn each_step_is_read_as_run_canonical_reads_it() {
    local(async {
        let e = engine();
        let kind = |step: &str| e.chain_kind(step, WalletCommand::parse(step).as_ref());
        assert_eq!(kind("chart SOL"), ChainKind::Now);
        assert_eq!(kind("lock"), ChainKind::Now);
        assert_eq!(kind("show approvals"), ChainKind::Now);
        assert!(matches!(kind("buy SOL for 10 USDC"), ChainKind::Move(WalletCommand::Buy { .. })));
        assert_eq!(kind("close ETH"), ChainKind::Later);
        assert!(e.knows("buy SOL") && e.knows("chart SOL") && e.knows("close ETH"));
        assert!(!e.knows("make me a sandwich"));
        assert!(!e.chain(&["chart SOL".into(), "make me a sandwich".into()], None, None, false).await);
        // A step by its words on the card.
        assert_eq!(e.said_step("close ETH").label, "HYPERLIQUID");
        assert_eq!(e.said_step("buy SOL").lines, ["how much\tasked when it's reached"]);
        let chart = e.said_step("chart SOL");
        assert!(chart.label == "THEN" && chart.title == "chart SOL" && chart.lines.is_empty());
    }).await;
}

/// The first move without its amount asks «How much?» and holds the steps after it: nothing runs before the answer
/// (ChainsTests.aStepWithoutAnAmountAsksAndHoldsTheRest, up to the answer).
#[tokio::test]
async fn a_step_without_an_amount_asks_and_holds_the_rest() {
    local(async {
        let e = engine();
        assert!(e.chain(&["buy SOL".into(), "chart SOL".into()], None, None, false).await);
        assert!(last_reply(&e).starts_with("For how many USDC should I buy SOL?"), "{}", last_reply(&e));
        let held = e.st().chain_state.held.clone().unwrap();
        assert!(matches!(&held, Held { command: WalletCommand::Buy { usdc: None, .. }, rest } if rest == &["chart SOL".to_string()]));
        assert_eq!(e.st().awaiting_amount.as_ref(), Some(&held.command));
        assert!(e.st().card.is_none() && e.st().boards().is_empty());
        // Declined, it's left to the plain «How much?»; another question waiting lets the held steps go.
        assert!(!e.answered_chain_amount("no", true).await);
        assert!(e.st().chain_state.held.is_some());
        e.st_mut().awaiting_amount = Some(WalletCommand::Stake(None));
        assert!(!e.answered_chain_amount("10", false).await);
        assert!(e.st().chain_state.held.is_none());
    }).await;
}

/// A send no balance can cover is said before any card: its network's USDC and what Solana and the other EVM networks
/// can add (ChainsTests.aSendToASavedNameIsRefusedBeforeItsCardWhenShort, by its parts).
#[tokio::test]
async fn a_send_no_balance_covers_is_said_before_its_card() {
    local(async {
        let e = engine();
        let usdc = |x: &str| Row { usd: Some(d(x)), ..Row::new("USDC", x, d(x)) };
        let net = |key: &str, x: &str| Network { key: key.into(), name: WalletCommand::place_name(key), usd: Decimal::ZERO, rows: vec![usdc(x)], note: None, hidden: vec![], hidden_more: 0 };
        e.st_mut().portfolio = PortfolioView { networks: vec![net("base", "3"), net("solana", "2")], updated_at: Some(chrono::Utc::now()), ..PortfolioView::default() };
        assert_eq!(e.transfer_shortfall("base", Decimal::ONE_HUNDRED).as_deref(),
                   Some("All the USDC this send can draw on is 5, less than 100 — nothing was prepared. Say up to that, or top up first."));
        assert!(e.transfer_shortfall("base", Decimal::from(5)).is_none());
        // One on Solana draws on Solana only.
        assert!(e.transfer_shortfall("solana", Decimal::from(3)).unwrap().contains("is 2, less than 3"));
        // A read that failed leaves it to the move.
        e.st_mut().portfolio.errors = vec!["Base: no answer".into()];
        assert!(e.transfer_shortfall("base", Decimal::ONE_HUNDRED).is_none());
        e.st_mut().portfolio = PortfolioView::default();
        assert!(e.transfer_shortfall("base", Decimal::ONE_HUNDRED).is_none());
    }).await;
}
