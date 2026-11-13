//! Cards16Tests' quote of a chain led by a Solana swap: the move a card prices, the output it showed (the Executor's
//! floor) and the step that shows it. Cards18Tests' (wave 17): a failed quote redraws its step («no quote now»), Refresh
//! outdates the loops still asking and keeps the last good quote, «Quoting…» until a new one lands. State set by hand,
//! no wallet (so no way is quoted); nothing asks Jupiter or any other place.

use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use rust_decimal::Decimal;

use crate::canvas::{Board, Step};
use crate::engine::{Card, ChainStep, Engine, JupiterShown, Pending, WayQuotes};
use crate::outbox::Outbox;
use crate::portfolio::{Network, PortfolioView, Row};
use crate::route_edit::Way;
use crate::signer_link::SignerLink;
use crate::store;

fn step(title: &str, lines: &[&str]) -> Step { Step { label: "BUY · JUPITER".into(), title: title.into(), lines: lines.iter().map(|l| l.to_string()).collect(), money: None, status: None } }

#[tokio::test]
async fn a_chain_led_by_a_solana_swap_is_held_to_its_price() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("L1's tests draw no state"));
        let sol = SpotAsset::all()[0].clone();
        let first = WalletCommand::Buy { asset: sol.clone(), usdc: Some(5.into()) };
        let then = WalletCommand::SolanaTo("arbitrum".into(), Some(2.into()));
        assert!(e.quoted_move().is_none());
        e.st_mut().pending = Some(Pending::Chain(vec![ChainStep::Wallet(first.clone()), ChainStep::Wallet(then.clone())]));
        assert_eq!(e.quoted_move(), Some(first.clone()));
        // A chain led by words, not a move: nothing to price.
        e.st_mut().pending = Some(Pending::Chain(vec![ChainStep::Said("chart SOL".into()), ChainStep::Wallet(then)]));
        assert!(e.quoted_move().is_none());
        e.st_mut().pending = Some(Pending::Wallet(first.clone()));
        assert_eq!(e.quoted_move(), Some(first.clone()));
        // What quote_card keeps once Jupiter answers for it.
        e.set_jupiter_shown(Some(JupiterShown { command: first.clone(), out: 51_200_000, title: "≈ 0.0512 SOL".into(), route: "Jupiter: USDC → SOL".into() }));
        assert_eq!(e.shown_out(&first), Some(51_200_000));
        assert_eq!(e.shown_out(&WalletCommand::Buy { asset: sol, usdc: Some(Decimal::from(6)) }), None);
        // The last step of its route gets the price and the route, once.
        let steps = e.with_jupiter(&first, vec![step("USDC", &[]), step("SOL", &["at the market"])]);
        assert_eq!((steps[1].title.as_str(), steps[1].lines.as_slice()), ("≈ 0.0512 SOL", &["Jupiter: USDC → SOL".to_string(), "at the market".into()][..]));
        assert_eq!(e.with_jupiter(&first, steps.clone()), steps);
        assert_eq!(steps[0].title, "USDC");
    }).await;
}

/// An engine in this test's own folder, English, its window locked (a state push sends the lock alone, so nothing is
/// drawn), holding `usdc` USDC on each network named.
async fn locked(usdc: &[(&str, i64)], test: impl AsyncFnOnce(Rc<Engine>)) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the window stays locked"));
        e.st_mut().lock.locked = true;
        e.set_settings(|s| s.language = Some("en".into()));
        e.st_mut().portfolio = PortfolioView {
            networks: usdc.iter().map(|(k, x)| Network {
                key: (*k).into(), name: WalletCommand::place_name(k), usd: Decimal::ZERO, note: None, hidden: vec![], hidden_more: 0,
                rows: vec![Row { usd: Some(Decimal::from(*x)), ..Row::new("USDC", &x.to_string(), Decimal::from(*x)) }],
            }).collect(),
            updated_at: Some(chrono::Utc::now()), ..Default::default()
        };
        test(e).await
    }).await;
}

/// A single move's card pinned by hand, as show() draws it (show() would ask for its quotes): its id.
fn card(e: &Engine, c: &WalletCommand) -> String {
    let pending = Pending::Wallet(c.clone());
    let steps = e.steps(&pending, "", &[]);
    let id = e.pin(Board { steps: Some(steps), state: Some("waiting".into()), ..Board::new("move", &c.summary()) });
    let mut st = e.st_mut();
    st.pending = Some(pending);
    st.card = Some(Card { id: id.clone(), title: c.summary(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![], over_limit: false });
    id
}

fn done(e: &Engine) {
    let id = e.card_id();
    e.set_boards(|b| b.retain(|b| Some(&b.id) != id.as_ref()));
    let mut st = e.st_mut();
    st.pending = None;
    st.card = None;
    st.route_way = None;
}

fn steps(e: &Engine, id: &str) -> Vec<Step> { e.st().boards().iter().find(|b| b.id == id).and_then(|b| b.steps.clone()).unwrap_or_default() }

fn aero(usdc: i64) -> WalletCommand { WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: Some(Decimal::from(usdc)), from: None } }

fn way(w: &str, gets: &str, worth: &str) -> Way { Way { way: w.into(), gets: gets.into(), worth: worth.into(), less: None, usd: None } }

#[tokio::test]
async fn a_failed_quote_redraws_the_step() {
    locked(&[("solana", 20)], async |e| {
        // A Base trade: «no quote now · asking again every 30 s» on its way step, not «comparing…».
        let buy = aero(5);
        let id = card(&e, &buy);
        assert!(steps(&e, &id).iter().any(|s| s.title == "comparing…"));
        e.quote_failed(&buy);
        let s = steps(&e, &id);
        assert!(s.iter().any(|s| s.label == "RELAY OR UNISWAP · COMPARING" && s.title == "no quote now" && s.lines.iter().any(|l| l == "asking again every 30 s")),
                "{s:?}");
        done(&e);
        // A swap across networks: «no quote now · asked again every 30 s», not «quoting…».
        let cross = WalletCommand::CrossSwap { from: "USDC".into(), on: "solana".into(), to: "ETH".into(), at: "base".into(), amount: Some(Decimal::TWO) };
        let id = card(&e, &cross);
        e.quote_failed(&cross);
        let s = steps(&e, &id);
        assert!(s.iter().any(|s| s.lines.iter().any(|l| l == "no quote now · asked again every 30 s")), "{s:?}");
        done(&e);
        // A stock: «BUY · UNISWAP», and its details say no way quotes it (boards.js: «No way quotes it right now.»).
        let stock = WalletCommand::BuyStock { symbol: "TSLA".into(), usdc: Some(Decimal::from(5)) };
        let id = card(&e, &stock);
        assert!(steps(&e, &id).iter().any(|s| s.label == "BUY · COMPARING") && e.step_edit().unwrap().quotes.is_none());
        e.quote_failed(&stock);
        let labels: Vec<String> = steps(&e, &id).into_iter().map(|s| s.label).collect();
        assert!(labels.iter().any(|l| l == "BUY · UNISWAP") && !labels.iter().any(|l| l == "BUY · COMPARING"), "{labels:?}");
        assert_eq!(e.step_edit().unwrap().quotes, Some(vec![]));
        done(&e);
    }).await;
}

#[tokio::test]
async fn refresh_outdates_the_loops_still_asking() {
    locked(&[("solana", 20)], async |e| {
        let buy = aero(5);
        let id = card(&e, &buy);
        let before = e.st().quote_round;
        assert!(e.quote_live(&buy, &id, before));
        e.requote();
        // Its new ask runs on the engine's next turn.
        for _ in 0..50 {
            if e.st().quote_round != before { break }
            tokio::task::yield_now().await;
        }
        let now = e.st().quote_round;
        // The loop of the earlier round stops at its next turn; only the new one asks and lands.
        assert!(now != before && !e.quote_live(&buy, &id, before));
        assert!(e.quote_live(&buy, &id, now));
        // Nor does a loop of this round land on another card or another move.
        assert!(!e.quote_live(&buy, "another card", now));
        assert!(!e.quote_live(&aero(6), &id, now));
        done(&e);
    }).await;
}

#[tokio::test]
async fn refresh_keeps_the_last_good_quote_and_says_quoting_until_a_new_one_lands() {
    locked(&[("solana", 20)], async |e| {
        let buy = aero(5);
        let id = card(&e, &buy);
        e.set_way_quotes(Some(WayQuotes { command: buy.clone(), ways: vec![way("relay", "2.41 AERO", "$4.90"), way("uniswap", "2.38 AERO", "$4.84")] }));
        e.redraw_route(&id, &buy);
        assert_eq!(e.quote_view().unwrap().state, "fresh");
        e.requote();
        assert_eq!(e.quote_view().unwrap().state, "quoting");
        // The place doesn't answer: still «Quoting…», never «over 15 s old» with the last good quote kept, as the Mac's.
        e.quote_failed(&buy);
        assert_eq!(e.quote_view().unwrap().state, "quoting");
        assert_eq!(e.step_edit().unwrap().quotes.map(|q| q.len()), Some(2));
        let s = steps(&e, &id);
        assert!(s.iter().any(|s| s.label == "RELAY SWAP · BEST NOW" && s.title == "≈ 2.41 AERO"), "{s:?}");
        // The new quote lands: Confirm counts down again.
        e.set_way_quotes(Some(WayQuotes { command: buy.clone(), ways: vec![way("relay", "2.45 AERO", "$4.95")] }));
        let q = e.quote_view().unwrap();
        assert!(q.state == "fresh" && q.line.as_deref().is_some_and(|l| l.contains("2.45 AERO")), "{q:?}");
        done(&e);
    }).await;
}
