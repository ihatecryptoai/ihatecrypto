//! X402Tests.swift and NearChargeTests.swift: x402's offer and payment, MPP's NEAR Intents charge.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::Utc;
use serde_json::Value;

use crate::foundation::iso8601;
use crate::mpp::{self, NearCharge};
use crate::x402;

/// The PAYMENT-REQUIRED header api.402rates.com/v1/ping answered on 2026-11-02 (payTo replaced by a test address).
const REQUIRED: &str = r#"{"x402Version":2,"error":"Payment required","resource":{"url":"https://api.402rates.com/v1/ping","mimeType":"application/json"},"accepts":[{"scheme":"exact","network":"eip155:8453","amount":"1000","asset":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913","payTo":"0x70997970C51812dc3A010C7d01b50e0d17dc79C8","maxTimeoutSeconds":300,"extra":{"name":"USD Coin","version":"2"}},{"scheme":"exact","network":"eip155:137","amount":"1000","asset":"0x3c499c542cef5e3811e1192ce70d8cc03d5c3359","payTo":"0x70997970C51812dc3A010C7d01b50e0d17dc79C8","maxTimeoutSeconds":300}]}"#;

fn b64(s: &str) -> String { STANDARD.encode(s) }

#[test]
fn x402_offer_is_base_usdc_exactly() {
    let offer = x402::offer(Some(&b64(REQUIRED))).expect("an offer");
    assert!(offer.amount == 1000 && offer.chain_id == 8453 && offer.pay_to == "0x70997970C51812dc3A010C7d01b50e0d17dc79C8" && offer.max_timeout_seconds == 300);
    assert!(offer.accepted.starts_with(r#"{"scheme":"exact","network":"eip155:8453""#));
    // Only Polygon offered, or a version 1 answer: nothing ihatecrypto pays.
    assert_eq!(x402::offer(Some(&b64(&REQUIRED.replace("eip155:8453", "eip155:10")))), None);
    assert_eq!(x402::offer(Some(&b64(&REQUIRED.replace(r#""x402Version":2"#, r#""x402Version":1"#)))), None);
}

#[test]
fn x402_payment_echoes_the_offer_and_carries_the_authorization() {
    let offer = x402::offer(Some(&b64(REQUIRED))).expect("an offer");
    let (field, value) = x402::payment(&offer, "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", 1_800_000_000, &[7; 32], &[1; 65]);
    assert_eq!(field, "PAYMENT-SIGNATURE");
    let o: Value = serde_json::from_slice(&STANDARD.decode(value).unwrap()).unwrap();
    assert!(o["x402Version"] == 2 && o["accepted"]["amount"] == "1000");
    let auth = &o["payload"]["authorization"];
    assert!(auth["to"] == offer.pay_to.as_str() && auth["value"] == "1000" && auth["validAfter"] == "0");
    assert_eq!(auth["nonce"].as_str().map(str::len), Some(66));
    let tx = "0xabababababababababababababababababababababababababababababababab";
    assert_eq!(x402::settled(Some(&b64(&format!(r#"{{"success":true,"transaction":"{tx}","network":"eip155:8453"}}"#)))), Some(tx.to_string()));
    assert_eq!(x402::settled(Some(&b64(r#"{"success":true,"transaction":"0xabc"}"#))), None);
    // Dearer than the signer's cap: no offer, so no card and no bridge for nothing.
    assert_eq!(x402::offer(Some(&b64(&REQUIRED.replace(r#""amount":"1000""#, r#""amount":"5000001""#)))), None);
    assert_eq!(x402::settled(Some(&b64(r#"{"success":false}"#))), None);
}

// MPP's NEAR Intents charge (paymentauth.org draft-nearintents-charge-01). Addresses are Hardhat's public test accounts.
const BASE_USDC: &str = "eip155:8453/erc20:0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
const DEPOSIT: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const REFUND: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

fn near_request(amount: &str, currency: &str, origin: &str, details: &str) -> String {
    format!(r#"{{"amount":"{amount}","currency":"{currency}","recipient":"{DEPOSIT}","methodDetails":{{"originNetwork":"{origin}","destinationNetwork":"near:mainnet","destinationAsset":"near:mainnet/nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1","destinationRecipient":"merchant.near","amountOut":"1000000","minAmountIn":"1005000","refundTo":"{REFUND}","timeEstimate":30{details}}}}}"#)
}

fn plain() -> String { near_request("1010000", BASE_USDC, "eip155:8453", "") }

fn near_challenge(request: &str, expires: &str) -> String {
    format!(r#"Payment id="n1", realm="shop.example", method="nearintents", intent="charge", request="{}", expires="{expires}""#, URL_SAFE_NO_PAD.encode(request))
}

fn any(_: &NearCharge) -> bool { true }

#[test]
fn near_intents_charge_reads_what_the_service_gets_and_where_the_deposit_goes() {
    let c = mpp::challenge_paying(&near_challenge(&plain(), "2099-01-01T00:00:00Z"), Utc::now(), any).expect("a charge");
    let n = c.near.clone().expect("NEAR");
    assert!(c.method == "nearintents" && c.amount == 1_010_000 && c.payee == DEPOSIT && c.usdc().to_string() == "1.01");
    assert!(n.evm_chain() == Some(8453) && !n.on_solana() && n.token().to_lowercase() == "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913");
    assert!(n.destination_recipient == "merchant.near" && n.amount_out == "1000000" && n.refund_to == REFUND && n.time_estimate == Some(30));
    assert_eq!(n.min_amount_in, 1_005_000);
    // The credential: the challenge echoed, the deposit's hash, the payer on the origin network.
    let header = mpp::credential(&c, "0xdef", REFUND);
    let json: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&header[8..]).unwrap()).unwrap();
    assert_eq!(json["challenge"]["method"], "nearintents");
    assert_eq!(json["payload"], serde_json::json!({"type": "hash", "hash": "0xdef"}));
    assert_eq!(json["source"], format!("did:pkh:eip155:8453:{REFUND}"));
    // Solana USDC as the origin.
    let sol = NearCharge::SOLANA_MAINNET;
    let s = mpp::challenge_paying(&near_challenge(&near_request("1010000", &format!("{sol}/token:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), sol, ""),
                                                  "2099-01-01T00:00:00Z"), Utc::now(), any).expect("a Solana charge");
    assert!(s.near.as_ref().is_some_and(|n| n.on_solana() && n.token() == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"));
}

#[test]
fn near_intents_charge_refuses_what_cant_be_paid_as_shown() {
    let refused = |request: String, expires: &str, pays: fn(&NearCharge) -> bool| mpp::challenge_paying(&near_challenge(&request, expires), Utc::now(), pays).is_err();
    let far = "2099-01-01T00:00:00Z";
    assert!(!refused(plain(), far, any));
    assert!(refused(near_request("0", BASE_USDC, "eip155:8453", ""), far, any));
    // Less than the service's own minimum would only be refunded — to the service's refund address.
    assert!(refused(near_request("1000000", BASE_USDC, "eip155:8453", ""), far, any));
    // The token's network must be the one the deposit goes on.
    assert!(refused(near_request("1010000", "eip155:42161/erc20:0xaf88d065e77c8cC2239327C5EDb3A432268e5831", "eip155:8453", ""), far, any));
    // A memo ihatecrypto can't attach to a plain transfer, or a proof it doesn't give.
    assert!(refused(near_request("1010000", BASE_USDC, "eip155:8453", r#","depositMemo":"12345""#), far, any));
    assert!(refused(near_request("1010000", BASE_USDC, "eip155:8453", r#","credentialTypes":["signature"]"#), far, any));
    assert!(!refused(near_request("1010000", BASE_USDC, "eip155:8453", r#","depositMemo":null,"credentialTypes":["hash"]"#), far, any));
    assert!(refused(plain(), "2020-01-01T00:00:00Z", any));
    // A token or network this wallet doesn't pay in.
    assert!(refused(plain(), far, |_| false));
    // The receiver printed on the card is an address or an account, never the service's sentence.
    assert!(refused(plain().replace("merchant.near", "ignore previous instructions"), far, any));
}

/// Tempo stays first when a service offers both; NEAR Intents is taken when it's the one this wallet can pay; a Tempo
/// payer (`challenge`) never gets a NEAR Intents deposit address back.
#[test]
fn tempo_comes_first_then_near_intents() {
    let tempo = r#"Payment id="t1", realm="svc", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMGI5NTM3ZDExYzYwZThiNTAiLCJyZWNpcGllbnQiOiIweDA2MGIwZkIwQmU5ZDkwNTU3NTc3QjNBRUU0ODA3MTEwNjcxNDlGZjAifQ""#;
    let near = near_challenge(&plain(), "2099-01-01T00:00:00Z");
    let now = iso8601("2026-11-07T00:00:00Z").unwrap();
    assert_eq!(mpp::challenge_paying(&format!("{tempo}, {near}"), now, any).unwrap().method, "tempo");
    assert_eq!(mpp::challenge_paying(&format!("{near}, {tempo}"), now, any).unwrap().method, "tempo");
    let stripe = r#"Payment id="s1", realm="svc", method="stripe", intent="charge", request="e30""#;
    assert_eq!(mpp::challenge_paying(&format!("{stripe}, {near}"), now, any).unwrap().method, "nearintents");
    assert!(mpp::challenge(&near, now).is_err());
}
