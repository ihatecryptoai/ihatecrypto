//! The cases of GaslessTests, OneClickTests and SolanaTests that read these services' answers, with their fixtures;
//! the calls against a local stand-in server.

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{agent_api, jupiter, one_click, recurring, relay};

fn d(s: &str) -> Decimal { Decimal::from_str(s).unwrap() }

/// A server at 127.0.0.1 that answers each request with the next (status, body), and keeps each request's first line.
async fn server(answers: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    tokio::spawn(async move {
        for (status, body) in answers {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                buf.extend(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if buf.len() >= end + 4 + length {
                        kept.lock().unwrap().push(text.lines().next().unwrap_or("").to_string());
                        break;
                    }
                }
                if n == 0 { break }
            }
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
        }
    });
    (url, seen)
}

const QUICK: [Duration; 4] = [Duration::from_millis(1); 4];

// MARK: Relay

#[test]
fn relays_price_v2_is_read_from_its_string() {
    // GET /currencies/token/price/v2?address=0x0…0&chainId=8453 (2026-11-05), and the docs' sub-micro example.
    assert_eq!(relay::price(br#"{"price":"2415.813385"}"#).unwrap(), Some(d("2415.813385")));
    assert_eq!(relay::price(br#"{"price":"0.000000000349076496006"}"#).unwrap(), Some(d("0.000000000349076496006")));
    assert_eq!(relay::price(br#"{"price":2415.670903}"#).unwrap(), Some(d("2415.670903"))); // v1's number
    assert_eq!(relay::price(br#"{"error":"Could not get price"}"#).unwrap(), None);
}

const RELAY_PROXY: &str = "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be";
const NONCE: &str = "1a66a6de1f9f27675cfd809e56c7d573a25922c450f6b56ced9013f360512be7";

/// Relay's authorize1 step for 3 USDC from Base, as /quote/v2 gave it with usePermit (2026-10-23, the public test address).
fn relay_item() -> Value {
    json!({"status": "incomplete", "data": {
        "sign": {"signatureKind": "eip712", "primaryType": "ReceiveWithAuthorization",
                 "domain": {"name": "USD Coin", "version": "2", "chainId": 8453, "verifyingContract": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"},
                 "value": {"from": "0x9858EfFD232B4033E47d90003D41EC34EcaEda94", "to": RELAY_PROXY, "value": "3000000", "validAfter": 0,
                           "validBefore": 1_792_706_124u64, "nonce": format!("0x{NONCE}")}},
        "post": {"endpoint": "/execute/permits", "method": "POST",
                 "body": {"kind": "eip3009", "requestId": "0x1792705524576d6b75c5353aa74b090990ce6143da4bf85d9c2661c7481d4793", "api": "swap"}},
    }})
}

fn relay_step(items: usize) -> Vec<Value> { vec![json!({"id": "authorize1", "kind": "signature", "items": vec![relay_item(); items]})] }

#[test]
fn relays_permit_step_is_read_and_anything_more_is_not() {
    let p = relay::permit_in(&relay_step(1)).unwrap();
    assert_eq!((p.chain_id, p.token.as_str(), p.to.as_str(), p.value.as_str()), (8453, evm::BASE_USDC, RELAY_PROXY, "3000000"));
    assert_eq!((p.valid_after, p.valid_before, wallet::to_hex(&p.nonce), p.endpoint.as_str()), (0, 1_792_706_124, NONCE.into(), "/execute/permits"));
    assert_eq!((p.domain_name.as_str(), p.domain_version.as_str()), ("USD Coin", "2"));
    assert_eq!(serde_json::from_slice::<Value>(&p.body).unwrap()["kind"], "eip3009");
    // Two items (v3.2's MulticallAuthorization first), or an approval step before it: not a payment by signature alone.
    assert!(relay::permit_in(&relay_step(2)).is_none());
    let mut with_approve = vec![json!({"id": "approve", "kind": "transaction", "items": []})];
    with_approve.extend(relay_step(1));
    assert!(relay::permit_in(&with_approve).is_none());
}

#[test]
fn relays_v32_step_is_read_in_order() {
    let (router, relayer) = ("0x0729e2a4716d9f7dd17858d5d504426d228697f7", "0xf70da97812cb96acdf810712aa562db8dfa3dbef");
    let first = json!({"status": "incomplete", "data": {
        "sign": {"signatureKind": "eip712", "primaryType": "MulticallAuthorization",
                 "domain": {"name": "RelayApprovalProxy", "version": "3.1", "chainId": 8453, "verifyingContract": evm::protocols::relay::PROXY},
                 "value": {"from": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "relayer": relayer, "msgValue": "0", "refundTo": relayer,
                           "nftRecipient": relayer, "metadata": "0x9a7b", "fundingIndex": 0,
                           "calls": [{"target": router, "allowFailure": false, "value": "0", "callData": "0x73b7bb2f"}],
                           "funding": [{"token": evm::BASE_USDC, "from": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "value": "1000000",
                                        "validAfter": 0, "validBefore": 1_792_716_428u64}]}},
        "post": {"endpoint": "/execute/permits", "method": "POST", "body": {"kind": "multicall-authorization", "requestId": "0x01", "api": "swap"}},
    }});
    let mut second = relay_item();
    second["data"]["sign"]["value"]["to"] = json!(evm::protocols::relay::PROXY);
    second["data"]["sign"]["value"]["value"] = json!("1000000");
    let steps = vec![json!({"id": "authorize1", "kind": "signature", "items": [first, second]})];
    let m = relay::multicall_in(&steps).unwrap();
    assert_eq!((m.domain_name.as_str(), m.domain_version.as_str(), m.funding_index, m.calls.len()), ("RelayApprovalProxy", "3.1", 0, 1));
    assert_eq!((m.calls[0].target.as_str(), m.calls[0].allow_failure, m.calls[0].call_data.clone()), (router, false, vec![0x73, 0xb7, 0xbb, 0x2f]));
    assert_eq!((m.funding[0].value.as_str(), m.funding[0].valid_before), ("1000000", 1_792_716_428));
    assert_eq!(m.permit.to, evm::protocols::relay::PROXY);
    assert_eq!(serde_json::from_slice::<Value>(&m.body).unwrap()["kind"], "multicall-authorization");
    // The EIP-3009 item alone is the v3 form, not this one; and the v3 parser won't take two items.
    assert!(relay::multicall_in(&relay_step(1)).is_none());
    assert!(relay::permit_in(&steps).is_none());
}

#[test]
fn a_relay_quote_is_read_whole() {
    let answer = json!({
        "steps": [
            {"id": "approve", "kind": "transaction", "requestId": "0xabc",
             "items": [{"data": {"to": evm::BASE_USDC, "data": "0x095ea7b3", "value": "0", "chainId": 8453}}]},
            {"id": "deposit", "kind": "transaction",
             "items": [{"data": {"to": "0x4cd00e387622c35bddb9b4c962c136462338bc31", "data": "0x01", "value": 7, "chainId": 8453}},
                       {"data": {"to": "0x1", "data": "not hex"}}]},
            {"id": "solana", "kind": "transaction", "depositAddress": "dep1",
             "items": [{"data": {"instructions": [{"programId": "ComputeBudget111111111111111111111111111111", "data": "03e803000000000000",
                                                    "keys": [{"pubkey": "me", "isSigner": true, "isWritable": true}]}]}}]},
        ],
        "details": {"sender": "me", "recipient": "you", "timeEstimate": 4,
                    "currencyIn": {"amount": "1000000", "amountFormatted": "1.0", "amountUsd": "1.00"},
                    "currencyOut": {"amount": "999000", "amountFormatted": "0.999", "amountUsd": "0.999"},
                    "currencyGasTopup": {"amountFormatted": "0.000037"}},
        "fees": {"relayer": {"amountUsd": "0.01"}, "relayerGas": {"amountUsd": "1.20"}},
        "protocol": {"v2": {"orderData": {"version": "v1"}}},
    });
    let q = relay::parse_quote(answer.to_string().as_bytes(), false, true).unwrap();
    assert_eq!((q.request_id.as_str(), q.sender.as_str(), q.recipient.as_str(), q.seconds), ("0xabc", "me", "you", 4));
    assert_eq!((q.amount_in.as_str(), q.amount_out.as_str(), q.amount_out_formatted.as_str(), q.gas_topup.as_str()), ("1000000", "999000", "0.999", "0.000037"));
    assert_eq!((q.fees_usd.as_str(), q.relayer_gas_usd.as_str(), q.amount_in_usd.as_str()), ("0.01", "1.20", "1.00"));
    // A transaction item without readable data is left out; a value given as a number reads as Swift interpolates it.
    assert_eq!(q.evm.iter().map(|s| (s.id.as_str(), s.value.as_str())).collect::<Vec<_>>(), [("approve", "0"), ("deposit", "7")]);
    assert_eq!(q.solana_instructions[0].accounts, [("me".to_string(), true, true)]);
    assert_eq!(q.solana_instructions[0].data[0], 3);
    assert_eq!(q.deposit_address.as_deref(), Some("dep1"));
    assert_eq!(serde_json::from_slice::<Value>(q.order.as_ref().unwrap()).unwrap(), json!({"version": "v1"}));
    assert!(q.permit.is_none() && q.hyperliquid.is_none());
    // A Solana instruction that can't be read is no quote: what isn't read isn't signed.
    let mut bad = answer.clone();
    bad["steps"][2]["items"][0]["data"]["instructions"][0]["data"] = json!("zz");
    assert!(relay::parse_quote(bad.to_string().as_bytes(), false, false).unwrap_err().0.starts_with("Relay: "));
    assert_eq!(relay::parse_quote(br#"{"message":"Amount too low"}"#, false, false).unwrap_err().0, r#"Relay: {"message":"Amount too low"}"#);
}

#[test]
fn a_hyperliquid_origin_reads_its_mapping_and_deposit() {
    let answer = json!({"requestId": "0xr", "details": {}, "steps": [
        {"id": "authorize", "kind": "signature", "items": [{"data": {
            "sign": {"value": {"id": "0xid", "nonce": 9, "wallet": "0xw", "depositor": "0xd"}},
            "post": {"endpoint": "/authorize", "body": {"a": 1}}}}]},
        {"id": "deposit", "kind": "signature", "items": [{"data": {"nonce": 77, "action": {"type": "sendAsset", "parameters": {
            "destination": "0xdep", "token": "USDC:0x6d", "amount": "1.5", "sourceDex": "", "destinationDex": "spot"}}}}]},
    ]});
    let h = relay::parse_quote(answer.to_string().as_bytes(), false, false).unwrap().hyperliquid.unwrap();
    assert_eq!((h.mapping_id.as_str(), h.mapping_nonce, h.wallet.as_str(), h.nonce), ("0xid", 9, "0xw", 77));
    assert_eq!((h.destination.as_str(), h.amount.as_str(), h.destination_dex.as_str()), ("0xdep", "1.5", "spot"));
    assert_eq!(serde_json::from_slice::<Value>(&h.authorize_body).unwrap(), json!({"a": 1}));
}

#[test]
fn a_quote_body_says_what_is_asked() {
    let ask = relay::Ask { user: "u".into(), recipient: "r".into(), origin_chain: relay::chain::BASE, destination_chain: relay::chain::SOLANA,
                           origin_currency: "a".into(), destination_currency: "b".into(), amount: 5, use_permit: true,
                           topup_gas_usd: Some(d("0.0123451")), ..Default::default() };
    let b = ask.body();
    assert_eq!((b["amount"].as_str(), b["tradeType"].as_str(), b["originChainId"].as_u64()), (Some("5"), Some("EXACT_INPUT"), Some(8453)));
    assert_eq!((b["usePermit"].as_bool(), b["useMulticallAuthorization"].as_bool(), b["topupGasAmount"].as_str()), (Some(true), Some(true), Some("12346")));
    assert!(b.get("useDepositAddress").is_none() && b.get("refundTo").is_none());
    let units = relay::Ask { units: Some("1000000000000000000000".into()), exact_output: true, ..ask }.body();
    assert_eq!((units["amount"].as_str(), units["tradeType"].as_str()), (Some("1000000000000000000000"), Some("EXACT_OUTPUT")));
}

#[test]
fn relays_chains_name_its_contracts() {
    let chains = br#"{"chains":[{"id":8453,"protocol":{"v2":{"depository":"0xdep"}},"contracts":{"approvalProxy":"0xproxy"}},{"id":1}]}"#;
    assert_eq!(relay::depository_in(chains, 8453).as_deref(), Some("0xdep"));
    assert_eq!(relay::approval_proxy_in(chains, 8453).as_deref(), Some("0xproxy"));
    assert_eq!(relay::depository_in(chains, 1), None);
}

#[tokio::test]
async fn relays_status_is_asked_again_while_it_throttles() {
    let (url, seen) = server(vec![(429, "slow".into()), (502, "<html>bad gateway</html>".into()),
                                  (200, r#"{"status":"success","txHashes":["0xt"]}"#.into())]).await;
    assert_eq!(relay::status_at(&url, "0xr", &QUICK).await.unwrap(), ("success".into(), vec!["0xt".into()]));
    assert_eq!(seen.lock().unwrap()[0], "GET /intents/status/v3?requestId=0xr HTTP/1.1");
    let (url, _) = server(vec![(400, r#"{"message":"Invalid request id"}"#.into())]).await;
    assert_eq!(relay::status_at(&url, "0xr", &QUICK).await.unwrap_err().0, r#"Relay: {"message":"Invalid request id"}"#);
    let (url, seen) = server(vec![(200, "{}".into()), (500, "{}".into())]).await;
    relay::post_signature_at(&url, "/authorize", b"{\"a\":1}", "0xsig").await.unwrap();
    assert_eq!(seen.lock().unwrap()[0], "POST /authorize?signature=0xsig HTTP/1.1");
    assert_eq!(relay::post_signature_at(&url, "/authorize", b"{}", "0xsig").await.unwrap_err().0, "Relay: {}");
}

// MARK: 1Click

#[test]
fn one_click_sends_the_partner_key_only_when_set() {
    one_click::set_key(None);
    assert!(one_click::headers().is_empty());
    one_click::set_key(Some("eyJ.test.key".into()));
    assert_eq!(one_click::headers(), [("X-API-Key", "eyJ.test.key".to_string())]);
    one_click::set_key(Some(String::new()));
    assert!(one_click::key().is_none() && one_click::headers().is_empty());
}

fn status(origin: &str, destination: &str, recipient: &str, refund_to: &str, state: &str) -> Value {
    json!({"status": state,
           "swapDetails": {"amountInFormatted": "1000.0", "amountInUsd": "999.75", "amountOutFormatted": "185.421463672726355985056693"},
           "quoteResponse": {"quoteRequest": {"originAsset": origin, "destinationAsset": destination, "recipient": recipient, "refundTo": refund_to}}})
}

/// A settled 1Click swap as the NEAR chart's cost reads it (the shape of /status for the 1,000 USDC buy on 2026-10-10).
#[test]
fn one_click_settled_swaps_move_near() {
    use one_click::{Settled, asset};
    let me = "8ec67b662faddd44e7d79875a4cf1e7f8c3f756a4b171ccea95a4149b8dddf2b";
    let buy = Settled::new(&status(asset::SOLANA_USDC, asset::NEAR, me, "AULW53", "SUCCESS")).unwrap();
    assert_eq!(buy.near(me), Some((d("185.421463672726355985056693"), d("1000"))));
    assert_eq!(Settled::new(&status(asset::SOLANA_SOL, asset::NEAR, me, "AULW53", "SUCCESS")).unwrap().near(me).unwrap().1, d("999.75"));
    assert_eq!(Settled::new(&status(asset::NEAR, asset::SOLANA_USDC, "AULW53", me, "SUCCESS")).unwrap().near(me).unwrap().0, d("-1000"));
    assert_eq!(buy.near("d523049b8db090f8b47bdbfc027ba4c42df08782835303be44ea61d9f1955dfb"), None);
    assert!(Settled::new(&status(asset::SOLANA_USDC, asset::NEAR, me, "AULW53", "REFUNDED")).is_none());
}

/// 1Click sends its failures as JSON (NestJS), so an error code, a page (a gateway's 404 too) or JSON with no status is no
/// answer (an error); a swap that isn't a success, or an address it doesn't know (its own 404), is one (None).
#[tokio::test]
async fn one_click_settled_tells_no_answer_from_no_swap() {
    let ok = status(one_click::asset::SOLANA_USDC, one_click::asset::NEAR, "me.near", "AULW53", "SUCCESS").to_string();
    let (url, seen) = server(vec![(200, ok.clone())]).await;
    assert_eq!(one_click::settled_at(&url, "a", &[]).await.unwrap().unwrap().near("me.near").unwrap().0, d("185.421463672726355985056693"));
    assert_eq!(seen.lock().unwrap()[0], "GET /status?depositAddress=a HTTP/1.1");
    let (url, _) = server(vec![(200, r#"{"status":"PROCESSING"}"#.into())]).await;
    assert_eq!(one_click::settled_at(&url, "a", &[]).await.unwrap(), None);
    let (url, _) = server(vec![(404, r#"{"message":"Deposit address stub-unknown not found","error":"Not Found","statusCode":404}"#.into())]).await;
    assert_eq!(one_click::settled_at(&url, "a", &[]).await.unwrap(), None);
    for (code, body) in [(500, r#"{"statusCode":500,"message":"Internal server error"}"#), (429, r#"{"statusCode":429,"message":"ThrottlerException: Too Many Requests"}"#),
                         (502, "<html><body>502 Bad Gateway</body></html>"), (200, "{}"), (404, "<html><body>404 Not Found</body></html>")] {
        let (url, _) = server(vec![(code, body.into())]).await;
        assert!(one_click::settled_at(&url, "a", &[]).await.is_err(), "{code} {body}");
    }
    // Throttled once, then the swap.
    let (url, seen) = server(vec![(429, r#"{"statusCode":429}"#.into()), (200, ok)]).await;
    assert!(one_click::settled_at(&url, "a", &QUICK).await.unwrap().is_some());
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn one_click_answers_are_read() {
    let tokens = br#"[{"assetId":"nep141:wrap.near","blockchain":"near","symbol":"wNEAR","decimals":24,"price":2.31,"contractAddress":"Wrap.Near"},
                      {"assetId":"x","blockchain":"sol"}]"#;
    let t = one_click::parse_tokens(tokens);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].decimals, t[0].contract.as_deref(), t[0].price), (24, Some("wrap.near"), Some(d("2.31"))));
    let q = one_click::parse_quote(br#"{"quote":{"depositAddress":"dep","amountIn":"5000000","amountOutFormatted":"1.2","amountInUsd":"5.00","timeEstimate":20}}"#).unwrap();
    assert_eq!((q.deposit_address.as_deref(), q.amount_in_formatted.as_str(), q.time_estimate, q.amount_in_usd), (Some("dep"), "5000000", Some(20), Some(d("5.00"))));
    assert_eq!(one_click::parse_quote(br#"{"message":"Amount is too low for bridge"}"#).unwrap_err().0, r#"1Click: {"message":"Amount is too low for bridge"}"#);
    let bound = one_click::parse_bound(br#"{"quoteResponse":{"quoteRequest":{"recipient":"r","refundTo":"f","originAsset":"o","destinationAsset":"d","amount":"1"},"quote":{"depositAddress":"dep"}}}"#).unwrap();
    assert_eq!((bound["recipient"].as_str(), bound["depositAddress"].as_str(), bound.len()), ("r", "dep", 6));
    let s = one_click::parse_status(br#"{"status":"SUCCESS","swapDetails":{"amountOutFormatted":"1.2","destinationChainTxHashes":[{"hash":"0xh"}]}}"#).unwrap();
    assert!(s.is_final() && s.destination_txs == ["0xh"] && s.amount_out_formatted.as_deref() == Some("1.2"));
    assert!(!one_click::parse_status(br#"{"status":"PROCESSING"}"#).unwrap().is_final());
    let body = one_click::Ask::new(true, one_click::asset::SOLANA_USDC, one_click::asset::NEAR, "5000000", "me", "you").body();
    assert_eq!((body["swapType"].as_str(), body["refundType"].as_str(), body["slippageTolerance"].as_i64()), (Some("EXACT_INPUT"), Some("ORIGIN_CHAIN"), Some(100)));
    let deadline = body["deadline"].as_str().unwrap();
    assert!(deadline.ends_with('Z') && deadline.len() == 20, "{deadline}");
    assert_eq!(one_click::asset::across("tron")[0].0, "USDT");
    assert_eq!(one_click::asset::usdc_on_evm("polygon"), None);
}

// MARK: Jupiter

/// Jupiter's price impact and the swap's USD worth, as each API says them: v1's quote in strings, v2's order in numbers
/// with the impact negative (2026-10-28, 2M WIF for USDC).
#[test]
fn jupiter_says_impact_and_worth_either_way() {
    assert_eq!(jupiter::impact_and_worth(&json!({"priceImpactPct": "0.1235947568", "swapUsdValue": "320500.59"})), (d("0.1235947568"), d("320500.59")));
    let (impact, usd) = jupiter::impact_and_worth(&json!({"priceImpactPct": "-0.0103491778", "swapUsdValue": 361914.39}));
    assert!(impact == d("0.0103491778") && usd > d("361914") && usd < d("361915"));
    assert_eq!(jupiter::impact_and_worth(&json!({})), (Decimal::ZERO, Decimal::ZERO));
}

#[test]
fn jupiters_answers_are_read() {
    let quote = br#"{"inAmount":"1000","outAmount":"990","otherAmountThreshold":"985","priceImpactPct":"0.01",
                     "routePlan":[{"swapInfo":{"label":"HumidiFi"}},{"swapInfo":{"label":"Hadron"}}]}"#;
    let q = jupiter::parse_quote(quote, 200).unwrap();
    assert_eq!((q.in_amount, q.out_amount, q.min_out, q.route.as_str()), (1000, 990, 985, "HumidiFi → Hadron"));
    assert_eq!(q.raw, quote.to_vec());
    let throttled = jupiter::parse_quote(b"<html>503 Service Temporarily Unavailable</html>", 503).unwrap_err();
    assert!(throttled.again && throttled.error.0 == "Jupiter: no answer right now (HTTP 503). Try again in a minute.");
    let refused = jupiter::parse_quote(br#"{"error":"Could not find any route"}"#, 400).unwrap_err();
    assert!(!refused.again && refused.error.0 == "Jupiter: Could not find any route");
    let order = br#"{"requestId":"r1","transaction":"AQID","inAmount":"10","outAmount":"9","otherAmountThreshold":"8","gasless":true,
                     "feeBps":12,"lastValidBlockHeight":"345","router":"jupiterz","priceImpactPct":-0.5}"#;
    let o = jupiter::parse_order(order, 200).unwrap();
    assert_eq!((o.wire.clone(), o.gasless, o.fee_bps, o.last_valid_block_height, o.router.as_str(), o.impact), (vec![1, 2, 3], true, 12, 345, "jupiterz", d("0.5")));
    let none = jupiter::parse_order(br#"{"errorMessage":"Insufficient funds","transaction":""}"#, 200).unwrap_err();
    assert!(!none.again && none.error.0 == "Jupiter: Insufficient funds");
    assert_eq!(jupiter::parse_order(br#"{"requestId":"r"}"#, 200).unwrap_err().error.0, "Jupiter: no transaction for this swap");
    assert_eq!(jupiter::parse_order_quote(br#"{"outAmount":"77","router":"metis","priceImpactPct":"0.2"}"#, 200).unwrap(), (77, d("0.2"), "metis".into()));
    assert_eq!(crate::query("Meteora DLMM,Whirlpool"), "Meteora%20DLMM,Whirlpool");
}

/// /build's answer becomes a v0 transaction: its instructions in order, the wallet's own token accounts kept in the message,
/// the rest loaded from its lookup table, and its own amounts and route.
#[test]
fn jupiters_build_compiles_to_a_v0_transaction() {
    let (me, usdc, wsol) = ("2EUrWmf5xMmWER9BtDbXbGbZjoL7R3eTDMXYR6H6cKPj", jupiter::USDC_MINT, jupiter::SOL_MINT);
    let (pool, table) = ("D8cy77BBepLMngZx6ZukaTff5hCt1HrWyKk3Hnd9oitf", "6rqx7NGgRAtGiAsBE6RAxWMaNQfSgzRy1tMSNML4GzQm");
    let ata = |m: &str| solana::spl::associated_account(me, m, solana::spl::TOKEN_PROGRAM).unwrap();
    let account = |k: &str, w: bool| json!({"pubkey": k, "isSigner": k == me, "isWritable": w});
    let answer = json!({
        "computeBudgetInstructions": [{"programId": "ComputeBudget111111111111111111111111111111", "accounts": [], "data": "AsBcFQA="}],
        "setupInstructions": [],
        "swapInstruction": {"programId": "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4", "data": "u2T6zDHErxQ=",
                            "accounts": [account(me, false), account(&ata(usdc), true), account(&ata(wsol), true), account(usdc, false), account(wsol, false), account(pool, true)]},
        "cleanupInstruction": null,
        "addressesByLookupTableAddress": {table: [pool, usdc, wsol, ata(wsol)]},
        "blockhashWithMetadata": {"blockhash": vec![7u8; 32], "lastValidBlockHeight": 345},
        "outAmount": "9386507", "otherAmountThreshold": "9339574",
        "routePlan": [{"swapInfo": {"label": "SolFi"}}],
    });
    let quote = jupiter::Quote { input: usdc.into(), output: wsol.into(), ..jupiter::parse_quote(br#"{"inAmount":"1000000","outAmount":"1","otherAmountThreshold":"1"}"#, 200).unwrap() };
    let b = jupiter::built(&jupiter::parse_build(&serde_json::to_vec(&answer).unwrap(), 200).unwrap(), &quote, me).unwrap();
    assert_eq!((b.out_amount, b.min_out, b.last_valid_block_height, b.route.as_str()), (9_386_507, 9_339_574, 345, "SolFi"));
    let m = solana::message::Message::parse_transaction(&b.wire).unwrap();
    assert_eq!(m.version, Some(0));
    assert_eq!(m.instructions.len(), 2);
    // The wallet's own accounts and both mints stay in the message; the pool loads from the table.
    assert!([ata(usdc), ata(wsol), usdc.into(), wsol.into()].iter().all(|k| m.static_keys.contains(k)) && !m.static_keys.iter().any(|k| k == pool));
    assert_eq!((m.lookups[0].writable.clone(), m.lookups[0].readonly.clone()), (vec![0], vec![]));
    // No swap in it: asked again only on a throttle.
    assert!(!jupiter::parse_build(br#"{"error":"No routes found"}"#, 400).unwrap_err().again);
}

#[tokio::test]
async fn jupiters_execute_says_its_code_and_asks_again_on_a_throttle() {
    let (url, seen) = server(vec![(429, r#"{"error":"Too many requests","code":429}"#.into()), (200, r#"{"status":"Success","signature":"5sig"}"#.into())]).await;
    assert_eq!(jupiter::execute_at(&url, &[1, 2], "r1", &QUICK).await.unwrap(), "5sig");
    assert_eq!(seen.lock().unwrap()[1], "POST /execute HTTP/1.1");
    let (url, _) = server(vec![(200, r#"{"status":"Failed","error":"Transaction expired","code":-1005}"#.into())]).await;
    assert_eq!(jupiter::execute_at(&url, &[1], "r1", &QUICK).await.unwrap_err().0, "Jupiter: Transaction expired (code -1005)");
}

#[test]
fn recurring_orders_are_read() {
    let page = br#"{"totalPages":1,"time":[
        {"orderKey":"k1","inputMint":"a","outputMint":"b","inDeposited":"100","inUsed":"50","inAmountPerCycle":"50","cycleFrequency":"86400",
         "outReceived":"0.4","createdAt":"2026-10-01T10:00:00.123Z","trades":[{},{}]},
        {"orderKey":"k2","inputMint":"a","outputMint":"b","inDeposited":100,"cycleFrequency":3600,"createdAt":"2026-10-02T10:00:00"},
        {"inputMint":"no key"}]}"#;
    let (orders, pages) = recurring::parse_page(page, 200).unwrap();
    assert_eq!((orders.len(), pages), (2, 1));
    assert_eq!((orders[0].every, orders[0].fills, orders[0].received, orders[0].deposited), (86400, 2, d("0.4"), d("100")));
    assert!(orders[1].created_at > orders[0].created_at);
    assert_eq!(orders[1].every, 3600);
    assert_eq!(recurring::parse_page(br#"{"error":"Each order valued at 30.00 USDC, minimum is 50.00 USDC"}"#, 400).unwrap_err().0,
               "Jupiter: Each order valued at 30.00 USDC, minimum is 50.00 USDC");
    assert_eq!(recurring::parse_transaction(br#"{"requestId":"r","transaction":"AQID"}"#, 200).unwrap(), ("r".into(), vec![1, 2, 3]));
    let body = recurring::create_body("u", "a", "b", 100_000_000, 2, 86400);
    assert_eq!(body["params"]["time"]["numberOfOrders"], 2);
    assert!(body["params"]["time"]["minPrice"].is_null());
}

/// Live and read-only (no money, no key): Relay prices Base's ETH and Jupiter quotes 1 USDC into SOL.
#[tokio::test]
#[ignore]
async fn live_quotes_answer() {
    assert!(relay::native_price(relay::chain::BASE).await.unwrap().unwrap() > Decimal::ZERO);
    let q = jupiter::quote(jupiter::USDC_MINT, jupiter::SOL_MINT, 1_000_000, 50, &[]).await.unwrap();
    assert!(q.out_amount > 0 && !q.dexes.is_empty());
}

/// A private quote asks 1Click for Confidential Intents; the binding read back carries what the guard checks for it
/// and for an MPP charge; a private send pays USDC on Solana and the EVM networks 1Click takes it on, and coins across.
#[test]
fn one_click_private_quotes_and_the_binding_they_are_checked_by() {
    use crate::one_click::{Ask, asset, parse_bound};
    let ask = Ask::new(true, asset::SOLANA_USDC, "nep141:btc.omft.near", "1000", "me", "you");
    assert!(ask.body().get("confidentiality").is_none());
    assert_eq!(Ask { confidential: true, ..ask }.body()["confidentiality"], "basic");
    let bound = parse_bound(br#"{"quoteResponse":{"quoteRequest":{"recipient":"r","refundTo":"f","originAsset":"o","destinationAsset":"d","amount":"1000000",
        "confidentiality":"basic","swapType":"EXACT_OUTPUT"},"quote":{"depositAddress":"0xdep","amountIn":"1020228","minAmountIn":"1010125","deadline":"2099-01-01T00:00:00.000Z"}}}"#).unwrap();
    for (k, v) in [("confidentiality", "basic"), ("swapType", "EXACT_OUTPUT"), ("depositAddress", "0xdep"), ("quoteAmountIn", "1020228"),
                   ("quoteMinAmountIn", "1010125"), ("deadline", "2099-01-01T00:00:00.000Z")] {
        assert_eq!(bound.get(k).map(String::as_str), Some(v), "{k}");
    }
    assert_eq!(asset::payout("usdc", "base").map(|a| a.1), asset::usdc_on_evm("base"));
    assert_eq!(asset::payout("USDC", "solana").map(|a| a.1), Some(asset::SOLANA_USDC));
    assert_eq!(asset::payout("BTC", "bitcoin").map(|a| a.0), Some("BTC"));
    assert_eq!(asset::payout("USDC", "polygon"), None);
    assert_eq!(asset::payout("ETH", "base"), None);
}

// MARK: Agent API

/// generate-intent's answer for an agent_create (the shape of a live one, 2026-11-04): the payload kept byte for byte to
/// be signed and spliced back into submit-intent, the preview's policy under the hash the API names; status and errors.
#[test]
fn agent_api_answers_are_read() {
    let answer = br#"{"correlation_id":"c-1","type":"agent_create","agent_id":"ab12","intent":{"standard":"nep366","payload":{"receiverId":"outlayer.near", "actions":[]}},"signer":{"account_id":"owner.near"},"expires_at":"2026-11-07T12:00:00Z","preview":{"policy":{"frozen":false,"budget":{"daily_usd":"20"}},"policy_hash":"00"}}"#;
    let g = agent_api::parse_generated(answer).unwrap();
    assert_eq!((g.correlation_id.as_str(), g.ty.as_str(), g.standard.as_str(), g.agent_id.as_deref()), ("c-1", "agent_create", "nep366", Some("ab12")));
    assert_eq!(g.payload, br#"{"receiverId":"outlayer.near", "actions":[]}"#);
    assert_eq!((g.signer_account.as_deref(), g.preview_hash.as_deref()), (Some("owner.near"), Some("00")));
    assert_eq!(agent_api::policy_hash(g.preview_policy.as_ref().unwrap()), agent_api::policy_hash(br#"{ "budget": {"daily_usd": "20"}, "frozen": false }"#));
    assert_eq!(String::from_utf8(agent_api::submit_body(&g, br#"{"standard":"nep366","payload":{"b":1, "a":2}}"#)).unwrap(),
               r#"{"correlation_id":"c-1","type":"agent_create","signed_data":{"standard":"nep366","payload":{"b":1, "a":2}}}"#);
    assert_eq!(agent_api::parse_generated(br#"{"correlation_id":"c-1","intent":{"payload":{}}}"#).unwrap_err().code, "http_200");

    let s = agent_api::parse_status(br#"{"status":"SUCCESS","details":{"policy_hash":"ab"}}"#).unwrap();
    assert!(s.is_final() && s.ok() && s.details.as_deref() == Some(&br#"{"policy_hash":"ab"}"#[..]));
    assert!(!agent_api::parse_status(br#"{"status":"UNCERTAIN"}"#).unwrap().is_final());
    assert!(agent_api::parse_status(br#"{"status":"NEEDS_REVIEW"}"#).unwrap().is_final());
    assert_eq!(agent_api::parse_status(b"[]").unwrap().status, "UNKNOWN");

    let f = agent_api::Failure::new(422, br#"{"errors":[{"code":"policy_violation","title":"Not allowed","detail":"over the daily cap"}]}"#);
    assert_eq!(f.to_string(), "Agent API: Not allowed: over the daily cap (policy_violation)");
    assert_eq!(agent_api::Failure::new(502, b"<html>bad gateway</html>").to_string(), "Agent API: <html>bad gateway</html> (http_502)");
}

/// Calls against a stand-in: none without a key, the API's own error, no answer told apart (status 0), and a status
/// asked until it's final — again after no answer while there's time.
#[tokio::test]
async fn agent_api_calls_and_waits_until_final() {
    agent_api::set_key(None);
    assert_eq!(agent_api::call_at("http://127.0.0.1:9", "GET", "/v1/whoami", None, None, None).await.unwrap_err().code, "no_key");
    agent_api::set_key(Some("naa_test".into()));
    let (url, seen) = server(vec![(403, r#"{"errors":[{"code":"grant_expired","title":"Grant expired"}]}"#.into())]).await;
    let e = agent_api::call_at(&url, "POST", "/v1/agents/a/withdraw", Some(b"{}"), Some("g"), Some("withdraw-1")).await.unwrap_err();
    assert_eq!((e.status, e.code.as_str(), e.is_no_answer()), (403, "grant_expired", false));
    assert_eq!(seen.lock().unwrap()[0], "POST /v1/agents/a/withdraw HTTP/1.1");

    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let nobody = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    assert!(agent_api::call_at(&nobody, "GET", "/v1/whoami", None, None, None).await.unwrap_err().is_no_answer());
    let e = agent_api::until_final_at(&nobody, "c-1", Duration::from_millis(30), Duration::from_millis(5)).await.unwrap_err();
    assert!(e.is_no_answer());

    let (url, seen) = server(vec![(200, r#"{"status":"UNCERTAIN"}"#.into()), (200, r#"{"status":"SUCCESS"}"#.into())]).await;
    assert!(agent_api::until_final_at(&url, "c-1", Duration::from_secs(5), Duration::from_millis(5)).await.unwrap().ok());
    assert_eq!(*seen.lock().unwrap(), ["GET /v1/status?correlation_id=c-1&wait_ms=30000 HTTP/1.1"; 2]);
    agent_api::set_key(None);
}
