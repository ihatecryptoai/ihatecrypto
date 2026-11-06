//! Browser.swift (Browser8Tests, Browser15Tests, Consent7Tests' site requests and call lines, EngineTests' and
//! Shell14Tests' browser bar): a page's request is answered only through its own block and the person's yes, refused
//! without https, a wallet, on a Ledger or on MetaMask's list; every refusal the EVM provider gives before anything is
//! signed; what a Solana transaction sends and gets; the phishing list never read empty while it downloads. Public test
//! data only (Hardhat's account #3, Base's USDC, Solana's system program as an address); nothing goes to the network
//! or a signer.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use evm::call::Call;
use ihatecrypto_core::site_check::Verdict;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use signer_api::{SignerReply, SignerWallet};

use super::{Lines, engine};
use crate::browser::{PhishingList, SiteReview};
use crate::consent::SiteBalance;
use crate::engine::Engine;
use crate::store;

const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
const USDC: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
/// 32 zero bytes in base58: the system program's address, standing in for a wallet's.
const OWNER: &str = "11111111111111111111111111111111";

fn en(lines: &Lines) -> Rc<Engine> {
    let e = engine(lines);
    e.set_settings(|s| s.language = Some("en".into()));
    e
}

fn say(en: &str, _: &str) -> String { en.into() }

fn d(s: &str) -> Decimal { ihatecrypto_core::foundation::decimal(s).unwrap() }

/// A wallet on, with both addresses, and MetaMask's list saved fresh (no download).
fn with_wallet(e: &Engine, automation: bool) {
    std::fs::write(store::url("phishing.json"), r#"{"blacklist":["phish-test.example"],"whitelist":[]}"#).unwrap();
    let w = SignerWallet { id: "2".into(), label: "Main".into(), kind: "phrase".into(), solana: Some(OWNER.into()), evm: Some(MOM.into()), near: None, test: None,
                           automation: automation.then_some(true) };
    e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), wallets: Some(vec![w]), active_wallet: Some("2".into()), ..Default::default() });
}

async fn dapp(e: &Engine, method: &str, params: Value) -> Result<String, String> {
    e.dapp(method, &params.to_string(), "https://jup.ag", "Jupiter").await.map_err(|err| err.0)
}

async fn eth(e: &Engine, method: &str, params: Value) -> Result<String, String> {
    dapp(e, "eth", json!({"method": method, "params": params.to_string()})).await
}

/// Waits until the site's request is on its block.
async fn asked(e: &Engine) -> crate::consent::SiteAskView {
    for _ in 0..200 {
        if let Some(v) = e.st().site_ask.as_ref().map(|a| a.view.clone()) { return v }
        tokio::task::yield_now().await;
    }
    panic!("the site's request never came")
}

/// The person's answer, as consent.rs gives it.
fn answer(e: &Engine, yes: bool) { e.st_mut().site_ask.take().expect("a request waiting").answer(yes) }

#[test]
fn a_solana_transaction_leads_with_what_you_send_and_get() {
    let sol = "So11111111111111111111111111111111111111112".to_string();
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string();
    let fee = ("SOL".to_string(), sol.clone(), d("-0.00204"));
    let usdc = ("USDC".to_string(), usdc_mint.clone(), d("-5"));
    let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // Fee-sized SOL beside a real move is the fee, not a send; Kamino's two programs are Kamino.
    assert_eq!(Engine::solana_lead(&[fee, usdc.clone()], &names(&["Compute Budget", "Kamino Vault", "Kamino Farms"]), say).as_deref(), Some("You send 5 USDC to Kamino"));
    assert_eq!(Engine::solana_lead(&[usdc.clone(), ("SOL".into(), sol.clone(), d("0.02"))], &names(&["Jupiter"]), say).as_deref(), Some("You send 5 USDC and get 0.02 SOL"));
    assert_eq!(Engine::solana_lead(&[("USDC".into(), usdc_mint.clone(), d("3"))], &[], say).as_deref(), Some("You get 3 USDC"));
    assert_eq!(Engine::solana_lead(&[("SOL".into(), sol.clone(), Decimal::ZERO)], &names(&["System"]), say).as_deref(), Some("Only the network fee"));
    assert_eq!(Engine::solana_lead(&[], &[], say), None);
    let three = [usdc.clone(), ("JUP".into(), "j".into(), d("-1")), ("BTC".into(), "b".into(), d("-2"))];
    assert_eq!(Engine::solana_lead(&three, &names(&["Jupiter Swap", "7xKX…sAsU"]), |_, ru| ru.into()).as_deref(), Some("Отправляешь 5 USDC, 1 JUP и 2 BTC в Jupiter Swap"));
    // A lone SOL move is a send, whatever its size; only beside another change is small SOL the fee.
    assert!(Engine::fee_like("SOL", d("-0.01"), 2) && !Engine::fee_like("SOL", d("-0.01"), 1));
    assert!(!Engine::fee_like("SOL", d("-0.2"), 2) && !Engine::fee_like("USDC", d("-1"), 2));
}

#[test]
fn its_programs_read_with_solanas_own_on_one_line() {
    let p = |n: &str, known: bool| (n.to_string(), known);
    let line = Engine::calls_line(&[p("System", true), p("Kamino Vault", true), p("7xKX…sAsU", false), p("Compute Budget", true)], say);
    assert_eq!(line, "It calls: Kamino Vault · 7xKX…sAsU (not on ihatecrypto's list) · and Solana's own: System, Compute Budget");
}

#[test]
fn a_quantity_is_read_as_swift_reads_it() {
    let q = |v: Value| Engine::quantity(Some(&v));
    assert_eq!(Engine::quantity(None), Some(0));
    assert_eq!(q(json!("0x")), Some(0));
    assert_eq!(q(json!("0x10")), Some(16));
    assert_eq!(q(json!("0x1ffffffffffffffff")), None);
    assert_eq!(q(json!("100")), None);
    assert_eq!(q(json!(5)), Some(5));
    // NSNumber's uint64Value: a negative number wraps (its simulation then fails on the balance), a bool is 0 or 1.
    assert_eq!(q(json!(-1)), Some(u64::MAX));
    assert_eq!(q(json!(true)), Some(1));
    assert_eq!(q(Value::Null), None);
}

#[tokio::test]
async fn a_sites_transaction_names_its_tokens() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let token = async |_: &str| ("USDC".to_string(), 6i64);
        let word = |n: u64| { let mut w = vec![0u8; 32]; w[24..].copy_from_slice(&n.to_be_bytes()); w };
        // 1.5 USDC to Mom: «Transfer 1.5 USDC to 0x…», never raw units.
        let line = async |c: Call, value: u64| e.call_line(&c, USDC, value, MOM, 8453, "ETH", &token).await;
        assert_eq!(line(Call::Transfer { to: MOM.into(), amount: word(1_500_000) }, 0).await, format!("Transfer 1.5 USDC to {MOM}."));
        assert_eq!(line(Call::Approve { spender: MOM.into(), amount: vec![0xff; 32] }, 0).await, format!("Allow {MOM} to spend up to unlimited USDC."));
        assert_eq!(e.call_line(&Call::Plain, MOM, 500_000_000_000_000_000, MOM, 8453, "ETH", &token).await, format!("Transfer 0.5 ETH to {MOM}."));
        assert_eq!(line(Call::Repay { asset: USDC.into(), amount: vec![0xff; 32], on_behalf_of: MOM.into() }, 0).await, format!("Repay all the USDC debt of this wallet to {USDC}."));
        assert_eq!(line(Call::Claim { recipients: vec![MOM.to_lowercase()] }, 0).await, "Merkl rewards claim paying this wallet.");
        assert_eq!(line(Call::Contract { selector: "0x12345678".into() }, 1_000_000_000_000_000_000).await, format!("Contract call {USDC}, function 0x12345678, with 1 ETH."));
        assert_eq!(line(Call::CctpBurn { recipient: None, domain: 5, amount: word(2_000_000) }, 0).await,
                   "Circle's CCTP: burn 2 USDC here to mint on Solana to a non-EVM account.");
        // A token that claims more places than Decimal holds reads as 0, never a crash.
        let odd = async |_: &str| ("ODD".to_string(), i64::MAX);
        assert_eq!(e.call_line(&Call::Transfer { to: MOM.into(), amount: word(5) }, USDC, 0, MOM, 8453, "ETH", odd).await, format!("Transfer 0 ODD to {MOM}."));
    }).await;
}

#[tokio::test]
async fn the_card_carries_the_lead_balances_checks_and_note_and_its_block_says_them() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = en(&lines);
        let mut review = SiteReview { lines: vec!["It calls: Kamino Vault".into()], ..SiteReview::default() };
        review.lead = Some("You send 5 USDC to Kamino".into());
        review.balances = vec![SiteBalance { symbol: "USDC".into(), amount: "−5".into(), up: false, usd: Some("$5.00".into()), note: None },
                               SiteBalance { symbol: "SOL".into(), amount: "−0.00204".into(), up: false, usd: None, note: Some("network fee and rent for new accounts".into()) }];
        review.checks = vec!["Every instruction is for a program on the list.".into()];
        let me = e.clone();
        let waiting = tokio::task::spawn_local(async move {
            me.ask("app.kamino.finance", "https://app.kamino.finance", &Verdict::Known, false, "app.kamino.finance asks to sign a transaction", &review.lines.clone(),
                   vec![], vec![], Some(&review), Some("The site gets the signed transaction and sends it itself.".into())).await
        });
        let shown = asked(&e).await;
        assert!(shown.lead.as_deref() == Some("You send 5 USDC to Kamino") && shown.balances.len() == 2 && shown.checks.len() == 1);
        assert!(shown.note.as_deref().is_some_and(|n| n.starts_with("The site gets")) && shown.canvas == e.st().canvas_id && shown.approve == "Sign");
        let said = e.st().boards().iter().find(|b| b.id == shown.id).and_then(|b| b.steps.clone()).unwrap()[0].lines.clone();
        assert_eq!(said[0], "You send 5 USDC to Kamino");
        assert!(said.contains(&"USDC −5 ≈ $5.00".to_string()) && said.contains(&"SOL −0.00204 · network fee and rent for new accounts".to_string()), "{said:?}");
        assert_eq!(e.st().boards()[0].state.as_deref(), Some("waiting"));
        // One at a time: a second request while this one waits is declined at once, never in its place.
        assert!(!e.ask("jup.ag", "https://jup.ag", &Verdict::Known, true, "jup.ag asks to connect", &[], vec![], vec![], None, None).await);
        assert_eq!(e.st().site_ask.as_ref().map(|a| a.view.id.clone()), Some(shown.id.clone()));
        e.decline_site("test");
        assert!(!waiting.await.unwrap());
        assert_eq!(e.st().boards()[0].state.as_deref(), Some("cancelled"));
        // The window was told to come forward, and a notice waits for when it isn't in front.
        let all = lines.all();
        assert!(all.iter().any(|l| l["event"] == "showMain" && l["data"]["why"] == "site"));
        assert!(all.iter().any(|l| l["event"] == "notify" && l["data"]["kind"] == "confirm" && l["data"]["whenAway"] == "1"));
    }).await;
}

#[tokio::test]
async fn a_lookalike_or_unknown_site_says_so_before_approve() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let me = e.clone();
        let waiting = tokio::task::spawn_local(async move {
            me.ask("jupp.ag", "https://jupp.ag", &Verdict::Lookalike("jup.ag".into()), true, "jupp.ag asks to connect", &[], vec![], vec![], None, None).await
        });
        let shown = asked(&e).await;
        assert!(shown.check == "lookalike" && shown.real.as_deref() == Some("jup.ag") && shown.ticks[0].id == "lookalike" && shown.approve == "Connect");
        assert!(e.st().boards()[0].lines[0].starts_with("⚠ The address looks like jup.ag"));
        answer(&e, true);
        assert!(waiting.await.unwrap());
        let me = e.clone();
        let waiting = tokio::task::spawn_local(async move {
            me.ask("example.com", "https://example.com", &Verdict::Unknown, false, "example.com asks to sign a message", &[], vec![], vec![], None, None).await
        });
        let shown = asked(&e).await;
        assert!(shown.check == "unknown" && shown.warnings == ["ihatecrypto doesn't know example.com: check the address bar."]);
        answer(&e, false);
        assert!(!waiting.await.unwrap());
    }).await;
}

#[tokio::test]
async fn disconnect_takes_the_wallet_away_whatever_wallet_is_on_and_the_rest_needs_one() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        assert!(e.st().active_wallet().is_none());
        {
            let mut st = e.st_mut();
            st.sites.solana.insert("https://jup.ag".into());
            st.sites.evm.insert("https://jup.ag".into());
            st.sites.chain.insert("https://jup.ag".into(), 8453);
        }
        assert_eq!(dapp(&e, "disconnect", json!({"everywhere": true})).await.unwrap(), "null");
        assert!(e.st().sites.solana.is_empty() && e.st().sites.evm.is_empty() && e.st().sites.chain.is_empty());
        // A page's own Solana disconnect leaves its EVM connection alone.
        e.st_mut().sites.solana.insert("https://jup.ag".into());
        e.st_mut().sites.evm.insert("https://jup.ag".into());
        dapp(&e, "disconnect", json!({})).await.unwrap();
        assert!(e.st().sites.solana.is_empty() && e.st().sites.evm.contains("https://jup.ag"));
        e.st_mut().sites.evm.clear();
        // Anything else needs a wallet; an EVM call says it with its code.
        assert_eq!(dapp(&e, "connect", json!({})).await.unwrap_err(), "Add a wallet first.");
        assert_eq!(eth(&e, "eth_requestAccounts", json!([])).await.unwrap_err(), "4100|Add a wallet first.");
        // Only https: the window's origin, never the page's say.
        for origin in ["http://jup.ag", "jup.ag", "file:///C:/x.html", "https://"] {
            assert_eq!(e.dapp("connect", "{}", origin, "").await.unwrap_err().0, "Only https sites get the wallet.", "{origin}");
        }
        // The automation wallet never serves a site; a Ledger doesn't yet.
        with_wallet(&e, true);
        assert_eq!(dapp(&e, "connect", json!({})).await.unwrap_err(), "Add a wallet first.");
        with_wallet(&e, false);
        e.st_mut().ledger = Some(crate::ledger::LedgerAccount { evm: Some(MOM.into()), active: Some(true), ..Default::default() });
        assert!(dapp(&e, "connect", json!({})).await.unwrap_err().starts_with("The browser signs with ihatecrypto's own wallets for now"));
        e.st_mut().ledger = None;
        // MetaMask's list: no wallet for a page on it.
        let listed = e.dapp("connect", "{}", "https://phish-test.example", "").await.unwrap_err().0;
        assert_eq!(listed, "phish-test.example is on MetaMask's list of phishing sites: ihatecrypto gives it no wallet.");
        assert!(e.dapp("eth", r#"{"method":"eth_chainId"}"#, "https://www.phish-test.example", "").await.unwrap_err().0.starts_with("4100|www.phish-test.example is on"));
    }).await;
}

#[tokio::test]
async fn a_solana_connect_is_the_persons_yes_and_signing_waits_for_it() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        with_wallet(&e, false);
        // A page asking silently gets nothing until the person connected it.
        assert_eq!(dapp(&e, "connect", json!({"silent": true})).await.unwrap(), "null");
        assert_eq!(dapp(&e, "signMessage", json!({"message": "aGVsbG8="})).await.unwrap_err(), "Connect the site first.");
        assert_eq!(dapp(&e, "signTransaction", json!({"transaction": "AA=="})).await.unwrap_err(), "Connect the site first.");
        assert_eq!(dapp(&e, "signIn", json!({})).await.unwrap_err(), "ihatecrypto doesn't support signIn.");
        // Declined: nothing connected.
        let me = e.clone();
        let call = tokio::task::spawn_local(async move { dapp(&me, "connect", json!({})).await });
        let shown = asked(&e).await;
        assert!(shown.title == "jup.ag asks to connect to your Solana wallet" && shown.lines.last().map(String::as_str) == Some(OWNER));
        answer(&e, false);
        assert_eq!(call.await.unwrap().unwrap_err(), "The user declined.");
        assert!(e.st().sites.solana.is_empty());
        // Approved: the address and its key, and the page stays connected for this run.
        let me = e.clone();
        let call = tokio::task::spawn_local(async move { dapp(&me, "connect", json!({})).await });
        asked(&e).await;
        answer(&e, true);
        assert_eq!(serde_json::from_str::<Value>(&call.await.unwrap().unwrap()).unwrap(),
                   json!({"address": OWNER, "publicKey": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}));
        assert!(e.st().sites.solana.contains("https://jup.ag"));
        assert!(dapp(&e, "connect", json!({})).await.is_ok() && e.st().site_ask.is_none());
        // A message to sign waits for its own yes; declined, the signer is never asked.
        let me = e.clone();
        let call = tokio::task::spawn_local(async move { dapp(&me, "signMessage", json!({"message": "aGVsbG8="})).await });
        let shown = asked(&e).await;
        assert!(shown.title == "jup.ag asks to sign a message" && shown.lines[0] == "hello");
        answer(&e, false);
        assert_eq!(call.await.unwrap().unwrap_err(), "The user declined.");
    }).await;
}

#[tokio::test]
async fn the_evm_provider_refuses_before_anything_is_signed() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = en(&lines);
        with_wallet(&e, false);
        // Arbitrum until the page switches; only Ethereum, Arbitrum and Base.
        assert_eq!(eth(&e, "eth_chainId", json!([])).await.unwrap(), "\"0xa4b1\"");
        assert_eq!(eth(&e, "net_version", json!([])).await.unwrap(), "\"42161\"");
        assert_eq!(eth(&e, "wallet_switchEthereumChain", json!([{"chainId": "0x2105"}])).await.unwrap(), "null");
        assert_eq!(eth(&e, "eth_chainId", json!([])).await.unwrap(), "\"0x2105\"");
        for other in ["0x89", "0x38", "0x", "nope"] {
            assert_eq!(eth(&e, "wallet_switchEthereumChain", json!([{"chainId": other}])).await.unwrap_err(), "4902|ihatecrypto works with Ethereum, Arbitrum and Base.");
        }
        // Not connected: no address, and every signature needs a connection first.
        assert_eq!(eth(&e, "eth_accounts", json!([])).await.unwrap(), "[]");
        assert_eq!(eth(&e, "personal_sign", json!(["0x68656c6c6f", MOM])).await.unwrap_err(), "4100|Connect the site first.");
        assert_eq!(eth(&e, "eth_signTypedData_v4", json!([MOM, "{}"])).await.unwrap_err(), "4100|Connect the site first.");
        assert_eq!(eth(&e, "eth_sendTransaction", json!([{"to": MOM}])).await.unwrap_err(), "4100|Connect the site first.");
        // Blind signatures never.
        assert_eq!(eth(&e, "eth_sign", json!([])).await.unwrap_err(), "4200|eth_sign signs a hash blindly — ihatecrypto doesn't support it.");
        assert!(eth(&e, "eth_signTransaction", json!([])).await.unwrap_err().starts_with("4200|ihatecrypto signs EVM transactions only when it also sends them"));
        assert_eq!(eth(&e, "eth_subscribe", json!([])).await.unwrap_err(), "4200|ihatecrypto doesn't support eth_subscribe.");

        // Connected by the person's yes.
        let me = e.clone();
        let call = tokio::task::spawn_local(async move { eth(&me, "eth_requestAccounts", json!([])).await });
        let shown = asked(&e).await;
        assert!(shown.approve == "Connect" && shown.lines.iter().any(|l| l.contains("Permit")));
        answer(&e, true);
        assert_eq!(call.await.unwrap().unwrap(), json!([MOM]).to_string());
        assert_eq!(eth(&e, "eth_accounts", json!([])).await.unwrap(), json!([MOM]).to_string());
        // A hash to sign, a contract to create, someone else's sender, an amount past 64 bits: refused before any card.
        let hash = format!("0x{}", "ab".repeat(32));
        assert_eq!(eth(&e, "personal_sign", json!([hash, MOM])).await.unwrap_err(), "4001|The site asks to sign a hash, not text: ihatecrypto doesn't sign that.");
        assert_eq!(eth(&e, "eth_sendTransaction", json!([{"data": "0x6080"}])).await.unwrap_err(), "4200|ihatecrypto doesn't sign contract creation.");
        assert_eq!(eth(&e, "eth_sendTransaction", json!([{"to": "0x1234"}])).await.unwrap_err(), "4200|ihatecrypto doesn't sign contract creation.");
        assert_eq!(eth(&e, "eth_sendTransaction", json!([{"to": USDC, "from": "0x0000000000000000000000000000000000000001"}])).await.unwrap_err(),
                   "4100|The sender isn't your address.");
        assert_eq!(eth(&e, "eth_sendTransaction", json!([{"to": USDC, "from": MOM.to_lowercase(), "value": "0x1ffffffffffffffff"}])).await.unwrap_err(),
                   "4200|The amount is larger than ihatecrypto will sign.");
        // A token permission as typed data: refused outright, and said in the chat.
        let permit = json!({"types": {"EIP712Domain": [{"name": "name", "type": "string"}], "Permit": [{"name": "owner", "type": "address"},
                            {"name": "spender", "type": "address"}, {"name": "value", "type": "uint256"}]}, "primaryType": "Permit", "domain": {"name": "USD Coin"},
                            "message": {"owner": MOM, "spender": MOM, "value": "1"}});
        let refused = eth(&e, "eth_signTypedData_v4", json!([MOM, permit.to_string()])).await.unwrap_err();
        assert!(refused.starts_with("4001|Permit: this is permission to spend your tokens (Permit)"), "{refused}");
        assert!(e.st().messages().last().unwrap().text.starts_with("Site jup.ag asked for a “Permit” signature. Declined — "));
        assert!(eth(&e, "eth_signTypedData_v4", json!([MOM, "not json"])).await.unwrap_err().starts_with("4200|"));
        // No card was ever left waiting for any of them.
        assert!(e.st().site_ask.is_none());
    }).await;
}

#[tokio::test]
async fn the_browser_bar_hears_what_the_site_is_whatever_the_lock() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        std::fs::write(store::url("phishing.json"), r#"{"blacklist":["phish-test.example"],"whitelist":[]}"#).unwrap();
        e.st_mut().lock.locked = true;
        let known = e.site_state("https://jup.ag").await;
        assert!(known["check"] == "known" && known["connected"] == "false" && known["blocked"] == "false");
        let fake = e.site_state("https://jupp.ag").await;
        assert!(fake["check"] == "lookalike" && fake["real"] == "jup.ag");
        assert_eq!(e.site_state("https://example.com").await["check"], "unknown");
        assert_eq!(e.site_state("https://phish-test.example").await["blocked"], "true");
        e.st_mut().sites.evm.insert("https://jup.ag".into());
        assert_eq!(e.site_state("https://jup.ag").await["connected"], "true");
        let nothing = e.site_state("not a url").await;
        assert!(nothing.len() == 3 && nothing["check"] == "unknown" && nothing["blocked"] == "false");
        // A held token's price, for the ≈ $ beside a site's balance change.
        let mut row = crate::portfolio::Row::new("JUP", "10", Decimal::from(10));
        (row.contract, row.usd) = (Some("JUPmint".into()), Some(Decimal::from(5)));
        e.st_mut().portfolio.networks = vec![crate::portfolio::Network { key: "solana".into(), name: "Solana".into(), usd: Decimal::from(5), rows: vec![row], note: None,
                                                                         hidden: vec![], hidden_more: 0 }];
        assert_eq!(e.held_solana_price("JUPmint"), Some(d("0.5")));
        assert_eq!(e.held_solana_price("other"), None);
    }).await;
}

fn list_file(name: &str) -> std::path::PathBuf {
    let mut id = [0u8; 6];
    getrandom::fill(&mut id).unwrap();
    std::env::temp_dir().join(format!("browser15-{name}-{}.json", id.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

const LIST: &str = r#"{"blacklist":["phish-test.example"],"whitelist":["ok.phish-test.example"]}"#;

#[tokio::test]
async fn a_day_old_saved_list_answers_while_the_new_one_downloads() {
    let file = list_file("stale");
    std::fs::write(&file, LIST).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(SystemTime::now() - Duration::from_secs(3 * 86_400)).unwrap();
    let (calls, finished) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (c, f) = (calls.clone(), finished.clone());
    let list = PhishingList::new(file.clone(), Arc::new(move || {
        let (c, f) = (c.clone(), f.clone());
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(30)).await;
            f.fetch_add(1, Ordering::SeqCst);
            None
        })
    }));
    let (a, b) = tokio::join!(list.contains("phish-test.example"), list.contains("www.phish-test.example"));
    assert!(a && b);
    for _ in 0..10 { tokio::task::yield_now().await }
    assert_eq!(finished.load(Ordering::SeqCst), 0, "a check waited for the download");
    assert!(!list.contains("ok.phish-test.example").await);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn with_nothing_saved_both_checks_wait_for_the_one_download() {
    let file = list_file("none");
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let list = PhishingList::new(file.clone(), Arc::new(move || {
        let c = c.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(300)).await;
            Some(LIST.as_bytes().to_vec())
        })
    }));
    let (a, b) = tokio::join!(list.contains("phish-test.example"), list.contains("phish-test.example"));
    assert!(a && b);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The download is saved for the next run.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), LIST);
    let _ = std::fs::remove_file(&file);
}
