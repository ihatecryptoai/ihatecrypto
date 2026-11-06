//! Top up's page on 127.0.0.1, from plain inputs (MoneyTests): a request read only when whole, only the page itself
//! answered, the server on loopback, the page carrying its order, USDC bringing its fees.

use std::collections::HashMap;

use ihatecrypto_core::top_up::{Coin, Network, TopUpOrder};
use rust_decimal::Decimal;

use super::{PageRequest, PageServer, TopUpPage, coin_named, coin_raw, evm_network, response, transfer_id};

const RECIPIENT: &str = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk";

#[test]
fn a_page_request_is_read_only_when_whole() {
    let head = "POST /abc/sent?x=1 HTTP/1.1\r\nHost: 127.0.0.1:5000\r\nOrigin: http://127.0.0.1:5000\r\nContent-Length: 8\r\n\r\n";
    assert!(PageRequest::parse(b"POST /abc HTTP/1.1\r\nHost: x").is_none());
    assert!(PageRequest::parse(format!("{head}{{\"id\"").as_bytes()).is_none());
    let r = PageRequest::parse(format!("{head}{{\"id\":1}}").as_bytes()).unwrap();
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/abc/sent"));
    assert_eq!(r.headers["origin"], "http://127.0.0.1:5000");
    assert_eq!(r.body, b"{\"id\":1}");
}

#[test]
fn only_the_page_itself_is_answered() {
    let request = |method: &str, host: &str, origin: Option<&str>| {
        let mut headers = HashMap::from([("host".to_string(), host.to_string())]);
        if let Some(o) = origin { headers.insert("origin".into(), o.into()); }
        PageRequest { method: method.into(), path: "/".into(), headers, body: vec![] }
    };
    assert!(TopUpPage::allows(&request("GET", "127.0.0.1:5000", None), 5000));
    // A name pointed at 127.0.0.1 (DNS rebinding) shows another Host; another site's POST shows another Origin.
    assert!(!TopUpPage::allows(&request("GET", "evil.example:5000", None), 5000));
    assert!(!TopUpPage::allows(&request("POST", "127.0.0.1:5000", None), 5000));
    assert!(!TopUpPage::allows(&request("POST", "127.0.0.1:5000", Some("https://evil.example")), 5000));
    assert!(TopUpPage::allows(&request("POST", "127.0.0.1:5000", Some("http://127.0.0.1:5000")), 5000));
}

#[tokio::test]
async fn the_server_answers_on_loopback() {
    tokio::task::LocalSet::new().run_until(async {
        let server = PageServer::start(|r| async move { response(200, "text/plain", format!("{} {} {}", r.method, r.path, r.body.len()).as_bytes()) }).unwrap();
        let answer = reqwest::Client::builder().no_proxy().build().unwrap()
            .post(format!("http://127.0.0.1:{}/page/sent", server.port)).body(vec![65u8; 3000]).send().await.unwrap();
        assert_eq!(answer.status().as_u16(), 200);
        assert_eq!(answer.headers()["x-frame-options"], "DENY");
        assert_eq!(answer.text().await.unwrap(), "POST /page/sent 3000");
        server.stop();
    }).await;
}

#[tokio::test]
async fn a_closed_page_answers_nothing_but_that_it_closed() {
    tokio::task::LocalSet::new().run_until(async {
        let r = PageRequest::parse(b"GET /abc HTTP/1.1\r\nHost: 127.0.0.1:5000\r\n\r\n").unwrap();
        let answer = String::from_utf8(TopUpPage::handle(r).await).unwrap();
        assert!(answer.starts_with("HTTP/1.1 403 Error\r\n") && answer.ends_with("This page has closed: open it again from ihatecrypto."));
    }).await;
}

#[test]
fn the_page_carries_the_order_and_nothing_can_close_its_script() {
    let order = TopUpOrder::new(Network::solana(), Coin::Usdc, Some(Decimal::from(20)), RECIPIENT);
    let page = TopUpPage::page(&order);
    assert!(page.contains(RECIPIENT) && page.contains("20 USDC") && !page.contains("{{"));
    assert!(page.contains("\"units\":\"20000000\""));
    assert!(!page.contains("on your Mac"));
    assert!(page.contains("<b>HAgk 14Jp … uT3D Kpqk</b>") && page.contains("on Solana · fee under 1¢ · lands in seconds"));
    // A recipient that tries to end the script is written as data.
    let hostile = TopUpOrder::new(Network::solana(), Coin::Native, None, "</script><script>alert(1)</script>");
    let config = TopUpPage::page(&hostile);
    let inline = &config[config.find("const C = ").unwrap()..];
    assert!(inline[..inline.find('\n').unwrap()].contains("\\u003c/script>"));
}

#[test]
fn an_evm_page_asks_its_network_first() {
    let base = evm_network(trade::EvmNetwork::base());
    assert_eq!((base.key.as_str(), base.chain_id, base.usdc_symbol.as_str(), base.coin.as_deref()), ("base", Some(8453), "USDC", Some("ETH")));
    assert_eq!(base.cost, "fee under 1¢ · lands in seconds");
    assert_eq!(evm_network(trade::EvmNetwork::ethereum()).cost, "fee 1–20¢ · lands in ~15 s");
    assert_eq!(evm_network(trade::EvmNetwork::robinhood()).usdc_symbol, "USDG");
    // Tempo pays its fees in its dollar: no coin of its own to top up.
    assert_eq!(evm_network(trade::EvmNetwork::tempo()).coin, None);
    let order = TopUpOrder::new(base, Coin::Usdc, Some(Decimal::from(5)), "0x9858EfFD232B4033E47d90003D41EC34EcaEda94");
    let page = TopUpPage::page(&order);
    assert!(page.contains("\"chainId\":\"0x2105\"") && page.contains("\"units\":\"5000000\"") && page.contains("MetaMask, Rabby, Phantom or Coinbase Wallet"));
}

#[test]
fn usdc_to_a_wallet_without_sol_brings_its_fees_along() {
    let order = TopUpOrder::new(Network::solana(), Coin::Usdc, Some(Decimal::from(5)), RECIPIENT);
    let message = TopUpPage::usdc_with_fees(&order, 5_000_000, "7cVfgArCheMR6Cs4t6vz5rfnqd56vZq4ndaBrY5xkxXy", "11111111111111111111111111111111").unwrap();
    assert!(!message.is_empty());
    // The fee payer signs alone; the SOL it brings is the top-up's.
    assert_eq!(&message[..3], &[1, 0, 5]);
    assert!(message.windows(12).any(|w| w == [&2u32.to_le_bytes()[..], &TopUpOrder::SOLANA_FEE_TOP_UP.to_le_bytes()].concat()));
}

#[test]
fn coins_and_transfer_ids_read_as_swift_s() {
    for c in [Coin::Usdc, Coin::Native, Coin::Any] { assert_eq!(coin_named(coin_raw(c)), Some(c)) }
    assert_eq!(coin_named("USDC"), None);
    assert!(transfer_id(&format!("0x{}", "ab".repeat(32))));
    assert!(transfer_id(&"5".repeat(88)));
    assert!(!transfer_id(&format!("0x{}", "ab".repeat(31))) && !transfer_id(&"0".repeat(88)) && !transfer_id(&"5".repeat(91)));
}
