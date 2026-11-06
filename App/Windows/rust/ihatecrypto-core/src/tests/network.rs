use rust_decimal::Decimal;

use super::d;
use crate::network::*;

/// UnifiedAccountTests' answers, shaped like a unified account read live on 2026-11-05: the perp state holds only what
/// positions use, the spot USDC is the whole account and its `hold` what positions and orders keep.
const PERP: &str = r#"{"marginSummary":{"accountValue":"55.8"},"withdrawable":"0.0","assetPositions":[]}"#;

fn spot(hold: &str) -> String { format!(r#"{{"balances":[{{"coin":"USDC","token":0,"total":"484.57","hold":"{hold}"}}]}}"#) }

#[test]
fn a_unified_account_reads_one_balance() {
    for mode in ["\"unifiedAccount\"", "\"portfolioMargin\""] {
        let b = Hyperliquid::parse_balance(PERP.as_bytes(), spot("55.8").as_bytes(), Hyperliquid::parse_unified(mode.as_bytes()).unwrap()).unwrap();
        assert!(b.account_value == d("484.57") && b.withdrawable == d("428.77") && b.spot_usdc.is_zero() && b.unified, "{mode}");
    }
    for mode in ["\"disabled\"", "\"default\"", "null"] {
        let b = Hyperliquid::parse_balance(PERP.as_bytes(), spot("55.8").as_bytes(), Hyperliquid::parse_unified(mode.as_bytes()).unwrap()).unwrap();
        assert!(b.account_value == d("55.8") && b.withdrawable.is_zero() && b.spot_usdc == d("484.57") && !b.unified, "{mode}");
    }
    // Portfolio margin's `hold` goes negative by what can be borrowed (read live: total 5.9M, hold −10.4M): that loan
    // room isn't free money, so «free» never reads above the account.
    let b = Hyperliquid::parse_balance(PERP.as_bytes(), spot("-10").as_bytes(), true).unwrap();
    assert!(b.account_value == d("484.57") && b.withdrawable == d("484.57"));
    assert!(Hyperliquid::parse_balance(b"{}", spot("1").as_bytes(), false).is_err());
}

/// userRole is read strictly: «missing» is a new account, any other role (one added later too) isn't, and no role —
/// none at all, one that isn't text, a page that isn't JSON — is an error, not «not new».
#[test]
fn user_role_is_read_strictly() {
    assert!(Hyperliquid::parse_new_account(br#"{"role":"missing"}"#).unwrap());
    for role in ["user", "agent", "vault", "subAccount", "builder"] {
        assert!(!Hyperliquid::parse_new_account(format!(r#"{{"role":"{role}"}}"#).as_bytes()).unwrap(), "{role}");
    }
    for body in ["{}", r#"{"role":42}"#, "<html>", r#"{"role":""}"#] {
        let e = Hyperliquid::parse_new_account(body.as_bytes()).unwrap_err();
        assert!(e.0.starts_with("Hyperliquid's userRole answered without a role: "), "{body}");
    }
}

#[test]
fn markets_from_meta_and_contexts() {
    let json = r#"[{"universe":[{"szDecimals":5,"name":"BTC","maxLeverage":40},{"szDecimals":4,"name":"ETH","maxLeverage":25},
                   {"szDecimals":2,"name":"OLD","maxLeverage":3,"isDelisted":true},{"szDecimals":0,"name":"NOPX","maxLeverage":3}]},
                  [{"funding":"0.0000125","markPx":"84000.0","prevDayPx":"83000.0","dayNtlVlm":"1234567.8"},{"markPx":"2680.5","funding":"-0.00001"},
                   {"markPx":"1.0"},{"markPx":null}]]"#;
    let main = Hyperliquid::parse_markets(json.as_bytes(), None, 0).unwrap();
    assert_eq!(main.len(), 2);
    let btc = &main["BTC"];
    assert!(btc.index == 0 && btc.mark_px == d("84000") && btc.prev_day_px == Some(d("83000")) && btc.volume24h == d("1234567.8") && btc.dex.is_none());
    assert!(main["ETH"].index == 1 && main["ETH"].prev_day_px.is_none() && main["ETH"].funding == Some(d("-0.00001")) && main["ETH"].volume24h.is_zero());
    // A HIP-3 exchange's asset ids follow its place in perpDexs.
    let xyz = Hyperliquid::parse_markets(json.as_bytes(), Some("xyz"), 1).unwrap();
    assert!(xyz["ETH"].index == 110_001 && xyz["ETH"].dex.as_deref() == Some("xyz"));
    assert_eq!(Hyperliquid::dex_names(br#"[null,{"name":"xyz","fullName":"XYZ"},{"name":"flx"}]"#), [None, Some("xyz".into()), Some("flx".into())]);
    assert!(Hyperliquid::parse_markets(br#"[{"universe":[]}]"#, None, 0).is_err());
    assert!(Hyperliquid::parse_markets(br#"[{"universe":[]},[{"markPx":1}]]"#, None, 0).is_err());   // a number where a string goes
}

#[test]
fn atr_is_the_average_range_of_the_last_14_bars() {
    let bars = |n: usize| format!("[{}]", vec![r#"{"h":"101","l":"99","c":"100"}"#; n].join(","));
    assert_eq!(Hyperliquid::parse_atr(bars(16).as_bytes(), "ETH").unwrap(), d("2"));
    assert_eq!(Hyperliquid::parse_atr(bars(14).as_bytes(), "ETH").unwrap_err().0, "Not enough ETH candles for ATR.");
    // A last close of 0 is NaN in Swift: an error here, not a panic.
    let zero = format!("[{},{}]", vec![r#"{"h":"1","l":"0","c":"1"}"#; 15].join(","), r#"{"h":"1","l":"0","c":"0"}"#);
    assert!(Hyperliquid::parse_atr(zero.as_bytes(), "ETH").is_err());
}

#[test]
fn near_says_what_it_lacks() {
    // TokenSendTests.
    assert!(Near::short_of_near(r#"{"TxExecutionError":{"InvalidTxError":{"NotEnoughBalance":{"balance":"1","cost":"2","signer_id":"ab"}}}}"#).unwrap().starts_with("Not enough NEAR"));
    assert!(Near::short_of_near(r#"{"ActionError":{"index":0,"kind":{"LackBalanceForState":{"account_id":"ab","amount":"1"}}}}"#).is_some());
    assert!(Near::short_of_near(r#"{"ActionError":{"index":0,"kind":{"AccountDoesNotExist":{"account_id":"bob.near"}}}}"#).is_none());
    assert_eq!(Near::parse_sent(br#"{"result":{"status":{"SuccessValue":""},"transaction":{"hash":"ABC"}}}"#).unwrap(), "ABC");
    let failed = Near::parse_sent(br#"{"result":{"status":{"Failure":{"ActionError":{"kind":{"LackBalanceForState":{}}}}}}}"#).unwrap_err();
    assert!(failed.0.starts_with("Not enough NEAR"));
    let failed = Near::parse_sent(br#"{"result":{"status":{"Failure":{"ActionError":{"kind":{"FunctionCallError":{}}}}}}}"#).unwrap_err();
    assert!(failed.0.starts_with("The NEAR transaction failed: "));
    assert_eq!(Near::parse_sent(b"{}").unwrap_err().0, "NEAR RPC send_tx: no result");
}

#[test]
fn near_balances_and_a_new_transactions_context() {
    assert_eq!(Near::parse_query(br#"{"error":{"cause":{"name":"UNKNOWN_ACCOUNT"},"message":"x"}}"#).unwrap(), None);
    assert_eq!(Near::parse_query(br#"{"error":{"message":"boom"}}"#).unwrap_err().0, "NEAR RPC: boom");
    let view = Near::parse_query(br#"{"result":{"amount":"2500000000000000000000000","storage_usage":182}}"#).unwrap();
    let wrapped: Vec<u8> = "\"1000000000000000000000000\"".bytes().collect();
    let call = serde_json::json!({"result": wrapped});
    assert_eq!(Near::amounts(view.as_ref(), Some(&call)), (d("2.5"), Decimal::ONE));
    assert_eq!(Near::amounts(None, None), (Decimal::ZERO, Decimal::ZERO));
    let key = serde_json::json!({"nonce": 41});
    let block = serde_json::json!({"header": {"hash": "11111111111111111111111111111111", "gas_price": "100000000"}});
    let c = Near::context(&key, view.as_ref().unwrap(), &block).unwrap();
    assert!(c.nonce == 42 && c.block_hash == [0u8; 32] && c.balance == "2500000000000000000000000" && c.storage_bytes == 182);
    assert_eq!(Near::context(&serde_json::json!({}), view.as_ref().unwrap(), &block).unwrap_err().0, "NEAR RPC: no nonce, block hash or balance.");
    let gone = Near::result("query", br#"{"error":{"cause":{"name":"UNKNOWN_ACCOUNT"}}}"#).unwrap_err();
    assert!(gone.0.starts_with("This NEAR account doesn't exist yet"));
}

#[test]
fn a_one_click_quote_keeps_its_slippage_reserve() {
    let q = protocols::one_click::parse_quote(br#"{"quote":{"amountIn":"10050000","amountInFormatted":"10.05","minAmountIn":"10040000","amountOutFormatted":"9.98"}}"#).unwrap();
    let b = Bridge::one_click_from(&q).unwrap();
    assert!(b.provider == "NEAR 1Click" && b.amount_in == d("10.05") && b.reserve == d("0.01") && b.amount_out == d("9.98") && b.time == "Usually 2–4 min.");
}

use std::collections::HashMap;

use crate::holdings::Holding;
use crate::network::{Solana, SolanaBalance};

/// SolanaHoldingsTests: bought tokens show, Jupiter Lend's shares are its row's, airdrops are tucked away.
#[test]
fn solana_holdings_show_bought_tokens_and_tuck_away_airdrops() {
    let json = r#"[{"id": "RAYmint", "symbol": "RAY", "icon": "https://cdn/ray.png", "usdPrice": 1.88, "liquidity": 18801373, "isVerified": true, "tags": ["verified"], "stats24h": {"priceChange": -2.5}},
     {"id": "jlUSDCmint", "symbol": "jlUSDC", "usdPrice": 1.06, "isVerified": true, "tags": ["verified", "jup-lend-earn"]},
     {"id": "MEMEmint", "symbol": "POKEMON", "usdPrice": 0.0004, "liquidity": 36500, "isVerified": false, "tags": ["token-2022"]},
     {"id": "SPAMmint", "symbol": "USDC", "usdPrice": 0.000001, "liquidity": 12, "isVerified": false, "tags": []}]"#;
    let amounts: HashMap<String, rust_decimal::Decimal> = [("RAYmint", "1.065743"), ("jlUSDCmint", "5"), ("MEMEmint", "2500"), ("SPAMmint", "1000"), ("Unknownmint", "3")]
        .into_iter().map(|(m, a)| (m.to_string(), super::d(a))).collect();
    let held = Solana::holdings_from(&amounts, &[json.as_bytes().to_vec()], false);
    // Jupiter Lend's shares are the Jupiter Lend row, not a token.
    assert!(!held.iter().any(|h| h.symbol == "jlUSDC"));
    // The most valuable first.
    assert_eq!(held[0].symbol, "RAY");
    assert_eq!(held[0].usd(), Some(super::d("1.88") * super::d("1.065743")));
    assert_eq!(held[0].change24h, Some(super::d("-2.5")));
    let by: HashMap<&str, &Holding> = held.iter().map(|h| (h.contract.as_str(), h)).collect();
    assert!(by["RAYmint"].looks_real());
    // Bought with Safe search off: unverified, but a real pool and ≈ $1 in it.
    assert!(by["MEMEmint"].looks_real());
    // A fake «USDC» airdrop: no pool to speak of.
    assert!(!by["SPAMmint"].looks_real());
    // Jupiter doesn't know it: shown by mint, unpriced, tucked away.
    assert_eq!(by["Unknownmint"].symbol, "Unkn…");
    assert_eq!(by["Unknownmint"].usd(), None);
    assert!(!by["Unknownmint"].looks_real());
    // Shares asked for are kept (a transfer of them as they are).
    assert!(Solana::holdings_from(&amounts, &[json.as_bytes().to_vec()], true).iter().any(|h| h.symbol == "jlUSDC"));
}

/// UnpricedBalanceTests: Jupiter didn't answer, so the tokens keep the last read's prices and their money stays in the balance.
#[test]
fn unpriced_tokens_keep_the_last_price() {
    let usdt = Holding { price: Some(super::d("1")), verified: true, ..Holding::new("solana", "Es9v", super::d("25"), "USDT") };
    let came = Holding::new("solana", "Es9v", super::d("24"), "Es9v…");
    let read = SolanaBalance { sol: super::d("0"), usdc: super::d("0"), tokens: vec![came], unpriced: true };
    let kept = read.pricing(std::slice::from_ref(&usdt));
    assert!(kept.tokens[0].usd() == Some(super::d("24")) && kept.tokens[0].symbol == "USDT" && kept.tokens[0].verified);
    assert!(!kept.unpriced);
    // A token the last read didn't have: still unpriced, so the balance waits for the next read.
    assert!(read.pricing(&[]).unpriced);
    // A priced read is left as it came.
    let priced = SolanaBalance { sol: super::d("0"), usdc: super::d("0"), tokens: vec![usdt.at(super::d("3"))], unpriced: false };
    assert_eq!(priced.pricing(std::slice::from_ref(&usdt)), priced);
}
