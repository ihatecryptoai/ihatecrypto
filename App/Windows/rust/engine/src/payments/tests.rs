//! Payments, offline: what's refused before a service is ever called, and the USDC on Tempo the card counts.

use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::position_command::WalletCommand;
use rust_decimal::Decimal;

use crate::engine::Engine;
use crate::outbox::Outbox;
use crate::portfolio::{Network, Row};
use crate::signer_link::SignerLink;
use crate::store;

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("drawn while locked"));
    e.st_mut().lock.locked = true;
    e
}

fn pay(url: &str) -> WalletCommand { WalletCommand::Pay { method: "GET".into(), url: url.into(), body: None } }

#[tokio::test]
async fn a_service_that_refuses_its_proof_or_isn_t_a_web_address_is_never_called() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let refused = |r: crate::payments::PriceAsked| panic!("asked: {r:?}");
        let said = e.ask_price(&WalletCommand::ClaimRewards).await.map(refused).unwrap_err().to_string();
        assert_eq!(said, "That isn't a web address ihatecrypto can call.");
        assert_eq!(e.ask_price(&pay("not a url")).await.map(refused).unwrap_err().to_string(), "That isn't a web address ihatecrypto can call.");
        assert_eq!(e.ask_price(&pay("https://API.aisa.one/v1/search?q=x")).await.map(refused).unwrap_err().to_string(),
                   // ponytail: the host as WHATWG reads it, lowercased; Foundation's URL keeps its case for the words.
                   "api.aisa.one takes the payment and then refuses its proof (checked 2026-10-23), so ihatecrypto doesn't pay it. Ask for another service.");
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(e.ask_price(&pay("https://stableenrich.dev/x")).await.map(refused).unwrap_err().to_string(),
                   "stableenrich.dev берёт оплату, а потом не принимает её подтверждение (проверено 2026-10-23), поэтому ihatecrypto его не оплачивает. Попроси другой сервис.");
        // Confirm with no price shown and no service to ask: nothing is paid.
        assert!(e.st().payment.is_none());
    }).await;
}

#[tokio::test]
async fn tempo_s_usdc_is_read_from_the_balances() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        assert_eq!(e.tempo_usdc(), Decimal::ZERO);
        let network = |key: &str, rows: Vec<Row>| Network { key: key.into(), name: key.into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 };
        e.st_mut().portfolio.networks = vec![network("base", vec![Row::new("USDC", "9", Decimal::from(9))]),
                                             network("tempo", vec![Row::new("pathUSD", "1", Decimal::ONE), Row::new("USDC", "2.5", Decimal::new(25, 1))])];
        assert_eq!(e.tempo_usdc(), Decimal::new(25, 1));
    }).await;
}
