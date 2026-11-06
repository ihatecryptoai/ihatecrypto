//! PortfolioTests, offline: tokens the person hides stay hidden after a restart and can be shown again, airdrops that
//! look like scams are tucked, and a bridge order on its way reads as the «In transit» card shows it.

use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use execution::Kind;

use super::*;
use crate::outbox::Outbox;
use crate::portfolio::PortfolioView;
use crate::signer_link::SignerLink;
use crate::store::{self, Settings};

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::writer(std::io::sink()), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("locked: no state is drawn"));
    e.st_mut().lock.locked = true;
    e
}

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(test));
}

/// A listed token: its id, its dollars, whether it looks real.
fn token(id: &str, usd: i64, real: bool) -> Row {
    Row { id: Some(id.into()), real: Some(real), usd: Some(Decimal::from(usd)), ..Row::new(id, "1", Decimal::ONE) }
}

fn network(rows: Vec<Row>) -> Network {
    Network { key: "base".into(), name: "Base".into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 }
}

#[test]
fn a_token_is_shown_or_tucked_by_the_persons_say_then_by_how_real_it_looks() {
    let dollars = Row { usd: Some(Decimal::from(20)), ..Row::new("USDC", "20", Decimal::from(20)) };
    let n = network(vec![dollars, token("base:0xreal", 5, true), token("base:0xscam", 900, false), token("base:0xhid", 3, true), token("base:0xshown", 1, false)]);
    let choices = HashMap::from([("base:0xhid".to_string(), false), ("base:0xshown".to_string(), true)]);
    let n = Engine::tucked(n, &choices);
    assert_eq!(n.rows.iter().map(|r| r.symbol.as_str()).collect::<Vec<_>>(), ["USDC", "base:0xreal", "base:0xshown"]);
    // Tucked ones worth the most first, out of the card's total.
    assert_eq!(n.hidden.iter().map(|r| r.symbol.as_str()).collect::<Vec<_>>(), ["base:0xscam", "base:0xhid"]);
    assert_eq!(n.usd, Decimal::from(26));
    // Shown again: back among the rows, from the tucked list too; without a say, how real it looks decides again.
    let n = Engine::tucked(n, &HashMap::from([("base:0xscam".to_string(), true)]));
    assert_eq!(n.rows.iter().map(|r| r.symbol.as_str()).collect::<Vec<_>>(), ["USDC", "base:0xreal", "base:0xscam", "base:0xhid"]);
    assert!(n.hidden.iter().map(|r| r.symbol.as_str()).eq(["base:0xshown"]) && n.usd == Decimal::from(928));
    // A spammed address: the hundred worth the most are listed, the rest only counted.
    let spam = Engine::tucked(network((0..150).map(|i| token(&format!("base:0x{i}"), i, false)).collect()), &HashMap::new());
    assert_eq!((spam.hidden.len(), spam.hidden_more, spam.hidden[0].usd), (100, 50, Some(Decimal::from(149))));
}

#[test]
fn a_token_the_user_hides_stays_hidden_and_can_be_shown_again() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.st_mut().portfolio = PortfolioView { networks: vec![network(vec![token("base:0x940181a94a35a4569e4529a3cdfb74e38fd98631", 7, true)])], ..Default::default() };
        e.show_token("base:0x940181a94a35a4569e4529a3cdfb74e38fd98631", false);
        e.show_token("DoVAVzViX8Bjy3r15nwikSaSbzE6dV4ovd28aWpJpump", true);
        e.show_token("", false);
        let choices = store::load::<Settings>("settings.json").unwrap().token_choices.unwrap();
        assert_eq!(choices, HashMap::from([("base:0x940181a94a35a4569e4529a3cdfb74e38fd98631".to_string(), false),
                                           ("DoVAVzViX8Bjy3r15nwikSaSbzE6dV4ovd28aWpJpump".to_string(), true)]));
        // The card on screen follows at once: tucked, out of the total.
        let p = e.st().portfolio.clone();
        assert!(p.networks[0].rows.is_empty() && p.networks[0].hidden.len() == 1 && p.total.is_zero());
        // A change of mind replaces it.
        e.show_token("base:0x940181a94a35a4569e4529a3cdfb74e38fd98631", true);
        assert!(e.st().settings.token_choices.as_ref().unwrap()["base:0x940181a94a35a4569e4529a3cdfb74e38fd98631"]);
        assert_eq!(e.st().portfolio.total, Decimal::from(7));
    });
}

#[test]
fn a_bridge_order_on_its_way_reads_as_the_in_transit_card() {
    let open = Engine::transit(&Entry::new("relay", "0xdemo-open", Kind::Signed, "Solana USDC→Base 5, tx 5xDemoSignature"));
    assert_eq!((open.what.as_str(), open.bridge.as_str(), open.logo.as_str(), open.tx.as_deref(), open.receiving), ("Solana USDC→Base 5", "Relay", "relay", Some("5xDemoSignature"), false));
    assert_eq!(open.links.iter().map(|l| (l.title.as_str(), l.url.as_str())).collect::<Vec<_>>(),
               [("Relay", "https://relay.link/transaction/0xdemo-open"), ("Solscan", "https://solscan.io/tx/5xDemoSignature")]);
    // CCTP: «domain:burn», its burn on the source network's explorer, never Solscan.
    let cctp = Engine::transit(&Entry::new("cctp", "6:0xburn", Kind::Signed, "Base USDC→Arbitrum 5 (CCTP), tx 0xburn"));
    assert_eq!(cctp.links.iter().map(|l| (l.title.as_str(), l.url.as_str())).collect::<Vec<_>>(), [("Burn on Base", "https://basescan.org/tx/0xburn")]);
    assert_eq!((cctp.bridge.as_str(), cctp.logo.as_str()), ("Circle's CCTP", "usdc"));
    // An EVM transaction isn't Solscan's; money coming in from elsewhere is said so.
    let near = Engine::transit(&Entry::new("1click", "0xdeposit", Kind::Signed, "Receive: 5 USDC from Bitcoin, tx 0xabc more"));
    assert_eq!((near.bridge.as_str(), near.receiving, near.tx.as_deref()), ("NEAR Intents", true, Some("0xabc")));
    assert_eq!(near.links.iter().map(|l| l.url.as_str()).collect::<Vec<_>>(), ["https://explorer.near-intents.org/transactions/0xdeposit"]);
    let lz = Engine::transit(&Entry::new("layerzero", "0xlz", Kind::Signed, "Arbitrum USDT→Optimism 5"));
    assert!(lz.tx.is_none() && lz.links[0].title == "LayerZero Scan" && lz.logo == "usdt");
}
