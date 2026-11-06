//! Cards16Tests' quote of a chain led by a Solana swap: the move a card prices, the output it showed (the Executor's
//! floor) and the step that shows it. State set by hand; nothing asks Jupiter.

use std::sync::Arc;

use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use rust_decimal::Decimal;

use crate::canvas::Step;
use crate::engine::{ChainStep, Engine, JupiterShown, Pending};
use crate::outbox::Outbox;
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
