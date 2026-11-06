//! AllocationTests, RebalanceTests, KaminoVaultTests and the Kamino half of KaminoBorrowTests with their vectors; the
//! services' answers (trimmed live ones, 2026-11-06) read offline, and the calls against a local stand-in server.

use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::allocation::{self, Slice};
use crate::jupiter_lend::{self, JupiterLend};
use crate::kamino::{self, Collateral, Debt, Kamino, Loan};
use crate::rebalance::{self, Move};
use crate::yields;
use crate::*;

fn d(s: &str) -> Decimal { Decimal::from_str(s).unwrap() }
fn n(i: i64) -> Decimal { Decimal::from(i) }

type Seen = Arc<Mutex<Vec<(String, String)>>>;
type Routes = Arc<Mutex<HashMap<String, VecDeque<(u16, String)>>>>;

/// A service at 127.0.0.1 as a test sets it: path → its answers (status, body), the last one kept for every later ask;
/// a path not set answers 404 «Cannot GET path», as Kamino does. Keeps each request's first line and body.
async fn server(routes: &[(&str, &[(u16, &str)])]) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let routes: HashMap<String, VecDeque<(u16, String)>> =
        routes.iter().map(|(p, a)| (p.to_string(), a.iter().map(|(s, b)| (*s, b.to_string())).collect())).collect();
    let routes = Arc::new(Mutex::new(routes));
    let seen: Seen = Default::default();
    let kept = seen.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer(socket, routes.clone(), kept.clone()));
        }
    });
    (url, seen)
}

async fn answer(mut socket: TcpStream, routes: Routes, kept: Seen) {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
    let (line, body) = loop {
        let n = socket.read(&mut chunk).await.unwrap_or(0);
        buf.extend(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
            if buf.len() >= end + 4 + length {
                break (text.lines().next().unwrap_or("").to_string(), String::from_utf8_lossy(&buf[end + 4..end + 4 + length]).to_string());
            }
        }
        if n == 0 { return }
    };
    let path = line.split(' ').nth(1).unwrap_or("").split('?').next().unwrap_or("").to_string();
    let (status, reply) = match routes.lock().unwrap().get_mut(&path) {
        Some(q) if q.len() > 1 => q.pop_front().unwrap(),
        Some(q) => q[0].clone(),
        None => (404, format!("Cannot GET {path}")),
    };
    kept.lock().unwrap().push((line, body));
    let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
    socket.write_all(reply.as_bytes()).await.ok();
    socket.shutdown().await.ok();
}

const QUICK: [Duration; 4] = [Duration::from_millis(1); 4];

fn message<T: std::fmt::Debug>(r: wallet::Result<T>) -> String { r.unwrap_err().0 }

// MARK: Foundation

#[test]
fn numbers_read_and_written_as_foundation_does() {
    // What Swift printed for these on macOS 27: formatted(.number.precision(.fractionLength(0...2)).locale(en_US)).
    for (x, grouped, plain) in [("2.345", "2.34", "2.34"), ("2.355", "2.36", "2.36"), ("1234.5", "1,234.5", "1234.5"), ("-0.001", "-0", "-0"),
                                ("0.005", "0", "0"), ("0.015", "0.02", "0.02"), ("50", "50", "50"), ("-1.5", "-1.5", "-1.5")] {
        assert_eq!((two_places(d(x), true).as_str(), two_places(d(x), false).as_str()), (grouped, plain), "{x}");
    }
    assert_eq!((text(d("1.50")), text(d("100")), text(d("0.0000001")), text(Decimal::ZERO)), ("1.5".into(), "100".into(), "0.0000001".into(), "0".into()));
    // Decimal(string:): «12abc» is 12, « 5» is 5, «abc» is nil.
    assert_eq!((decimal("12abc"), decimal(" 5"), decimal("abc"), decimal("1e-7")), (Some(n(12)), Some(n(5)), None, Some(d("0.0000001"))));
    // NSNumber from JSONSerialization: a fraction as written, true as 1, intValue cut toward zero, -1 as 2⁶⁴ − 1.
    let j = json!({"a": 5.58, "b": 0.0523456789, "t": true, "e": 1e-7, "j": 6.7, "i": -1});
    assert_eq!((ns_dec(j.get("a")), ns_dec(j.get("b")), ns_dec(j.get("t")), ns_dec(j.get("e"))), (Some(d("5.58")), Some(d("0.0523456789")), Some(n(1)), Some(d("0.0000001"))));
    assert_eq!((ns_int(j.get("j")), ns_u64(j.get("i")), ns_dec(Some(&json!("5")))), (Some(6), Some(u64::MAX), None));
    // Not JSON, or a bare value: Foundation's words.
    for bad in [&b"<html>"[..], b"", b"5"] { assert_eq!(message(json(bad)), NOT_JSON) }
}

// MARK: Allocation (AllocationTests)

fn place(name: &str, apy: &str) -> allocation::Place { allocation::Place::new(name, d(apy)) }

#[test]
fn best_rates_first_under_the_limit_counting_what_is_already_there() {
    let places = [
        allocation::Place { existing: n(1), ..place("Jupiter Lend", "5.58") },
        allocation::Place { blocked: Some("аренда больше 10% суммы".into()), ..place("Kamino Lend", "4.79") },
        place("Aave", "4"),
    ];
    // 3 free + 1 already in Jupiter = 4 in total; 50% cap = 2 per protocol.
    let r = allocation::plan(n(3), &places, n(50), d("0.5"));
    assert_eq!(r.slices, [Slice { place: "Jupiter Lend".into(), amount: n(1), apy: d("5.58") }, Slice { place: "Aave".into(), amount: n(2), apy: n(4) }]);
    assert!(r.leftover.is_zero() && r.notes.iter().any(|n| n.starts_with("Kamino Lend")));
    assert_eq!(r.notes, ["Kamino Lend (4.79%): skipped, аренда больше 10% суммы."]);

    // One usable protocol and a 50% cap: half stays unplaced, and the note says how to change that.
    let alone = allocation::plan(n(4), &[place("Jupiter Lend", "5")], n(50), n(1));
    assert_eq!((alone.slices.iter().map(|s| s.amount).collect::<Vec<_>>(), alone.leftover), (vec![n(2)], n(2)));
    assert_eq!(alone.yearly(), d("0.1"));
    assert_eq!(alone.notes, ["The rest is not placed: the other protocols are at the 50% limit or unavailable. To raise the limit, add “max 70% per protocol” to the request."]);
    // Crumbs below the minimum slice are not worth a transaction.
    assert!(allocation::plan(d("0.3"), &places, n(50), d("0.5")).slices.is_empty());
    // A protocol already over the limit says so; a blocked rate is written as en_US writes it.
    let full = allocation::plan(n(1), &[allocation::Place { existing: n(10), ..place("Jupiter Lend", "5") },
                                       allocation::Place { blocked: Some("no".into()), ..place("Odd", "1234.567") }], n(50), d("0.5"));
    assert_eq!(full.notes[..2], ["Odd (1,234.57%): skipped, no.", "Jupiter Lend: Jupiter Lend is already at the 50% limit."]);
}

#[test]
fn the_limit_is_per_protocol_across_networks() {
    // Aave on two networks shares one 50% cap: 10 USDC free goes 5 to Aave (both markets together) and 5 elsewhere.
    let aave = |name: &str, apy: &str| allocation::Place { protocol_name: "Aave".into(), ..place(name, apy) };
    let plan = allocation::plan(n(10), &[aave("Aave · Arbitrum", "6"), aave("Aave · Polygon", "5.5"), place("Jupiter Lend", "5")], n(50), d("0.5"));
    assert_eq!(plan.slices.iter().filter(|s| s.place.starts_with("Aave")).map(|s| s.amount).sum::<Decimal>(), n(5));
    assert_eq!(plan.slices.iter().find(|s| s.place == "Jupiter Lend").map(|s| s.amount), Some(n(5)));
}

// MARK: Rebalance (RebalanceTests)

fn spot(name: &str, apy: i64, amount: &str) -> rebalance::Place { rebalance::Place::new(name, n(apy), d(amount)) }

#[test]
fn rebalance_fills_the_best_rate_up_to_the_cap() {
    let plan = rebalance::plan(&[spot("Jupiter Lend", 4, "1000"), rebalance::Place { entry_cost: d("0.5"), ..spot("Kamino Lend", 6, "0") }], n(50));
    assert_eq!(plan.moves, [Move { from: "Jupiter Lend".into(), to: "Kamino Lend".into(), amount: n(500), from_apy: n(4), to_apy: n(6), cost: d("0.5") }]);
    assert_eq!(plan.yearly_gain(), n(10)); // 500 × 2% a year
    assert!(plan.moves[0].payback_days().unwrap() < n(30)); // 0.5 / 10 × 365 ≈ 18 days
    assert_eq!(plan.cost(), d("0.5"));
}

#[test]
fn rebalance_skips_a_move_that_does_not_pay_back() {
    // 3 USDC two points better earns 6 cents a year; the bridges cost 6 cents: a year to pay back.
    let plan = rebalance::plan(&[spot("Jupiter Lend", 4, "3"), rebalance::Place { entry_cost: d("0.06"), ..spot("Aave · Base", 6, "0") }], n(100));
    assert!(plan.moves.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert!(plan.skipped[0].reason.contains("pays back in 365 days"));
    assert_eq!(plan.skipped[0].reason, "+$0.06 a year against ≈ $0.06 to move: pays back in 365 days, more than 30");
    // Under a 50% cap the half that would move doesn't pay either, and the place over the cap says why it stays.
    let capped = rebalance::plan(&[spot("Jupiter Lend", 4, "3"), rebalance::Place { entry_cost: d("0.06"), ..spot("Aave · Base", 6, "0") }], n(50));
    assert_eq!(capped.skipped[0].reason, "+$0.03 a year against ≈ $0.06 to move: pays back in 730 days, more than 30");
    assert_eq!(capped.notes, ["Jupiter Lend stays above 50% (3 of 3 USDC): moving the rest out wouldn't pay for itself."]);
    // Crumbs: 0.3 USDC to move is skipped with its own words.
    let crumbs = rebalance::plan(&[spot("Jupiter Lend", 4, "0.6"), spot("Kamino Lend", 6, "0")], n(50));
    assert_eq!(crumbs.skipped[0].reason, "0.3 USDC is too little to move");
}

#[test]
fn rebalance_leaves_a_locked_place_alone() {
    let plan = rebalance::plan(&[rebalance::Place { locked: Some("locked until Oct 1".into()), ..spot("HLP (Hyperliquid)", 3, "5") },
                                 spot("Jupiter Lend", 6, "0")], n(100));
    assert!(plan.moves.is_empty());
    assert!(plan.notes.iter().any(|n| n.contains("HLP (Hyperliquid) keeps 5 USDC: locked until Oct 1")));
}

#[test]
fn rebalance_has_nothing_to_do_at_the_best_rates() {
    let plan = rebalance::plan(&[spot("Jupiter Lend", 6, "50"), spot("Kamino Lend", 5, "50"), spot("Aave · Base", 4, "0")], n(50));
    assert!(plan.moves.is_empty() && plan.skipped.is_empty());
    assert_eq!(plan.notes, ["Already at the best rates within 50% per protocol: nothing to move."]);
    assert!(rebalance::plan(&[], n(50)).notes.first().is_some_and(|n| n.contains("Nothing is deposited")));
}

#[test]
fn rebalance_keeps_what_the_cap_cannot_place_where_it_is() {
    // One better place and a 50% cap: half moves, half stays in the old place instead of going anywhere worse.
    let plan = rebalance::plan(&[spot("Jupiter Lend", 4, "100"), spot("Kamino Lend", 6, "0"),
                                 rebalance::Place { closed: Some("not now".into()), ..spot("Aave · Base", 3, "0") }], n(50));
    assert_eq!(plan.moves.iter().map(|m| m.amount).collect::<Vec<_>>(), [n(50)]);
    assert_eq!(plan.moves[0].to, "Kamino Lend");
}

#[test]
fn rebalance_caps_aave_across_networks_together() {
    // 10 USDC in Jupiter Lend; two Aave markets pay more. Together they may take only half.
    let aave = |name: &str, apy| rebalance::Place { protocol_name: "Aave".into(), ..spot(name, apy, "0") };
    let plan = rebalance::plan(&[spot("Jupiter Lend", 3, "10"), aave("Aave · Arbitrum", 9), aave("Aave · Polygon", 8)], n(50));
    assert_eq!(plan.moves.iter().filter(|m| m.to.starts_with("Aave")).map(|m| m.amount).sum::<Decimal>(), n(5));
}

// MARK: Jupiter Lend

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const JL_USDC: &str = "9BEcn9aPEmhSPbPQeFGjidRiEKki46fVQDyPpSQXPA2D";
const JL_WSOL: &str = "2uQsyo1fXXQkDtcpXnLofWy88PxcvnfH2L8FPSE62FVU";

/// /earn/tokens (2026-11-06, trimmed), a rate as a number with no assetAddress, and a vault without its asset.
const JL_TOKENS: &str = r#"[
 {"id":2,"address":"9BEcn9aPEmhSPbPQeFGjidRiEKki46fVQDyPpSQXPA2D","name":"jupiter  lend USDC","symbol":"jlUSDC","uiSymbol":"jlUSDC","decimals":6,
  "assetAddress":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","asset":{"address":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","chainId":"solana","name":"USD Coin","symbol":"USDC","decimals":6},
  "totalAssets":"42133328482194","rewardsRate":"0","supplyRate":"581","totalRate":"581"},
 {"id":3,"address":"2uQsyo1fXXQkDtcpXnLofWy88PxcvnfH2L8FPSE62FVU","symbol":"jlWSOL","decimals":9,"assetAddress":"So11111111111111111111111111111111111111112",
  "asset":{"address":"So11111111111111111111111111111111111111112","symbol":"WSOL","decimals":9},"totalRate":"362"},
 {"address":"X","symbol":"jlX","asset":{"address":"Xmint"},"totalRate":4.5},
 {"symbol":"jlNoAsset","assetAddress":"Y"}]"#;

#[test]
fn jupiter_lend_vaults_are_read_with_their_rates_in_percent() {
    let v = jupiter_lend::parse_vaults(JL_TOKENS.as_bytes()).unwrap();
    assert_eq!(v.len(), 3);
    assert_eq!((v[0].symbol.as_str(), v[0].address.as_str(), v[0].asset_symbol.as_str(), v[0].asset_mint.as_str(), v[0].decimals, v[0].apy),
               ("jlUSDC", JL_USDC, "USDC", USDC, 6, d("5.81")));
    assert_eq!((v[1].asset_symbol.as_str(), v[1].decimals, v[1].apy), ("WSOL", 9, d("3.62")));
    assert_eq!((v[2].asset_symbol.as_str(), v[2].asset_mint.as_str(), v[2].decimals, v[2].apy), ("?", "Xmint", 6, d("0.045")));
    // Not a list: the service's words; not JSON: Foundation's.
    assert_eq!(message(jupiter_lend::parse_vaults(br#"{"code":429,"message":"[API Gateway] Too many requests"}"#)),
               r#"Jupiter Lend: {"code":429,"message":"[API Gateway] Too many requests"}"#);
    assert_eq!(message(jupiter_lend::parse_vaults(b"<html>502</html>")), NOT_JSON);
}

#[test]
fn jupiter_lend_positions_keep_each_coins_decimals() {
    let answer = json!([
        {"token": {"address": JL_USDC, "symbol": "jlUSDC", "decimals": 6, "asset": {"symbol": "USDC", "decimals": 6}, "totalRate": "581"},
         "ownerAddress": "owner", "shares": "4780000", "underlyingAssets": "5001510", "underlyingBalance": "0", "allowance": "0"},
        {"token": {"address": JL_WSOL, "symbol": "jlWSOL", "decimals": 9, "asset": {"symbol": "WSOL"}, "totalRate": 362},
         "shares": 1_400_000_000u64, "underlyingAssets": 1_500_000_000u64},
        {"token": {"symbol": "jlBad"}, "shares": "1"},
        {"shares": "7", "underlyingAssets": "8"},
    ]);
    let p = jupiter_lend::parse_positions(answer.to_string().as_bytes()).unwrap();
    assert_eq!(p[0], jupiter_lend::Position { vault: "jlUSDC".into(), assets: 5_001_510, shares: 4_780_000, address: JL_USDC.into(), coin: "USDC".into(),
                                              decimals: 6, apy: Some(d("5.81")) });
    assert_eq!((p[1].coin.as_str(), p[1].decimals, p[1].assets, p[1].apy), ("WSOL", 9, 1_500_000_000, Some(d("3.62"))));
    // Without its shares a row is left out; without its token it is «?» at 6 decimals.
    assert_eq!(p.len(), 3);
    assert_eq!(p[2], jupiter_lend::Position { vault: "?".into(), assets: 8, shares: 7, ..Default::default() });
}

#[test]
fn jupiter_lend_earnings_by_vault_token() {
    let answer = json!([{"address": JL_USDC, "earnings": "1510"}, {"address": JL_WSOL, "earnings": 12.5}, {"address": JL_USDC, "earnings": "999"}, {"earnings": "1"}]);
    let e = jupiter_lend::parse_earnings(answer.to_string().as_bytes()).unwrap();
    assert_eq!(e, HashMap::from([(JL_USDC.to_string(), n(1510)), (JL_WSOL.to_string(), d("12.5"))]));
}

#[tokio::test]
async fn jupiter_lend_transactions_are_asked_again_on_a_throttle_and_refusals_are_its_words() {
    let wire = STANDARD.encode(b"unsigned wire");
    let built = json!({"transaction": wire}).to_string();
    let (url, seen) = server(&[("/deposit", &[(429, r#"{"error":"Too many requests"}"#), (200, &built)]),
                               ("/withdraw", &[(400, r#"{"error":"amount too small"}"#)]),
                               ("/redeem", &[(503, "<html>503 Service Temporarily Unavailable</html>")])]).await;
    let lend = JupiterLend { base: url, delays: QUICK[..1].to_vec() };
    assert_eq!(lend.deposit_transaction(USDC, "me", 5_000_000).await.unwrap(), b"unsigned wire");
    let body: Value = serde_json::from_str(&seen.lock().unwrap()[1].1).unwrap();
    assert_eq!(body, json!({"asset": USDC, "signer": "me", "amount": "5000000"}));
    // A refusal is final; a gateway's page is asked again, then said without its HTML.
    assert_eq!(message(lend.withdraw_transaction(USDC, "me", 1).await), r#"Jupiter Lend: {"error":"amount too small"}"#);
    assert_eq!(message(lend.redeem_transaction(USDC, "me", 7).await), "Jupiter Lend: no answer right now (HTTP 503). Try again in a minute.");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.iter().filter(|s| s.0.starts_with("POST /withdraw")).count(), 1);
    assert_eq!(seen.iter().filter(|s| s.0.starts_with("POST /redeem")).count(), 2);
    assert_eq!(serde_json::from_str::<Value>(&seen.last().unwrap().1).unwrap()["shares"], "7");
}

// MARK: Kamino

const SOL_RESERVE: &str = "d4A2prbA2whesmvHaL88BH6Ewn5N4bTSU2Ze8P6Bc4Q";
const USDC_RESERVE: &str = "D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59";

/// The main market's /reserves/metrics (2026-11-06): four USDC reserves, the one holding the money the real one.
const RESERVES: &str = r#"[{"reserve": "64AZMUHLB6NYvQSt41JTer4v8NAFDCz5sUPb7dYpCxa", "liquidityToken": "adraSOL", "liquidityTokenMint": "sctmY8fJucsJatwHz6P48RuWBBkdBMNmSMuBYrWFdrw", "maxLtv": "0.45", "borrowApy": "0.023878764430421473", "supplyApy": "0", "totalSupply": "13.546330433959613697", "totalBorrow": "0.0000000000000004667466058259558958631885161594254896045", "totalBorrowUsd": "0.00000000000004688933014998950831764836553445727207468", "totalSupplyUsd": "1360.8633723533347366"},
 {"reserve": "d4A2prbA2whesmvHaL88BH6Ewn5N4bTSU2Ze8P6Bc4Q", "liquidityToken": "SOL", "liquidityTokenMint": "So11111111111111111111111111111111111111112", "maxLtv": "0.74", "borrowApy": "0.05762781348225632", "supplyApy": "0.04425244393610317", "totalSupply": "2406045.1051866678992", "totalBorrow": "2187643.186368003369206945039148384107852", "totalBorrowUsd": "197477424.8274626474923800743050820792456", "totalSupplyUsd": "217192453.66509102819"},
 {"reserve": "5xXxt9uVHrcT5b1KveAT5ZWgk2f74aDjnLEDDEeAxgpN", "liquidityToken": "USDC", "liquidityTokenMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "maxLtv": "0.8", "borrowApy": "0.11424557768564703", "supplyApy": "0", "totalSupply": "0.1", "totalBorrow": "0", "totalBorrowUsd": "0", "totalSupplyUsd": "0.099985627"},
 {"reserve": "6pazpY4icuXZ5sb2jMWAqdG4TtbUoY1SJ45237Fjht9h", "liquidityToken": "USDC", "liquidityTokenMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "maxLtv": "0.8", "borrowApy": "0.10439654057356984", "supplyApy": "0", "totalSupply": "0.1", "totalBorrow": "0", "totalBorrowUsd": "0", "totalSupplyUsd": "0.099985627"},
 {"reserve": "AWnKJ9dsiHcoDCThxE5E93ikDTAXkApoNwrKM2tp9KFJ", "liquidityToken": "USDC", "liquidityTokenMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "maxLtv": "0.8", "borrowApy": "0.10931013633685005", "supplyApy": "0.10931013633685005", "totalSupply": "104.90845988487823668", "totalBorrow": "104.9084598848782366830221018591529080766", "totalBorrowUsd": "104.8933813919389831340636510924526920291", "totalSupplyUsd": "104.89338139193898313"},
 {"reserve": "D6q6wuQSrifJKZYpR1M8R4YawnLDtDsMmWM1NbBmgJ59", "liquidityToken": "USDC", "liquidityTokenMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "maxLtv": "0.8", "borrowApy": "0.055912318778821524", "supplyApy": "0.038601989729266206", "totalSupply": "147274875.99471571341", "totalBorrow": "125450660.5948515968450471497253060140786", "totalBorrowUsd": "125432629.5714042988250326110984759958452", "totalSupplyUsd": "147253708.17678899292"},
 {"liquidityToken": "nameless"}]"#;

/// /staking-yields (2026-11-06), the first six.
const STAKING: &str = r#"[{"apy": "0.0505720236462861", "token": "mSOL", "tokenMint": "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So"}, {"apy": "0.04938694588109982", "token": "jitoSOL", "tokenMint": "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn"}, {"apy": "0.0487", "token": "bSOL", "tokenMint": "bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1"}, {"apy": "0", "token": "aeroSOL", "tokenMint": "aeroXvCT6tjGVNyTvZy86tFDwE4sYsKCh7FbNDcrcxF"}, {"apy": "0", "token": "nxSOL", "tokenMint": "sctmTAsDn4tLUcemqoqYijfuRkiEfAMPi84PNq2EueR"}, {"apy": "0", "token": "edgeSOL", "tokenMint": "edge86g9cVz87xcpKpy3J77vbp4wYd9idEV562CCntt"}]"#;

#[test]
fn kamino_keeps_one_reserve_per_token_the_one_holding_the_money() {
    let r = kamino::parse_reserves(RESERVES.as_bytes()).unwrap();
    assert_eq!(r.iter().map(|r| (r.symbol.as_str(), r.address.as_str())).collect::<Vec<_>>(),
               [("SOL", SOL_RESERVE), ("USDC", USDC_RESERVE), ("adraSOL", "64AZMUHLB6NYvQSt41JTer4v8NAFDCz5sUPb7dYpCxa")]);
    assert_eq!((r[0].supply_apy, r[0].borrow_apy, r[0].tvl_usd), (d("4.425244393610317"), d("5.762781348225632"), d("217192453.66509102819")));
    assert_eq!(r[0].price_usd.round_dp(12), d("90.269485470947"));
    assert_eq!((r[1].supply_apy, r[1].borrow_apy), (d("3.8601989729266206"), d("5.5912318778821524")));
    assert_eq!(message(kamino::parse_reserves(br#"{"error":"An internal error occurred"}"#)), r#"Kamino: {"error":"An internal error occurred"}"#);
}

#[test]
fn kamino_staking_yields_in_percent_and_none_at_zero() {
    let s = kamino::parse_staking_yields(STAKING.as_bytes()).unwrap();
    assert_eq!(s.len(), 3);
    assert_eq!(s[yields::JITO_SOL], d("4.938694588109982"));
}

#[test]
fn kamino_risk_takes_a_borrow_factor_of_at_least_one() {
    // /reserves/d4A2…/stats (2026-11-06): SOL borrows up to 74%, liquidates at 80%.
    assert_eq!(kamino::parse_risk(br#"{"token":"So11111111111111111111111111111111111111112","liqLTV":"0.8","maxLTV":"0.74","borrowFactor":"1.25"}"#).unwrap(),
               kamino::Risk { max_ltv: d("0.74"), liquidation_ltv: d("0.8"), borrow_factor: d("1.25") });
    assert_eq!(kamino::parse_risk(br#"{"liqLTV":0.5,"maxLTV":0.3,"borrowFactor":0.5}"#).unwrap().borrow_factor, n(1));
    assert_eq!(message(kamino::parse_risk(br#"{"error":"Not found"}"#)), r#"Kamino: {"error":"Not found"}"#);
}

/// From KaminoBorrowTests: 0.05 SPCX at $150 with $1 owed; 0.5 SOL at $96 with 20 USDC owed.
#[test]
fn kamino_loan_keeps_the_health_and_names_the_liquidation_price() {
    let spcx = Loan { collateral: vec![Collateral { reserve: "r".into(), symbol: "SPCX".into(), mint: "m".into(), tokens: d("0.05"), value_usd: d("7.5") }],
                      debts: vec![Debt { reserve: "u".into(), symbol: "USDC".into(), tokens: n(1) }], deposit_usd: d("7.5"), debt_usd: n(1),
                      borrow_limit_usd: d("2.25"), liquidation_limit_usd: d("3.75"), adjusted_debt_usd: n(1) };
    assert_eq!(spcx.health(), Some(d("3.75")));
    // Liquidation when 0.05 SPCX × p × 0.5 = $1: p = $40.
    assert_eq!(spcx.liquidation_price(), Some(n(40)));
    let sol = Loan { collateral: vec![Collateral { reserve: "r".into(), symbol: "SOL".into(), mint: "So11111111111111111111111111111111111111112".into(),
                                                   tokens: d("0.5"), value_usd: n(48) }],
                     debts: vec![Debt { reserve: "u".into(), symbol: "USDC".into(), tokens: n(20) }], deposit_usd: n(48), debt_usd: n(20),
                     borrow_limit_usd: d("35.52"), liquidation_limit_usd: d("38.4"), adjusted_debt_usd: n(20) };
    assert_eq!(sol.liquidation_price(), Some(n(50)));
    // No debt, or two coins up: no single price to name.
    assert_eq!(Loan { debts: vec![], debt_usd: n(0), adjusted_debt_usd: n(0), ..sol.clone() }.liquidation_price(), None);
    assert_eq!(Loan { collateral: [sol.collateral.clone(), sol.collateral.clone()].concat(), deposit_usd: n(96), borrow_limit_usd: n(71),
                      liquidation_limit_usd: d("76.8"), ..sol.clone() }.liquidation_price(), None);
}

#[test]
fn backpack_stocks_go_to_the_backpack_market_and_repay_looks_by_collateral() {
    assert_eq!(kamino::market("SOL"), kamino::MAIN_MARKET);
    assert_eq!(kamino::market("USDC"), kamino::MAIN_MARKET);
    for t in ["SPCX", "MU", "SKHY", "SNDK", "spcx"] { assert_eq!(kamino::market(t), kamino::BACKPACK_MARKET, "{t}") }
    assert_eq!(kamino::BACKPACK_STOCKS.map(|s| s.0), ["SPCX", "MU", "SKHY", "SNDK"]);
    assert_eq!(kamino::markets(Some("SOL")), [kamino::MAIN_MARKET]);
    assert_eq!(kamino::markets(Some("SPCX")), [kamino::BACKPACK_MARKET]);
    assert_eq!(kamino::markets(None), [kamino::BACKPACK_MARKET, kamino::MAIN_MARKET]);
}

/// A wallet's obligations as Kamino answers them: a tagged one first, then the plain one with 0.5 SOL ($48.50 now,
/// stored at $48) and 20 USDC owed; the system program's slots are empty ones.
fn obligations() -> String {
    let none = "11111111111111111111111111111111";
    json!([
        {"obligationAddress": "tagged", "obligationTag": 1, "state": {"deposits": []}, "refreshedStats": {"userTotalDeposit": "1"}},
        {"obligationAddress": "plain", "obligationTag": 0,
         "state": {"deposits": [{"depositReserve": SOL_RESERVE, "depositedAmount": "480000000", "marketValueSf": "55340232221128654848"},
                                {"depositReserve": none, "depositedAmount": "0", "marketValueSf": "0"}],
                   "borrows": [{"borrowReserve": USDC_RESERVE, "borrowedAmountSf": "23058430092136939520000000"},
                               {"borrowReserve": none, "borrowedAmountSf": "0"}]},
         "refreshedStats": {"userTotalDeposit": "48.5", "userTotalBorrow": "20.01", "borrowLimit": "35.89", "borrowLiquidationLimit": "38.8",
                            "userTotalBorrowBorrowFactorAdjusted": "20"}},
    ]).to_string()
}

fn reserve(address: &str, symbol: &str, price: &str) -> kamino::Reserve {
    kamino::Reserve { address: address.into(), symbol: symbol.into(), mint: format!("{symbol}mint"), price_usd: d(price), supply_apy: n(4), ..Default::default() }
}

#[test]
fn kamino_loan_is_read_from_the_plain_obligation() {
    let rows = kamino::rows(obligations().as_bytes()).unwrap();
    let (state, stats) = kamino::vanilla(&rows).unwrap();
    let loan = kamino::read_loan(state, stats, &[reserve(SOL_RESERVE, "SOL", "97"), reserve(USDC_RESERVE, "USDC", "1")]);
    assert_eq!(loan.collateral, [Collateral { reserve: SOL_RESERVE.into(), symbol: "SOL".into(), mint: "SOLmint".into(), tokens: d("0.5"), value_usd: d("48.5") }]);
    assert_eq!(loan.debts, [Debt { reserve: USDC_RESERVE.into(), symbol: "USDC".into(), tokens: n(20) }]);
    assert_eq!((loan.deposit_usd, loan.debt_usd, loan.borrow_limit_usd, loan.health(), loan.liquidation_price()),
               (d("48.5"), d("20.01"), d("35.89"), Some(d("1.94")), Some(n(50))));
    // A Backpack stock counts its shares as tokens (6 decimals); a debt of 100,000 USDC (past 28 digits times 2⁶⁰) and
    // one of 5 SOL (9 decimals) read whole; a reserve the market doesn't list is «?».
    let state = json!({"deposits": [{"depositReserve": "spcx", "depositedAmount": "50000", "marketValueSf": 0}],
                       "borrows": [{"borrowReserve": USDC_RESERVE, "borrowedAmountSf": "115292150460684697600000000000"},
                                   {"borrowReserve": SOL_RESERVE, "borrowedAmountSf": "5764607523034234880000000000"},
                                   {"borrowReserve": "gone", "borrowedAmountSf": 1152921504606846976u64}]});
    let loan = kamino::read_loan(state.as_object().unwrap(), &Object::new(), &[reserve("spcx", "SPCX", "150"), reserve(USDC_RESERVE, "USDC", "1"),
                                                                                 reserve(SOL_RESERVE, "SOL", "97")]);
    assert_eq!((loan.collateral[0].tokens, loan.collateral[0].value_usd), (d("0.05"), n(0)));
    assert_eq!(loan.debts.iter().map(|d| (d.symbol.as_str(), d.tokens)).collect::<Vec<_>>(), [("USDC", n(100_000)), ("SOL", n(5)), ("?", d("0.000001"))]);
    // No plain obligation, or one without its stats: no loan.
    assert!(kamino::vanilla(&kamino::rows(br#"[{"obligationTag":1,"state":{},"refreshedStats":{}}]"#).unwrap()).is_none());
    assert!(kamino::vanilla(&kamino::rows(br#"[{"obligationTag":0,"state":{}}]"#).unwrap()).is_none());
    assert!(kamino::vanilla(&kamino::rows(br#"[{"obligationTag":false,"state":{},"refreshedStats":{}}]"#).unwrap()).is_some());
}

#[test]
fn kamino_positions_split_the_refreshed_total_by_the_stored_values() {
    let rows = json!([
        {"state": {"deposits": [{"depositReserve": USDC_RESERVE, "depositedAmount": "1000000", "marketValueSf": "34587645138205409280"},
                                {"depositReserve": SOL_RESERVE, "depositedAmount": "5", "marketValueSf": "11529215046068469760"}]},
         "refreshedStats": {"userTotalDeposit": "44"}},
        // Right after a first deposit nothing is stored yet: split evenly. A number isn't read here, only strings.
        {"state": {"deposits": [{"depositReserve": "a", "depositedAmount": 3, "marketValueSf": 1152921504606846976u64}, {"depositReserve": "b"}]},
         "refreshedStats": {"userTotalDeposit": "10"}},
        {"state": {"deposits": [{"depositReserve": "c", "marketValueSf": "11529215046068469760"}]}},
    ]).to_string();
    let p = kamino::read_positions(&kamino::rows(rows.as_bytes()).unwrap(), &[reserve(SOL_RESERVE, "SOL", "97"), reserve(USDC_RESERVE, "USDC", "1")]);
    assert_eq!(p.iter().map(|p| (p.symbol.as_str(), p.amount, p.value_usd, p.apy)).collect::<Vec<_>>(),
               [("USDC", n(1_000_000), n(33), Some(n(4))), ("SOL", n(5), n(11), Some(n(4))), ("?", n(0), n(5), None), ("?", n(0), n(5), None), ("?", n(0), n(10), None)]);
}

#[tokio::test]
async fn kamino_loan_reads_the_wallets_fresh_state_and_then_the_market() {
    let market = kamino::BACKPACK_MARKET;
    let obligations_path = format!("/kamino-market/{market}/users/W/obligations");
    let reserves_path = format!("/kamino-market/{market}/reserves/metrics");
    let (url, seen) = server(&[(&obligations_path, &[(200, &obligations()), (200, "[]")]), (&reserves_path, &[(200, RESERVES)])]).await;
    let kamino = Kamino { base: url };
    let loan = kamino.loan("W", market).await.unwrap().unwrap();
    assert_eq!((loan.collateral[0].symbol.as_str(), loan.debts[0].tokens), ("SOL", n(20)));
    assert!(seen.lock().unwrap()[0].0.starts_with(&format!("GET {obligations_path}?env=mainnet-beta&fresh=")));
    // Before the first deposit: none, and the market isn't asked.
    assert_eq!(kamino.loan("W", market).await.unwrap(), None);
    assert_eq!(seen.lock().unwrap().iter().filter(|s| s.0.contains("/reserves/")).count(), 1);
}

#[tokio::test]
async fn kamino_transactions_send_the_amount_as_swift_writes_it() {
    let built = json!({"transaction": STANDARD.encode(b"wire")}).to_string();
    let (url, seen) = server(&[("/ktx/klend/deposit", &[(200, &built)]), ("/ktx/klend/borrow", &[(500, r#"{"error":"obligation not found"}"#)]),
                               ("/ktx/kvault/withdraw", &[(502, "<html>Bad gateway</html>")])]).await;
    let kamino = Kamino { base: url };
    assert_eq!(kamino.deposit_transaction("W", USDC_RESERVE, d("1.50"), kamino::MAIN_MARKET).await.unwrap(), b"wire");
    assert_eq!(serde_json::from_str::<Value>(&seen.lock().unwrap()[0].1).unwrap(),
               json!({"wallet": "W", "market": kamino::MAIN_MARKET, "reserve": USDC_RESERVE, "amount": "1.5"}));
    assert_eq!(message(kamino.borrow_transaction("W", USDC_RESERVE, n(1), kamino::BACKPACK_MARKET).await), r#"Kamino: {"error":"obligation not found"}"#);
    assert_eq!(message(kamino.vault_withdraw_transaction("W", "V", d("4.687558")).await), NOT_JSON);
    assert_eq!(serde_json::from_str::<Value>(&seen.lock().unwrap()[2].1).unwrap(), json!({"wallet": "W", "kvault": "V", "amount": "4.687558"}));
    // A status past 2xx is Kamino's refusal, never data: at most 240 bytes of it.
    let long = format!(r#"{{"error":"{}"}}"#, "x".repeat(300));
    let (url, _) = server(&[("/reserves/R/stats", &[(429, &long)])]).await;
    let refused = message(Kamino { base: url }.risk("R").await);
    assert_eq!(refused, format!("Kamino: {}", &long[..240]));
}

// MARK: Kamino Vaults (KaminoVaultTests)

const ALLEZ: &str = "A1USdzqDHmw5oz97AkqAGLxEQZfFjASZFuy4T6Qdvnpo";
const METRICS: (u16, &str) = (200, r#"{"tokensPerShare":"1.0666","tokenPrice":"1","apy":"0.0509"}"#);

fn allez(name: &str) -> String { format!(r#"{{"address":"{ALLEZ}","state":{{"name":"{name}","tokenMint":"{USDC}","tokenMintDecimals":6}}}}"#) }

/// A Kamino Vault deposit as the portfolio row in 52f1973 read it: 4.687558 shares of «Allez USDC» at 1.0666 USDC a share.
async fn read_vault(vault: (u16, &str), metrics: (u16, &str)) -> wallet::Result<Vec<kamino::VaultPosition>> {
    let positions = format!(r#"[{{"vaultAddress":"{ALLEZ}","totalShares":"4.687558"}}]"#);
    let (url, _) = server(&[("/kvaults/users/StubWallet/positions", &[(200, &positions)]), (&format!("/kvaults/{ALLEZ}/metrics"), &[metrics]),
                            (&format!("/kvaults/vaults/{ALLEZ}"), &[vault])]).await;
    Kamino { base: url }.vault_positions("StubWallet").await
}

#[tokio::test]
async fn a_failed_read_isnt_kept_until_a_restart() {
    let error = message(read_vault((500, r#"{"error":"An internal error occurred"}"#), METRICS).await);
    assert!(error.starts_with("Kamino: ") && error.contains("internal error"), "{error}");
    // Kamino answers again: the same launch reads the vault as it is.
    let p = read_vault((200, &allez("Allez USDC")), METRICS).await.unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].name.as_str(), p[0].symbol.as_str(), p[0].mint.as_str(), p[0].vault.as_str()), ("Allez USDC", "USDC", USDC, ALLEZ));
    assert_eq!(p[0].tokens, d("4.687558") * d("1.0666"));
    assert_eq!((p[0].shares, p[0].value_usd, p[0].apy), (d("4.687558"), d("4.687558") * d("1.0666"), d("5.09")));
}

/// An error body is no data, nor a vault answer without its token: never a «?» vault with no mint.
#[tokio::test]
async fn an_answer_without_the_vaults_token_is_an_error() {
    for answer in [(401, r#"{"error":"Not authorized"}"#), (200, r#"{"error":"Not authorized"}"#), (200, r#"{"state":{"name":"Allez USDC"}}"#)] {
        let error = message(read_vault(answer, METRICS).await);
        assert!(error.starts_with("Kamino: "), "{answer:?}: {error}");
    }
}

/// Eight of Kamino's vaults have no name: the vault's token is still read.
#[tokio::test]
async fn a_vault_without_a_name_keeps_its_token() {
    let p = read_vault((200, &allez("")), METRICS).await.unwrap();
    assert_eq!((p[0].name.as_str(), p[0].symbol.as_str(), p[0].mint.as_str()), ("Kamino Vault", "USDC", USDC));
}

/// A refused metrics read is an error, not a deposit worth $0.
#[tokio::test]
async fn a_refused_metrics_read_is_an_error() {
    assert!(message(read_vault((200, &allez("Allez USDC")), (429, r#"{"error":"Too many requests"}"#)).await).starts_with("Kamino: "));
}

#[test]
fn a_vault_of_another_token_is_named_by_its_mint() {
    let p = kamino::vault_position("V".into(), n(2), "Odd".into(), "Zzzz1111".into(), None);
    assert_eq!((p.symbol.as_str(), p.tokens, p.value_usd, p.apy), ("Zzzz…", n(0), n(0), n(0)));
    // A holding of no shares is no holding.
    assert_eq!(kamino::parse_held(br#"[{"vaultAddress":"A","totalShares":"0"},{"vaultAddress":"B","totalShares":2},{"totalShares":"1"}]"#).unwrap(), [("B".to_string(), n(2))]);
}

/// Live (reads only): Kamino's own answer for «Allez USDC», and its error for a vault that isn't.
#[tokio::test]
#[ignore = "reads api.kamino.finance"]
async fn live_vault_by_address() {
    let kamino = Kamino::default();
    assert_eq!(kamino.vault(ALLEZ).await.unwrap(), ("Allez USDC".to_string(), USDC.to_string()));
    assert!(message(kamino.vault("11111111111111111111111111111111").await).starts_with("Kamino: "));
    assert!(kamino.reserves(kamino::MAIN_MARKET).await.unwrap().iter().any(|r| r.symbol == "USDC" && r.supply_apy > Decimal::ZERO));
}

// MARK: Yields

#[test]
fn yields_list_every_source_best_first_and_narrow_to_one_asset() {
    let vaults = jupiter_lend::parse_vaults(JL_TOKENS.as_bytes()).unwrap();
    let reserves = kamino::parse_reserves(RESERVES.as_bytes()).unwrap();
    let staking = kamino::parse_staking_yields(STAKING.as_bytes()).unwrap();
    let all = yields::gather(Some(vaults.clone()), Some(reserves.clone()), Some(staking.clone()), Some(d("-0.02562933272203738")), None);
    assert_eq!(all.iter().map(|o| (o.asset.as_str(), o.place.as_str(), o.apy)).collect::<Vec<_>>(), [
        ("USDC", "Jupiter Lend", d("5.81")), ("SOL", "JitoSOL staking", d("4.938694588109982")), ("SOL", "Kamino Lend", d("4.425244393610317")),
        ("USDC", "Kamino Lend", d("3.8601989729266206")), ("SOL", "Jupiter Lend", d("3.62")), ("?", "Jupiter Lend", d("0.045")),
        ("USDC", "HLP (Hyperliquid)", d("-0.02562933272203738")),
    ]);
    assert_eq!(all[2], yields::YieldOption { asset: "SOL".into(), place: "Kamino Lend".into(), apy: d("4.425244393610317"), tvl_usd: Some(d("217192453.66509102819")),
                                            how: Some("deposit 50 SOL to Kamino".into()),
                                            note: "lending. The first deposit opens Kamino accounts for ≈ 0.03 SOL rent, worth it from ≈ $35".into() });
    assert_eq!((all[4].how.as_deref(), all[4].tvl_usd), (Some("deposit 10 SOL to the vault"), None));
    assert_eq!(all[6].note, "the exchange's market making: some weeks lose money. From $5, withdraw 4 days after a deposit");
    let sol = yields::gather(Some(vaults), Some(reserves), None, None, Some("sol"));
    assert_eq!(sol.iter().map(|o| o.place.as_str()).collect::<Vec<_>>(), ["Kamino Lend", "Jupiter Lend"]);
    assert!(yields::gather(None, None, None, None, None).is_empty());
}

#[tokio::test]
async fn yields_skip_a_source_that_does_not_answer() {
    let staking_path = "/staking-yields";
    let reserves_path = format!("/kamino-market/{}/reserves/metrics", kamino::MAIN_MARKET);
    let (url, seen) = server(&[("/lend/tokens", &[(200, JL_TOKENS)]), (&reserves_path, &[(200, RESERVES)]), (staking_path, &[(429, r#"{"error":"slow"}"#)]),
                               ("/info", &[(200, r#"{"name":"Hyperliquidity Provider (HLP)","apr":0.0523}"#)])]).await;
    let jupiter = JupiterLend { base: format!("{url}/lend"), delays: vec![] };
    let usdc = yields::options_from(&jupiter, &Kamino { base: url.clone() }, &format!("{url}/info"), Some("USDC")).await;
    assert_eq!(usdc.iter().map(|o| (o.place.as_str(), o.apy)).collect::<Vec<_>>(),
               [("Jupiter Lend", d("5.81")), ("HLP (Hyperliquid)", d("5.23")), ("Kamino Lend", d("3.8601989729266206"))]);
    let info = seen.lock().unwrap().iter().find(|s| s.0.starts_with("POST /info")).unwrap().1.clone();
    assert_eq!(serde_json::from_str::<Value>(&info).unwrap(), json!({"type": "vaultDetails", "vaultAddress": yields::HLP}));
    // HLP's answer without a number: no rate.
    assert_eq!(message(yields::parse_vault_apr(br#"{"apr":"0.05"}"#)), "cannot parse response");
    assert_eq!(message(yields::parse_vault_apr(b"null")), NOT_JSON);
}
