//! A route step's details (Route3Tests, RouteEditTests, Cards16Tests): each move's footer in both languages, which moves
//! get their ways quoted, what of a waiting move can change, an amount that isn't one number refused with the Mac's
//! words, a picked way taken once, and «Why this way?» answered from the step itself. In-process, nothing sent.

use std::sync::Arc;

use super::*;
use crate::canvas::{Board, Money};
use crate::engine::Card;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

fn en(a: &str, _: &str) -> String { a.into() }
fn ru(_: &str, b: &str) -> String { b.into() }


fn with_engine(test: impl AsyncFnOnce(&Engine)) {
    let _dir = store::test_folder();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    tokio::task::LocalSet::new().block_on(&rt, async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the tests keep the window locked"));
        // Locked, a state push sends the lock alone: no whole Snapshot is drawn here.
        e.st_mut().lock.locked = true;
        e.set_settings(|s| s.language = Some("en".into()));
        test(&e).await;
    });
}

/// A move waiting for Confirm on its card, as show() leaves it.
fn waiting(e: &Engine, command: WalletCommand) -> String {
    let board = Board::new("move", &command.summary());
    let id = board.id.clone();
    e.set_boards(|b| b.insert(0, board));
    let mut st = e.st_mut();
    st.pending = Some(Pending::Wallet(command));
    st.card = Some(Card { id: id.clone(), title: "Buy".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] });
    id
}

#[test]
fn notes_say_what_is_signed_in_both_languages() {
    let has = |c: WalletCommand, say: fn(&str, &str) -> String, part: &str| note(&c, say).is_some_and(|n| n.contains(part));
    assert!(has(WalletCommand::WithdrawToSolana(Some(Decimal::from(5))), en, "the trading key can’t withdraw"));
    assert!(has(WalletCommand::BuyStock { symbol: "TSLA".into(), usdc: Some(Decimal::TWO) }, en, "Uniswap v4, v3 or Relay’s swap"));
    assert!(has(WalletCommand::SellOn { place: "base".into(), token: "AERO".into(), amount: Some(Decimal::ONE), keep: false }, en, "Uniswap’s token list"));
    assert!(has(WalletCommand::Bridge { from: "base".into(), to: "arbitrum".into(), amount: Some(Decimal::TWO) }, ru, "Платит только на твой же адрес в Arbitrum"));
    assert!(note(&WalletCommand::Stake(Some(Decimal::ONE)), ru).unwrap().starts_with("Одна подпись в Solana"));
    assert_eq!(note(&WalletCommand::Stake(Some(Decimal::ONE)), en).unwrap(),
               "One Solana signature, checked and simulated before it’s signed. Press Confirm. Cancel or “cancel” drops it.");
    // Base's way back is the EVM network's, by its name.
    assert_eq!(note(&WalletCommand::BaseToSolana(None), en).unwrap(),
               "Two Base transactions: an exact approve and Relay’s deposit — checked field by field before signing. Press Confirm.");
    assert!(has(WalletCommand::Usdt { from: "arbitrum".into(), to: "polygon".into(), amount: None, layer_zero: true }, en, "minted on Polygon later"));
    assert!(has(WalletCommand::Gas { place: "robinhood".into(), usd: None }, en, "$1.00 of the gas coin"));
    // A payment's and a call's cards say it themselves.
    assert_eq!(note(&WalletCommand::Pay { method: "GET".into(), url: "https://example.com".into(), body: None }, en), None);
}

#[test]
fn trades_on_evm_networks_and_robinhood_are_quoted() {
    assert!(quoted(&WalletCommand::BuyStock { symbol: "TSLA".into(), usdc: Some(Decimal::TWO) }) && quoted(&WalletCommand::SellStock { symbol: "TSLA".into(), amount: None }));
    assert!(quoted(&WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: Some(Decimal::ONE), from: None }));
    assert!(!quoted(&WalletCommand::Stake(Some(Decimal::ONE))) && !quoted(&WalletCommand::SendTo { name: "mom".into(), amount: Decimal::ONE, network: None }));
    assert_eq!((stock_way_name("v4"), stock_way_name("relay")), ("Uniswap v4", "Relay swap"));
    // trade's ways in the words the window's picker and the route go by.
    assert_eq!([window_way("relay"), window_way("bridge:KyberSwap"), window_way("here:Uniswap"), window_way("here")], ["relay", "uniswap", "here", "here"]);
}

#[test]
fn a_waiting_moves_amount_is_edited_only_as_one_number() {
    with_engine(async |e| {
        // Nothing waits: nothing to edit.
        assert_eq!(e.step_edit(), None);
        assert!(e.edit_step(None, Some("2"), None).await.unwrap_err().0.contains("isn't waiting for Confirm"));
        let first = waiting(e, WalletCommand::BuyNear { usdc: Some(Decimal::ONE) });
        assert_eq!(e.step_edit(), Some(Edit { board: first.clone(), amount: Some("1".into()), unit: "USDC".into(), ways: false, stock: false, way: None, quotes: None }));
        // Not one number: refused, and the card is the same one.
        for bad in ["5abc", "1,000", "", "0", "1.2.3"] {
            let refused = e.edit_step(Some(&first), Some(bad), None).await.unwrap_err();
            assert_eq!(refused.0, "Type the amount as one number, for example 2.5 or 1000.", "«{bad}»");
            assert_eq!(e.st().card.as_ref().unwrap().id, first);
        }
        // Another block's step: not this move.
        assert!(e.edit_step(Some("another"), Some("2"), None).await.unwrap_err().0.contains("isn't waiting"));
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(e.edit_step(Some(&first), Some("x"), None).await.unwrap_err().0, "Напиши сумму одним числом, например 2,5 или 1000.");
    });
}

#[test]
fn an_evm_trades_step_offers_its_ways_and_takes_the_picked_one_once() {
    with_engine(async |e| {
        let buy = WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: Some(Decimal::from(5)), from: None };
        let id = waiting(e, buy.clone());
        let edit = e.step_edit().unwrap();
        assert!(edit.ways && edit.amount.as_deref() == Some("5") && edit.quotes.is_none() && edit.way.is_none() && edit.board == id);
        let way = |w: &str| Way { way: w.into(), gets: "2.41 AERO".into(), worth: "$4.90".into(), less: None, usd: None };
        // Quoted ways show once there are some; an empty answer (a failed quote) says no way quotes it now, as the Mac's
        // empty swapQuote (Cards18Tests).
        e.set_way_quotes(Some(WayQuotes { command: buy.clone(), ways: vec![] }));
        assert_eq!(e.step_edit().unwrap().quotes, Some(vec![]));
        e.set_way_quotes(Some(WayQuotes { command: buy.clone(), ways: vec![way("relay"), way("uniswap")] }));
        assert_eq!(e.step_edit().unwrap().quotes.map(|q| q.len()), Some(2));
        e.st_mut().route_way = Some(RouteWay { key: "base:aero".into(), way: "uniswap".into() });
        assert_eq!(e.step_edit().unwrap().way.as_deref(), Some("uniswap"));
        // Taken at Confirm, once; another trade's isn't taken.
        assert_eq!(e.take_way("arbitrum", "AERO"), None);
        assert_eq!(e.take_way("base", "AERO").as_deref(), Some("bridge"));
        assert_eq!(e.take_way("base", "AERO"), None);
        // The name trade's swap takes it by: its own «bridge» way, never a refusal for a way it doesn't know.
        let ways = [trade::swap::SwapWay::new("relay", Decimal::ONE, Decimal::ONE, ""), trade::swap::SwapWay::new("bridge", Decimal::TWO, Decimal::TWO, "")];
        e.st_mut().route_way = Some(RouteWay { key: "base:aero".into(), way: "uniswap".into() });
        assert_eq!(trade::Executor::pick(&ways, e.take_way("base", "AERO").as_deref()).map(|w| w.name), Some("bridge".to_string()));
        // A Robinhood Chain trade's ways are compared, not picked.
        waiting(e, WalletCommand::SellStock { symbol: "TSLA".into(), amount: None });
        let stock = e.step_edit().unwrap();
        assert!(stock.stock && !stock.ways && stock.amount.is_none() && stock.unit == "USDC");
        // A move with no amount and no way has nothing to edit.
        waiting(e, WalletCommand::ClaimRewards);
        assert_eq!(e.step_edit(), None);
    });
}

#[test]
fn why_this_way_answers_from_the_step_and_the_route_s_note() {
    with_engine(async |e| {
        let note = "One Solana signature, checked and simulated before it’s signed. Press Confirm. Cancel or “cancel” drops it.";
        let board = Board {
            steps: Some(vec![Step { label: "STAKE · JITOSOL".into(), title: "1 SOL".into(), lines: vec!["your SOL becomes JitoSOL".into(), String::new()],
                                    money: Some(Money { place: "Solana".into(), symbol: "SOL".into(), was: "2".into(), after: "1".into() }), status: None }]),
            note: Some(note.into()), ..Board::new("move", "stake 1 SOL")
        };
        let id = board.id.clone();
        e.set_boards(|b| b.insert(0, board));
        e.why_step(Some(&id), Some(0)).unwrap();
        let said = e.st().messages().last().cloned().unwrap();
        assert_eq!(said.text, format!("Why this way — Stake · JitoSOL: 1 SOL · Solana 2 → 1 SOL · your SOL becomes JitoSOL. {note}"));
        assert_eq!(said.note.as_deref(), Some("from the route"));
        assert!(!said.from_user);
        // A step that isn't there.
        for (board, index) in [(Some(id.as_str()), Some(1)), (Some(id.as_str()), Some(-1)), (Some("gone"), Some(0)), (None, Some(0)), (Some(id.as_str()), None)] {
            assert_eq!(e.why_step(board, index).unwrap_err().0, "That step isn't on the canvas any more.");
        }
    });
}

#[test]
fn a_way_picked_with_a_refused_amount_still_shows_on_the_waiting_card() {
    // Cards18Tests (wave 17): the route shows the picked way at once, whatever the amount then does.
    with_engine(async |e| {
        let usdc = |k: &str, x: i64| crate::portfolio::Network {
            key: k.into(), name: WalletCommand::place_name(k), usd: Decimal::ZERO, note: None, hidden: vec![], hidden_more: 0,
            rows: vec![crate::portfolio::Row { usd: Some(Decimal::from(x)), ..crate::portfolio::Row::new("USDC", &x.to_string(), Decimal::from(x)) }],
        };
        e.st_mut().portfolio = crate::portfolio::PortfolioView { networks: vec![usdc("base", 1), usdc("solana", 20)], updated_at: Some(chrono::Utc::now()), ..Default::default() };
        let buy = WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: Some(Decimal::from(5)), from: None };
        let id = waiting(e, buy.clone());
        let steps = e.steps(&Pending::Wallet(buy.clone()), "", &[]);
        e.update_board(Some(&id), |b| b.steps = Some(steps));
        let labels = |e: &Engine| -> Vec<String> {
            e.st().boards().iter().find(|b| b.id == id).and_then(|b| b.steps.clone()).unwrap_or_default().into_iter().map(|s| s.label).collect()
        };
        assert!(!labels(e).iter().any(|l| l == "RELAY + UNISWAP"));
        // Uniswap picked with 50 USDC, more than the wallet can bring: refused before a card (the ways on as cards), the
        // old one keeps waiting — and its Confirm takes Uniswap, so its route says so, with the exact approve.
        e.edit_step(Some(&id), Some("50"), Some("uniswap")).await.unwrap();
        let last = e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default();
        assert!(last.contains("Pick a way below"), "{last}");
        assert!(e.card_id().as_deref() == Some(id.as_str()) && e.quoted_move() == Some(buy));
        let l = labels(e);
        assert!(l.iter().any(|l| l == "RELAY + UNISWAP") && l.iter().any(|l| l == "APPROVE · EXACT"), "{l:?}");
        assert_eq!(e.st().route_way.as_ref().map(|r| r.way.as_str()), Some("uniswap"));
    });
}
